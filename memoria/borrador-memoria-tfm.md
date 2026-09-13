---
titulo: Quirón: un editor nativo con índice de código verificable y una red neuronal local para el aporte de contexto a modelos de lenguaje
titulo_corto: Quirón
autor: Lorenzo Juan Santacreu Pascual
director: [COMPLETAR — Nombre del director/a]
titulacion: Máster en Formación Permanente en Inteligencia Artificial Aplicada
curso: 2025–2026
fecha: Septiembre de 2026
---

# RESUMEN

Los asistentes de programación necesitan localizar información relevante dentro de un repositorio para responder y proponer cambios. Este trabajo presenta Quirón, un editor nativo escrito en Rust que integra un índice de código mantenido por un worker local. Al abrir un proyecto, el sistema detecta sus archivos, extrae unidades sintácticas de Rust, genera fichas descriptivas y produce representaciones vectoriales para su consulta. La configuración inicial utiliza Qwen2.5-Coder-1.5B cuantizado para redactar las fichas y BGE-M3 para vectorizarlas; ambos modelos se ejecutan en el equipo del usuario. Qdrant almacena los vectores y Neo4j representa la pertenencia de las unidades y relaciones aproximadas de llamadas. Cada ficha conserva proyecto, ruta, símbolo, líneas y hash del archivo, y la recuperación comprueba su vigencia frente al código actual antes de entregarla al asistente.

La aportación consiste en integrar ese recorrido con el editor, con la descarga y selección de modelos compatibles y con distintas conexiones a asistentes (Claude, Codex, servidores compatibles con OpenAI y Ollama). El entrenamiento de una red propia, previsto en el planteamiento inicial, quedó fuera del alcance final por razones de recursos, plazo y prioridad de validación, y se documenta como estudio de alternativas. Las evidencias registradas incluyen indexación incremental, aislamiento entre proyectos, cambios, borrados y uso de herramientas mediante proveedores reales y simulados, junto con una evaluación exploratoria de localización. También muestran limitaciones de fidelidad en los resúmenes generados. La evaluación disponible acredita un prototipo funcional; no permite afirmar todavía ahorro de tokens en tareas completas, prevención de duplicados ni análisis exhaustivo de dependencias.

**Palabras clave:** modelos de lenguaje, recuperación aumentada, índice de código, inferencia local, editor nativo, trazabilidad.

# ABSTRACT

Programming assistants need to locate relevant information within a repository to answer questions and propose changes. This work presents Quirón, a native Rust editor that integrates a code index maintained by a local worker. When a project is opened, the system discovers its files, extracts Rust syntax units, generates descriptive records and produces vector representations for retrieval. The initial configuration uses quantized Qwen2.5-Coder-1.5B for descriptions and BGE-M3 for embeddings; both run on the user's machine. Qdrant stores the vectors, while Neo4j represents unit membership and approximate call relationships. Each record keeps the project, path, symbol, line range and file hash, and retrieval checks its freshness against the current source before handing it to the assistant.

The contribution is the integration of this workflow with the editor, with downloadable and replaceable compatible models, and with several assistant connections (Claude, Codex, OpenAI-compatible servers and Ollama). Training a proprietary worker model, part of the initial plan, was excluded from the final scope because of resource constraints, the available schedule and validation priorities, and is documented as a study of alternatives. Recorded evidence covers incremental indexing, isolation between projects, updates, deletions and tool interactions through real and simulated providers, together with an exploratory retrieval evaluation. It also exposes limitations in the fidelity of generated descriptions. The available evaluation supports a functional prototype; it does not yet establish token savings for complete tasks, duplicate prevention or exhaustive dependency analysis.

**Keywords:** language models, retrieval augmentation, code indexing, local inference, native editor, traceability.

# TABLA RESUMEN

<!-- tabla: Datos generales del trabajo -->
| Campo | Datos |
| --- | --- |
| Nombre y apellidos | Lorenzo Juan Santacreu Pascual |
| Título | Quirón: un editor nativo con índice de código verificable y una red neuronal local para el aporte de contexto a modelos de lenguaje |
| Director/a | [COMPLETAR] |
| Colaboración con empresa | No |
| Producto implementado | Sí: prototipo de editor nativo con índice de código local |
| Investigación o innovación | Ingeniería aplicada y estudio de alternativas de modelos |
| Objetivo general | Integrar un índice consultable con fuentes comprobables y un worker local configurable dentro de un editor nativo |

# Capítulo 1. RESUMEN DEL PROYECTO

## 1.1 Contexto y justificación

Trabajar sobre proyectos grandes exige conocer qué contiene cada archivo y dónde se implementa cada responsabilidad. La experiencia de trabajo con asistentes de programación durante el máster puso de manifiesto tres problemas recurrentes: el asistente reescribe lógica que ya existe porque no la encuentra, interpreta mal el código por falta de contexto, y consume una parte considerable de su ventana de contexto en leer archivos que no necesita. De ahí surge la idea de un mapa persistente del proyecto que facilite localizar código existente y seleccionar contexto. Reducir esos problemas exige una evaluación específica y no se deduce de disponer de un índice, así que la memoria distingue en todo momento lo que se ha implementado y probado de lo que sigue siendo hipótesis.

Quirón separa el trabajo continuo de descripción y búsqueda del razonamiento que se pide al asistente. El procesamiento del índice se realiza en local, con modelos preentrenados que se ejecutan en el equipo del usuario. El asistente puede ser un proveedor remoto o un servidor compatible elegido por el usuario. La aplicación mantiene una relación explícita entre la ficha recuperada y el archivo que debe leerse para comprobarla.

## 1.2 Planteamiento del problema

La pregunta de implementación es la siguiente: **¿puede un editor mantener un mapa consultable de los archivos y las lógicas de un proyecto mediante un worker local, y entregar al asistente referencias cuya vigencia se compruebe contra el código?** La pregunta experimental adicional es si ese mapa mejora la localización y reduce el contexto necesario manteniendo la calidad de las respuestas. Ambas preguntas requieren evidencias distintas: integración funcional para la primera y comparación entre métodos para la segunda. Este trabajo responde a la primera y aporta una evaluación exploratoria de la segunda.

## 1.3 Objetivos del proyecto

El alcance final comprende el editor, la guardia de rutas, la indexación incremental, las fichas generadas mediante un modelo preentrenado, los embeddings, las proyecciones en Qdrant y Neo4j y la recuperación conectada al chat. La descarga y selección de modelos compatibles permite adaptar el worker al dispositivo. La evolución de los objetivos iniciales se documenta en el Capítulo 3 y en el apartado 4.2.8.

## 1.4 Resultados obtenidos

