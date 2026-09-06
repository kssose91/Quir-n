# Manual de Quirón

Quirón es un editor con un chat de proyecto. Su fundamento es un **mapa
vectorial local que se actualiza solo**: un vectorizador (el *worker*) lee los
archivos y carpetas del proyecto, escribe qué hace cada función y lo vectoriza
mientras trabajas. Cuando preguntas, el agente que hayas elegido recibe ese
guion, con las fichas que respaldan cada respuesta, para ayudarte en archivos y
trabajos grandes sin tener que leerlo todo cada vez.

## El vectorizador (worker)

El worker no chatea: su único trabajo es leer y vectorizar.

- Al abrir una carpeta recorre el proyecto archivo por archivo. Del árbol
  sintáctico saca unidades **Archivo** y **Lógica** (funciones, métodos,
  tipos) y, para cada una, escribe una **ficha**: qué hace, qué recibe y qué
  devuelve, y qué no está claro.
- Cada ficha se vectoriza (BGE-M3) y se guarda con el **hash blake3** del
  archivo en ese momento. Si el archivo cambia, la ficha deja de valer hasta
  que se rehace: una respuesta nunca cita código que ya no existe.
- Después se queda **vigilando**: solo rehace lo que cambia. También registra
  quién llama a quién (aristas CALLS), que es lo que la búsqueda vectorial
  no ve.
- El panel **Segundo plano** (arriba a la derecha) enseña cuántos archivos
  lleva, cuál analiza y cuántas fichas escribió el modelo o el analizador.
- Sin GPU o sin modelo, las fichas son **estructurales** (del analizador
  sintáctico): la búsqueda sigue funcionando, con menos matiz.

El modelo de serie es Qwen2.5-Coder 1.5B (Q4_K_M) servido por llama.cpp en la
GPU (Vulkan) o en CPU. Es pequeño a propósito: tiene que caber al lado del
editor y de los almacenes.

## Los agentes

El chip **● Agentes** (barra superior) abre la paleta; también sale sola en
el primer arranque si no hay ningún agente configurado.

- **Claude** y **ChatGPT**: suscripciones por su CLI oficial (`claude`,
  `codex`). «Iniciar sesión» abre una terminal con el flujo de la CLI; la
  aplicación no lee ni guarda credenciales.
- **OpenAI / compatible**: endpoint, modelo y clave. La clave se escribe
  enmascarada y viaja al archivo privado por la entrada estándar.
- **Servidor en red local**: un servidor compatible en tu red (llama-server,
  vLLM, SGLang…), con clave opcional.
- **Ollama local**: descubre los modelos descargados y usa su API compatible.
- Con proyecto abierto el agente tiene **manos**: `search_text`, `read_file`
  y `list_files`, que ejecuta el editor sobre el proyecto (nunca fuera de él,
  nunca sobre secretos). Bajo cada respuesta van las fichas que la respaldan
  como `archivo:líneas · símbolo`; al pulsar una se abre el archivo ahí.

## Modelos del vectorizador

En la paleta Agentes, la tarjeta **Vectorizador (worker)** dice qué modelo
está en marcha y permite cambiarlo:

- **Modelos**: lista los `.gguf` de la carpeta del worker; elegir uno lo
  aplica (el servicio se reinicia con él). Debajo, el **catálogo** de modelos
  descargables con su tamaño y para qué equipo valen; «Descargar» abre una
  terminal con la descarga verificada (SHA-256).
- **Añadir .gguf…**: copia a la carpeta del worker cualquier modelo que ya
  tengas en el disco.
- **Mejor worker, mejores fichas.** Cada salto de tamaño entiende mejor el
  código y escribe fichas más precisas, y tarda más por archivo. La lista de
  Modelos dice qué GPU y memoria tiene este equipo y qué modelo le va; las
  entradas que no caben quedan marcadas.
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
- Cambiar de modelo re-etiqueta las fichas: el proyecto se vuelve a resumir
  con el nuevo, y las fichas del anterior se conservan por si vuelves.
- Sin interfaz: `scripts/setup-worker.py --model qwen3-8b-q4_k_m` y
  `scripts/configure-provider.py worker-model --worker-model ruta.gguf --apply`.

## Trabajar con proyectos grandes

- **Abrir carpeta** (botón azul o Ctrl+O). La primera vez el índice tarda lo
  que tarde el worker en leerlo todo; puedes preguntar mientras.
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
  Los adjuntos pasan por la guardia: nunca secretos ni archivos de fuera del
  proyecto.
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

- En el proyecto: `.llore/` (identidad del proyecto y los hilos del chat).
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
