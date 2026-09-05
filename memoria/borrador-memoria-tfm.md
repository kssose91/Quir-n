# QUIRÓN — MEMORIA DEL TRABAJO FIN DE MÁSTER (BORRADOR v1)

> **Actualización de cierre — 5 de septiembre de 2026.** El cuerpo conserva el
> borrador de julio para revisión. Se ha implementado y probado un worker local
> Qwen2.5-Coder-1.5B Q4_K_M con BGE-M3 separado: apertura de proyecto, fichas,
> firmas Rust, vectores Qdrant, proyección Neo4j, cambios y borrados. También se
> ha probado Claude mediante su CLI oficial con sesión de suscripción y retorno
> de herramientas de Quirón. Evidencias y límites en
> `docs/WORKER_Y_PROVEEDORES.md` y `docs/CIERRE_2026-09-05.md`.
> Las fichas v4 distinguen resumen neuronal de ficha estructural cuando falla
> la validación. El modelo pequeño sirve como ayuda de localización: omitió una rama de error
> en la muestra, por lo que no se afirma fidelidad semántica completa. No hay
> entrenamiento propio ni corpus descargado. La reconstrucción del índice desde
> el ledger, la expansión AST y la evaluación en proyectos grandes siguen
> pendientes. Existe un candidato de instalador Linux; falta probarlo en equipo
> limpio. La hipótesis de ahorro de contexto todavía necesita mediciones.

> **[NOTA PARA EL TUTOR: este documento es el borrador de control de julio. Las marcas
> `[COMPLETAR]` señalan datos administrativos pendientes y las marcas `[PENDIENTE · sept]`
> señalan resultados cuya medición final está planificada antes del 10 de septiembre de 2026,
> fecha de cierre. La estructura sigue la plantilla oficial de memoria TFM de la Escuela de
> Arquitectura, Ingeniería y Diseño.]**

---

**UNIVERSIDAD EUROPEA DE MADRID**
ESCUELA DE ARQUITECTURA, INGENIERÍA Y DISEÑO
MÁSTER EN FORMACIÓN PERMANENTE EN INTELIGENCIA ARTIFICIAL APLICADA

**TRABAJO FIN DE MÁSTER**

**Quirón: un editor nativo con índice de código verificable y una red neuronal local para el aporte de contexto a modelos de lenguaje** *(título propuesto — [COMPLETAR/confirmar])*

**Autor:** Lorenzo Juan Santacreu Pascual
**Dirigido por:** [COMPLETAR — Nombre del director/a]
**Curso 2025–2026**

---

# RESUMEN

Los asistentes de programación basados en modelos de lenguaje conocen el repositorio volcándolo, total o parcialmente, en la ventana de contexto. El enfoque no escala: el coste crece con el tamaño del proyecto, la atención del modelo se diluye y, en proyectos largos, se pierde la consistencia — se duplican lógicas que ya existen, divergen nombres y tipados, se recrean carpetas que ya estaban. Este trabajo presenta Quirón, un entorno de desarrollo que no pretende sustituir el contexto del modelo, sino hacerlo preciso: un índice consultable y verificable le da la localización exacta de cada lógica. Un registro inmutable de eventos, encadenado por hash, actúa como única fuente de verdad; de él se derivan dos proyecciones desechables: un índice vectorial (Qdrant), que responde a «qué se parece a esto», y un grafo de dependencias (Neo4j), que responde a «de qué depende esto y qué se rompe si lo cambio». Un worker de recuperación une ambos almacenes y entrega al modelo fragmentos mínimos con ruta, símbolo, rango y hash, verificables contra el código de origen. El mantenimiento continuo del índice se encomienda a una red neuronal propia, pequeña y local —la red obrera—, diseñada a partir de la matemática publicada de los modelos recientes, entrenada por destilación a nivel de secuencia y desplegada cuantizada en hardware propio; propone y nunca ejecuta. Se han construido el editor nativo en Rust, la infraestructura de servicios y el registro inmutable, junto con el plan de evaluación del ahorro de tokens y de la eliminación de duplicados. **[PENDIENTE · sept: cifras finales.]**

**Palabras clave:** modelos de lenguaje, recuperación aumentada, índice de código, grafo de dependencias, destilación de conocimiento, inferencia local.

# ABSTRACT

LLM-based programming assistants learn about a repository by dumping it, in whole or in part, into the context window. This approach does not scale: cost grows with project size, the model's attention gets diluted and, in long projects, consistency erodes — existing logic gets duplicated, names and typings diverge, folders are re-created for things that already exist. This work presents Quirón, a development environment that does not aim to replace the model's context but to make it precise: a queryable, verifiable index gives the model the exact location of every piece of logic. An immutable, hash-chained event log acts as the single source of truth; two disposable projections are derived from it: a vector index (Qdrant), answering "what is similar to this", and a dependency graph (Neo4j), answering "what does this depend on and what breaks if I change it". A retrieval worker joins both stores and delivers minimal fragments to the model — each carrying path, symbol, range and hash — so that every claim can be verified against the source code. Continuous index maintenance is entrusted to a small, local, purpose-built neural network — the worker network — designed from the published mathematics of recent language models, trained via sequence-level knowledge distillation and deployed quantized on local hardware; the network proposes and never executes. The native Rust editor, the service infrastructure and the immutable log have been built; measurements dimensioning the system are presented, together with the evaluation plan for context-consumption reduction. **[PENDING · Sept: final result figures.]**

**Keywords:** large language models, retrieval-augmented generation, code indexing, dependency graph, knowledge distillation, local inference.

# TABLA RESUMEN

