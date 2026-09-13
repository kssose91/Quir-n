# Quirón para Linux x86_64 — candidato de instalación

Requisitos: Python 3.12+, Bash, Docker Engine accesible por el usuario, systemd
de usuario, curl y coreutils. Escritorio Wayland o X11. No requiere Rust ni GPU.
BGE-M3 trabaja en CPU. `--with-worker` prepara Qwen 1.5B Q4_K_M y llama.cpp
Vulkan/CPU (1,15 GB de descarga adicional). Para NVIDIA hace falta un controlador
Vulkan funcional; sin NVIDIA se usa CPU. No se incluye entrenamiento.

**Base de compilación de la entrega:** Ubuntu 24.04, glibc 2.39. El
empaquetador impide incluir símbolos de glibc posteriores. Esta compilación
sustituye al primer candidato del día 10 que exigía `GLIBC_2.43`.
Las bibliotecas requeridas por cada binario figuran en `BUILD.json` y
`docs/*-ldd.txt`; disponer de esa glibc no basta si falta otra dependencia.
Las pruebas de compatibilidad se detallan en `docs/ESTABILIZACION_2026-09-10.md`.

Bibliotecas de escritorio en Ubuntu 24.04: `libssl3t64`, `libxkbcommon0`,
`libwayland-client0` y, para X11/XWayland, `libx11-6`, `libx11-xcb1`,
`libxcursor1` y `libxi6`. El worker Vulkan utiliza `libvulkan1` y el controlador
del dispositivo. El instalador comprueba también las bibliotecas que la ventana
carga dinámicamente y que no aparecen en `ldd`.

```sh
tar -xzf quiron-linux-x86_64.tar.gz
cd quiron-linux-x86_64
sha256sum -c SHA256SUMS
python3 install.py --with-worker
```

Instala en `~/.local/share/quiron`, registra `quiron.desktop` y el lanzador
`~/.local/bin/quiron`. Datos en `~/.local/share/quiron-data`; configuración privada
en `~/.config/quiron/quiron-brain.env`. Las credenciales locales son aleatorias.
Antes del chat hay que elegir un agente: al pulsar «Empezar» en un equipo sin
ninguno configurado, la paleta **Agentes** se abre sola (suscripción de Claude
o de ChatGPT por sus CLI, OpenAI o un servidor compatible con clave, un
servidor en la red local u Ollama). No se incluye ninguna sesión o clave
personal. Guía paso a paso para quien evalúa: `docs/GUIA_EVALUACION.md`.

La primera apertura necesita Internet para obtener las dos imágenes fijadas por
digest y BGE-M3. No es un instalador offline ni un AppImage. `BUILD.json` y
`docs/*-ldd.txt` indican el sistema de compilación y bibliotecas necesarias: falta
certificar la compatibilidad en una segunda distribución/VM limpia.

```sh
systemctl --user start quiron-brain
systemctl --user status quiron-brain
journalctl --user -u quiron-brain
systemctl --user stop quiron-brain
```

El servicio no se habilita al iniciar sesión. Cerrar la ventana deja el trabajo
en segundo plano hasta el apagado por inactividad descrito abajo; el comando
`stop` permite detenerlo inmediatamente. Los puertos nuevos se
publican solo en `127.0.0.1`. Los contenedores existentes no se migran ni adoptan
por su puerto. Esta versión rechaza una configuración previa: la migración del
equipo de desarrollo necesita verificar rutas, credenciales y copias de datos.

Para probar la instalación sin cambiar la sesión ni iniciar Docker:

```sh
python3 install.py --destdir /tmp/quiron-install-check
```

Para generar la entrega desde el repositorio: `bash scripts/package-linux-portable.sh`.
Requiere Docker, Rust 1.93.0 y las dependencias locales de Cargo y ONNX Runtime;
prepara la imagen y compila sin red sobre Ubuntu 24.04. La salida queda en
`dist/portable/`. `package-linux.sh` permite compilar directamente en el host,
pero esa variante no fija una ABI compatible con equipos más antiguos.
Se compilan cinco binarios y el cerebro lleva `full` activado.
El archivo `source.tar.gz` contiene el código correspondiente. En `docs/` se
incluyen la memoria en Markdown, DOCX y PDF de revisión, el estudio anexo y el
informe de cierre. Las exportaciones se verifican contra sus fuentes al
empaquetar. La memoria conserva los campos que debe completar el autor.
La revisión completa de licencias transitivas y la certificación en equipo
limpio siguen pendientes antes de publicar. El monitor y las conexiones se
describen en `docs/WORKER_Y_PROVEEDORES.md`.

El paquete incluye `ledger_admin` para verificar y anclar una base histórica con
el servicio detenido y una copia previa. El procedimiento y sus límites están
en `docs/ESTABILIZACION_2026-09-10.md`. No sustituir el cerebro por una versión
anterior sobre datos v2 sin revisar también la restauración de los datos.

## Memoria

Las unidades instaladas llevan topes de cgroup: el cerebro `MemoryHigh=4G`/`MemoryMax=5G`
(BGE-M3 en proceso ocupa unos 3 GB en reposo) y el worker `MemoryHigh=2G`/`MemoryMax=3G`.
Los contenedores se crean con `--memory` (Qdrant 2 GiB, Neo4j 1,5 GiB) y sin swap;
si ya existían, `quiron-stores.sh` les aplica el tope y `restart=no` antes de
arrancarlos. Ajustables con `QUIRON_QDRANT_MEMORY`, `QUIRON_NEO4J_MEMORY` y, en el
worker, `QUIRON_WORKER_CACHE_RAM_MIB` (caché de prompts de llama-server, 256 MiB;
el valor por defecto del servidor son 8 GiB en RAM del host). El modelo del
worker se cambia con `QUIRON_WORKER_MODEL_FILE` (un `.gguf` de su carpeta), que
fija la paleta Agentes del editor o `configure-provider.py worker-model`.

En equipos con 16 GB o menos conviene un protector OOM (`earlyoom` o
`systemd-oomd`): el kernel prefiere paginar a matar, y sin él una sobrecarga deja
la máquina congelada en vez de perder solo el proceso más grande.

## Ciclo de vida

El editor arranca el cerebro al abrirse (`systemctl --user start quiron-brain`),
y el cerebro levanta Qdrant y Neo4j antes de escuchar. Al cerrar el editor nada
se para de inmediato: el cerebro se apaga solo tras `QUIRON_IDLE_EXIT_SECS`
segundos (900 por defecto; `0` lo desactiva) sin peticiones y sin barridos en
curso, y systemd para los almacenes y el worker con él. Mientras un editor está
abierto sondea `/health`, así que en uso nunca se apaga; con varios editores, el
último en cerrarse deja correr la cuenta.
