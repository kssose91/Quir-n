# Manual de Quirón

Quirón es un editor con un chat de proyecto. Su fundamento es un **mapa
vectorial local que se actualiza solo**: un vectorizador (el *worker*) lee los
archivos del proyecto, genera descripciones y las vectoriza mientras trabajas.
En Rust también extrae funciones, métodos y tipos. Cuando preguntas, el agente
que hayas elegido puede recibir fichas con referencias al código para localizar
lo que necesita leer.

## El vectorizador (worker)

El worker no chatea: su único trabajo es leer y vectorizar.

- Al abrir una carpeta recorre el proyecto archivo por archivo. Genera fichas de
  **Archivo** para las extensiones admitidas. En Rust extrae además unidades de
  **Lógica** (funciones, métodos y tipos) con símbolo, firma y líneas. El modelo
  redacta una frase breve y señala lo que no puede resolver. La ficha guarda
  líneas de código como referencia para comprobar la explicación.
- Cada ficha se vectoriza (BGE-M3) y se guarda con el **hash blake3** del
  archivo en ese momento. Si el archivo cambia, la ficha deja de valer hasta
  que se rehace: la búsqueda comprueba el archivo antes de devolverla. Una
  respuesta anterior del chat puede conservar referencias antiguas.
- Después se queda **vigilando**: solo rehace lo que cambia. También registra
  llamadas aproximadas entre símbolos Rust (aristas CALLS). No resuelve todas
  las llamadas, importaciones y dependencias del programa.
- El panel **Segundo plano** (arriba a la derecha) enseña cuántos archivos
  lleva, cuál analiza y cuántas fichas escribió el modelo o el analizador.
- Sin GPU, el modelo puede ejecutarse en CPU. Si devuelve una salida inválida,
  se conserva una ficha **estructural** del analizador, sin atribuirle una
  explicación al modelo. Si el servidor o el modelo no están disponibles, la
  generación falla y se reintenta; las fichas existentes siguen sujetas a vigencia.

Las comprobaciones rechazan, entre otros casos, frases incompletas, referencias
a líneas inexistentes y cifras ausentes del contexto enviado. Aun así, el modelo
puede omitir detalles o equivocarse: conviene leer el código citado antes de
cambiar una función.

El modelo de serie es Qwen2.5-Coder 1.5B (Q4_K_M) servido por llama.cpp en la
GPU (Vulkan) o en CPU. Es pequeño a propósito: tiene que caber al lado del
editor y de los almacenes.

## Los agentes

El chip **● Agentes** (barra superior) abre la paleta; también sale sola en
el primer arranque si no hay ningún agente configurado.

- **Claude** y **ChatGPT**: «Iniciar sesión» abre una terminal con el flujo de
  `claude` o `codex`. Ambos responden mediante sus CLI oficiales. Cada proveedor
  aplica sus condiciones y límites; el número de suscripción no es una clave.
- **OpenAI / compatible**: endpoint, modelo y clave. La clave se escribe
  enmascarada y viaja al archivo privado por la entrada estándar.
- **Servidor en red local**: un servidor compatible en tu red (llama-server,
  vLLM, SGLang…), con clave opcional.
- **Ollama local**: descubre los modelos descargados y usa su API compatible.
- Con proyecto abierto el agente tiene **manos**: `search_text`, `read_file`
  y `list_files`, que ejecuta el editor sobre el proyecto con una guardia de
  rutas, enlaces y nombres sensibles. No detecta cualquier secreto incrustado
  en un archivo de código. Bajo la respuesta se muestran las fichas recuperadas
  como `archivo:líneas · símbolo`; al pulsar una se abre el archivo ahí.

El menú del modelo reúne **Claude · Anthropic** (Sonnet, Opus y Haiku) y
**ChatGPT · Codex**. Puedes cambiar entre ambos sin reiniciar el cerebro ni el
worker; cada pregunta lleva su proveedor y modelo. Las dos CLI necesitan su
propia sesión. Los nombres de Claude son alias que resuelve su CLI.

La sección de Codex lee su catálogo. **Actualizar modelos de Codex** vuelve a
consultarlo; la lista depende de tu sesión. Seleccionar un modelo lo envía también
cuando se usan herramientas. En Codex, el menú **Razonamiento**
ofrece los niveles admitidos por ese modelo, incluido **Extra alto** cuando está
disponible. La elección entre Claude y Codex, el modelo y el nivel se conservan
al reabrir el proyecto. No se sustituye silenciosamente el modelo si el servidor
lo rechaza.

La conexión crea conversaciones propias de Quirón: recibe las fichas, el contexto
y las lecturas de este editor. No hereda automáticamente las conversaciones del
IDE. Este adaptador usa razonamiento sin delegación nativa; el nivel Ultra no
forma parte de sus opciones. El worker Qwen/BGE-M3 sigue siendo independiente.

## Modelos del vectorizador

En la paleta Agentes, la tarjeta **Vectorizador (worker)** dice qué modelo
está en marcha y permite cambiarlo:

- **Modelos**: lista los `.gguf` de la carpeta del worker; elegir uno lo
  aplica (el servicio se reinicia con él). Debajo, el **catálogo** de modelos
  descargables con su tamaño y para qué equipo valen; «Descargar» abre una
  terminal con la descarga verificada (SHA-256).
- **Añadir .gguf…**: copia a la carpeta del worker cualquier modelo que ya
  tengas en el disco.
- Un modelo mayor puede mejorar algunas descripciones, pero hay que comprobar
  calidad, tiempo y memoria en el proyecto. La lista de Modelos muestra una
  orientación de hardware, no una garantía de rendimiento.