Se ha construido un prototipo funcional en el que abrir una carpeta produce, en segundo plano, un índice de fichas vectorizadas por proyecto que el chat consulta y devuelve como fuentes clicables. Las pruebas registradas muestran el funcionamiento del worker Qwen/BGE-M3 con Qdrant y Neo4j sobre proyectos sintéticos y sobre el propio repositorio de Quirón: apertura, consultas, cambios, retirada de unidades, exclusión de archivos sensibles y enlaces, y aislamiento entre proyectos. Hay evidencias de rondas completas desde la interfaz con Claude y con Codex, y pruebas de transporte con un servidor compatible simulado. Una evaluación exploratoria de localización sobre diez preguntas obtuvo el objetivo entre los cinco primeros resultados en todos los casos. Los resultados negativos del generador se conservaron para estudiar errores de contenido y truncamiento, y motivaron una versión de fichas con citas verificadas por el programa.

La revisión final del código encontró límites de integridad en el registro de eventos, en la resolución de llamadas y en la reconstrucción del índice de código, y llevó a corregir los dos primeros. El apartado 4.6 separa las evidencias de cada tipo.

## 1.5 Estructura de la memoria

El Capítulo 2 presenta los antecedentes; el 3 fija los objetivos y los cambios de alcance; el 4 describe planificación, implementación y evaluación; el 5 discute los límites; el 6 presenta las conclusiones; el 7 recoge el trabajo futuro; el 8 contiene las referencias y el 9 identifica los anexos y las evidencias.

# Capítulo 2. ANTECEDENTES / ESTADO DEL ARTE

## 2.1 Recuperación de contexto en repositorios

Existen trabajos previos que recuperan información del repositorio para asistir a modelos de código. RepoCoder combina recuperación por similitud y generación iterativa [25]. GraphCoder utiliza grafos de contexto de código para recuperar fragmentos [26]. RepoGraph ofrece una estructura de repositorio que sirve de apoyo a agentes de ingeniería de software [27]. Estos antecedentes sitúan a Quirón: la recuperación de código mediante grafos y vectores no es una invención de este trabajo.

Lo que se estudia aquí es la integración de un worker local configurable con un editor nativo, proyecciones incrementales y comprobación de vigencia de las fichas. No se presenta una comparación experimental que establezca superioridad frente a los sistemas anteriores. La matriz siguiente describe el alcance de Quirón, no una clasificación exhaustiva de productos.

<!-- tabla: Aspectos cubiertos por Quirón frente a los antecedentes -->
| Aspecto | Aportación desarrollada en Quirón |
| --- | --- |
| Localización | Consulta de fichas con ruta, símbolo, líneas y hash |
| Mantenimiento | Monitor incremental del proyecto y retirada de unidades desaparecidas |
| Modelos | Generador local sustituible y embeddings independientes |
| Interfaz | Editor nativo con estado del worker, proveedores y fuentes recuperadas |
| Dependencias | Pertenencia y aproximación de llamadas Rust; cobertura incompleta |
| Evaluación | Integración y casos exploratorios; falta comparación de tareas completas |

## 2.2 Representación, generación y almacenes

Un generador de texto y un modelo de embeddings cumplen tareas distintas. BGE-M3 [19][29] proporciona las representaciones utilizadas por esta implementación. Qwen2.5-Coder [23] genera descripciones breves. La calidad de estas descripciones puede influir en la recuperación, pero el tamaño del generador no garantiza una mejora en todas las tareas.

La literatura sobre InfoNCE [13], last-token pooling [14][15] y Matryoshka [16] sirvió de base para explorar modelos de representación propios. El uso del estado del último token no acredita por sí solo calidad de embeddings, y las representaciones truncables requieren un entrenamiento apropiado: dos modelos con vectores de la misma dimensión no comparten necesariamente un espacio semántico. En esta versión no se entrena una representación propia ni se demuestra que los vectores de BGE-M3 puedan truncarse conservando calidad.

Qdrant [17] almacena los vectores y permite restringir candidatos mediante filtros de carga útil. Neo4j [18] almacena nodos y relaciones consultables. La identidad de las unidades permite vincular ambas proyecciones. La existencia de un grafo no implica que sus relaciones resuelvan todos los tipos, importaciones o llamadas del programa.

## 2.3 Estudio de alternativas para la red obrera

El estudio inicial revisó MLA [1][2], atención dispersa [3], mecanismos lineales [4–7], mezcla de expertos [8][9], destilación [10–12c] y cuantización [20][21]. Su resultado se conserva en el Anexo A como exploración de alternativas. Ninguno de esos mecanismos se implementó como un nuevo backbone entrenado en este trabajo.

La recurrencia de estado fijo puede tener coste constante por paso respecto a la longitud del contexto, para dimensiones fijas. Esta propiedad no se extiende al modelo completo si se añaden capas de atención completa sobre todo el historial. La exportación y el rendimiento de una arquitectura híbrida en ONNX/Rust quedaron como cuestiones por medir.

Sequence-Level KD [11] permite utilizar respuestas textuales de un profesor. QLoRA [24] muestra que es posible ajustar modelos preentrenados reduciendo el consumo de memoria. La decisión de no entrenar en esta versión es de alcance y viabilidad; no supone que cualquier ajuste resulte imposible con el equipo disponible.

## 2.4 Justificación del proyecto

Se busca facilitar la navegación por responsabilidades del código, conservar referencias comprobables y adaptar la inferencia al equipo del usuario. El ahorro económico y la prevención de lógica duplicada permanecen como hipótesis de aplicación. El cómputo local tiene costes de descarga, almacenamiento, memoria, energía y mantenimiento; disponer del hardware no los elimina. Las suscripciones y las conexiones por API son vías de acceso diferentes, con condiciones y límites propios.

# Capítulo 3. OBJETIVOS

## 3.1 Objetivo general y evolución del alcance

El objetivo general es **construir y evaluar un prototipo de editor nativo que mantenga un índice de código mediante un worker local configurable y aporte al asistente referencias comprobables contra los archivos del proyecto**. La versión inicial del proyecto incluía el entrenamiento de una red propia, un historial completo de cambios y un análisis amplio de dependencias. Durante el desarrollo se priorizó el recorrido funcional con modelos preentrenados, y se conserva la numeración de los objetivos para hacer visible esa evolución en lugar de presentar una reformulación como cumplimiento retroactivo del planteamiento inicial.

## 3.2 Objetivos específicos y estado