| | DATOS |
| --- | --- |
| Nombre y apellidos | Lorenzo Juan Santacreu Pascual |
| Título del proyecto | Quirón: un editor nativo con índice de código verificable y una red neuronal local para el aporte de contexto a modelos de lenguaje |
| Directores del proyecto | [COMPLETAR] |
| El proyecto se ha realizado en colaboración de una empresa o a petición de una empresa | NO |
| El proyecto ha implementado un producto | SÍ |
| El proyecto ha consistido en el desarrollo de una investigación o innovación | SÍ |
| Objetivo general del proyecto | Construir un editor nativo cuyo contexto para el modelo de lenguaje procede de un índice verificable —vectores y grafo derivados de un registro inmutable—, mantenido por una red neuronal propia, local y pequeña. |

# Capítulo 1. RESUMEN DEL PROYECTO

## 1.1 Contexto y justificación

Trabajar con repositorios grandes exige saber qué hace cada archivo, de qué depende, qué cambió y por qué. Los asistentes actuales obtienen ese conocimiento volcando código en la ventana de contexto, y el volcado tiene dos costes que crecen con el proyecto: el económico —el autor lo mide cada mes como usuario de suscripciones, y en un entorno empresarial con repositorios de gran escala, facturado por token de API, el desglose resultaría insostenible— y el de consistencia: a cada relectura parcial, el asistente renombra lo que ya tenía nombre, duplica lógicas y recrea carpetas, y ese daño se acumula. A la vez, el hardware de sobremesa con memoria unificada y los aceleradores locales hacen viable un reparto nuevo: una red pequeña en casa realiza el trabajo continuo y barato —indexar, clasificar, resumir—, y el modelo grande en línea se reserva para lo que solo él puede hacer. Los modelos locales no sirven hoy para el trabajo difícil de generación, y este proyecto no se lo pide: la pieza local mantiene el índice.

## 1.2 Planteamiento del problema

La pregunta motriz del trabajo es: **¿puede un índice consultable —vectores, grafo y una red local pequeña que lo mantiene— dar al modelo de lenguaje la localización exacta de cada lógica de un repositorio grande, de modo que gane agilidad, ahorre tokens y deje de perder consistencia —lógicas duplicadas, nombres y tipados divergentes, rutas recreadas—, con toda afirmación verificable contra el código de origen?** El índice no sustituye al contexto del modelo: lo hace preciso. El proyecto combina desarrollo de producto (el editor y su infraestructura) con investigación aplicada (el diseño de la red obrera a partir de la literatura reciente).

## 1.3 Objetivos del proyecto

Diseñar y construir un editor nativo cuyo contexto proviene de un índice verificable mantenido por una red neuronal propia. Los objetivos específicos —del arnés de confinamiento al despliegue cuantizado de la red— se detallan en el Capítulo 3.

## 1.4 Resultados obtenidos

A fecha del borrador: infraestructura completa operativa (API, embeddings y reranker locales, gateway de modelos), editor nativo de ~17 700 líneas integrado en el escritorio con confinamiento probado (19 pruebas), registro inmutable verificado (5 028 eventos, cadena válida) y estudio matemático de la red obrera sobre más de 40 fuentes primarias con verificación adversarial. **[PENDIENTE · sept: índice de código, grafo de dependencias, worker de recuperación, red obrera entrenada y mediciones de reducción de contexto.]**

## 1.5 Estructura de la memoria

El Capítulo 2 revisa el estado del arte; el 3 fija los objetivos; el 4 describe la planificación, la solución y los resultados; el 5 discute limitaciones y decisiones; el 6 concluye; el 7 propone líneas futuras; el 8 recoge las referencias y el 9 los anexos con el estudio matemático completo y las evidencias de medición.

# Capítulo 2. ANTECEDENTES / ESTADO DEL ARTE

## 2.1 Estado del arte

El problema general es el del contexto como recurso escaso: un modelo de lenguaje solo razona sobre lo que cabe en su ventana, y llenarla tiene coste económico y cognitivo.

**El contexto en los asistentes de programación.** La práctica dominante es una combinación de volcado (archivos completos o repositorios resumidos) y recuperación aumentada (RAG): fragmentos seleccionados por similitud vectorial se anteponen a la consulta. El RAG clásico sobre código presenta dos carencias conocidas: la similitud responde a «qué se parece a esto», pero no a «de qué depende esto», y los fragmentos recuperados no suelen ser verificables contra su origen, lo que deja pasar afirmaciones sin evidencia.

**Representación vectorial del código.** Los embeddings modernos se entrenan con pérdidas contrastivas tipo InfoNCE [13], que acercan pares positivos y separan negativos. Sobre backbones autoregresivos, el estado del último token sirve como vector de la secuencia (*last-token pooling*) [14][15], lo que permite que un mismo modelo genere texto y produzca embeddings alineados. El aprendizaje de representaciones anidadas (*Matryoshka Representation Learning*) [16] ordena la información por importancia en las primeras dimensiones del vector, de modo que un embedding de 1024 dimensiones puede truncarse a 256 o 64 sin reentrenar y con pérdida acotada. En este trabajo se emplea `bge-m3` (1024 dimensiones) y el reranker `bge-reranker-v2-m3` [19], servidos localmente.

**Almacenes especializados.** Las bases vectoriales (Qdrant [17]) resuelven similitud aproximada a escala con filtros exactos sobre la carga útil; las bases de grafo (Neo4j [18]) resuelven recorridos de dependencia. La literatura y la práctica coinciden en que ninguna de las dos responde por sí sola a las dos preguntas del índice de código; la contribución está en unirlas con identidad estable por unidad.

**La matemática de los modelos recientes, como caja de piezas.** El estudio realizado para este trabajo (Anexo A) extrae los mecanismos matemáticos de los modelos de lenguaje recientes y evalúa su reutilización en una red pequeña, local y exportable:

- *Atención eficiente.* La atención latente multi-cabeza (MLA) comprime el KV-cache a un latente de rango bajo [1][2]; la atención dispersa nativa (NSA) combina compresión, selección top-n y ventana local con un gate diferenciable, entrenable de extremo a extremo [3]; la atención lineal reordena el producto para mantener un estado de tamaño fijo d×d con coste constante por paso [4][5][6], mitigando su pérdida de recuperación asociativa con actualizaciones dispersas de estado (SSE) [7].
- *Mezcla de expertos.* El experto compartido más expertos de grano fino desacopla capacidad de cómputo por token [8], y el balanceo de carga sin pérdida auxiliar corrige el enrutado sin contaminar el objetivo de entrenamiento [9].
- *Destilación.* En la destilación *teacher–student* —el modelo grande enseña, la red pequeña aprende—, la variante clásica con temperatura requiere los logits del modelo *teacher* [10]; la destilación a nivel de secuencia (Sequence-Level KD) solo requiere su texto [11]. Las variantes on-policy (MiniLLM, GKD) exigen de nuevo logits [12a][12b], y la superioridad del KL inverso es cuestión abierta en distribuciones discretas [12c].
- *Cuantización.* AWQ protege por reescalado los canales salientes identificados por la magnitud de la activación, no del peso [20]; GPTQ redistribuye el error de redondeo con información de segundo orden [21]. Ambas son post-entrenamiento.

**Hardware.** La generalización de equipos con memoria unificada de gran capacidad (estaciones de escritorio orientadas a IA y SoC integrados) y de runtimes portables (ONNX Runtime [22]) hace práctico ejecutar de forma continua redes pequeñas en local, sin dependencia de servicios de pago por uso.

## 2.2 Contexto y justificación

La motivación es triple.

**Económica.** El volcado de contexto se paga por token. Con suscripción, el coste lo absorbe el proveedor hasta el límite del plan; por API, el coste es proporcional al tamaño del proyecto y a la frecuencia de consulta. Un índice que entrega solo los fragmentos necesarios ataca la raíz del gasto: el proyecto entero deja de viajar en cada turno. Este trabajo no emplea ninguna clave de API: el acceso a los modelos se realiza por las credenciales de sesión de las suscripciones existentes, y el diseño convierte esa restricción en principio (§4.2.6). La contrapartida —la pérdida de control sobre los parámetros de muestreo del modelo, como la temperatura— se asume explícitamente y se mide en una comparativa acotada del plan de pruebas (§4.6; Capítulo 5).

**De fiabilidad y consistencia.** Un modelo sin mapa del repositorio comete errores por omisión: reimplementa lógica existente, ignora dependencias, afirma sin evidencia. Y en proyectos largos comete además errores de consistencia, los más costosos de deshacer: cada vez que relee una parte del proyecto, bautiza con otro nombre lo que ya tenía uno, cambia una tipografía de identificadores, duplica una lógica o crea una carpeta nueva para algo que ya existía. El resultado es un repositorio con dobles lógicas y vocabularios divergentes donde todo se pierde. Un índice con identidad estable por unidad ataca exactamente eso: las rutas no se duplican, los nombres se conservan y el modelo va directamente a la lógica localizada en lugar de redescubrirla. Cada fragmento que Quirón entrega lleva ruta, símbolo, rango y hash; toda afirmación es contrastable con el código de origen. La verificabilidad no es una capa estética: es la diferencia entre una respuesta y una conjetura.

**De soberanía.** El código no abandona la máquina salvo en los fragmentos mínimos que el usuario decide enviar al modelo. Los secretos (`.env`, claves, material criptográfico) se niegan siempre, incluso dentro del proyecto. La API del sistema adopta el formato de mensajes de Anthropic mientras el modelo servido es de OpenAI: el índice no depende del proveedor, por diseño.

## 2.3 Planteamiento del problema

Del análisis anterior se desprende la carencia: no existe una solución que una (a) recuperación híbrida vectorial y de grafo con identidad estable y verificable por unidad de código, (b) mantenimiento continuo e incremental del índice por una red neuronal local pequeña que **propone y no ejecuta**, (c) independencia total de claves de API y de un proveedor concreto, y (d) la conservación de la consistencia del repositorio —nombres, tipados, rutas sin duplicados— como responsabilidad activa del sistema, no como efecto que se espera del modelo. Los asistentes comerciales vuelcan contexto y no exponen su índice; las soluciones RAG de código no integran el grafo de dependencias ni anclan sus fragmentos a hashes; las redes pequeñas locales no se han aplicado como *mantenedoras* de un índice bajo validación determinista. Ese hueco define los objetivos del capítulo siguiente.

# Capítulo 3. OBJETIVOS

## 3.1 Objetivos generales

El objetivo general del presente trabajo consiste en **diseñar y construir un entorno de desarrollo nativo cuyo contexto para el modelo de lenguaje procede de un índice verificable —un índice vectorial y un grafo de dependencias derivados de un registro inmutable—, mantenido de forma continua por una red neuronal propia, local y pequeña**.

## 3.2 Objetivos específicos

