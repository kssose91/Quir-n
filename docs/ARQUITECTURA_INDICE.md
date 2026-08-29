# Arquitectura mínima del índice de código

## Propósito

Dar al editor y al chat un mapa compacto y actualizado de proyectos grandes.
El índice no es una memoria personal ni un almacén de conversaciones.

El índice es una **proyección** del registro inmutable de eventos, no una fuente
de verdad. Puede destruirse y reconstruirse. El modelo general —registro,
promoción, proyecciones, conversación efímera— está en `ARQUITECTURA_MEMORIA.md`,
y este documento desarrolla una de sus dos proyecciones.

Este documento fija un punto de partida, no un límite. El esquema crece a medida
que se mide: pueden añadirse tipos de unidad, relaciones del grafo, proveedores
de modelo y consumidores del índice sin que el resto del sistema se reescriba.

Lo que no cambia son los invariantes. Toda unidad tiene identificador estable,
hash y proyecto. Todo fragmento entregado a un modelo es verificable contra su
origen. Ningún motivo se infiere sin evidencia. Cuanto se añada después deberá
respetarlos, y por eso conviene que sean pocos y explícitos.

## Unidad de información

### Archivo

- Ruta relativa, lenguaje y hash de contenido.
- Responsabilidad principal en un máximo de 120 palabras.
- Símbolos principales.
- Dependencias entrantes y salientes.
- Indicadores de código generado, test, configuración o secreto.

### Lógica

Una unidad por función, método, clase, módulo o bloque equivalente:

- Identificador estable: proyecto, ruta y símbolo.
- Firma y rango de líneas.
- Propósito, entradas, salidas y efectos laterales.
- Llamadas, invariantes y errores relevantes.
- Hash normalizado para detectar implementaciones equivalentes.
- Texto semántico limitado a 180 palabras.

El código completo no se copia al texto vectorizado. El punto conserva la ruta,
el rango y el hash para recuperar el código original cuando haga falta.

### Cambio

- Identificador del archivo o símbolo afectado.
- Hash anterior y posterior.
- Motivo con fuente explícita: tarea de usuario, commit o `unknown`.
- Evidencia de validación disponible.

El motivo nunca se deduce ni se inventa si no existe evidencia.

## Aislamiento por proyecto

Cada proyecto es un mundo cerrado. Abrir la carpeta de un proyecto determina qué
unidades existen para el sistema; ninguna unidad de otro proyecto puede aparecer
en un resultado, ni en el contexto entregado al modelo.

El aislamiento se apoya en tres puntos, y los tres son obligatorios:

1. Toda unidad lleva un campo `project` en su carga útil. Una unidad sin proyecto
   es un defecto, no un caso admisible.
2. Qdrant aplica el filtro exacto por `project` **antes** de calcular similitud.
3. Neo4j cuelga todo nodo de un nodo raíz `Project`, y ninguna consulta atraviesa
   dos raíces.

El identificador de proyecto se deriva de la ruta canónica de la carpeta, no del
nombre visible, para que renombrar un directorio no funda dos mundos.

Mantener este invariante es la primera responsabilidad de la red obrera.

### Confinamiento del editor

El editor arranca sin ningún proyecto abierto. Hasta que el usuario elige una
carpeta no existe raíz, y sin raíz no se lee nada: el sistema falla cerrado.

Toda ruta se clasifica antes de tocar el disco, comparando formas canónicas para
que ni `..` ni un enlace simbólico permitan salir. Hay tres veredictos y no
admiten la misma respuesta:

| Veredicto | Significado | Respuesta |
| --- | --- | --- |
| `Secret` | credencial o material criptográfico | negación absoluta |
| `Outside` | fuera de la raíz del proyecto | abrir ese proyecto o autorizarlo |
| `Noise` | artefacto generado o binario | oculto; legible bajo petición |

Un `.env` es secreto; un `.env.example` es una plantilla y se lee. Un secreto
dentro de un directorio de ruido sigue siendo secreto. El chat permanece
deshabilitado mientras no haya proyecto: sin identidad de proyecto, el contexto
recuperado no pertenece a ningún mundo.

La distinción entre `Secret` y `Noise` es la que permite confinar sin cegar: el
usuario puede pedir ver `target/`, pero nadie —ni él— hace que el modelo lea una
clave privada.