<!-- tabla: Objetivos específicos, planteamiento inicial y estado al cierre -->
| Objetivo | Planteamiento inicial | Alcance y estado al cierre |
| --- | --- | --- |
| OE1 | Registro inmutable y dos proyecciones reconstruibles desde él | Registro encadenado v2 con hash de todos los campos, transacciones y anclaje compatible; herramientas de replay de eventos; reconstrucción completa del índice de código pendiente |
| OE2 | Editor con confinamiento al proyecto | Editor y guardia de rutas implementados, con pruebas; quedan límites ante carreras del sistema de archivos |
| OE3 | Indexador incremental de Archivo, Lógica y Cambio | Archivo y lógica Rust operativos; tipo Cambio definido sin historial completo integrado |
| OE4 | Grafo de dependencias resuelto por análisis estático | Pertenencia y llamadas aproximadas Rust; resolución de tipos, imports y dependencias completas pendientes |
| OE5 | Vector, expansión, reranker y presupuesto de contexto | Filtro en la búsqueda de código y expansión limitada implementados; reranker y presupuesto global pendientes |
| OE6 | Estudiar mecanismos para una red propia exportable | Estudio de alternativas documentado (Anexo A); no se construyó un nuevo backbone |
| OE7 | Entrenar por destilación y desplegar una red propia | Entrenamiento excluido; objetivo reformulado a integrar, descargar y seleccionar modelos preentrenados compatibles |
| OE8 | Medir ahorro, recuperación, aislamiento y fidelidad | Evidencias funcionales y evaluación exploratoria; la hipótesis de ahorro en tareas completas no está demostrada |

## 3.3 Beneficios previstos y límites de la aportación

El índice facilita localizar código que podría reutilizarse y revisar la procedencia de la información recuperada. Un hash coincidente acredita la vigencia del archivo, no la corrección de su resumen. Quirón no dispone de un detector automático de lógica duplicada conectado al editor ni de resúmenes consolidados de responsabilidades por carpeta. Esas extensiones no forman parte de los resultados de esta versión.

# Capítulo 4. DESARROLLO DEL PROYECTO

## 4.1 Planificación

La planificación inicial situaba la infraestructura y el editor antes del entrenamiento. En septiembre se sustituyó el entrenamiento propio por la integración de modelos preentrenados y se priorizó el cierre funcional: monitor por proyecto, fichas y vectores, proveedores desde la interfaz, paquete de instalación y evaluación. La tabla resume las etapas.

<!-- tabla: Etapas del proyecto -->
| Etapa | Periodo | Trabajo realizado |
| --- | --- | --- |
| Infraestructura | Febrero–junio de 2026 | Servicios locales, registro de eventos, editor nativo y guardia de rutas |
| Memoria de eventos y estudio | Junio–julio de 2026 | Memoria de eventos con proyecciones, diagnóstico de la recuperación y estudio matemático de la red obrera (Anexo A) |
| Índice de código | Agosto–septiembre de 2026 | Indexador tree-sitter, worker Qwen/BGE-M3, colecciones por proyecto, grafo de llamadas, recuperación conectada al chat |
| Interfaz y proveedores | Septiembre de 2026 | Rediseño de la interfaz, paleta Agentes, conexiones Claude, Codex, compatibles y Ollama, catálogo del vectorizador |
| Cierre | 10–13 de septiembre de 2026 | Contraste entre código y memoria, ledger v2, fichas v5, paquete portátil, evaluación exploratoria, limpieza del código y redacción final |

La entrega se realiza por repositorio, según lo acordado con el tutor: **[COMPLETAR — URL del repositorio de entrega]**. La revisión entregada está identificada con la etiqueta `entrega-tfm`; el paquete binario incluye un inventario de hashes (`BUILD.json` y `SHA256SUMS`).

## 4.2 Solución, metodología y herramientas

Quirón se compone de cuatro piezas: el editor nativo, el cerebro (índice, registro de eventos y API local), la pasarela hacia los asistentes y el vectorizador local. La figura resume los componentes y los flujos de datos entre ellos.

![Componentes de Quirón y flujo de datos entre editor, cerebro, almacenes, vectorizador y asistentes](figuras/fig-arquitectura.png)

### 4.2.1 Memoria de eventos e índice de código

El cerebro utiliza Sled como almacenamiento local; el nombre histórico del módulo `storage/rocks.rs` no significa que se utilice RocksDB. Los eventos se encadenan mediante Blake3. Existen envolturas de memoria con estados y relaciones de sustitución o retractación, y herramientas de reconstrucción de proyecciones de eventos.

El índice de código mantiene su manifiesto y la caché de fichas y vectores en Sled, y obtiene el contenido de los archivos del disco. Confirma un archivo después de recibir el acuse de Qdrant y Neo4j, y comprueba periódicamente la presencia de las unidades para reparar determinadas pérdidas mediante la caché. Este mecanismo no equivale a reconstruir íntegramente el índice desde el registro de eventos: no se registran eventos suficientes para reproducir todo su historial.

La revisión final del registro encontró que la versión inicial del hash no cubría todos los campos del evento y que los índices se escribían en operaciones separadas. La corrección, denominada v2, calcula el hash sobre la serialización completa del evento (salvo su propio hash), con separación de dominio y longitudes delimitadas. Evento, índices, versión, anclaje y cabecera se escriben en una transacción de Sled que solicita persistencia antes de confirmar. Los escritores comparten un bloqueo y los identificadores duplicados se rechazan, también dentro de un lote. La verificación propaga los registros ilegibles en lugar de omitirlos.

Los eventos históricos conservan sus bytes y su algoritmo. El primer evento v2 incorpora un anclaje del contenido completo observado al migrar, lo que permite detectar cambios posteriores; no demuestra la originalidad del contenido anterior a ese momento. La prueba de interrupción termina un proceso después de confirmar lotes y verifica su recuperación íntegra; no se ha simulado una pérdida de alimentación. Tampoco se acredita resistencia frente a un actor capaz de reescribir toda la base y su cabecera, al no existir una raíz de confianza externa. Las envolturas derivadas de memoria siguen fuera de la transacción autoritativa.

### 4.2.2 Identidad y recorrido del proyecto

La identidad de cada proyecto es un ULID guardado en `.quiron/project.id`; la aplicación migra el estado histórico de `.llore`. El worker exige una raíz válida y una identidad coincidente. Una copia con la misma identidad no puede monitorizarse simultáneamente como otro proyecto independiente. La separación entre proyectos se verifica con identidades distintas.

El recorrido excluye enlaces simbólicos, nombres sensibles y directorios generados. Cuando la raíz es un repositorio Git, respeta el conjunto de archivos seguidos o no ignorados por Git. Sus límites son 512 KiB por archivo, 10 000 archivos y 64 MiB de texto por barrido. Las reglas se basan en rutas y nombres; no garantizan detectar credenciales incrustadas dentro de cualquier archivo de código.

### 4.2.3 Archivos, lógicas y fichas

