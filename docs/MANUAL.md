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
- El catálogo es la familia Qwen2.5-Coder (0.5B, 1.5B, 3B y 7B). Regla
  práctica: 0.5B para portátiles sin GPU, 1.5B para 6 GB de VRAM, 3B y 7B
  para tarjetas más grandes o para quien prefiera mejores fichas a cambio
  de tiempo.
- Cambiar de modelo re-etiqueta las fichas: el proyecto se vuelve a resumir
  con el nuevo, y las fichas del anterior se conservan por si vuelves.
- Sin interfaz: `scripts/setup-worker.py --model qwen2.5-coder-3b-q4_k_m` y
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
