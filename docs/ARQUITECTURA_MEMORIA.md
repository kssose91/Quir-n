# Arquitectura de la memoria

## Propósito

Un modelo de lenguaje que trabaja sobre un repositorio grande tiene dos maneras
de saber qué hace el código: leerlo entero, o preguntarle a algo que ya lo ha
leído. La primera no escala. Este documento describe la segunda.

La tesis es que un registro inmutable de todo lo ocurrido, más dos proyecciones
derivadas de él —una vectorial y una de grafo—, bastan para situar a un modelo en
cualquier punto de un proyecto sin volcarle el repositorio, y sin que arrastre el
peso de conversaciones anteriores.

## Los tres almacenes, y por qué no son iguales

| Almacén | Naturaleza | Se puede perder |
| --- | --- | --- |
| Registro de eventos | inmutable, encadenado por hash | no |
| Índice vectorial (Qdrant) | proyección | sí |
| Grafo (Neo4j) | proyección | sí |

Esta asimetría es la decisión central del diseño y conviene entender por qué.

El **registro** es la única fuente de verdad. Cada evento se encadena con el hash
del anterior, de modo que alterar uno invalida todos los posteriores. No se
borra ni se sobrescribe: una afirmación equivocada se corrige emitiendo otra que
la reemplaza (`supersedes`) o la retracta (`retracted_by`). El historial de la
equivocación forma parte del registro.

Las **proyecciones** son desechables por definición. Si el índice se corrompe o
el grafo acumula errores, se destruyen y se reconstruyen desde el registro. Esa
propiedad es la que permite que una red neuronal escriba en ellos sin poner en
riesgo nada irreversible: **su peor error posible es un grafo que hay que
reproyectar**, no una memoria falsificada.

Si el grafo fuese también inmutable, una alucinación de la red obrera quedaría
grabada para siempre. Al no serlo, no.

## Identidad de proyecto

Cada carpeta a la que se concede acceso recibe un identificador propio, generado
la primera vez y guardado dentro del proyecto:

```
<proyecto>/.llore/project.id     01KX7Q2M8V3N4P5R6S7T8U9V0W
```

Ese identificador —y no la ruta ni el nombre visible— es lo que aparece en el
registro, en la carga útil de cada punto de Qdrant y en el nodo raíz del grafo.

La razón es que ni la ruta ni el nombre son estables. Mover la carpeta cambia la
ruta; dos proyectos distintos pueden llamarse `backend`. Un identificador propio
sobrevive a mover, renombrar y duplicar. El nombre visible se guarda aparte, y
solo sirve para mostrarlo.

### Identidad heredada

Un proyecto puede tener historial anterior a su identificador. Los eventos de
Quirón, por ejemplo, se registraron bajo el nombre `quiron`.

Esos eventos **no se reescriben**. Reescribir el pasado rompería la cadena de
hashes y convertiría el registro en algo editable, es decir, en algo que ya no es
un registro. La identidad heredada se resuelve anotando, no corrigiendo:

```
evento nuevo:   ProjectAliased { canonical: 01KX7Q…, legacy: "quiron" }
```

Filtrar por el identificador canónico incluye sus alias. El registro sigue
diciendo lo que dijo; la consulta sabe que son el mismo mundo. Es el mismo
mecanismo que `supersedes` aplicado a la identidad.

Las proyecciones no necesitan migrarse: el alias basta para que la recuperación
funcione. Cuando el worker vuelva a pasar por cada unidad, la reescribirá con el
identificador canónico. **Reproyectar a mano no es trabajo de nadie.**

## La concesión de acceso arranca el sistema

La concesión de acceso es el momento en que un directorio pasa a ser un proyecto.
Antes de eso no existe: el editor arranca sin proyecto abierto y la guardia de
rutas (`workspace_guard`) niega toda lectura.

El worker no es una pieza separada del arnés: **nace de él**. La secuencia es una
sola, y no admite pasos sueltos:

```
1. El usuario abre una carpeta y concede acceso.
2. El arnés fija la raíz, canoniza rutas y decide qué es legible:
   ni secretos, ni artefactos, ni nada fuera de la raíz.
3. Se crea o se lee `<proyecto>/.llore/project.id`.
4. Ese identificador queda asociado a la sesión y a todo lo que se escriba.
5. El worker arranca sobre ese proyecto y empieza a vectorizar
   exactamente lo que el arnés permite leer.
```

Sin acceso no hay identificador. Sin identificador no hay proyecto. Sin proyecto
el worker no tiene nada que vectorizar, y el modelo no recibe contexto.

El worker hereda el alcance del arnés y no lo amplía. Lo que la guardia de rutas
niega al editor, se lo niega al worker: si un `.env` no puede abrirse en una
pestaña, tampoco puede acabar convertido en un vector. **El confinamiento no es
una capa de la interfaz; es la frontera del sistema.**

## Aislamiento entre mundos

Un mundo es todo lo que pertenece a un identificador de proyecto.

- El registro es **global**: guarda los eventos de todos los proyectos de la
  máquina, encadenados en un único hilo verificable.
- Las proyecciones son **particionadas**: toda consulta al índice y al grafo
  lleva el identificador del proyecto activo como filtro, aplicado en el almacén
  y no después de recuperar.
- La red obrera sabe siempre sobre qué proyecto trabaja.

Un fragmento de un proyecto no puede aparecer en las respuestas de otro. Cuando
el trabajo abarque varios proyectos, el filtro admitirá una lista explícita de
identificadores; nunca su ausencia.

## Ciclo de vida de una unidad de memoria

Todo evento entra al registro como observación. A partir de ahí, un estado de
promoción decide hasta dónde llega:

```
WorkingSet  ──►  Candidate  ──►  Promoted
                     │              │
                     └──────────────┴──►  Suppressed
```

- **WorkingSet**: registrado, sin proyectar. Vive solo en el registro.
- **Candidate**: entra al índice vectorial.
- **Promoted**: entra además al grafo.
- **Suppressed**: se retira de las proyecciones. Permanece en el registro.

Las dos puertas son deliberadamente distintas:

```rust
fn indexes_semantic()  -> destino Semantic  &&  estado ∈ {Candidate, Promoted}
fn projects_to_neo4j() -> destino Graph     &&  estado == Promoted
```

El índice admite candidatos porque la similitud vectorial es tolerante al ruido:
un vector poco relevante baja en el ranking y no daña nada. El grafo exige
promoción porque una arista falsa no se degrada, se propaga: quien recorra el
grafo tomará esa relación por cierta.

### Este ciclo no gobierna el índice de código

La promoción existe porque una afirmación sobre el mundo puede ser dudosa, y
conviene distinguir lo observado de lo confirmado. Es un mecanismo pensado para
la memoria del agente: si aquella observación era fiable, si aquella decisión
quedó confirmada.

**Una unidad de código no tiene incertidumbre.** Una función existe o no existe;
su hash coincide o no coincide. No hay nada que puntuar ni que ascender. Si el
archivo cambió, se recalcula la ficha de las unidades afectadas y se sustituye la
anterior. Si el símbolo desapareció, su punto se borra.

El índice de código, por tanto, no atraviesa estados: se escribe y se retira
según el hash. La promoción gobierna la memoria de eventos, que es otra cosa y
está fuera del alcance de este trabajo.

## El trabajo continuo, y quién lo hace

El sistema observa los cambios sin descanso. Cuando se modifica un archivo, no se
reindexa el proyecto entero: se toma **lo que cambió en ese momento**, se
localizan las unidades afectadas y se actualizan solo sus vectores, bajo el
identificador de proyecto que les corresponde.

Modificar `crates/llore_ui/src/display.rs` actualiza las unidades de
`display.rs`, y ninguna más.