Se genera una unidad de archivo para las extensiones admitidas. Tree-sitter añade unidades de lógica Rust: funciones, métodos, estructuras, enumeraciones y traits, con firma y líneas. La cobertura no incluye toda construcción del lenguaje, y los demás lenguajes tienen por ahora ficha de archivo.

El worker proporciona al generador hasta 6 000 caracteres de la unidad, numerados por línea, y las definiciones de constantes Rust referenciadas cuando caben en un contexto acotado. Esas definiciones participan en la huella de caché, de modo que un cambio en ellas invalida la ficha. Las entradas recortadas se marcan como parciales. La respuesta solicitada es un propósito en una frase completa, el contexto no resuelto y entre una y tres líneas de evidencia; el programa copia los fragmentos citados desde el código y convierte sus líneas a posiciones del archivo, de modo que el modelo no redacta las citas.

Se rechazan frases incompletas, referencias inexistentes, cifras ausentes del contexto y determinadas menciones de lenguajes sin respaldo en la entrada, además de salidas inválidas o que repiten instrucciones. En esos casos se guarda una ficha estructural con el motivo y `summary_origin=parser`; las aceptadas llevan `model`. Los errores de conexión son reintentables. Las reglas y las citas verifican restricciones concretas, no toda afirmación semántica de la descripción.

BGE-M3 vectoriza ruta, símbolo, firma y descripción. La ficha mantiene la relación con el archivo sin copiar todo su código al texto del vector. El extractor calcula además una huella del cuerpo normalizado por espacios, pero no existe un comparador automático de duplicados conectado a esa huella.

![Pantalla de vectorización al abrir un proyecto: el editor muestra el progreso del worker y queda vigilando cambios](figuras/fig-vectorizado.png)

### 4.2.4 Grafo y recuperación

Neo4j contiene los nodos `CodeProject` y `CodeUnit` y las relaciones `HAS_UNIT`, `DEFINED_IN` y `CALLS`. Las llamadas se extraen sintácticamente y se enlazan mediante reglas por nombre y archivo. Se omiten los casos ambiguos según esas reglas, pero una coincidencia única de nombre no prueba la resolución semántica del destino: se pierden determinados prefijos de módulo y no se resuelve el tipo del receptor. No se ofrecen garantías de análisis de impacto completo.

La búsqueda de código aplica dentro de Qdrant filtros de proyecto y de versiones de modelos, y revalida ruta y hash antes de devolver cada acierto. Añade un vecino por llamadas cuando puede obtenerlo del grafo. La consulta de vecinos filtra explícitamente proyecto y versiones de modelos en la semilla y en el vecino, y conserva la ficha JSON y su indicador de entrada parcial; estas condiciones se comprobaron introduciendo aristas entre proyectos de contenido idéntico y nodos de versiones anteriores. No se ejecuta un reranker de fichas ni se aplica un presupuesto global medido con el tokenizador del asistente. La memoria de eventos utiliza otro recorrido, en el que parte del filtrado por proyecto sigue ocurriendo después de buscar; las garantías verificadas del índice de código no se extienden automáticamente a todos los endpoints.

### 4.2.5 Editor y herramientas

El editor utiliza `winit`, `tiny-skia`, `softbuffer`, `taffy` y `cosmic-text`, con las tipografías incluidas en el producto. Permite abrir proyectos, editar archivos y consultar el chat; muestra el estado del worker en el panel «Segundo plano» y las fuentes recibidas como enlaces al archivo y a la línea. La paleta «Agentes» integra la selección de proveedores y del modelo del worker, y el manual de uso se abre desde la propia aplicación.

![Pantalla de bienvenida del editor](figuras/fig-bienvenida.png)

Las herramientas ofrecidas al asistente son lectura de archivos, listado y búsqueda textual. Se ejecutan mediante la guardia del proyecto y con límites de tamaño de salida. Su existencia no implica edición autónoma ni que cada respuesta consulte todos los archivos pertinentes. Persisten carreras posibles entre inspeccionar una ruta y abrirla; el modelo de uso evaluado es un escritorio local de un solo usuario.

![Ronda de chat con Claude sobre un proyecto sintético: el asistente lee el archivo mediante la herramienta del editor y la respuesta enlaza las fuentes con ruta y líneas](figuras/fig-chat-fuentes.png)

### 4.2.6 Proveedores y suscripciones

La pasarela admite cuatro conexiones: la CLI oficial de Claude (`claude_cli`), la CLI oficial de Codex (`codex_cli`), servidores compatibles con la API de OpenAI (`openai_compatible`, que cubre OpenAI, un servidor en la red local u Ollama por su interfaz compatible) y la API nativa de Ollama (`ollama_native`). Una suscripción no proporciona un identificador universal de acceso: cada adaptador tiene su propio mecanismo y restricciones, y las conexiones compatibles pueden requerir una clave de API. El selector reúne los alias de Claude (Sonnet, Opus y Haiku) y el catálogo de modelos visibles en la sesión de Codex, que puede actualizarse desde la interfaz. Cada conversación transmite su proveedor y modelo sin cambiar la configuración del servicio, de modo que elegir Claude o Codex no detiene el worker. En Codex se transmite también el esfuerzo de razonamiento.

![Selector de modelos de la paleta Agentes con los alias de Claude y el catálogo de la sesión de Codex](figuras/fig-selector-modelos.png)

Codex CLI se ejecuta en un directorio temporal, sin la configuración personal del usuario y con sus herramientas nativas de acceso a archivos, comandos y servicios desactivadas; la respuesta sigue un contrato JSON cuyas llamadas a herramientas ejecuta la guardia del editor. Quirón elimina del entorno del subproceso las credenciales del cerebro, de los almacenes y de otros proveedores. Es una conversación propia de Quirón, sin heredar el hilo del IDE.

Las evidencias incluyen proveedores reales y un servidor simulado que comprueba el transporte de herramientas. Una prueba con ese servidor verifica integración, no la calidad de un modelo comercial. Los resultados históricos no garantizan disponibilidad futura ni uso ilimitado de una suscripción.

### 4.2.7 Worker descargable y adaptación al dispositivo

La configuración inicial emplea Qwen2.5-Coder-1.5B-Instruct Q4_K_M mediante llama.cpp y BGE-M3 en CPU. El catálogo de `setup-worker.py` permite descargar modelos con revisión y SHA-256 fijos, y la paleta permite seleccionar cualquier GGUF compatible. El razonamiento del vectorizador va desactivado: su tarea es describir, no conversar. El techo recomendado para una GPU dedicada es Qwen3 8B a 4 bits; por encima solo tiene sentido en equipos con memoria unificada. Las recomendaciones de memoria del catálogo son orientativas y no equivalen a benchmarks de todos sus modelos.