- **OE1.** Diseñar la arquitectura de memoria del sistema: registro inmutable encadenado por hash como única fuente de verdad, y proyecciones desechables (índice vectorial y grafo) reconstruibles desde él.
- **OE2.** Implementar el editor nativo con confinamiento al proyecto: guardia de rutas con veredictos diferenciados (secreto, exterior, ruido), fallo cerrado sin proyecto abierto.
- **OE3.** Construir el indexador determinista e incremental sobre tres tipos de unidad (Archivo, Lógica, Cambio), con identificador estable, hash y proyecto en cada unidad.
- **OE4.** Construir el grafo de dependencias particionado por proyecto, con relaciones extraídas por análisis estático y nunca por inferencia del modelo.
- **OE5.** Implementar el worker de recuperación: filtro exacto por proyecto dentro del almacén, búsqueda vectorial, expansión por el grafo, reordenación con reranker y recorte al presupuesto de contexto.
- **OE6.** Estudiar la matemática de los modelos de lenguaje recientes y seleccionar, con criterio de evidencia, los mecanismos reutilizables en una red pequeña exportable a ONNX/Rust.
- **OE7.** Entrenar la red obrera por destilación a nivel de secuencia desde un modelo *teacher* accesible solo por texto, y desplegarla cuantizada (INT4) en el runtime local.
- **OE8.** Medir el sistema: reducción del consumo de contexto frente al volcado, calidad de recuperación, aislamiento entre proyectos y fidelidad de las fichas generadas.

## 3.3 Beneficios del proyecto

El beneficio directo es económico y de agilidad: con las lógicas localizadas, el modelo va directo a donde debe ir, y el ahorro de tokens crece precisamente donde más duele, en los repositorios grandes y los proyectos largos. El beneficio de calidad es la conservación de la consistencia: nombres, tipados y rutas se mantienen sin duplicados, y el modelo no repite una lógica que ya existe porque el índice se la pone delante, con evidencia — se acaban las dobles lógicas y los errores de los que nadie se da cuenta hasta que ya son mil. El beneficio de soberanía es que el código permanece en la máquina y el sistema no depende de claves de API ni de un proveedor único. Por último, la reconstruibilidad de las proyecciones convierte el peor error posible de la red obrera en un incidente reversible: un grafo que hay que reproyectar, nunca una memoria falsificada.

# Capítulo 4. DESARROLLO DEL PROYECTO

## 4.1 Planificación del proyecto

El sistema se levanta en fases, y cada una exige que la anterior funcione. La decisión rectora es que **la red neuronal es la última pieza, no la primera**: las cinco primeras fases producen un sistema útil sin una sola red entrenada por el autor; la sexta lo mejora, no lo sostiene. Construir la red antes que el indexador significaría entrenarla sobre un corpus que no existe.

| Fase | Periodo (2026) | Contenido | Naturaleza |
| --- | --- | --- | --- |
| F0 | febrero | Anteproyecto y definición del alcance | — |
| F1 | febrero–abril | Infraestructura de servicios: `quiron-brain` (API), `semantic-ia-local` (embeddings y reranker), `vertex-gateway` (salida a modelos), Qdrant y Neo4j en contenedores | determinista |
| F2 | abril–junio | Editor nativo de Quirón (~17 700 líneas), integración de escritorio, confinamiento con 19 pruebas | determinista |
| F3 | junio–julio | Arquitectura de memoria: registro inmutable (Blake3), estados de promoción, conversación efímera; diagnóstico medido de la recuperación | determinista |
| F4 | julio | Estudio matemático de la red obrera: >40 fuentes primarias, verificación adversarial en tres pasadas | investigación |
| F5 | julio–agosto | Correcciones deterministas del diagnóstico; índice de código (Archivo/Lógica/Cambio); grafo de dependencias; worker de recuperación | determinista |
| F6 | agosto–10 septiembre | Red obrera: corpus, destilación, cuantización y despliegue; medición final; cierre de la memoria | aprendida |

**[COMPLETAR: ajustar fechas exactas de F0–F2 y estimar el esfuerzo en horas por fase para el presupuesto.]**

La entrega del programa, acordada con el tutor en videoconferencia, se realiza mediante **repositorio público en GitHub** **[COMPLETAR: URL]**, lo que garantiza la trazabilidad del desarrollo y preserva la autoría del alumno sobre el código.

## 4.2 Descripción de la solución, metodologías y herramientas empleadas

### 4.2.1 Arquitectura de memoria: un registro, dos proyecciones

La decisión central del diseño es una asimetría. El **registro de eventos** es inmutable: cada evento se encadena con el hash del anterior (Blake3), de modo que alterar uno invalida todos los posteriores; una afirmación equivocada no se borra, se corrige emitiendo otra que la reemplaza (`supersedes`) o la retracta (`retracted_by`). Las **proyecciones** —índice vectorial en Qdrant, grafo en Neo4j— son desechables por definición: si se corrompen, se destruyen y se reconstruyen desde el registro. Esa propiedad es la que permite que una red neuronal escriba en ellas sin poner en riesgo nada irreversible.

Cada proyecto recibe un identificador propio (`.llore/project.id`), estable frente a mover, renombrar o duplicar la carpeta. El identificador —no la ruta ni el nombre— aparece en el registro, en la carga útil de cada punto vectorial y en el nodo raíz del grafo. La identidad heredada de eventos antiguos se resuelve anotando (`ProjectAliased`), nunca reescribiendo: reescribir el pasado rompería la cadena y convertiría el registro en algo editable, es decir, en algo que ya no es un registro.

Las conversaciones son efímeras: cada chat empieza en blanco y, al cerrarse, se vuelca íntegro al registro como evidencia. Un turno de chat no es memoria; es materia prima de la que se destila memoria.

### 4.2.2 El índice de código

Tres tipos de unidad: **Archivo** (ruta, lenguaje, hash, responsabilidad en ≤120 palabras, símbolos, dependencias), **Lógica** (una por función, método o clase: firma, rango, propósito, entradas, salidas, efectos, hash normalizado, texto semántico ≤180 palabras) y **Cambio** (hash anterior y posterior, motivo con fuente explícita o `unknown`). El código completo no se copia al texto vectorizado: el punto conserva ruta, rango y hash para recuperar el original cuando haga falta. El motivo de un cambio nunca se deduce sin evidencia.