## Grafo de dependencias

La similitud vectorial responde a «qué se parece a esto». No puede responder a
«de qué depende esto» ni a «qué se rompe si lo cambio». Esas dos preguntas son
requisitos del índice, y exigen un grafo. Qdrant y Neo4j no se integran entre
sí: son almacenes independientes con responsabilidades separadas.

- **Qdrant** guarda los vectores y el texto semántico de cada unidad. Es el
  almacén canónico de similitud.
- **Neo4j** guarda las relaciones estructurales entre unidades, extraídas por
  análisis estático y nunca por inferencia del modelo.

Nodos: `Project`, `File`, `Module`, `Symbol`, `Change`.

Relaciones mínimas: `IMPORTS`, `CALLS`, `DEPENDS_ON`, `DEFINED_IN`, `CHANGED_IN`.

Un nodo del grafo y un punto de Qdrant se corresponden por el identificador
estable de la unidad —proyecto, ruta y símbolo—, de modo que un resultado
vectorial pueda expandirse por el grafo y viceversa.

Las relaciones se derivan del árbol sintáctico. Si el análisis estático no puede
resolver una llamada, la relación no se crea; no se completa con conjeturas.

## Worker de recuperación

Une los dos almacenes. Es el componente que hace útil tenerlos separados, y no
existe como pieza de terceros:

1. Filtros exactos sobre el proyecto y la ruta.
2. Búsqueda vectorial en Qdrant. Devuelve candidatos, no respuestas.
3. Expansión de cada candidato por el grafo en Neo4j, hasta una profundidad
   acotada, para recoger dependencias y llamadas relacionadas.
4. Reordenación del conjunto con el reranker (`bge-reranker-v2-m3`), que ya
   forma parte del servicio semántico local.
5. Recorte al presupuesto de contexto del consumidor.

El reranker es una red neuronal preentrenada que ordena; no es la red obrera
descrita más abajo, y no debe confundirse con ella.

## Flujo

1. Escaneo determinista respetando exclusiones del proyecto.
2. Cálculo de hashes y análisis de símbolos.
3. Cola duradera únicamente para unidades nuevas, modificadas o eliminadas.
4. Resumen semántico validado contra el hash de origen.
5. Escritura en una colección canónica de Qdrant.
6. Escritura de nodos y relaciones en Neo4j, con el mismo identificador estable.
7. Recuperación mediante el worker: filtros exactos, similitud, expansión por el
   grafo y reordenación.

El checkpoint solo avanza después de confirmar la escritura o eliminación en
**ambos** almacenes. Una unidad presente en Qdrant y ausente del grafo es un
fallo, no un estado intermedio aceptable. Los fallos quedan visibles y
reintentables; no se omiten en silencio.

## Integración con el editor

Ningún modelo recibe acceso directo a la base de datos ni al grafo completo. La
API local construye un paquete de contexto limitado y verificable a partir de:

- Proyecto y archivos activos de la ventana.
- Selección actual del editor.
- Símbolos o cambios seleccionados en la vista de grafo.
- Resultados exactos y vectoriales relevantes para la consulta.

Cada fragmento entregado a un modelo incluye ruta, símbolo, rango y hash. La
respuesta conserva esas referencias para poder abrir la evidencia desde el
editor.

La integración es interna, por HTTP contra la API local. El modelo de lenguaje se
alcanza a través de `vertex-gateway`, y no se emplea ninguna clave de API.

### Contrato de la API

`quiron-brain` escucha en `127.0.0.1:8766` y exige autenticación por *bearer
token*. Es la única puerta al índice: ningún cliente habla directamente con
Qdrant ni con Neo4j.

```
GET  /context                 contexto del proyecto abierto
GET  /search                  búsqueda semántica
GET  /crag                    recuperación aumentada
GET  /recall
GET  /project/:id/timeline    historia del proyecto

POST /v1/messages             inferencia, formato Anthropic Messages
POST /event  /events/batch    escritura
POST /invariants
```

`/v1/messages` recupera el contexto limitado al proyecto de la carpeta abierta,
enruta por la vía `primary` o `worker` y envía la petición al modelo. La vía
`worker` exige una tarea explícita: no existe enrutado autónomo.