Un dispositivo con más recursos puede evaluar un generador de mayor capacidad manteniendo el mismo recorrido. Una ficha más precisa puede aportar mejores datos para recuperar contexto, pero hay que medir calidad, latencia y memoria. La identificación del generador utiliza su nombre de archivo; sustituir pesos con el mismo nombre requiere invalidar la caché manualmente.

### 4.2.8 Selección de modelos preentrenados y delimitación del entrenamiento propio

Para el desarrollo se dispuso de un portátil con una RTX 3060 de 6 GB y de dos estaciones de trabajo adicionales accesibles por SSH con dos NVIDIA RTX 3090. Esa disponibilidad permite plantear inferencia y ajustes acotados de modelos existentes, pero no se realizó en esta versión un entrenamiento en esas estaciones.

Entrenar desde cero y ajustar un modelo preentrenado son tareas diferentes. Como referencia de escala, el informe de Qwen2.5-Coder describe un entrenamiento continuado sobre más de 5,5 billones de tokens [23]; esa cifra no es un requisito mínimo universal. Un ajuste mediante técnicas como QLoRA reduce las exigencias de memoria [24]. La presencia de dos GPU no constituye automáticamente un único espacio de memoria; el reparto del modelo y la comunicación requieren configuración específica.

El planteamiento inicial contemplaba destilar una red propia. Se decidió excluir ese entrenamiento del alcance final para concentrar el trabajo en la indexación, la recuperación y su validación. Pesaron la preparación de un corpus propio revisado, el coste de iteración y el plazo disponible. No se dispone de una comparación que demuestre que un modelo propio rendiría necesariamente peor, ni de un presupuesto medido del entrenamiento descartado.

Quirón conserva la función prevista del worker mediante modelos preentrenados sustituibles: describir archivos y lógicas, producir datos para la vectorización y mantener un mapa consultable. El código propio integra y controla ese recorrido. La selección por dispositivo permite evolucionar el componente aprendido sin atribuir a este trabajo el entrenamiento de los pesos descargados. Un ajuste futuro quedaría condicionado a demostrar una mejora frente a esta base.

### 4.2.9 Metodología y reproducibilidad

Se distinguen cuatro niveles de afirmación: diseño, implementación inspeccionada, prueba funcional registrada y resultado experimental comparativo. Las salidas JSON y las capturas sirven como evidencia de recorridos concretos. Una prueba que pasa no acredita comportamientos que no ejercita. Cada evaluación registra revisión del código, modelos, corpus, preguntas, métricas y limitaciones, y los errores se conservan junto con los aciertos.

## 4.3 Recursos

<!-- tabla: Recursos empleados -->
| Recurso | Uso y alcance de la evidencia |
| --- | --- |
| Portátil con RTX 3060 de 6 GB y 15 GiB de RAM utilizable | Equipo de desarrollo y de todas las pruebas locales registradas |
| Dos estaciones adicionales con dos RTX 3090 | Disponibles; no se atribuyen resultados de entrenamiento |
| Rust, Sled, Qdrant, Neo4j y servicios de usuario de systemd | Editor, almacenamiento y procesos locales |
| Qwen2.5-Coder, llama.cpp y BGE-M3 sobre ONNX Runtime | Inferencia de fichas y embeddings |
| CLI de Claude y Codex, endpoints configurables | Acceso a asistentes; condiciones dependientes del proveedor |
| Python, Docker y Xvfb | Instalador, pruebas de entrega y pruebas de ventana sin pantalla |

El coste inicial incluye descargas: aproximadamente 1,15 GB para el runtime y el modelo inicial del worker, además de BGE-M3 y las imágenes de los almacenes. No se descargó un corpus para entrenar una red propia. La VRAM observada del servidor Qwen fue de unos 1 130 MiB en la prueba registrada; no representa su pico máximo ni el consumo de todas las configuraciones. Tras una incidencia de presión de memoria durante el desarrollo, las unidades de systemd limitan la memoria del cerebro y del worker, y el servidor de Qwen arranca con la caché de prompts acotada.

## 4.4 Presupuesto

<!-- tabla: Presupuesto del proyecto -->
| Concepto | Importe o base |
| --- | --- |
| Dedicación del autor | [COMPLETAR — horas y criterio de valoración] |
| Hardware | [COMPLETAR — coste imputado o amortización; distinguir equipos disponibles y usados] |
| Suscripciones y servicios | [COMPLETAR — gastos reales por periodo] |
| Electricidad y almacenamiento | [COMPLETAR — estimación y método, si se incluyen] |
| Software y modelos | 0 €: componentes de código abierto y pesos descargables bajo sus licencias |
| Entrenamiento propio | No realizado; no se declara un gasto ni un ahorro medido |

La disponibilidad de software y pesos descargables no elimina las condiciones de licencia ni los costes operativos. La revisión de licencias de distribución acompaña al paquete.

## 4.5 Viabilidad y despliegue

Existe un paquete de instalación por usuario para Linux x86_64 que prepara configuración privada, servicios y lanzador. Requiere Python, Docker, systemd de usuario y acceso a las descargas. No es un paquete offline ni una distribución certificada para cualquier Linux. La comprobación mediante `--destdir` verifica archivos y configuración sin activar servicios; no sustituye una prueba de arranque en un equipo limpio.

El primer candidato, compilado en el equipo de desarrollo, exigía `GLIBC_2.43` y no cargaba en una base Debian 13 con glibc 2.41. La compilación de entrega pasa a Ubuntu 24.04 (glibc 2.39), con imagen base fijada por digest y Rust 1.93.0, y el empaquetador rechaza símbolos superiores a esa versión. Ubuntu 22.04 se descartó porque el archivo estático de ONNX Runtime utiliza funciones de C23 que su glibc no proporciona. El paquete incluye cinco binarios (editor, cerebro, pasarela, indexador y herramienta del registro), las fuentes, la memoria y las evidencias. Los cinco binarios resuelven sus bibliotecas en Ubuntu 24.04 y Debian 13; en Ubuntu se abrió una ventana Xvfb a 1280 × 720 y se comprobó el instalador en staging. Esa prueba descubrió bibliotecas X11 cargadas dinámicamente que `ldd` no detecta (`libXcursor`, `libXi`); se añadieron al control previo del instalador. No se certifica el ciclo completo con Docker y systemd en una máquina virtual limpia.

El cerebro mantiene los almacenes y el worker: Qdrant y Neo4j se levantan justo antes del cerebro y se paran con él, sin arrancar al encender la máquina. Cerrar la ventana permite continuar el trabajo en segundo plano, y el apagado por inactividad tiene en cuenta las peticiones y los barridos activos.

## 4.6 Resultados y evaluación

### 4.6.1 Pruebas automatizadas

