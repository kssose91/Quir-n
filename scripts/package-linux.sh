#!/usr/bin/env bash
# Compila siempre el árbol actual; bin/ puede contener una interfaz antigua.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
[[ "$(uname -sm)" == 'Linux x86_64' ]] || { echo 'Solo Linux x86_64' >&2; exit 1; }
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"
EDITOR_TARGET="$ROOT/Quirón/llore_editor/target"
BRAIN_TARGET="$ROOT/Quirón/quiron-brain/target"
GATEWAY_TARGET="$ROOT/Quirón/vertex-gateway/target"
if [[ -n "${QUIRON_BUILD_DIR:-}" ]]; then
    EDITOR_TARGET="$QUIRON_BUILD_DIR/editor"
    BRAIN_TARGET="$QUIRON_BUILD_DIR/brain"
    GATEWAY_TARGET="$QUIRON_BUILD_DIR/gateway"
fi
OUT="${QUIRON_PACKAGE_DIR:-${ROOT}/dist}"
mkdir -p "$OUT"
STAGE="$(mktemp -d "${OUT}/.package-XXXXXX")"
trap 'rm -rf "$STAGE"' EXIT
PKG="$STAGE/quiron-linux-x86_64"
mkdir -p "$PKG/bin" "$PKG/scripts" "$PKG/assets" "$PKG/docs" "$PKG/licenses"

# Impide entregar exportaciones de una versión distinta de la memoria.
python3 - "$ROOT" <<'PY'
import hashlib, json, pathlib, sys
root = pathlib.Path(sys.argv[1])
report = json.loads((root / "docs/evidencias/2026-09-10/exportacion.json").read_text())
checks = [(root / "memoria/borrador-memoria-tfm.md", report["source_sha256"]),
          (root / "docs/ESTUDIO_RED_OBRERA.md", report["annex_sha256"])]
checks.extend((root / "memoria" / name, digest) for name, digest in report["outputs"].items())
for path, expected in checks:
    if hashlib.sha256(path.read_bytes()).hexdigest() != expected:
        raise SystemExit(f"Exportación desactualizada: {path.name}. Ejecutar scripts/export-memory.py.")
PY

cargo build --locked --release --manifest-path "$ROOT/Quirón/llore_editor/Cargo.toml" --target-dir "$EDITOR_TARGET" -p llore_ui --bin llore_gui
cargo build --locked --release --manifest-path "$ROOT/Quirón/quiron-brain/Cargo.toml" --target-dir "$BRAIN_TARGET" --features full --bin quiron-brain --bin index_repo --bin ledger_admin
cargo build --locked --release --manifest-path "$ROOT/Quirón/vertex-gateway/Cargo.toml" --target-dir "$GATEWAY_TARGET" --bin vertex-gateway
install -m755 "$EDITOR_TARGET/release/llore_gui" "$PKG/bin/"
install -m755 "$BRAIN_TARGET/release/quiron-brain" "$BRAIN_TARGET/release/index_repo" "$BRAIN_TARGET/release/ledger_admin" "$PKG/bin/"
install -m755 "$GATEWAY_TARGET/release/vertex-gateway" "$PKG/bin/"
install -m755 "$ROOT/scripts/quiron-stores.sh" "$ROOT/scripts/setup-worker.py" "$ROOT/scripts/run-worker.sh" "$ROOT/scripts/configure-provider.py" "$PKG/scripts/"
install -m755 "$ROOT/deploy/install-linux.py" "$PKG/install.py"
install -m644 "$ROOT/Quirón/llore_editor/assets/llore.svg" "$PKG/assets/quiron.svg"
install -m644 "$ROOT/deploy/LINUX.md" "$PKG/README.md"
install -m644 "$ROOT/docs/WORKER_Y_PROVEEDORES.md" "$ROOT/docs/GUIA_EVALUACION.md" "$ROOT/docs/MANUAL.md" "$PKG/docs/"
install -m755 "$ROOT/scripts/demo-consulta-codigo.py" "$ROOT/scripts/demo-agentes.py" "$ROOT/scripts/smoke-gui-x11.py" "$PKG/scripts/"
mkdir -p "$PKG/scripts/tests" && install -m755 "$ROOT/scripts/tests/mock-openai.py" "$PKG/scripts/tests/"
install -m644 "$ROOT/memoria/borrador-memoria-tfm.md" "$ROOT/memoria/Borrador memoria TFM - Quirón.docx" "$ROOT/memoria/Memoria TFM - Quirón.pdf" "$PKG/docs/"
install -m644 "$ROOT/docs/ESTUDIO_RED_OBRERA.md" "$ROOT/docs/CIERRE_TFM_2026-09-10.md" "$ROOT/docs/ESTABILIZACION_2026-09-10.md" "$PKG/docs/"
install -m644 "$ROOT/docs/CODEX_EN_QUIRON_2026-09-10.md" "$PKG/docs/"
cp -R "$ROOT/docs/evaluacion" "$ROOT/docs/evidencias" "$PKG/docs/"
install -m644 "$ROOT/Quirón/llore_editor/crates/llore_ui/assets/fonts/"*.txt "$PKG/licenses/"
# Inventario y dependencias reales del equipo constructor: no promete ABI universal.
python3 - "$ROOT" "$PKG" <<'PY'
import json, os, pathlib, re, subprocess, sys
from datetime import datetime, timezone
root, pkg = map(pathlib.Path, sys.argv[1:])
run = lambda *a: subprocess.check_output(a, cwd=root, text=True).strip()
metadata = {"built_utc": datetime.now(timezone.utc).isoformat(),
            "commit": run("git", "rev-parse", "HEAD"),
            "modified_tree": bool(run("git", "status", "--porcelain")),
            "rustc": run("rustc", "--version"), "platform": run("uname", "-sm"),
            "libc": run("getconf", "GNU_LIBC_VERSION"),
            "profile": "release", "brain_features": ["full"],
            "scope": "Candidato de revisión; instalación limpia completa pendiente",
            "binary_abi": {}, "build_image": os.environ.get("QUIRON_BUILD_IMAGE")}