Que la API adopte el formato de mensajes de Anthropic mientras el modelo servido
es de OpenAI demuestra que el índice no depende del proveedor. Es una propiedad
del diseño, no un accidente.

## Historial de conversaciones

Las sesiones de Codex/VS Code son una fuente de evidencia, no puntos vectoriales
en bruto. Un importador incremental debe enumerar todas las sesiones, conservar
su cursor por archivo y extraer únicamente decisiones, intentos, errores,
validaciones y relaciones con archivos o símbolos.

El texto completo permanece en el archivo de origen. El índice guarda un
resumen estructurado y la referencia a sesión, turno y timestamp. Así se evita
llenar Qdrant con saludos, respuestas repetidas o contexto personal irrelevante.

## Aplicación y ventanas

La aplicación comparte una sola instancia de servicios —gateway, cliente del
índice y estado de trabajos—, pero mantiene estado independiente por ventana:

- Raíz del proyecto.
- Pestañas y selección del editor.
- Conversación y contexto activo.
- Vista, filtros y selección del grafo.
- Layout y posición de la ventana.

El workbench ofrece cinco vistas principales: Explorer, Search, Source Control,
Graph e History. El chat es un panel opcional. Graph e History pueden abrirse
como pestaña central o desprenderse a una ventana nativa nueva.

Cerrar una ventana no termina la aplicación mientras existan otras. `New
Window` crea una sesión vacía; `Open Graph in New Window` reutiliza el proyecto
y la conexión compartida sin duplicar procesos ni caches.

## Exclusiones obligatorias

- `.git`, `target`, `node_modules`, dependencias vendorizadas y builds.
- Bases de datos, modelos, binarios, imágenes y archivos generados.
- Logs, caches, históricos y archivos fuera del límite configurado.
- Claves, tokens y archivos detectados como secretos.
- Comentarios o fragmentos sin relación con la responsabilidad de la unidad.

## Lógica duplicada

La similitud vectorial solo genera candidatos. Una alerta requiere además:

1. Huella normalizada compatible.
2. Entradas, salidas o efectos comparables.
3. Evidencia de ambos símbolos y sus ubicaciones.
4. Umbral medido sobre un conjunto de casos etiquetados.

El grafo aporta una señal que el vector no tiene: dos símbolos que hacen lo
mismo y además dependen del mismo conjunto de nodos son candidatos más fuertes
que dos símbolos meramente parecidos en su texto.

La herramienta informa; nunca elimina ni fusiona código automáticamente.

## Red neuronal obrera

Objetivo principal del mes. Es una red neuronal pequeña y propia, entrenada sobre
las unidades del índice.

No confundirla con el reranker (`bge-reranker-v2-m3`), que es un modelo
preentrenado de terceros y solo ordena resultados; ni con el worker de
recuperación, que es determinista.

Trabaja de forma continua sobre el índice, y sus tareas son, por orden:

1. **Asignar y mantener el `project` de cada unidad.** Sin este invariante los
   mundos se contaminan y todo lo demás carece de valor.
2. **Clasificar unidades** por tipo y responsabilidad.
3. **Producir el resumen estructurado** de cada archivo y cada lógica, dentro de
   los límites de palabras fijados arriba.
4. **Priorizar candidatos** a lógica duplicada para revisión humana.

No controla el sistema de archivos, ni Qdrant, ni el grafo. Propone; no ejecuta.

Toda salida cumple un esquema, se refiere al hash vigente de la unidad y supera
validaciones deterministas antes de escribirse. Una salida que no valida se
descarta y queda registrada; no se corrige a mano ni se acepta a medias.

El tamaño, la arquitectura y la memoria necesaria se deciden midiendo sobre un
corpus de proyectos reales y métricas, no por estimación previa.

## Criterios de aceptación