Los tres crates y el instalador disponen de pruebas automatizadas. La tabla recoge la ejecución final sobre la revisión entregada, después de retirar del código los adaptadores y el servicio de embeddings externo que ya no se usaban.

<!-- tabla: Pruebas automatizadas de la revisión entregada -->
| Componente | Pruebas aprobadas | Observaciones |
| --- | --- | --- |
| Editor (workspace de siete crates) | 329 | Tres pruebas ignoradas por defecto; las que usan un servidor simulado necesitan acceso a localhost |
| Cerebro (`--features full`) | 183 | Incluye mutaciones del hash v2, rechazo atómico de lotes, cuatro escritores concurrentes, corrupción, migración y recuperación tras terminar un proceso |
| Pasarela | 12 | Selección explícita de proveedor por conversación y adaptadores vigentes |
| Entrega (instalador) | 9 | Docker simulado; no acreditan una instalación limpia con bases reales |

Las pruebas del cerebro no son una prueba de corte eléctrico ni de todos los fallos de disco posibles. Un error de entrada/salida después de confirmar deja un resultado incierto; un reintento con el mismo identificador se rechaza si ya estaba guardado.

### 4.6.2 Integración del worker y del editor

<!-- tabla: Evidencias funcionales registradas -->
| Evidencia | Qué acredita | Límite |
| --- | --- | --- |
| `2026-09-05/worker-integracion.json` | Dos proyectos sintéticos: consulta, cambios, borrados, exclusión de secretos y enlaces, caché | No mide precisión en proyectos arbitrarios |
| `2026-09-05/agentes/resumen.json` | Rondas de interfaz con Claude, Codex, servidor local y escenarios compatibles simulados | Distinguir inferencia real de transporte simulado |
| `2026-09-05/consulta-codigo.json` | Consultas con fuentes, control de vigencia y uso del chat | Muestra pequeña; incluye respuestas incompletas |
| `2026-09-10/worker-integracion.json` | Repetición sobre dos proyectos temporales: autenticación, consultas, cambios, borrados, proyecciones y ausencia de inferencia sin cambios | Equipo de desarrollo |
| `2026-09-10/gui/gui-edit.json` | Abrir, editar, guardar, deshacer y rehacer con estados intermedios, cerrar y reabrir conservando contenido e identidad, actualizar el índice | Una ventana, un archivo |
| `2026-09-10-estabilidad/worker-integracion.json` | 13 comprobaciones con Qwen, BGE-M3, Qdrant y Neo4j reales, incluidos filtros de vecinos entre proyectos y modelos | Equipo de desarrollo |

La verificación histórica del registro de eventos contó 5 057 eventos y una cadena válida bajo el algoritmo inicial; el significado de esa validación queda limitado por el apartado 4.2.1. La herramienta `ledger_admin` se ensayó sobre una copia antes de aplicarla al servicio: conservó byte a byte los 5 284 eventos existentes y añadió un evento de anclaje, con 5 285 eventos válidos en la comprobación posterior. Una ejecución intermedia de la integración fue interrumpida por el apagado por inactividad del servicio, que detuvo sus almacenes; se conservó ese fallo y se repitió la prueba manteniendo el servicio activo.

### 4.6.3 Evaluación exploratoria de localización

El protocolo utiliza preguntas de localización con símbolos esperados identificados en el código antes de consultar el índice. Mide si el objetivo aparece entre los cinco primeros resultados vectoriales y en el conjunto ampliado por llamadas, y registra el rango de la primera coincidencia. Una coincidencia de símbolo es un indicador de localización; no demuestra que la respuesta explique correctamente su comportamiento. Se compara además el volumen de las fichas recuperadas con el texto de los archivos de código bajo el mismo tokenizador: es una medida del material de contexto, no del coste de una tarea completa ni de la facturación de un proveedor.

La ejecución se conserva en `docs/evidencias/2026-09-10/localizacion.json`, junto con el protocolo `docs/evaluacion/localizacion-v1.json` y el script `scripts/evaluate-index.py`. El conjunto de referencia contiene 140 archivos Rust versionados bajo `src/`, de hasta 512 KiB cada uno. La búsqueda se hizo sobre el índice del proyecto completo, con los modelos ya cargados.

<!-- tabla: Resultados de la evaluación de localización sobre diez preguntas -->
| Medida | Resultado observado |
| --- | --- |
| Preguntas con objetivo entre los cinco resultados directos (Hit@5) | 10 de 10 |
| Media del inverso del rango del objetivo, hasta cinco (MRR@5) | 0,75 |
| Preguntas con objetivo tras la expansión por llamadas | 10 de 10; no aumenta los aciertos en esta muestra |
| Mediana de duración de la petición de búsqueda | 0,111 s |
| Fichas devueltas, contando apariciones entre preguntas | 75; todas coinciden con el proyecto solicitado y con el hash del archivo actual |
| Texto de referencia de los 140 archivos | 420 881 tokens |
| Mediana del conjunto de fichas por pregunta, incluida expansión | 2 491,5 tokens; 0,592 % del volumen de referencia |

Hit@5 cuenta las preguntas cuyo símbolo esperado aparece en los cinco primeros resultados; MRR@5 premia que aparezca antes. Se empleó el tokenizador de Qwen2.5-Coder-1.5B-Instruct mediante `/tokenize`, sin tokens especiales. El informe conserva las preguntas, los objetivos, los resultados completos, los hashes del corpus y los tiempos, y no se detectaron cambios de esos archivos durante la evaluación.

El volumen menor de las fichas no se presenta como un porcentaje de ahorro económico: enviar todo el código es solo una referencia de tamaño. Las preguntas proceden del desarrollo, no de un conjunto independiente; tampoco se puntuó la fidelidad de cada resumen ni se comparó el éxito de una tarea completa. Los diez aciertos permiten repetir una prueba de localización, pero no estimar una precisión general ni atribuir una mejora al grafo.

### 4.6.4 Fidelidad de las fichas

Una inspección de las fichas de esos objetivos encontró errores concretos en la versión v4 del worker: `collect_calls` se describía como análisis de TypeScript aunque su integración utiliza Rust; `read_file` mencionaba 1 000 caracteres cuando la constante es 24 000; y `stable_id` situaba al final dos bytes cero que el código intercala como separadores. También había descripciones que terminaban a mitad de frase. Los ejemplos y los fragmentos de contraste se conservan en `fidelidad-observaciones.json`. Esta inspección forma parte del desarrollo, no de una valoración independiente ni de una tasa de error representativa, y confirma que la firma y el código deben prevalecer sobre el texto generado.