El aislamiento entre proyectos se apoya en tres puntos obligatorios: toda unidad lleva `project` en su carga útil; Qdrant aplica el filtro exacto **antes** de calcular similitud; y en Neo4j todo nodo cuelga de un nodo raíz `Project` que ninguna consulta atraviesa. Un fragmento de un proyecto no puede aparecer en las respuestas de otro.

### 4.2.3 Confinamiento del editor

El editor arranca sin proyecto y falla cerrado: sin raíz no se lee nada y el chat queda deshabilitado. Toda ruta se clasifica antes de tocar el disco comparando formas canónicas —ni `..` ni un enlace simbólico permiten salir— con tres veredictos: `Secret` (negación absoluta, incluso dentro del proyecto), `Outside` (requiere autorización) y `Noise` (oculto, legible bajo petición). Un `.env` es secreto; un `.env.example` es una plantilla y se lee. Lo que la guardia niega al editor se lo niega también al worker: si un archivo no puede abrirse en una pestaña, tampoco puede acabar convertido en un vector. El confinamiento no es una capa de la interfaz; es la frontera del sistema. La implementación está cubierta por 19 pruebas.

### 4.2.4 Grafo de dependencias y worker de recuperación

La similitud vectorial no puede responder «de qué depende esto» ni «qué se rompe si lo cambio»; esas preguntas exigen un grafo. Neo4j almacena nodos `Project`, `File`, `Module`, `Symbol`, `Change` y relaciones `IMPORTS`, `CALLS`, `DEPENDS_ON`, `DEFINED_IN`, `CHANGED_IN`, extraídas del árbol sintáctico; si el análisis estático no resuelve una llamada, la relación no se crea. Un nodo del grafo y un punto vectorial se corresponden por el identificador estable de la unidad, de modo que un resultado de similitud puede expandirse por el grafo y viceversa.

El worker de recuperación une los dos almacenes en cinco pasos: filtro exacto por proyecto, búsqueda vectorial (candidatos, no respuestas), expansión acotada por el grafo, reordenación con el reranker y recorte al presupuesto de contexto del consumidor. El checkpoint del indexador solo avanza tras confirmar la escritura en ambos almacenes: una unidad presente en Qdrant y ausente del grafo es un fallo visible y reintentable, no un estado intermedio aceptable.

### 4.2.5 El editor nativo

El editor de Quirón es una aplicación nativa en Rust: `winit` para la ventana, `tiny-skia` y `softbuffer` para el rasterizado, `taffy` para el layout y `cosmic-text` para el texto. (En el repositorio, el crate conserva el nombre `llore_editor`, heredado de un prototipo anterior; el producto es Quirón.) No hay navegador ni webview; las tipografías viajan dentro del binario. Un fotograma de interfaz cuesta 2,4 ms medidos (1,0 ms de modelado de texto, 1,4 ms de composición de glifos); con repintado por eventos no hay motivo para mover el dibujado a la GPU. La aplicación está integrada en el escritorio (entrada de menú, icono, `app_id` coherente en Wayland y X11) y el workbench ofrece cinco vistas: Explorer, Search, Source Control, Graph e History, con el chat como panel opcional.

### 4.2.6 Conexión con los modelos de lenguaje

`vertex-gateway` concentra la salida hacia proveedores en un único punto de egreso con backends intercambiables (`openai_compatible`, `ollama_native`, `openclaw`, `codex_direct`). La configuración vigente lee las credenciales de sesión de la suscripción de Codex y envía las peticiones sin ninguna clave de API. La API local (`quiron-brain`, `127.0.0.1:8766`, autenticación por bearer token) es la única puerta al índice: ningún cliente habla directamente con Qdrant ni Neo4j. `/v1/messages` adopta el formato de mensajes de Anthropic sobre un modelo de OpenAI: la independencia de proveedor es una propiedad del diseño, no un accidente.

### 4.2.7 La red obrera

Es una red neuronal pequeña y propia, entrenada sobre las unidades del índice, que trabaja de forma continua. Sus tareas, por orden: mantener el `project` de cada unidad, clasificar unidades por tipo y responsabilidad, redactar la ficha de cada archivo y cada lógica dentro de los límites de palabras, y priorizar candidatos a lógica duplicada para revisión humana. Sus límites no se negocian: toda salida cumple un esquema, se refiere al hash vigente de la unidad y supera validaciones deterministas antes de escribirse; una salida que no valida se descarta y queda registrada. La red no escribe en el registro, no controla el sistema de archivos ni los almacenes: **propone; no ejecuta**.

Del estudio del estado del arte (Anexo A) se deriva su diseño: (1) backbone híbrido mayormente lineal —estado recurrente d×d, coste constante por token, exportable a ONNX— con capas esporádicas de atención completa (~7:1) [4][7]; (2) una cabeza compartida más cabezas por tipo de lógica, inspiración del experto compartido de MoE sin enrutado disperso [8]; (3) embeddings de código de 1024 dimensiones entrenados con InfoNCE, obtenidos por last-token pooling y truncables por Matryoshka [13][14][16]; (4) entrenamiento por Sequence-Level KD [11], la única destilación viable cuando el modelo *teacher* solo expone texto; (5) despliegue cuantizado INT4 con AWQ [20] sobre el mismo runtime ONNX de Rust (`ort`) que ya sirve los embeddings. El tamaño y la arquitectura definitivos se deciden midiendo sobre un corpus de proyectos reales, no por estimación previa. **[PENDIENTE · sept: corpus, entrenamiento y medición.]**

### 4.2.8 Metodología

Tres reglas ordenaron el trabajo. **Medir antes de decidir:** el tamaño de la red se decide sobre corpus real; los diagnósticos se confirman con datos (§4.6). **Fases deterministas antes que aprendidas:** el indexador debe ser determinista precisamente para que la red pueda ser la última pieza. **Honestidad de evidencia:** el estudio del estado del arte etiqueta cada mecanismo (verificado adversarialmente, canónico no re-verificado, principio de diseño, cuestión abierta) y distingue las cifras autoreportadas por los autores de las propiedades deterministas de cada arquitectura.

