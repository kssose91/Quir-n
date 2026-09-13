"""Pruebas de fallos de arranque e instalación aislada, sin Docker real."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]

DOCKER = '''#!/usr/bin/env python3
import json, os, pathlib, sys
args = sys.argv[1:]
root = pathlib.Path(os.environ["MOCK_STATE"])
with (root / "commands").open("a") as f: f.write(json.dumps(args) + "\\n")
if args[0] == "info": sys.exit(int(os.environ.get("MOCK_NO_DOCKER", "0")))
if args[:2] == ["container", "inspect"]: sys.exit(0 if (root / args[-1]).exists() else 1)
if args[0] == "inspect":
    print("true" if (root / args[-1]).exists() else "false")
    sys.exit(0)
if args[0] == "run":
    (root / args[args.index("--name") + 1]).touch()
if args[0] == "exec": sys.exit(int(os.environ.get("MOCK_BOLT_FAIL", "0")))
if args[0] == "stop": (root / args[-1]).unlink(missing_ok=True)
'''


class StoreTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.env = dict(os.environ, PATH=str(self.root) + os.pathsep + os.environ["PATH"],
                        MOCK_STATE=str(self.root), NEO4J_PASSWORD="test-password",
                        QUIRON_ROOT=str(self.root), QUIRON_STORES_TIMEOUT="1")
        for key in ("QUIRON_QDRANT_CONTAINER_NAME", "QUIRON_NEO4J_CONTAINER_NAME",
                    "QUIRON_QDRANT_DATA_DIR", "QUIRON_NEO4J_DATA_DIR"):
            self.env.pop(key, None)
        for name, text in {"docker": DOCKER,
                           "curl": '#!/bin/sh\nexit "${MOCK_HTTP_FAIL:-0}"\n'}.items():
            p = self.root / name
            p.write_text(text)
            p.chmod(0o755)

    def run_stores(self, action, **extra):
        return subprocess.run(["bash", str(ROOT / "scripts/quiron-stores.sh"), action],
                              env=dict(self.env, **extra), capture_output=True, text=True, timeout=15)

    def commands(self):
        return [json.loads(s) for s in (self.root / "commands").read_text().splitlines()]

    def test_nuevos_almacenes_solo_locales_y_con_imagen_fija(self):
        result = self.run_stores("up")
        self.assertEqual(result.returncode, 0, result.stderr)
        runs = [c for c in self.commands() if c[0] == "run"]
        self.assertEqual(len(runs), 2)
        for cmd in runs:
            self.assertIn("@sha256:", cmd[-1])
            self.assertEqual(cmd[cmd.index("--restart") + 1], "no")
            self.assertEqual(cmd[cmd.index("--memory") + 1], cmd[cmd.index("--memory-swap") + 1])
            self.assertTrue(all(cmd[i + 1].startswith("127.0.0.1:")
                                for i, arg in enumerate(cmd) if arg == "-p"))
            self.assertNotIn("test-password", " ".join(cmd))
        self.assertTrue(any(c[0] == "exec" and "cypher-shell" in c for c in self.commands()))

    def test_contenedor_heredado_recibe_tope_y_sin_reinicio_antes_de_arrancar(self):
        (self.root / "quiron-qdrant").touch()
        (self.root / "quiron-neo4j").touch()
        result = self.run_stores("up", QUIRON_NEO4J_MEMORY="1g")
        self.assertEqual(result.returncode, 0, result.stderr)
        cmds = self.commands()
        self.assertFalse(any(c[0] == "run" for c in cmds))
        updates = {c[-1]: c for c in cmds if c[0] == "update"}
        self.assertEqual(set(updates), {"quiron-qdrant", "quiron-neo4j"})
        for name, cmd in updates.items():
            self.assertEqual(cmd[cmd.index("--restart") + 1], "no")
            self.assertEqual(cmd[cmd.index("--memory") + 1], cmd[cmd.index("--memory-swap") + 1])
        self.assertEqual(updates["quiron-neo4j"][updates["quiron-neo4j"].index("--memory") + 1], "1g")
        first_update = next(i for i, c in enumerate(cmds) if c[0] == "update")
        self.assertFalse(any(c[0] in ("start", "exec") for c in cmds[:first_update]))

    def test_http_500_no_se_anuncia_como_listo(self):
        result = self.run_stores("up", MOCK_HTTP_FAIL="22")
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("Almacenes listos", result.stdout)

    def test_bolt_o_credencial_incorrectos_impiden_exito(self):
        result = self.run_stores("up", MOCK_BOLT_FAIL="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Neo4j no está listo", result.stderr)

    def test_sin_docker_no_crea_ni_anuncia_almacenes(self):
        result = self.run_stores("up", MOCK_NO_DOCKER="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.commands(), [["info"]])

    def test_stop_no_descubre_ni_adopta_contenedores_ajenos(self):
        (self.root / "unrelated-db").touch()
        (self.root / "quiron-qdrant").touch()
        result = self.run_stores("down")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.root / "unrelated-db").exists())
        self.assertFalse(any(c[0] == "ps" for c in self.commands()))
        self.assertEqual([c[-1] for c in self.commands() if c[0] == "stop"], ["quiron-qdrant"])

    def test_rechaza_password_vacio_antes_de_crear(self):
        result = self.run_stores("up", NEO4J_PASSWORD="")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(any(c[0] == "run" for c in self.commands()))


class InstallTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="quiron instalación ")
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.pkg = self.root / "package"
        self.pkg.mkdir()
        shutil.copy2(ROOT / "deploy/install-linux.py", self.pkg / "install.py")
        for d in ("bin", "scripts", "assets", "docs", "licenses"):
            (self.pkg / d).mkdir()
        for b in ("llore_gui", "quiron-brain", "index_repo", "vertex-gateway", "ledger_admin"):
            p = self.pkg / "bin" / b
            p.write_text("#!/bin/sh\nexit 0\n")
            p.chmod(0o755)
        for name in ("assets/quiron.svg", "README.md", "BUILD.json", "source.tar.gz"):
            (self.pkg / name).write_text("fixture")
        lines = [hashlib.sha256(p.read_bytes()).hexdigest() + "  " + str(p.relative_to(self.pkg))
                 for p in self.pkg.rglob("*") if p.is_file()]
        (self.pkg / "SHA256SUMS").write_text("\n".join(lines) + "\n")
        self.staging = self.root / "staging"

    def install(self):
        return subprocess.run(["python3", str(self.pkg / "install.py"), "--destdir", str(self.staging)],
                              capture_output=True, text=True, timeout=10)

    def test_instalacion_aislada_y_credenciales_privadas(self):
        r = self.install()
        self.assertEqual(r.returncode, 0, r.stderr)
        home = self.staging / Path.home().relative_to("/")
        config = home / ".config/quiron/quiron-brain.env"
        self.assertEqual(config.stat().st_mode & 0o777, 0o600)
        self.assertIn("QUIRON_REQUIRE_AUTH=\"true\"", config.read_text())
        unit = (home / ".config/systemd/user/quiron-brain.service").read_text()
        self.assertNotIn("target/release", unit)
        self.assertNotIn(str(self.staging), unit)
        self.assertIn("MemoryMax=", unit)
        self.assertIn("MemoryMax=", (home / ".config/systemd/user/quiron-worker.service").read_text())
        desktop = (home / ".local/share/applications/quiron.desktop").read_text()
        self.assertIn("StartupWMClass=quiron", desktop)
        self.assertTrue((home / ".local/bin/quiron").is_file())
        original = config.read_bytes()
        self.assertNotEqual(self.install().returncode, 0)
        self.assertEqual(config.read_bytes(), original)

    def test_paquete_corrupto_no_escribe_instalacion(self):
        (self.pkg / "bin/llore_gui").write_text("corrupt")
        r = self.install()
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("Checksum", r.stderr)
        self.assertFalse(self.staging.exists())


if __name__ == "__main__":
    unittest.main()