La versión v5 de las fichas, descrita en el apartado 4.2.3, se probó sobre una muestra de ocho funciones que comprendía los tres ejemplos problemáticos y cinco funciones sintéticas adicionales. Siete descripciones fueron aceptadas y una se sustituyó por una ficha estructural al no terminar una frase completa. Las citas y los hashes coincidieron con las fuentes, y los tres errores anteriores no reaparecieron. Las descripciones todavía pueden omitir ramas, unidades o efectos. No se establece una tasa de fidelidad semántica ni se extrapola el Hit@5 de v4 a v5 sin repetir la evaluación.

### 4.6.5 Proveedores desde la interfaz

Las dos CLI se comprobaron desde la ventana del editor sobre la misma función sintética de cuatro líneas, que duplica un entero mediante `checked_mul`. El worker produjo dos fichas, de archivo y de función, sin recurrir al analizador estructural. En ambos casos el chat recuperó las fichas con sus referencias y hash, el asistente solicitó leer el archivo mediante la herramienta del editor y devolvió `Some(14)` para la entrada siete y `None` para el máximo de `u32`. La CLI de Claude resolvió el alias `sonnet` como `claude-sonnet-5`; Codex utilizó `gpt-6-astra` con esfuerzo `xhigh`. El editor registró 12,8 y 32,8 segundos respectivamente; dos rondas no permiten comparar rendimiento general. Al pasar de Claude a Codex no cambiaron los procesos del cerebro ni del worker ni el archivo privado de configuración. Las ventanas cerraron normalmente y el código no se modificó. Las evidencias están en `docs/evidencias/2026-09-10-proveedores/` y `docs/evidencias/2026-09-10-codex/`.

El catálogo consultado con Codex CLI 0.153.0 anunciaba GPT-6 Astra, GPT-5.6 Sol, Terra y Luna, GPT-5.5 y GPT-5.3 Codex Spark. Son los modelos visibles en esa cuenta y fecha; no se presenta esa lista como disponibilidad universal. Una prueba de transporte frente a un servidor de respuestas simulado verificó el modelo, el esfuerzo, el contrato de lectura, la retirada de una credencial sintética del entorno y la ausencia de herramientas nativas con acceso al proyecto.

### 4.6.6 Validación pendiente

Quedan pendientes una evaluación independiente de la fidelidad de las fichas, tareas completas con y sin índice, una referencia de recuperación léxica o sintáctica, la repetición con varios repositorios y una instalación limpia completa en otro equipo. El benchmark `ledger_bench.rs` mide lectura y escritura del registro; no mide ahorro de contexto, detección de duplicados ni durabilidad de cada evento frente a pérdida de alimentación.

# Capítulo 5. DISCUSIÓN

La elección de modelos preentrenados permite aprovechar capacidades desarrolladas por terceros y concentrar el trabajo propio en el recorrido del producto. La separación entre generador y embeddings permite estudiar mejoras de cada componente por separado. Las dos estaciones adicionales amplían las posibilidades de experimentación; su existencia no convierte en necesario entrenar un modelo para esta entrega.

Las fichas ya permiten describir archivos y determinadas lógicas. Las responsabilidades agregadas por carpeta y la detección automática de duplicados no están implementadas. La búsqueda por similitud puede proporcionar candidatos para una revisión, pero similitud no equivale a identidad de comportamiento. El hash comprueba la correspondencia con el archivo; no valida la semántica del resumen.

Las garantías deben expresarse con precisión. El índice depende de los archivos y de la caché local; el registro de eventos no contiene todo lo necesario para reconstruirlo. Las relaciones de llamadas emplean aproximaciones; la recuperación ampliada aplica filtros explícitos y conserva metadatos, pero eso no convierte las aristas en un análisis semántico completo. Los resultados de aislamiento sobre dos proyectos son evidencia acotada, no una prueba formal de todas las consultas posibles.

La validación comparativa puede comenzar con un presupuesto pequeño. Contar tokens offline no requiere pagar una petición que incluya todo el repositorio, pero por sí solo tampoco demuestra calidad equivalente. Una evaluación de tareas debe contabilizar instrucciones, fichas, lecturas adicionales, historial, resultados de herramientas y respuesta. No se presenta el coste de una prueba que no se realizó como evidencia de ahorro.

Por último, la reproducibilidad de la entrega exige que Markdown, DOCX y PDF, fuentes, binarios y evidencias indiquen su revisión y su alcance. Las capturas muestran una ejecución; el paquete se verifica por separado con su inventario de hashes.

# Capítulo 6. CONCLUSIONES

## 6.1 Conclusiones del trabajo

Se ha construido un prototipo de editor nativo que integra un worker local, fichas de archivos y de lógica Rust, búsqueda vectorial, un grafo parcial y referencias comprobables contra el código vigente. El sistema incorpora la descarga y selección de modelos compatibles y distintas conexiones con asistentes. Las pruebas registradas demuestran varios recorridos completos de integración y han permitido identificar y corregir fallos de calidad de las fichas, de integridad del registro y de recursos del equipo.

El entrenamiento propio fue un objetivo inicial reformulado de manera explícita. La contribución implementada se encuentra en la integración, el mantenimiento incremental y el control del contexto que recibe el asistente. El estudio matemático del Anexo A conserva valor como exploración de alternativas, sin presentarse como una nueva arquitectura entrenada o evaluada.

La hipótesis de reducción de contexto manteniendo la calidad de tareas completas sigue abierta. Siguen pendientes la reconstrucción completa del índice desde el registro y un análisis completo de dependencias. El registro v2 cubre todos los campos y confirma escrituras transaccionales; su anclaje histórico y la prueba de interrupción tienen los límites explícitos del apartado 4.2.1. El grado de cumplimiento se recoge en el Capítulo 3 para que los resultados puedan evaluarse sin confundir el prototipo con la totalidad del diseño inicial.

## 6.2 Conclusiones personales

[COMPLETAR — reflexión personal del autor sobre el aprendizaje, las decisiones tomadas y las dificultades encontradas durante el proyecto.]

# Capítulo 7. FUTURAS LÍNEAS DE TRABAJO

- Ampliar las pruebas del registro v2 a fallos de almacenamiento y pérdida de alimentación, y estudiar puntos de control externos si se requiere detectar la reescritura de toda la base.
- Registrar cambios de código suficientes para reconstruir las proyecciones desde evidencia histórica y probar su equivalencia lógica.
- Resolver mejor módulos, tipos y llamadas, y ampliar las pruebas de recuperación y aislamiento a más repositorios.
- Incorporar reranking y presupuesto de contexto con el tokenizador del asistente, medidos en tareas completas.
- Comparar generadores y cuantizaciones por fidelidad, latencia y memoria antes de seleccionar una configuración superior.
- Evaluar LoRA/QLoRA o destilación especializada sobre ejemplos revisados, si aporta mejoras frente al modelo base.
- Detectar candidatos a duplicados mediante estructura y similitud, con confirmación humana y métricas de falsos positivos.
- Presentar responsabilidades agregadas por carpetas y ampliar la extracción sintáctica a otros lenguajes.
- Certificar instalaciones limpias, actualizar paquetes y completar la revisión de licencias y portabilidad.