## 4.3 Recursos requeridos

- Estación de trabajo con dos aceleradores gráficos (48 GB de VRAM en total) y Linux (Arch), con servicios de usuario systemd.
- Software libre: Rust y su ecosistema (`winit`, `tiny-skia`, `softbuffer`, `taffy`, `cosmic-text`), Qdrant, Neo4j, ONNX Runtime (`ort`), contenedores Podman/Docker.
- Modelos preentrenados abiertos: `bge-m3` (embeddings, 1024 dim) y `bge-reranker-v2-m3` (BAAI).
- Acceso a modelos de lenguaje por suscripción (sesión de Codex; sin claves de API).
- Fuentes primarias de investigación en abierto (arXiv).

## 4.4 Presupuesto

| Tipo de coste | Valor | Comentarios |
| --- | --- | --- |
| Horas de trabajo en el proyecto | [COMPLETAR: ~___ h × ___ €/h = ___ €] | Estimación de dedicación del autor de febrero a septiembre de 2026 |
| Equipo técnico utilizado | [COMPLETAR: ~___ €] | Estación de trabajo con dos aceleradores (48 GB VRAM); valor aproximado de mercado si se adquiriese nueva |
| Software utilizado | 0 € | Todo el software empleado es libre (Rust, Qdrant, Neo4j Community, ONNX Runtime, modelos BAAI) |
| Suscripciones de asistentes | [COMPLETAR: ___ €/mes × ___ meses = ___ €] | Codex y Claude, usadas como modelo *teacher* de la destilación y asistencia al desarrollo |
| Estudios e informes | 0 € | Fuentes primarias en abierto (arXiv) |
| Materiales empleados | 0 € | — |

## 4.5 Viabilidad

La relación coste/beneficio se apoya en un desplazamiento: el gasto recurrente por token (proporcional al tamaño del repositorio y a la frecuencia de uso) se sustituye por un coste fijo de hardware local ya amortizado y un consumo mínimo de modelo grande. La sostenibilidad futura se apoya en tres propiedades del diseño: los backends de modelo son intercambiables (añadir un proveedor no toca el índice), las proyecciones son reconstruibles (el sistema sobrevive a sus propios errores) y todo el stack es software libre ejecutable en una sola máquina. **[PENDIENTE · sept: cuantificar la reducción de tokens con las mediciones del plan de pruebas.]**

## 4.6 Resultados del proyecto

**Resultados a fecha del borrador (mediciones del 10 de julio de 2026):**

*Construido y verificado:*

- Infraestructura operativa completa: API `quiron-brain` en `:8766` (`/health` → 200, servicio systemd), `semantic-ia-local` en `:8091` con `bge-m3` y reranker, `vertex-gateway` verificado extremo a extremo contra el modelo de suscripción, Qdrant y Neo4j en contenedores.
- Editor nativo compilando e integrado en el escritorio; coste de fotograma medido: 2,4 ms.
- Confinamiento al proyecto con 19 pruebas: guardia de rutas con formas canónicas, negación de secretos, fallo cerrado.
- Registro inmutable: 5 028 eventos encadenados por Blake3, `GET /chain/verify` → `valid: true`; corrección sin borrado (`supersedes`, `retracted_by`) y estados de promoción implementados.
- Bucle de herramientas del asistente verificado de extremo a extremo a través del gateway.
- Estudio matemático de la red obrera: ~75 afirmaciones verificadas adversarialmente en tres pasadas sobre más de 40 fuentes primarias (Anexo A).

*Diagnóstico medido de la recuperación (resultado metodológico):* la investigación de por qué la recuperación no consultaba el índice identificó cuatro causas, todas medidas: el binario se compilaba sin sus funcionalidades opcionales (`default = []`), el identificador de proyecto difería entre el editor (ruta canónica) y los eventos (nombre), el filtro de proyecto se aplicaba después de la búsqueda en lugar de dentro de Qdrant, y las conversaciones anteriores reaparecían por una búsqueda de palabras clave sobre el registro. Se refutó además, midiendo, una hipótesis plausible: los 1 652 puntos de memoria personal de ámbito global **no** desplazan a los del proyecto en consultas reales (los veinticinco vecinos más próximos pertenecían todos al proyecto); la hipótesis del sumidero vectorial procedía de sondear el índice con vectores aleatorios, que gravitan hacia el grupo más denso.

**Plan de pruebas para el cierre [PENDIENTE · sept]:**

1. Reducción de consumo: tokens por tarea con índice frente a volcado, sobre un conjunto de tareas reales en este mismo repositorio.
2. Calidad de recuperación: precisión de los fragmentos recuperados contra un conjunto etiquetado.
3. Aislamiento: ninguna consulta filtrada por proyecto devuelve unidades de otro (criterio de aceptación binario).
4. Reconstruibilidad: borrar índice y grafo y reproyectar desde el registro produce el mismo contenido lógico.
5. Fidelidad de fichas: tasa de alucinación de la red obrera sobre un conjunto etiquetado propio, con descarte por validación como métrica secundaria.
6. Comparativa acotada de vías de acceso al modelo *teacher*: suscripción (sin control de parámetros) frente a API con temperatura controlada, sobre un conjunto reducido de peticiones idénticas, comparando consistencia de las salidas y coste.
7. Curva de escalado del consumo: tokens por tarea bajo volcado —computados offline con un tokenizador, sin coste de API— frente a la vía del índice, sobre repositorios de código abierto de tamaño creciente (del orden de 10, 100 y 1 000 archivos). Predicción a contrastar: el consumo del volcado crece con el tamaño del repositorio; el del índice permanece aproximadamente constante.

