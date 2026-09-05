# Quirón para Linux x86_64 — candidato de instalación

Requisitos: Python 3.12+, Bash, Docker Engine accesible por el usuario, systemd
de usuario, curl y coreutils. Escritorio Wayland o X11. No requiere Rust ni GPU.
BGE-M3 trabaja en CPU. `--with-worker` prepara Qwen 1.5B Q4_K_M y llama.cpp
Vulkan/CPU (1,15 GB de descarga adicional). Para NVIDIA hace falta un controlador
Vulkan funcional; sin NVIDIA se usa CPU. No se incluye entrenamiento.

```sh
tar -xzf quiron-linux-x86_64.tar.gz
cd quiron-linux-x86_64
sha256sum -c SHA256SUMS
python3 install.py --with-worker
```

Instala en `~/.local/share/quiron`, registra `quiron.desktop` y el lanzador
`~/.local/bin/quiron`. Datos en `~/.local/share/quiron-data`; configuración privada
en `~/.config/quiron/quiron-brain.env`. Las credenciales locales son aleatorias.
Antes del chat hay que configurar el backend, endpoint y modelo del proveedor
en ese archivo; no se incluye ninguna sesión o clave personal.

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

El servicio no se habilita al iniciar sesión. Cerrar la ventana todavía no
detiene el servicio: usar el comando `stop` anterior. Los puertos nuevos se
publican solo en `127.0.0.1`. Los contenedores existentes no se migran ni adoptan
por su puerto. Esta versión rechaza una configuración previa: la migración del
equipo de desarrollo necesita verificar rutas, credenciales y copias de datos.

Para probar la instalación sin cambiar la sesión ni iniciar Docker:

```sh
python3 install.py --destdir /tmp/quiron-install-check
```

Para generar el paquete desde el repositorio: `bash scripts/package-linux.sh`.
Se compilan todos los binarios incluidos y el cerebro lleva `full` activado.
El archivo `source.tar.gz` contiene el código correspondiente y la memoria en
Markdown. La revisión de licencias transitivas y la exportación final a PDF/DOCX
siguen pendientes antes de publicar la entrega. El nuevo monitor y las conexiones se describen en
`docs/WORKER_Y_PROVEEDORES.md`.

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
