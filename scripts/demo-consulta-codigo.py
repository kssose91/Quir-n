#!/usr/bin/env python3
"""Consulta de demostración al índice de código con fuentes verificables.

Ejecuta, contra el cerebro en marcha, tres comprobaciones y deja un JSON:

1. Búsqueda directa (`/index/search`): ruta, símbolo, rango y hash de cada acierto.
2. Control negativo: modifica un archivo limpio del proyecto, comprueba que sus
   unidades desaparecen de la búsqueda (el hash ya no está vigente) y lo restaura
   con git, comprobando que vuelven.
3. Consulta por el chat (`/v1/messages`, misma ruta que el editor): modelo real,
   tokens, duración, fichas inyectadas (`quiron_context.code_hints`) y texto.

Si se indica `--b3sum`, reproduce el hash blake3 de cada archivo citado con esa
herramienta externa y lo compara con el del índice.
"""
import argparse
import json
import pathlib
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

PREGUNTAS = [
    "¿Dónde se comprueba que el hash del archivo sigue vigente antes de devolver un "
    "acierto de la búsqueda de código, y qué ocurre con el acierto si el archivo cambió?",
    "¿Qué backend elige el gateway cuando QUIRON_GATEWAY_BACKEND no está definida, "
    "y qué valores acepta esa variable?",
]
SISTEMA = (
    "Responde en español y de forma breve. Para cada afirmación sobre el código indica "
    "la ficha que la respalda como ruta:línea_inicio-línea_fin y su content_hash. Si "
    "ninguna ficha respalda una afirmación, dilo explícitamente en vez de suponer."
)


def leer_env(path):
    values = {}
    for line in path.read_text().splitlines():
        if "=" in line and not line.lstrip().startswith("#"):
            key, value = line.split("=", 1)
            values[key.strip()] = value.strip().strip('"')
    return values