# Capítulo 5. DISCUSIÓN

**La restricción del modelo *teacher* sin logits.** El acceso a los modelos grandes es por suscripción: el *teacher* devuelve texto, no distribuciones. Eso descarta la destilación clásica con temperatura y las variantes on-policy (MiniLLM, GKD), y deja la destilación a nivel de secuencia como única vía matemáticamente honesta [11]. Se asume el matiz: la justificación de SeqKD es la tratabilidad, no una garantía de que la moda del *teacher* concentre la probabilidad. La vía por suscripción tiene además una segunda contrapartida: no permite controlar los parámetros de muestreo del *teacher* —en particular la temperatura—, que sí controla el acceso por API. Para la generación de un corpus de destilación esa diferencia importa: un *teacher* a temperatura baja produce salidas más consistentes y reproducibles. Por ello el plan de pruebas incluye una comparativa acotada entre ambas vías (§4.6), con un conjunto reducido de peticiones para mantener el coste marginal. Las alternativas —acceso por API con control de parámetros, o un modelo *teacher* local que exponga logits— quedan como líneas futuras tras esa medición.

**Fricciones no medidas.** La exportación a ONNX de pesos cuantizados por canal y de mecanismos como MLA no está caracterizada en fuentes primarias para el stack Rust del proyecto; se medirá en la fase F6. La fidelidad anti-alucinación de resúmenes code-to-text con redes pequeñas tampoco tiene evidencia primaria: por eso el plan de pruebas incluye un conjunto etiquetado propio y por eso la red opera bajo validación determinista con descarte registrado.

**El alcance económico de la validación.** La hipótesis del trabajo se manifestaría con mayor claridad justo donde no puede pagarse la prueba: un proyecto industrial con miles de archivos y horas de trabajo acumuladas, donde una sola tarea resuelta por volcado de contexto costaría del orden de miles de euros en tokens. Esa barrera no es solo una limitación del estudio: es la evidencia del problema que este trabajo ataca — el coste que impide el experimento a escala completa es el mismo coste que Quirón pretende eliminar. En su lugar se diseña una evaluación proporcional con dos propiedades. Primera: el consumo del volcado no se compra, se computa — el número de tokens que costaría una tarea por volcado se calcula offline con un tokenizador, sin realizar la petición. Segunda: la medición se repite sobre repositorios de código abierto de tamaño creciente, lo que produce una **curva de escalado** (consumo frente a tamaño del repositorio) en lugar de un punto aislado; la predicción a contrastar es que el volcado crece con el tamaño del proyecto y la vía del índice se mantiene aproximadamente plana. La comparación de calidad de las respuestas, que sí exige llamadas reales, se realiza a escala modesta mediante la suscripción disponible y proveedores de bajo coste por token —entre ellos los del propio estado del arte citado [1][2]—, dentro de la comparativa acotada del plan de pruebas. La extrapolación de la curva sugiere que el beneficio crece con la escala; su confirmación empírica en un entorno industrial queda como línea futura.

**Lo que enseñó el diagnóstico.** Tres hipótesis plausibles sobre el fallo de recuperación resultaron falsas al medirse, y la causa raíz (compilación sin funcionalidades) era invisible desde la conducta del sistema. La lección metodológica quedó incorporada al proyecto: ningún diagnóstico se acepta sin medición, y las mediciones con sondas sintéticas (vectores aleatorios) pueden fabricar patologías que no existen ante consultas reales.

**Cambios respecto al planteamiento inicial.** La memoria de eventos y el índice de código compartían colección y ciclo de vida; el trabajo los separó conceptual y físicamente: la promoción gobierna la memoria del agente (afirmaciones con incertidumbre), mientras que una unidad de código no tiene incertidumbre —existe o no existe, su hash coincide o no— y se escribe y retira según el hash. Este deslinde simplificó el diseño del indexador.

**Limitaciones de despliegue.** La instalación actual sirve a un solo usuario (falta empaquetado distribuible); el gateway no renueva aún el token de sesión expirado; y los contenedores de los almacenes escuchan en todas las interfaces sin autenticación en Qdrant, aceptable en una máquina personal y no en una red no confiable.

# Capítulo 6. CONCLUSIONES

## 6.1 Conclusiones del trabajo

Respecto al objetivo general, el trabajo ha construido y verificado la infraestructura completa del sistema —editor nativo confinado, registro inmutable, servicios semánticos locales y salida a modelos sin claves de API— y ha producido, con metodología de evidencia explícita, el diseño fundamentado de la red obrera a partir de la matemática publicada de los modelos recientes. La arquitectura registro-más-proyecciones demostró su valor durante el propio desarrollo: permitió diagnosticar con mediciones, corregir sin reescribir el pasado y acotar el riesgo de la pieza aprendida a errores reversibles. **[PENDIENTE · sept: conclusión sobre la hipótesis central con las mediciones de reducción de contexto y calidad de recuperación.]**

## 6.2 Conclusiones personales

**[BORRADOR — personalizar por el autor]** Este proyecto ha sido un año de aprendizaje acelerado: de usuario de asistentes de programación a diseñador de la infraestructura que los alimenta. Trabajar a diario con modelos de lenguaje —siendo a la vez su primer usuario, su crítico y su arquitecto— ha enseñado al autor más sobre atención, memoria y evidencia que cualquier asignatura aislada: la disciplina de medir antes de afirmar, de distinguir lo verificado de lo plausible y de diseñar sistemas cuyo peor error sea reversible.

# Capítulo 7. FUTURAS LÍNEAS DE TRABAJO