- Un archivo sin cambios no vuelve a procesarse.
- Crear, modificar, renombrar o borrar actualiza solo las unidades afectadas.
- Reiniciar tras un fallo no pierde operaciones pendientes.
- Una búsqueda devuelve ruta, símbolo, rango y hash verificables.
- Las alertas de duplicación muestran las dos evidencias.
- El índice puede reconstruirse desde cero con el mismo resultado lógico.
- Toda unidad escrita en Qdrant tiene su nodo correspondiente en el grafo.
- El grafo responde «qué llama a este símbolo» sin recurrir a similitud.
- El worker devuelve resultados dentro del presupuesto de contexto fijado.
- Ninguna unidad carece de `project`.
- Una consulta sobre un proyecto nunca devuelve unidades de otro.
- Renombrar la carpeta de un proyecto no crea un mundo nuevo.
- Toda salida de la red obrera que no valida contra el esquema queda descartada
  y registrada.

## Estado medido (10 de julio de 2026)

Este documento describe el objetivo. Lo construido hoy es la infraestructura:

| Componente | Estado |
| --- | --- |
| Embeddings `bge-m3` (1024 dim) y reranker | operativos en `:8091` |
| API HTTP en `:8766`, con `/v1/messages` | operativa |
| `vertex-gateway` (`codex_direct`, `gpt-5.6-sol`) | operativo y verificado |
| Editor `llore_ui` (17 691 líneas, nativo) | compila; integrado en el escritorio (`app_id=llore`) |
| Qdrant | operativo; una colección, `quiron_events`, 4223 puntos, todos `kind=Action` |
| Neo4j | operativo; 844 `Event`, 32 `Tag`, 10 `File`, 6 `Module`, 1 `Symbol` |
| Relaciones del grafo | `TAGGED`, `MENTIONS`, `IN_MODULE`. Ninguna de dependencia |
| Unidades Archivo / Lógica / Cambio | no existen |
| Aislamiento por proyecto | el filtro se aplica después de la búsqueda, no en Qdrant |
| Identificador de proyecto | incoherente: ruta en el editor, nombre en los eventos |
| Worker de recuperación | no existe; los dos almacenes duplican eventos |
| Red obrera | no existe |

Reparto real de los 4223 puntos por `memory_scope`: 2571 de ámbito `project`
—`quantum-llore-hub` 2036, `quiron` 445, y unos 90 repartidos— y **1652 de
ámbito `global`**, que son memoria personal del agente y restos de pruebas.

Mientras el grafo no contenga relaciones de dependencia, los requisitos
«dependencias entrantes y salientes» y «llamadas» de este documento no se
cumplen, y la detección de lógica duplicada carece de su segunda señal.

La recuperación falla por causas medidas, y ninguna es que falte el campo
`project`:

1. **El binario se compilaba sin sus funcionalidades.** `default = []` en el
   manifiesto: sin `--features full`, los bloques `#[cfg(feature = "semantic")]`
   y `#[cfg(feature = "neo4j")]` desaparecen. `recall_async` no consultaba ni el
   almacén vectorial ni el grafo; recuperaba solo por palabras clave. Los 4223
   vectores y los 1055 nodos existían y nadie los leía.
2. **El identificador de proyecto no es el mismo en los dos extremos.** El
   editor envía la ruta canónica (`/home/kssose/Quirón`); los eventos guardan un
   nombre (`quiron`). El filtro compara cadenas y nunca coinciden.
3. **El filtro se aplica después de la búsqueda.** `SearchPointsBuilder` se
   invoca sin `filter`, y Qdrant carece de índice de carga útil sobre `project`.
   Funciona, pero recupera candidatos que después descarta.
4. **El chat se alimenta de sí mismo, pero no por la vía vectorial.** El índice
   no contiene ninguna conversación: la política de promoción ya las excluye. Se
   reinyectan porque la recuperación combina cinco fuentes, y una de ellas es una
   búsqueda por palabras clave directamente sobre el registro de eventos. Una
   pregunta repetida recupera su propia respuesta anterior.

Se comprobó, por el contrario, que las memorias de ámbito global **no** desplazan
a las del proyecto: ante consultas reales, los veinticinco candidatos más
próximos pertenecen todos al proyecto consultado. La hipótesis de que formaban un
sumidero vectorial se midió y resultó falsa; procedía de sondear el índice con
vectores aleatorios, que gravitan hacia el grupo más denso.

Las correcciones son deterministas y preceden a la red obrera: compilar con sus
funcionalidades, unificar el identificador de proyecto, filtrar dentro de Qdrant,
y trasladar la memoria personal y las conversaciones fuera del índice de código.