Ese trabajo continuo se reparte entre dos piezas que conviene no confundir,
porque una no necesita a la otra para existir:

| | Quién | Naturaleza |
| --- | --- | --- |
| Detectar qué archivos cambiaron | indexador | hash y `mtime` |
| Extraer las unidades afectadas | indexador | análisis sintáctico |
| Calcular sus vectores | indexador | `bge-m3` |
| Sustituir la ficha anterior, borrar lo que ya no existe | indexador | determinista |
| Redactar la ficha: responsabilidad, entradas, salidas | red obrera | aprendida |
| Proponer candidatos a lógica duplicada | red obrera | aprendida |

Las cuatro primeras filas no requieren una sola red entrenada por nosotros. Un
diff, un árbol sintáctico y un modelo de embeddings ya existente bastan para que
el índice se mantenga vivo, incremental y correcto.

La red obrera no vota ni asciende nada. Describe.

Si la actualización incremental dependiera de la red obrera, el sistema no
funcionaría hasta que la red estuviera entrenada, y la red no podría entrenarse
porque no habría corpus. **El indexador debe ser determinista precisamente para
que la red pueda ser la última pieza.**

## La red obrera

Trabaja de forma continua sobre el registro, no por lotes ni bajo demanda. Es una
red neuronal pequeña y propia, entrenada sobre las unidades del índice.

Su trabajo es uno solo: **redactar la ficha de cada unidad**.

1. **Clasificar** la unidad por tipo y responsabilidad.
2. **Describir** qué hace, qué recibe, qué devuelve y a qué llama, dentro de los
   límites de palabras fijados.
3. **Proponer** candidatos a lógica duplicada, para revisión humana.

No decide qué se guarda ni qué se borra: eso lo dicta el hash. No puntúa
relevancia. No asciende ni suprime nada.

Lo que no puede hacer:

- No escribe en el registro. Solo lee.
- No controla el sistema de archivos, ni Qdrant, ni Neo4j directamente. Propone;
  otro proceso determinista aplica.
- No emite una salida que no cumpla su esquema, no se refiera al hash vigente de
  la unidad y no supere las validaciones. Una salida que no valida se descarta y
  queda registrada como descartada.
- No infiere el motivo de un cambio sin evidencia. Si no la hay, el motivo es
  `unknown`.

El tamaño, la arquitectura y la memoria necesaria se deciden midiendo sobre un
corpus de proyectos reales, no por estimación previa. La máquina de desarrollo
dispone de 48 GB de memoria de vídeo repartidos en dos aceleradores.

No confundir la red obrera con dos piezas que ya existen y son deterministas en
su interfaz: el **reranker** (`bge-reranker-v2-m3`, preentrenado, solo ordena
resultados) y el **worker de recuperación** (filtra, busca, expande el grafo y
recorta al presupuesto de contexto).

## Conversación efímera

Cada conversación empieza en blanco. No hereda el historial de la anterior.

La razón es que un contexto conversacional largo diluye la atención del modelo
sobre lo que importa y encarece cada turno. Un chat que recuerda tres días de
divagaciones responde peor que uno que no recuerda nada y consulta un índice
fiable.

Lo que sí es permanente es el proyecto. Al abrir una conversación, el modelo no
sabe qué se dijo ayer, pero sabe qué hace cada archivo hoy, de qué depende y qué
cambió. Está situado sin estar lastrado.

Las conversaciones **se copian íntegras al registro** cuando terminan. Cerrar una
conversación, del usuario o del agente, la vuelca entera al registro. Ese es su
único destino: son la materia prima con la que el worker sabrá, más adelante, qué
se decidió y por qué. Un turno de chat no es memoria; es evidencia de la que se
destila memoria.

Por eso no vuelven al chat en bruto, ni se recuperan por similitud, ni por
palabras clave. El registro las guarda para el worker, no para el modelo.

## Orden de construcción

El sistema se levanta en fases, y cada una exige que la anterior funcione. La red
neuronal es la última, no la primera:

| Fase | Contenido | Naturaleza |
| --- | --- | --- |
| 1 | Arnés, concesión de acceso, identificador de proyecto y sus alias | determinista |
| 2 | Registro inmutable; la conversación se copia a él al cerrarse | determinista |
| 3 | Indexador incremental: detecta el cambio, actualiza solo lo afectado | determinista |
| 4 | Grafo de dependencias, particionado por proyecto | determinista |
| 5 | Worker de recuperación: buscar, expandir, reordenar | determinista |
| 6 | Red obrera: clasificar, resumir, puntuar, promover | aprendida |

Las cinco primeras fases producen un sistema útil sin una sola red entrenada por
nosotros. La sexta lo mejora; no lo sostiene.

Construir la red obrera antes que el indexador sería entrenarla sobre un corpus
que no existe, y validar sus salidas contra hashes que nadie calcula.

## Invariantes

Lo que no cambia, aunque el esquema crezca:

1. El registro es inmutable y su cadena de hashes, verificable en cualquier
   momento.
2. Toda unidad tiene identificador estable, hash y proyecto.
3. Las proyecciones se reconstruyen desde el registro con el mismo resultado
   lógico.
4. Todo fragmento entregado a un modelo incluye ruta, símbolo, rango y hash.
5. Ningún motivo se infiere sin evidencia.
6. Una consulta sobre un proyecto nunca devuelve unidades de otro.
7. La red obrera propone; nunca ejecuta.

## Criterios de aceptación

- `GET /chain/verify` devuelve `valid: true` tras cualquier operación.
- Borrar el índice y el grafo y reproyectarlos desde el registro produce el mismo
  contenido lógico.
- Una conversación nueva no contiene ningún turno de una conversación anterior.
- Ninguna consulta filtrada por proyecto devuelve unidades de otro proyecto.
- Mover o renombrar la carpeta de un proyecto no crea un mundo nuevo.
- Una salida de la red obrera que no valida contra el esquema queda descartada y
  registrada.
- Suprimir una unidad la retira de las proyecciones y la conserva en el registro.

## Estado medido (10 de julio de 2026)

Construido y verificado:

| Pieza | Estado |
| --- | --- |
| Registro encadenado (Blake3) | `valid: true`, 5028 eventos |
| Corrección sin borrado (`supersedes`, `retracted_by`) | implementada |
| Estados de promoción | implementados |
| Puertas `indexes_semantic` / `projects_to_neo4j` | implementadas |
| Índice vectorial | 4236 puntos: 3392 candidatos, 844 promovidos |
| Grafo | 1055 nodos |
| Conversaciones en el índice vectorial | 0 — la política ya las excluye |

Sin construir, o construido y apagado:

- **La red obrera.** Los procesos de destilación y enriquecimiento existen y
  están desactivados por configuración.
- **El alias de identidad.** El editor ya resuelve `<proyecto>/.llore/project.id`
  y envía ese ULID. El registro guarda nombres heredados (`quiron`). Falta el
  evento `ProjectAliased` y su resolución en el filtro; hasta entonces, filtrar
  por el ULID no encuentra nada.
- **El aislamiento del grafo.** La consulta al grafo usa el proyecto como campo
  de texto donde buscar, no como filtro. Un proyecto puede ver a otro.
- **La conversación efímera.** La recuperación combina cinco fuentes, y una de
  ellas busca por palabras clave sobre el registro, de donde reaparecen las
  conversaciones anteriores.
- **El índice de código.** El registro contiene eventos, no unidades de tipo
  Archivo, Lógica y Cambio. Ver `ARQUITECTURA_INDICE.md`.

Durante meses el binario se compiló sin sus funcionalidades opcionales
(`default = []` en el manifiesto), de modo que la recuperación no consultaba ni
el índice ni el grafo. Ambos existían y nadie los leía. El arranque debe fijar
`--features full`.
