# Guía de evaluación: conectar Quirón y probarlo en un equipo nuevo

Para quien evalúa la aplicación sin conocer el proyecto: qué instalar, cómo se
conecta cada agente desde la propia interfaz y qué debe verse en cada paso.
Tiempo estimado: 15 minutos más las descargas de la primera apertura.

## 1. Instalar

Requisitos: Linux x86_64, Python 3.12+, Docker accesible por el usuario,
systemd de usuario, curl. No hace falta Rust ni GPU; sin GPU, el modelo local
puede ejecutarse en CPU. Consulte `BUILD.json` para la ABI de los binarios.
La entrega se compila sobre Ubuntu 24.04 y limita los símbolos requeridos a
glibc 2.39. Sustituye al primer candidato que exigía `GLIBC_2.43`.
Las comprobaciones de bibliotecas y staging se documentan en
`ESTABILIZACION_2026-09-10.md`; la instalación limpia completa sigue pendiente.

```sh
tar -xzf quiron-linux-x86_64.tar.gz
cd quiron-linux-x86_64
sha256sum -c SHA256SUMS
python3 install.py --with-worker      # sin --with-worker: sin Qwen local (1,15 GB menos)
quiron                                 # o desde el menú de aplicaciones
```

La primera apertura descarga las dos imágenes fijadas por digest (Qdrant y
Neo4j) y el modelo de vectores BGE-M3. El cerebro arranca solo al abrir la
aplicación y se apaga solo tras 15 minutos sin uso; no queda nada residente.

## 2. Elegir un agente (paleta «Agentes»)

Tras «Empezar», si el equipo no tiene ningún agente configurado, la paleta
**Agentes** se abre sola. También se abre en cualquier momento con el chip
**● Agentes** de la barra superior (el punto es la salud del cerebro) o desde la
paleta de órdenes. Cada tarjeta enseña el estado real del equipo, en verde si
está lista y en ámbar si falta algo, y qué hacer:

| Tarjeta | Qué hace falta | Botones |
|---|---|---|
| **Claude** | suscripción de claude.ai y su CLI (`npm install -g @anthropic-ai/claude-code`) | Instalar → Iniciar sesión (abre una terminal con `claude auth login`) → Usar |
| **ChatGPT** | suscripción de ChatGPT y Codex (`npm install -g @openai/codex`) | Instalar → Iniciar sesión (`codex login`) → Usar |
| **OpenAI / compatible** | endpoint, modelo y clave de API | Configurar… (tres campos; la clave no se muestra ni sale en ninguna orden) → Usar |
| **Servidor en red local** | un servidor compatible en la LAN (llama-server, vLLM, SGLang…) | Configurar… (endpoint, modelo y clave opcional) → Usar |
| **Ollama local** | Ollama en marcha con un modelo descargado (`ollama pull qwen2.5-coder:7b`) | Instalar → Configurar… (propone el primer modelo descargado) → Usar |
| **Vectorizador (worker)** | Qwen describe archivos y unidades Rust; BGE-M3 genera los vectores | Modelos (orientación de hardware, `.gguf` locales y catálogo descargable con SHA-256) · Añadir .gguf… |

«Usar» reescribe el archivo privado (`~/.config/quiron/quiron-brain.env`) y
reinicia el cerebro; el editor reconecta solo. El inicio de sesión se hace en
la CLI. Claude y ChatGPT responden mediante sus CLI oficiales. El selector del
chat reúne ambos proveedores; pasar entre ellos desde ese menú no reinicia los
servicios. En Codex puedes actualizar el catálogo y elegir el nivel de
razonamiento. «Comprobar» vuelve a sondear
el equipo. No se admite un identificador universal de suscripción.

Sin interfaz (por ejemplo por SSH), lo mismo con el script que usa la paleta:

```sh
scripts/configure-provider.py claude-cli --model sonnet --apply
scripts/configure-provider.py openai-compatible --endpoint https://api.openai.com --model gpt-5.5 --api-key-from-stdin --apply
scripts/configure-provider.py openai-compatible --endpoint http://127.0.0.1:11434 --model qwen2.5-coder:7b --apply   # Ollama
```

El manual de la aplicación está dentro: **F1**, el chip «Manual» de la barra
superior o la paleta de órdenes. Explica el vectorizador, los agentes, los
modelos y los atajos (`docs/MANUAL.md` es el mismo texto).

## 3. Abrir un proyecto y preguntar

1. **Abrir carpeta** (botón azul de la barra lateral o Ctrl+O) y elegir un
   repositorio. La primera vez el índice recorre el proyecto archivo por
   archivo: la esquina superior derecha («Segundo plano») dice cuántos lleva y
   cuál analiza. Se puede preguntar antes de que termine.
2. Escribir en el chat, por ejemplo: «¿Qué hace la función `ensure_current` y
   en qué archivo está?». La respuesta trae, debajo, las fichas que la
   respaldan como `archivo:líneas · símbolo`; al pulsar una se abre el archivo
   en esa línea.
3. Con Claude, ChatGPT o un servidor compatible el modelo además tiene manos:
   puede pedir `search_text` o `read_file`, que el editor ejecuta sobre el
   proyecto (el gateway solo transporta la petición). En el chat se ve qué
   herramienta pidió y qué devolvió.
4. **Nuevo chat** abre un hilo sin memoria vectorial; la pestaña
   «Conversaciones» guarda los hilos por proyecto (`.quiron/state/chats.json`).

## 4. Comprobar que las fuentes son verificables

Cada ficha lleva el hash blake3 del archivo en el momento del índice. Si el
archivo cambia, la ficha deja de valer hasta que se reindexa (control negativo
reproducible). Con el cerebro en marcha:

```sh
python3 scripts/demo-consulta-codigo.py --output /tmp/consulta.json --b3sum "$(command -v b3sum)"
```

Deja un JSON con la búsqueda directa, el control negativo (altera un archivo
limpio, comprueba que sus fichas desaparecen y lo restaura) y la respuesta del
chat con las fichas inyectadas y el modelo real usado.

## 5. Probar todos los agentes de una vez

`scripts/demo-agentes.py` hace la ronda completa como la haría un evaluador:
aplica cada agente con los mismos argumentos que la paleta, teclea la pregunta
en el editor real (arnés `scripts/smoke-gui-x11.py`, Hyprland/X11) y guarda
capturas y un resumen. Los agentes de pago o con sesión se prueban contra un
servidor simulado (`scripts/tests/mock-openai.py`) que exige la clave y pide
una herramienta, así que no hace falta ninguna cuenta para verificar el camino
completo. Restaura la configuración privada al terminar.

```sh
python3 scripts/demo-agentes.py --output-dir /tmp/agentes            # los cinco escenarios
python3 scripts/demo-agentes.py --only compatible ollama --output-dir /tmp/agentes
```

Resultados de la ronda hecha en el portátil de desarrollo el 5 de septiembre de
2026: `docs/evidencias/2026-09-05/agentes/resumen.json` y sus capturas.

## 6. Qué mirar si algo falla

- **El chat no responde**: la paleta Agentes dice qué le falta a la tarjeta en
  uso; el registro del cerebro deja una línea por llamada:
  `journalctl --user -u quiron-brain | grep '\[gateway\]'`.
- **El índice no avanza**: `systemctl --user status quiron-brain quiron-worker`
  y la esquina «Segundo plano» del editor. Sin servidor o modelo, la generación
  falla y se reintenta. La ficha estructural se usa ante una respuesta inválida
  del modelo; la falta de GPU permite usar CPU.
- **Memoria**: las unidades llevan topes (`MemoryHigh`/`MemoryMax`); en equipos
  de 16 GB conviene `earlyoom`. Detalles en `deploy/LINUX.md`.
