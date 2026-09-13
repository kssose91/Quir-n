#!/usr/bin/env python3
"""Instalación por usuario. --destdir permite comprobarla sin activar servicios."""
import argparse
import ctypes
import hashlib
import os
import platform
from pathlib import Path
import secrets
import shlex
import shutil
import subprocess
import sys


def require_gui_libraries():
    # winit opens these at runtime; ldd alone does not report their absence.
    libraries = ["libxkbcommon.so.0"]
    if os.environ.get("WAYLAND_DISPLAY"):
        libraries += ["libwayland-client.so.0"]
    else:
        libraries += ["libX11.so.6", "libXcursor.so.1", "libX11-xcb.so.1", "libXi.so.6"]
    missing = []
    for library in libraries:
        try:
            ctypes.CDLL(library)
        except OSError:
            missing.append(library)
    if missing:
        raise SystemExit("Faltan bibliotecas del escritorio: " + ", ".join(missing) +
                         ". Consulte las dependencias de X11/Wayland en README.md.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--destdir", type=Path, help="raíz de staging; no ejecuta systemctl")
    parser.add_argument("--with-worker", action="store_true", help="descargar Qwen 1.5B y llama.cpp verificados (1,15 GB)")
    args = parser.parse_args()
    package = Path(__file__).resolve().parent
    home = Path.home()
    root = home / ".local/share/quiron"
    config = home / ".config/quiron/quiron-brain.env"
    data = home / ".local/share/quiron-data"
    unit = home / ".config/systemd/user/quiron-brain.service"
    destdir = args.destdir.resolve() if args.destdir else None

    def dest(p):
        return destdir / p.relative_to("/") if destdir else p

    def write(p, content, mode=0o644):
        target = dest(p)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content)
        target.chmod(mode)

    # Verifica antes de escribir o registrar nada.
    for line in (package / "SHA256SUMS").read_text().splitlines():
        expected, name = line.split("  ", 1)
        path = (package / name).resolve()
        if not path.is_relative_to(package) or not path.is_file():
            raise SystemExit("Ruta de paquete inválida: " + name)
        if hashlib.sha256(path.read_bytes()).hexdigest() != expected:
            raise SystemExit("Checksum incorrecto: " + name)
    for binary in ("llore_gui", "quiron-brain", "vertex-gateway", "index_repo", "ledger_admin"):
        if not (package / "bin" / binary).is_file():
            raise SystemExit("Falta binario: " + binary)
    if dest(config).exists() or dest(unit).exists() or dest(home / ".config/systemd/user/quiron-worker.service").exists():
        raise SystemExit("Ya existe configuración o servicio de Quirón. Esta versión instala en limpio; "
                         "la migración de datos existentes debe revisarse por separado.")
    if not destdir:
        if (platform.system(), platform.machine()) != ("Linux", "x86_64"):
            raise SystemExit("Este paquete requiere Linux x86_64.")
        for command in ("docker", "systemctl", "curl", "timeout", "ldd"):
            if not shutil.which(command):
                raise SystemExit("Falta requisito: " + command)
        for binary in (package / "bin").iterdir():
            check = subprocess.run(["ldd", str(binary)], capture_output=True, text=True)
            if check.returncode or "not found" in check.stdout + check.stderr:
                raise SystemExit("Bibliotecas incompatibles para " + binary.name + ":\n" +
                                 check.stdout + check.stderr)
        require_gui_libraries()
        subprocess.run(["docker", "info"], stdout=subprocess.DEVNULL, check=True)

    dest(root).mkdir(parents=True, exist_ok=True)
    for name in ("bin", "scripts", "assets", "docs", "licenses"):
        shutil.copytree(package / name, dest(root / name), dirs_exist_ok=True)
    for name in ("README.md", "BUILD.json", "source.tar.gz", "SHA256SUMS"):
        shutil.copy2(package / name, dest(root / name))
    dest(data).mkdir(parents=True, exist_ok=True)
    if args.with_worker and not destdir:
        subprocess.run([sys.executable, str(root / "scripts/setup-worker.py"),
                        "--directory", str(data / "worker")], check=True)
    # Sintaxis compartida por EnvironmentFile y shell: valores entre comillas dobles.
    def env_quote(s):
        return '"' + str(s).replace("\\", "\\\\").replace('"', '\\"').replace("$", "\\$").replace("`", "\\`") + '"'

    values = {
        "QUIRON_ROOT": root, "QUIRON_DATA": data / "brain", "QUIRON_PORT": "8766",
        "QUIRON_REQUIRE_AUTH": "true", "QUIRON_API_TOKEN": secrets.token_hex(32),
        "QUIRON_VERTEX_GATEWAY_PATH": root / "bin/vertex-gateway",
        "QUIRON_QDRANT_CONTAINER_NAME": f"quiron-{os.getuid()}-qdrant",
        "QUIRON_NEO4J_CONTAINER_NAME": f"quiron-{os.getuid()}-neo4j",
        "QUIRON_QDRANT_DATA_DIR": data / "qdrant", "QUIRON_NEO4J_DATA_DIR": data / "neo4j",
        "QDRANT_URL": "http://127.0.0.1:6334", "QDRANT_COLLECTION": "quiron_events",
        "QDRANT_WORKER_COLLECTION": "quiron_code_worker_v1",
        "QUIRON_LOCAL_WORKER_DIR": data / "worker",
        "QUIRON_LOCAL_WORKER_URL": "http://127.0.0.1:8092",
        "QDRANT_CODE_COLLECTION": "quiron_code", "NEO4J_URI": "bolt://127.0.0.1:7687",
        "NEO4J_USER": "neo4j", "NEO4J_PASSWORD": secrets.token_hex(24),
        "SEMANTIC_BACKEND": "inprocess", "EMBED_MODEL": "BAAI/bge-m3",
        # Sin endpoint ni modelo: la paleta Agentes del editor se abre sola en el
        # primer arranque y guarda aquí lo que se elija.
        "QUIRON_GATEWAY_BACKEND": "openai_compatible",
    }
    write(config, "# El agente se elige desde la paleta Agentes del editor (o con scripts/configure-provider.py).\n" +
          "\n".join(k + "=" + env_quote(v) for k, v in values.items()) + "\n", 0o600)

    def unit_quote(p):
        return '"' + str(p).replace("%", "%%").replace("\\", "\\\\").replace('"', '\\"') + '"'

    write(unit, f"""[Unit]
Description=Quirón Brain con Qdrant y Neo4j
After=network-online.target
Wants=quiron-worker.service

[Service]
Type=simple
WorkingDirectory={unit_quote(data)}
EnvironmentFile={unit_quote(config)}
ExecStartPre={unit_quote(root / 'scripts/quiron-stores.sh')} up
ExecStart={unit_quote(root / 'bin/quiron-brain')}
ExecStopPost={unit_quote(root / 'scripts/quiron-stores.sh')} down
Restart=no
TimeoutStartSec=240
TimeoutStopSec=60
# Pico medido en el barrido completo del 05-09-2026: 3,4 GB (BGE-M3 en CPU).
# MemoryHigh frena y reclama antes de que MemoryMax mate el servicio.
MemoryHigh=4G
MemoryMax=5G
UMask=0077
""")
    write(home / ".config/systemd/user/quiron-worker.service", f"""[Unit]
Description=Quirón worker local Qwen 1.5B
BindsTo=quiron-brain.service
After=quiron-brain.service
ConditionPathExists={unit_quote(data / 'worker/manifest.json')}
[Service]
Type=simple
WorkingDirectory={unit_quote(root)}
EnvironmentFile={unit_quote(config)}
ExecStart=/bin/bash {unit_quote(root / 'scripts/run-worker.sh')}
Restart=on-failure
RestartSec=10
# Tope de RAM del host: el modelo va a la GPU; la RAM la usan el mapeo del GGUF,
# la caché de prompts (acotada en run-worker.sh) y el runtime.
MemoryHigh=2G
MemoryMax=3G
NoNewPrivileges=true
UMask=0077
""")
    launcher = home / ".local/bin/quiron"
    write(launcher, "#!/usr/bin/env bash\nset -euo pipefail\n" +
          "export QUIRON_BRAIN_ENV_FILE=" + shlex.quote(str(config)) + "\n" +
          "exec " + shlex.quote(str(root / "bin/llore_gui")) + ' "$@"\n', 0o755)
    # Desktop Entry usa sus propias reglas de escape, además de las de Exec.
    desktop_exec = '"' + str(launcher).replace("\\", "\\\\\\\\").replace('"', '\\\\"').replace("`", "\\\\`").replace("$", "\\\\$").replace("%", "%%") + '"'
    write(home / ".local/share/applications/quiron.desktop", f"""[Desktop Entry]
Type=Application
Name=Quirón
Comment=Editor y mapa local de proyectos
Exec={desktop_exec}
Icon=quiron
Terminal=false
Categories=Development;IDE;
StartupWMClass=quiron
""")
    icon = dest(home / ".local/share/icons/hicolor/scalable/apps/quiron.svg")
    icon.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(package / "assets/quiron.svg", icon)
    if not destdir:
        subprocess.run(["systemctl", "--user", "daemon-reload"], check=True)
    print("Instalación preparada en " + str(dest(root)))
    print("Configura proveedor/modelo en " + str(dest(config)))
    print("Los almacenes se descargan y arrancan al abrir Quirón; BGE-M3 se descarga en el primer arranque.")


if __name__ == "__main__":
    try:
        main()
    except (OSError, subprocess.CalledProcessError) as exc:
        sys.exit(str(exc))
