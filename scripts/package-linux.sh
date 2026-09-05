#!/usr/bin/env bash
# Compila siempre el árbol actual; bin/ puede contener una interfaz antigua.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
[[ "$(uname -sm)" == 'Linux x86_64' ]] || { echo 'Solo Linux x86_64' >&2; exit 1; }
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
OUT="${QUIRON_PACKAGE_DIR:-${ROOT}/dist}"
mkdir -p "$OUT"
STAGE="$(mktemp -d "${OUT}/.package-XXXXXX")"
trap 'rm -rf "$STAGE"' EXIT
PKG="$STAGE/quiron-linux-x86_64"
mkdir -p "$PKG/bin" "$PKG/scripts" "$PKG/assets" "$PKG/docs" "$PKG/licenses"

cargo build --locked --release --manifest-path "$ROOT/Quirón/llore_editor/Cargo.toml" --target-dir "$ROOT/Quirón/llore_editor/target" -p llore_ui --bin llore_gui
cargo build --locked --release --manifest-path "$ROOT/Quirón/quiron-brain/Cargo.toml" --target-dir "$ROOT/Quirón/quiron-brain/target" --features full --bin quiron-brain --bin index_repo
cargo build --locked --release --manifest-path "$ROOT/Quirón/vertex-gateway/Cargo.toml" --target-dir "$ROOT/Quirón/vertex-gateway/target" --bin vertex-gateway
install -m755 "$ROOT/Quirón/llore_editor/target/release/llore_gui" "$PKG/bin/"
install -m755 "$ROOT/Quirón/quiron-brain/target/release/quiron-brain" "$ROOT/Quirón/quiron-brain/target/release/index_repo" "$PKG/bin/"
install -m755 "$ROOT/Quirón/vertex-gateway/target/release/vertex-gateway" "$PKG/bin/"
install -m755 "$ROOT/scripts/quiron-stores.sh" "$ROOT/scripts/setup-worker.py" "$ROOT/scripts/run-worker.sh" "$ROOT/scripts/configure-provider.py" "$PKG/scripts/"
install -m755 "$ROOT/deploy/install-linux.py" "$PKG/install.py"
install -m644 "$ROOT/Quirón/llore_editor/assets/llore.svg" "$PKG/assets/quiron.svg"
install -m644 "$ROOT/deploy/LINUX.md" "$PKG/README.md"
install -m644 "$ROOT/docs/WORKER_Y_PROVEEDORES.md" "$ROOT/docs/GUIA_EVALUACION.md" "$PKG/docs/"
install -m755 "$ROOT/scripts/demo-consulta-codigo.py" "$ROOT/scripts/demo-agentes.py" "$ROOT/scripts/smoke-gui-x11.py" "$PKG/scripts/"
mkdir -p "$PKG/scripts/tests" && install -m755 "$ROOT/scripts/tests/mock-openai.py" "$PKG/scripts/tests/"
install -m644 "$ROOT/memoria/borrador-memoria-tfm.md" "$PKG/docs/"
install -m644 "$ROOT/Quirón/llore_editor/crates/llore_ui/assets/fonts/"*.txt "$PKG/licenses/"
# Inventario y dependencias reales del equipo constructor: no promete ABI universal.
python3 - "$ROOT" "$PKG" <<'PY'
import json, pathlib, subprocess, sys
root, pkg = map(pathlib.Path, sys.argv[1:])
run = lambda *a: subprocess.check_output(a, cwd=root, text=True).strip()
metadata = {"commit": run("git", "rev-parse", "HEAD"),
            "modified_tree": bool(run("git", "status", "--porcelain")),
            "rustc": run("rustc", "--version"), "platform": run("uname", "-sm"),
            "libc": run("getconf", "GNU_LIBC_VERSION"),
            "profile": "release", "brain_features": ["full"]}
for binary in sorted((pkg / "bin").iterdir()):
    deps = run("ldd", str(binary))
    if "not found" in deps:
        raise SystemExit(f"Dependencias ausentes: {binary.name}\n{deps}")
    (pkg / "docs" / (binary.name + "-ldd.txt")).write_text(deps + "\n")
(pkg / "BUILD.json").write_text(json.dumps(metadata, indent=2) + "\n")
# Código correspondiente: lista explícita, sin datos, cachés ni configuración personal.
import tarfile
tracked = run("git", "ls-files", "--cached", "--others", "--exclude-standard", "-z").split("\0")
with tarfile.open(pkg / "source.tar.gz", "w:gz") as tar:
    for name in tracked:
        p = root / name
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
tar -C "$STAGE" -czf "$OUT/quiron-linux-x86_64.tar.gz" quiron-linux-x86_64
(cd "$OUT" && sha256sum quiron-linux-x86_64.tar.gz > quiron-linux-x86_64.tar.gz.sha256)
printf 'Paquete: %s/quiron-linux-x86_64.tar.gz\n' "$OUT"
