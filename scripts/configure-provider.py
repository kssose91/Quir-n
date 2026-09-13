#!/usr/bin/env python3
"""Selecciona un proveedor conservando secretos y configuración del worker local."""
import argparse
import json
import os
from pathlib import Path
import shutil
import sys
import subprocess
import tempfile
from urllib.parse import urlparse


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('provider', choices=['claude-cli', 'codex-cli', 'codex-direct', 'openai-compatible', 'ollama-native', 'worker-model'])
    parser.add_argument('--model')
    parser.add_argument('--reasoning-effort', choices=['low','medium','high','xhigh','max'])
    parser.add_argument('--endpoint', help='endpoint del proveedor compatible; no incluye claves')
    parser.add_argument('--config', type=Path, default=Path.home()/'.config/quiron/quiron-brain.env')
    parser.add_argument('--apply', action='store_true', help='guardar y reiniciar el cerebro')
    parser.add_argument('--api-key-from-stdin', action='store_true', help='lee la clave de API de la entrada estándar (nunca en argumentos)')
    parser.add_argument('--worker-model', type=Path, help='archivo .gguf para el worker local (con provider=worker-model)')
    args = parser.parse_args()
    changes = {} if args.provider == 'worker-model' else {'QUIRON_GATEWAY_BACKEND': args.provider.replace('-', '_')}
    if args.provider == 'claude-cli':
        cli = shutil.which('claude') or str(Path.home()/'.local/bin/claude')
        result = subprocess.run([cli,'auth','status'], capture_output=True, text=True, timeout=15)
        status = json.loads(result.stdout)
        if not status.get('loggedIn') or status.get('authMethod') != 'claude.ai':
            raise SystemExit('Inicia sesión de suscripción con: claude auth login')
        changes.update(QUIRON_CLAUDE_CLI=cli, QUIRON_LLM_MODEL_PRIMARY=args.model or 'sonnet')
        print('Claude: sesión de claude.ai disponible. La CLI gestiona la autenticación.')
    elif args.provider == 'codex-cli':
        cli = shutil.which('codex')
        if not cli:
            candidates = sorted((Path.home()/'.vscode/extensions').glob('openai.chatgpt-*/bin/linux-x86_64/codex'), reverse=True)
            cli = str(candidates[0]) if candidates else None
        if not cli:
            raise SystemExit('Falta Codex CLI. Instala la CLI oficial y ejecuta codex login.')
        changes.update(QUIRON_CODEX_CLI=cli, QUIRON_LLM_MODEL_PRIMARY=args.model or 'gpt-6-astra',
                       QUIRON_CODEX_REASONING_EFFORT=args.reasoning_effort or 'medium')
        print('Codex CLI: sesión oficial; Quirón conserva la ejecución de sus herramientas de lectura.')
    elif args.provider == 'codex-direct':
        changes['QUIRON_LLM_MODEL_PRIMARY'] = args.model or 'gpt-5.6-sol'
        print('Codex: adaptador existente. Renueva la sesión mediante codex login si devuelve 401.')
    elif args.provider == 'ollama-native':
        if not args.model:
            raise SystemExit('Indica --model con el nombre del modelo de Ollama (p. ej. qwen2.5-coder:7b).')
        changes.update(QUIRON_LLM_ENDPOINT_PRIMARY=args.endpoint or 'http://127.0.0.1:11434', QUIRON_LLM_MODEL_PRIMARY=args.model)
        print('Ollama: servidor local; el modelo debe estar descargado con ollama pull.')
    elif args.provider == 'worker-model':
        modelo = args.worker_model
        if not modelo or modelo.suffix != '.gguf' or not modelo.is_file():
            raise SystemExit('Indica --worker-model con un archivo .gguf existente.')
        changes['QUIRON_WORKER_MODEL_FILE'] = str(modelo.resolve())
        print('Worker local: las fichas del modelo nuevo se etiquetan aparte y el proyecto se vuelve a resumir.')
    else:
        if not args.endpoint or not args.model:
            raise SystemExit('Indica --endpoint y --model. La API key se configura aparte en el archivo privado de entorno.')
        url = urlparse(args.endpoint)
        if url.scheme not in ['http','https'] or not url.hostname or url.username or url.password or url.query or url.fragment or '\n' in args.endpoint:
            raise SystemExit('Endpoint inválido o con credenciales. Configura la clave en el archivo privado.')
        changes.update(QUIRON_LLM_ENDPOINT_PRIMARY=args.endpoint, QUIRON_LLM_MODEL_PRIMARY=args.model)
    if args.api_key_from_stdin:
        clave = sys.stdin.readline().strip()
        if clave:
            changes['QUIRON_LLM_API_KEY_PRIMARY'] = clave
    print('Configuración propuesta: '+json.dumps({k: ('***' if 'KEY' in k else v) for k, v in changes.items()},ensure_ascii=False))
    if not args.apply:
        print('Para guardar esta selección: repite el comando con --apply.');return
    path=args.config.expanduser()
    if path.is_symlink() or not path.is_file() or path.stat().st_mode & 0o077:
        raise SystemExit('La configuración debe ser un archivo regular privado (600).')
    original=path.read_text()
    lines=[line for line in original.splitlines() if line.split('=',1)[0].strip() not in changes]
    def quote(value):
        return '"'+str(value).replace('\\','\\\\').replace('"','\\"').replace('$','\\$').replace('`','\\`')+'"'
    lines.extend(k+'='+quote(v) for k,v in changes.items())
    fd, name=tempfile.mkstemp(prefix='.provider-',dir=path.parent)
    try:
        with os.fdopen(fd,'w') as f:f.write('\n'.join(lines)+'\n')
        os.replace(name,path)
    finally:
        Path(name).unlink(missing_ok=True)
    subprocess.run(['systemctl','--user','restart','quiron-brain.service'],check=True)
    print('Proveedor guardado. Reabre el editor para actualizar su selector de modelos.')


if __name__=='__main__':
    main()