- **Empaquetado y distribución:** AppImage/Flatpak para Linux e instalable para Windows (cross-compilación), como continuación natural de la entrega por repositorio.
- **Renovación de credenciales:** usar el token de refresco de la sesión del proveedor para eliminar la caducidad manual.
- **Validación a escala industrial:** repetir la evaluación sobre un proyecto real de gran tamaño con presupuesto de API completo — el escenario donde la hipótesis del trabajo predice el mayor beneficio y del que la curva de escalado medida aquí es la extrapolación.
- **Vía API con control de parámetros:** si el proyecto se consolida, incorporar acceso por API para fijar temperatura y muestreo del modelo *teacher*, decidido con los datos de la comparativa acotada del plan de pruebas.
- **Modelo *teacher* local con logits:** habilitar la destilación clásica con temperatura y comparar contra SeqKD.
- **Red obrera ampliada:** inferencia de motivos de cambio con evidencia, detección de lógica duplicada con umbral medido sobre casos etiquetados, y priorización activa de qué reindexar.
- **Importador de sesiones:** destilar decisiones, intentos y errores de las conversaciones archivadas (Codex/VS Code) como evidencia estructurada del registro.
- **Multi-proyecto explícito:** filtros con lista de identificadores para trabajos que abarquen varios mundos, nunca por ausencia de filtro.
- **Exposición como servidor MCP:** ofrecer el índice como herramientas estándar a otros clientes, como complemento de la vía directa del editor.
- **Cliente móvil** para consulta del índice y del grafo.

# Capítulo 8. REFERENCIAS

*(Estilo IEEE. [COMPLETAR: revisar títulos exactos y autores completos antes de la entrega final.])*

[1] DeepSeek-AI, "DeepSeek-V2: A Strong, Economical, and Efficient Mixture-of-Experts Language Model", arXiv:2405.04434, 2024.
[2] DeepSeek-AI, "DeepSeek-V3 Technical Report", arXiv:2412.19437, 2024.
[3] DeepSeek-AI, "Native Sparse Attention: Hardware-Aligned and Natively Trainable Sparse Attention", arXiv:2502.11089, 2025.
[4] MiniMax, "MiniMax-01: Scaling Foundation Models with Lightning Attention", arXiv:2501.08313, 2025.
[5] MiniMax, "MiniMax-M1", arXiv:2506.13585, 2025.
[6] "Ring-linear", arXiv:2510.19338, 2025. [COMPLETAR título exacto]
[7] "SSE: Sparse State Expansion", arXiv:2507.16577, 2025. [COMPLETAR título exacto]
[8] DeepSeek-AI, "DeepSeekMoE: Towards Ultimate Expert Specialization in Mixture-of-Experts Language Models", arXiv:2401.06066, 2024.
[9] "Auxiliary-Loss-Free Load Balancing Strategy for Mixture-of-Experts", arXiv:2408.15664, 2024.
[10] G. Hinton, O. Vinyals, J. Dean, "Distilling the Knowledge in a Neural Network", arXiv:1503.02531, 2015.
[11] Y. Kim, A. M. Rush, "Sequence-Level Knowledge Distillation", arXiv:1606.07947, 2016.
[12a] Y. Gu et al., "MiniLLM: Knowledge Distillation of Large Language Models", arXiv:2306.08543, 2023.
[12b] R. Agarwal et al., "On-Policy Distillation of Language Models (GKD)", arXiv:2306.13649, 2023.
[12c] "Forward vs. Reverse KL for LLM distillation", arXiv:2404.02657, COLING 2025. [COMPLETAR título exacto]
[13] A. van den Oord, Y. Li, O. Vinyals, "Representation Learning with Contrastive Predictive Coding", arXiv:1807.03748, 2018.
[14] L. Wang et al., "Improving Text Embeddings with Large Language Models (e5-mistral)", arXiv:2401.00368, 2024.
[15] "jina-code-embeddings", arXiv:2508.21290, 2025. [COMPLETAR título exacto]
[16] A. Kusupati et al., "Matryoshka Representation Learning", arXiv:2205.13147, 2022.
[17] Qdrant — Vector Database. https://qdrant.tech
[18] Neo4j Graph Database. https://neo4j.com
[19] BAAI, modelos `bge-m3` y `bge-reranker-v2-m3`. https://huggingface.co/BAAI
[20] J. Lin et al., "AWQ: Activation-aware Weight Quantization for LLM Compression and Acceleration", arXiv:2306.00978, 2023.
[21] E. Frantar et al., "GPTQ: Accurate Post-Training Quantization for Generative Pre-trained Transformers", arXiv:2210.17323, 2022.
[22] ONNX Runtime. https://onnxruntime.ai

# Capítulo 9. ANEXOS

- **Anexo A.** Estudio: la matemática de la red obrera de Quirón — algoritmos y ecuaciones de los LLM recientes y su reutilización, con etiquetas de evidencia y verificación adversarial (documento íntegro: `docs/ESTUDIO_RED_OBRERA.md`).
- **Anexo B.** Arquitectura de la memoria: desarrollada en §4.2.1. **[PENDIENTE: extraer el anexo; el archivo `docs/ARQUITECTURA_MEMORIA.md` citado en julio no existe.]**
- **Anexo C.** Arquitectura del índice y API: descritas en §4.2.2–4.2.6 y README. **[PENDIENTE: consolidar el contrato implementado; el archivo `docs/ARQUITECTURA_INDICE.md` citado en julio no existe.]**
- **Anexo D.** Evidencias de medición: verificación de la cadena de eventos, conteos de los almacenes, pruebas de confinamiento y coste de fotograma del editor. **[PENDIENTE · sept: añadir las mediciones finales del plan de pruebas.]**
- **Anexo E.** Repositorio del código en GitHub **[COMPLETAR: URL]** — vía de entrega del programa acordada con el tutor.