for binary in sorted((pkg / "bin").iterdir()):
    deps = run("ldd", str(binary))
    if "not found" in deps:
        raise SystemExit(f"Dependencias ausentes: {binary.name}\n{deps}")
    (pkg / "docs" / (binary.name + "-ldd.txt")).write_text(deps + "\n")
    versions = set(re.findall(r"GLIBC_([0-9.]+)", run("readelf", "--version-info", str(binary))))
    ordered = sorted(versions, key=lambda s: tuple(map(int, s.split("."))))
    metadata["binary_abi"][binary.name] = {"highest_required_glibc": ordered[-1] if ordered else None}
    ceiling = os.environ.get("QUIRON_GLIBC_CEILING")
    if ceiling and ordered and tuple(map(int, ordered[-1].split("."))) > tuple(map(int, ceiling.split("."))):
        raise SystemExit(f"ABI fuera de la base objetivo: {binary.name} necesita GLIBC_{ordered[-1]}, máximo {ceiling}")
(pkg / "BUILD.json").write_text(json.dumps(metadata, indent=2) + "\n")
# Código correspondiente: lista explícita, sin datos, cachés ni configuración personal.
import tarfile
tracked = run("git", "ls-files", "--cached", "--others", "--exclude-standard", "-z").split("\0")
with tarfile.open(pkg / "source.tar.gz", "w:gz") as tar:
    for name in tracked:
        p = root / name
        # Los apuntes sueltos del autor se conservan en disco, fuera de la entrega.
        if name.startswith("memoria/") and p.suffix not in (".md", ".docx", ".pdf"):
            continue
        if name.startswith(("Quirón/", "memoria/", "docs/", "scripts/", "deploy/")) or name == "README.md":
            if p.is_file() and not p.is_symlink() and not any(x.startswith(".env") for x in p.parts):
                tar.add(p, arcname=name, recursive=False)
    for name in ("scripts/package-linux.sh", "deploy/install-linux.py", "deploy/LINUX.md"):
        if name not in tracked:
            tar.add(root / name, arcname=name, recursive=False)
import hashlib
with (pkg / "SHA256SUMS").open("w") as out:
    for p in sorted(pkg.rglob("*")):
        if p.is_file() and p.name != "SHA256SUMS":
            out.write(hashlib.sha256(p.read_bytes()).hexdigest() + "  " + str(p.relative_to(pkg)) + "\n")
PY
tar -C "$STAGE" -czf "$STAGE/quiron-linux-x86_64.tar.gz" quiron-linux-x86_64
mv "$STAGE/quiron-linux-x86_64.tar.gz" "$OUT/quiron-linux-x86_64.tar.gz"
(cd "$OUT" && sha256sum quiron-linux-x86_64.tar.gz > quiron-linux-x86_64.tar.gz.sha256)
printf 'Paquete: %s/quiron-linux-x86_64.tar.gz\n' "$OUT"