# Capítulo 8. REFERENCIAS

Los informes técnicos de modelos se citan como resultados de sus autores; no son mediciones reproducidas en este trabajo.

[1] DeepSeek-AI, "DeepSeek-V2: A Strong, Economical, and Efficient Mixture-of-Experts Language Model", arXiv:2405.04434, 2024.

[2] DeepSeek-AI, "DeepSeek-V3 Technical Report", arXiv:2412.19437, 2024.

[3] DeepSeek-AI, "Native Sparse Attention: Hardware-Aligned and Natively Trainable Sparse Attention", arXiv:2502.11089, 2025.

[4] MiniMax, "MiniMax-01: Scaling Foundation Models with Lightning Attention", arXiv:2501.08313, 2025.

[5] MiniMax, "MiniMax-M1", arXiv:2506.13585, 2025.

[6] "Every Attention Matters: An Efficient Hybrid Architecture for Long-Context Reasoning", arXiv:2510.19338, 2025. https://arxiv.org/abs/2510.19338

[7] "Scaling Linear Attention with Sparse State Expansion", arXiv:2507.16577, 2025. https://arxiv.org/abs/2507.16577

[8] DeepSeek-AI, "DeepSeekMoE: Towards Ultimate Expert Specialization in Mixture-of-Experts Language Models", arXiv:2401.06066, 2024.

[9] "Auxiliary-Loss-Free Load Balancing Strategy for Mixture-of-Experts", arXiv:2408.15664, 2024.

[10] G. Hinton, O. Vinyals, J. Dean, "Distilling the Knowledge in a Neural Network", arXiv:1503.02531, 2015.

[11] Y. Kim, A. M. Rush, "Sequence-Level Knowledge Distillation", arXiv:1606.07947, 2016.

[12a] Y. Gu et al., "MiniLLM: Knowledge Distillation of Large Language Models", arXiv:2306.08543, 2023.

[12b] R. Agarwal et al., "On-Policy Distillation of Language Models (GKD)", arXiv:2306.13649, 2023.

[12c] "Rethinking Kullback-Leibler Divergence in Knowledge Distillation for Large Language Models", arXiv:2404.02657, 2024. https://arxiv.org/abs/2404.02657

[13] A. van den Oord, Y. Li, O. Vinyals, "Representation Learning with Contrastive Predictive Coding", arXiv:1807.03748, 2018.

[14] L. Wang et al., "Improving Text Embeddings with Large Language Models (e5-mistral)", arXiv:2401.00368, 2024.

[15] "Efficient Code Embeddings from Code Generation Models", arXiv:2508.21290, 2025. https://arxiv.org/abs/2508.21290

[16] A. Kusupati et al., "Matryoshka Representation Learning", arXiv:2205.13147, 2022.

[17] Qdrant — Vector Database. https://qdrant.tech

[18] Neo4j Graph Database. https://neo4j.com

[19] BAAI, modelos `bge-m3` y `bge-reranker-v2-m3`. https://huggingface.co/BAAI

[20] J. Lin et al., "AWQ: Activation-aware Weight Quantization for LLM Compression and Acceleration", arXiv:2306.00978, 2023.

[21] E. Frantar et al., "GPTQ: Accurate Post-Training Quantization for Generative Pre-trained Transformers", arXiv:2210.17323, 2022.

[22] ONNX Runtime. https://onnxruntime.ai

[23] B. Hui et al., "Qwen2.5-Coder Technical Report", arXiv:2409.12186, 2024. https://arxiv.org/abs/2409.12186

[24] T. Dettmers, A. Pagnoni, A. Holtzman y L. Zettlemoyer, "QLoRA: Efficient Finetuning of Quantized LLMs", arXiv:2305.14314, 2023. https://arxiv.org/abs/2305.14314

[25] F. Zhang et al., "RepoCoder: Repository-Level Code Completion Through Iterative Retrieval and Generation", arXiv:2303.12570, 2023. https://arxiv.org/abs/2303.12570

[26] W. Liu et al., "GraphCoder: Enhancing Repository-Level Code Completion via Code Context Graph-based Retrieval and Language Model", arXiv:2406.07003, 2024. https://arxiv.org/abs/2406.07003

[27] S. Ouyang et al., "RepoGraph: Enhancing AI Software Engineering with Repository-level Code Graph", arXiv:2410.14684, 2024. https://arxiv.org/abs/2410.14684

[28] ggml-org, "llama.cpp server", documentación y código. https://github.com/ggml-org/llama.cpp/tree/master/tools/server

[29] J. Chen et al., "M3-Embedding: Multi-Linguality, Multi-Functionality, Multi-Granularity Text Embeddings Through Self-Knowledge Distillation", arXiv:2402.03216, 2024. https://arxiv.org/abs/2402.03216

# Capítulo 9. ANEXOS Y TRAZABILIDAD

- **Anexo A — Estudio de alternativas para la red obrera:** se incorpora a continuación de este capítulo desde `docs/ESTUDIO_RED_OBRERA.md`. Se conserva como propuesta inicial, con una nota que aclara su relación con la implementación.
- **Anexo B — Informes técnicos de cierre:** `docs/CIERRE_TFM_2026-09-10.md` (contraste entre código y memoria, con los límites técnicos encontrados), `docs/ESTABILIZACION_2026-09-10.md` (correcciones del registro, de las fichas y del paquete) y `docs/CODEX_EN_QUIRON_2026-09-10.md` (integración de Codex).
- **Anexo C — Manual e instalación:** `docs/MANUAL.md` (manual de uso, accesible desde la aplicación), `docs/GUIA_EVALUACION.md` (guía para evaluar la aplicación en otro equipo) y `deploy/LINUX.md` (requisitos y límites del paquete Linux). El código es la referencia para resolver cualquier discrepancia con la documentación.
- **Anexo D — Evidencias:** `docs/evidencias/2026-09-05/` para las pruebas de la primera integración y `docs/evidencias/2026-09-10*/` para las del cierre. Los archivos de cada ejecución indican su alcance; una captura o una simulación no se presenta como ensayo de instalación limpia.
- **Anexo E — Fuentes y artefactos:** repositorio de entrega [COMPLETAR — URL], revisión etiquetada `entrega-tfm`. Este Markdown es la fuente de las exportaciones DOCX y PDF, generadas con `scripts/export-memory.py`; el paquete binario se construye con `scripts/package-linux-portable.sh` y lleva su inventario de hashes.