class Brain:
    def __init__(self, url, token):
        self.url, self.token = url.rstrip("/"), token

    def _call(self, method, path, query=None, body=None, timeout=60):
        url = self.url + path + ("?" + urllib.parse.urlencode(query) if query else "")
        data = json.dumps(body).encode() if body is not None else None
        req = urllib.request.Request(url, data=data, method=method, headers={
            "Authorization": "Bearer " + self.token, "Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=timeout) as r:
                return r.status, json.loads(r.read())
        except urllib.error.HTTPError as e:
            return e.code, {"error": e.read().decode(errors="replace")}

    def search(self, project, q, limit=5):
        status, body = self._call("GET", "/index/search",
                                  {"project_id": project, "q": q, "limit": limit}, timeout=120)
        if status != 200:
            raise SystemExit(f"/index/search devolvió {status}: {body}")
        return body["results"]

    def chat(self, project, model, q):
        t0 = time.time()
        status, body = self._call("POST", "/v1/messages", body={
            "model": model, "project_id": project, "route": "primary", "max_tokens": 1200,
            "system": SISTEMA, "messages": [{"role": "user", "content": q}]}, timeout=300)
        return status, body, round(time.time() - t0, 1)


def resumen(hit):
    campos = ("path", "symbol", "kind", "start_line", "end_line", "content_hash",
              "summary_origin", "partial", "score", "relation", "via")
    return {k: hit[k] for k in campos if k in hit}


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--root", type=pathlib.Path, default=pathlib.Path.cwd())
    parser.add_argument("--env", type=pathlib.Path,
                        default=pathlib.Path.home() / ".config/quiron/quiron-brain.env")
    parser.add_argument("--brain-url", default="http://127.0.0.1:8766")
    parser.add_argument("--model", default="sonnet")
    parser.add_argument("--control-file", default="Quirón/quiron-brain/src/types/artifact.rs",
                        help="archivo limpio en git que se altera y restaura")
    parser.add_argument("--b3sum", type=pathlib.Path,
                        help="binario externo que imprime 'hash  ruta' en blake3")
    parser.add_argument("--sin-chat", action="store_true", help="omite las llamadas al modelo")
    parser.add_argument("--output", type=pathlib.Path, required=True)
    args = parser.parse_args()

    root = args.root.resolve()
    project = (root / ".llore/project.id").read_text().strip()
    brain = Brain(args.brain_url, leer_env(args.env)["QUIRON_API_TOKEN"])
    evidencia = {"fecha": time.strftime("%Y-%m-%dT%H:%M:%S%z"), "proyecto": project,
                 "raiz": str(root), "modelo_pedido": args.model, "consultas": []}

    # 1 y 3: búsqueda directa y chat por cada pregunta.
    for q in PREGUNTAS:
        entrada = {"pregunta": q, "busqueda_directa": [resumen(h) for h in brain.search(project, q)]}
        if not args.sin_chat:
            status, body, segundos = brain.chat(project, args.model, q)
            texto = "\n".join(c.get("text", "") for c in body.get("content", []) if c.get("type") == "text")
            hints = body.get("quiron_context", {}).get("code_hints", [])
            entrada["chat"] = {"http": status, "segundos": segundos, "modelo_real": body.get("model"),
                               "usage": body.get("usage"), "stop_reason": body.get("stop_reason"),
                               "fichas_inyectadas": [resumen(h) for h in hints], "texto": texto,
                               "respuesta_sospechosamente_corta": len(texto.strip()) < 40}
        evidencia["consultas"].append(entrada)

    # 2: control negativo sobre un archivo limpio.
    control = root / args.control_file
    limpio = subprocess.run(["git", "status", "--short", "--", str(control)], cwd=root,
                            capture_output=True, text=True).stdout.strip() == ""
    q = PREGUNTAS[0]
    if limpio:
        antes = [f"{h['path']}:{h['start_line']}-{h['end_line']}" for h in brain.search(project, q)]
        with control.open("a") as f:
            f.write("\n// control negativo\n")
        try:
            alterado = [f"{h['path']}:{h['start_line']}-{h['end_line']}" for h in brain.search(project, q)]
        finally:
            subprocess.run(["git", "checkout", "--", str(control)], cwd=root, check=True)
        despues = [f"{h['path']}:{h['start_line']}-{h['end_line']}" for h in brain.search(project, q)]
        citado = [a for a in antes if a.startswith(args.control_file + ":")]
        evidencia["control_negativo"] = {
            "archivo": args.control_file, "antes": antes, "alterado": alterado, "restaurado": despues,
            "unidades_del_archivo_antes": citado,
            "desaparecen_al_alterar": bool(citado) and not any(a.startswith(args.control_file + ":") for a in alterado),
            "vuelven_al_restaurar": despues == antes}
    else:
        evidencia["control_negativo"] = {"archivo": args.control_file, "omitido": "tiene cambios locales"}

    # Reproducción externa del hash de cada archivo citado.
    if args.b3sum:
        rutas = sorted({h["path"] for c in evidencia["consultas"] for h in c["busqueda_directa"]})
        salida = subprocess.run([str(args.b3sum)] + [str(root / r) for r in rutas],
                                capture_output=True, text=True, check=True).stdout
        externo = {line.split("  ", 1)[1]: line.split("  ", 1)[0] for line in salida.splitlines()}
        evidencia["hash_externo"] = {
            "herramienta": str(args.b3sum),
            "archivos": {r: {"indice": next(h["content_hash"] for c in evidencia["consultas"]
                                             for h in c["busqueda_directa"] if h["path"] == r),
                             "externo": externo.get(str(root / r))} for r in rutas}}
        evidencia["hash_externo"]["todos_coinciden"] = all(
            v["indice"] == v["externo"] for v in evidencia["hash_externo"]["archivos"].values())

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(evidencia, ensure_ascii=False, indent=2) + "\n")
    ok = evidencia.get("control_negativo", {}).get("desaparecen_al_alterar") and \
        evidencia.get("hash_externo", {}).get("todos_coinciden", True)
    print(("OK" if ok else "REVISAR") + ": evidencia en " + str(args.output))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