- Guía por equipo (todo a 4 bits, Q4_K_M):
  sin GPU → Coder 0.5B (rápido) o Coder 1.5B en CPU (lento);
  GPU de 4-6 GB (portátil) → Coder 1.5B de serie, Coder 3B o Qwen3 4B si
  sobra memoria;
  GPU de 8-12 GB → Coder 7B o **Qwen3 8B, el techo del catálogo para GPU
  dedicada**;
  memoria unificada de 32 GB o más (DGX Spark, Mac, Strix Halo) → Qwen3-Coder
  30B-A3B (mezcla de expertos con 3B activos) o cualquier GGUF con «Añadir
  .gguf…».
- El vectorizador nunca «piensa»: el razonamiento de los Qwen3 va apagado
  (miles de fichas no pueden esperar). Un modelo que razone tiene su sitio
  como agente del chat, servido en local (tarjetas Ollama o Servidor en red).
- Cambiar a un modelo con otro nombre de archivo hace que el proyecto vuelva
  a resumirse. Sustituir los pesos conservando el mismo nombre no fuerza esa
  regeneración. No debe darse por conservada una versión histórica completa
  del índice por modelo.
- Sin interfaz: `scripts/setup-worker.py --model qwen3-8b-q4_k_m` y
  `scripts/configure-provider.py worker-model --worker-model ruta.gguf --apply`.

## Trabajar con proyectos grandes

- **Abrir carpeta** (botón azul o Ctrl+O). La primera vez con una carpeta,
  Quirón pide permiso antes de tocarla: explica que va a leerla, escribir
  fichas del código admitido y vectorizarlas, y que los agentes que elijas podrán
  leer sus archivos mediante la guardia de rutas. Al dar el permiso se
  crea la identidad del proyecto en `.quiron/` y la pantalla enseña la mente
  vectorial girando con el avance: fase, archivos leídos, archivo actual y
  fichas escritas. Según el tamaño del proyecto tarda más o menos; «Ir al
  chat» deja la vectorización en segundo plano y «Cancelar la vectorización»
  la para (el chat sigue, sin fichas; se retoma en Agentes → Vectorizador →
  Vectorizar). Cancelar en la pantalla de permiso no crea nada.
- Hay un tope: 10 000 archivos o 64 MiB de texto por proyecto. Una carpeta
  con muchos proyectos dentro (por ejemplo `~/Projects`) lo supera; la
  pantalla lo dice tal cual y ofrece elegir otra carpeta. Abre el proyecto
  concreto, no la carpeta que los contiene.
- Pregunta en el chat como preguntarías a alguien que conoce el proyecto:
  «¿dónde se comprueba el hash antes de devolver un acierto?». La respuesta
  cita fichas; cada ficha lleva a su línea.
- **Nuevo chat** abre un hilo sin memoria vectorial. Los hilos se guardan
  por proyecto en «Conversaciones»; los repositorios recientes, en
  «Repositorios».
- **La barra del chat**: el campo crece hasta seis líneas (Mayús+Intro salta
  de línea, Intro envía), el cursor se mueve con las flechas, Inicio/Fin y
  Ctrl+Retroceso borra una palabra; flecha arriba recupera las preguntas
  anteriores. El menú **+** tiene: adjuntar un archivo del proyecto,
  mencionar un archivo (`@ruta`, que se adjunta al enviar), añadir la
  selección del editor, vaciar la conversación, rebobinar la última pregunta
  (vuelve al campo para corregirla), cambiar de modelo, activar o quitar las
  manos y elegir la longitud de la respuesta (corta, normal, larga). A la
  derecha, el botón de enviar se vuelve **parar** mientras piensa; los chips
  del pie enseñan el modelo, las manos, la longitud y el contexto gastado.
  Los adjuntos pasan por la misma guardia de rutas y nombres sensibles.
- **El explorador (Archivos)**: botón derecho sobre una carpeta o un archivo
  abre su menú: nuevo archivo y nueva carpeta (piden el nombre), abrir en el
  gestor de archivos o en una terminal, buscar en la carpeta, añadir la
  carpeta o el archivo al chat, cortar, copiar y pegar (mover o copiar a otra
  carpeta), copiar la ruta o la ruta relativa, renombrar y eliminar (pide
  confirmación; las pestañas del archivo se cierran). Sobre el fondo del
  árbol, el menú es el de la raíz del proyecto. Todo queda dentro del
  proyecto; la raíz no se elimina.
- Atajos: Ctrl+O abrir carpeta · Ctrl+P buscar archivo · Ctrl+Shift+P
  paleta de órdenes · F1 este manual · Esc cierra cualquier paleta.

## Qué se guarda y dónde

- En el proyecto: `.quiron/` (identidad del proyecto y los hilos del chat). Si la
  carpeta tenía `.llore/` de otro editor, se copia una vez y no se toca.
- En el equipo: los vectores en Qdrant y el grafo en Neo4j (`data/`), y el
  archivo privado `~/.config/quiron/quiron-brain.env` con la elección de
  agente y las claves locales. Nada sale del equipo salvo lo que va al
  agente que elegiste, y solo lo que el chat le manda.
- El cerebro (índice, almacenes y gateway) arranca al abrir la aplicación y
  se apaga solo tras 15 minutos sin uso.

## Si algo falla

- El chat no responde: la paleta Agentes dice qué le falta a la tarjeta en
  uso. Cada llamada deja una línea `[gateway]` en el registro del cerebro:
  `journalctl --user -u quiron-brain`.
- El índice no avanza: `systemctl --user status quiron-brain quiron-worker`
  y el panel Segundo plano.
- Memoria: las unidades llevan topes; en equipos de 16 GB conviene un
  protector como `earlyoom`.
