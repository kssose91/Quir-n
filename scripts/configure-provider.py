#!/usr/bin/env python3
"""Selecciona un proveedor conservando secretos y configuración del worker local."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
from urllib.parse import urlparse


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('provider', choices=['claude-cli', 'codex-direct', 'openai-compatible'])
    parser.add_argument('--model')
    parser.add_argument('--endpoint', help='endpoint del proveedor compatible; no incluye claves')
    parser.add_argument('--config', type=Path, default=Path.home()/'.config/quiron/quiron-brain.env')
    parser.add_argument('--apply', action='store_true', help='guardar y reiniciar el cerebro')
    args = parser.parse_args()
    changes = {'QUIRON_GATEWAY_BACKEND': args.provider.replace('-', '_')}
    if args.provider == 'claude-cli':
        cli = shutil.which('claude') or str(Path.home()/'.local/bin/claude')
        result = subprocess.run([cli,'auth','status'], capture_output=True, text=True, timeout=15)
        status = json.loads(result.stdout)
        if not status.get('loggedIn') or status.get('authMethod') != 'claude.ai':
            raise SystemExit('Inicia sesión de suscripción con: claude auth login')
        changes.update(QUIRON_CLAUDE_CLI=cli, QUIRON_LLM_MODEL_PRIMARY=args.model or 'sonnet')
        print('Claude: sesión de claude.ai disponible. La CLI gestiona la autenticación.')
    elif args.provider == 'codex-direct':
        changes['QUIRON_LLM_MODEL_PRIMARY'] = args.model or 'gpt-5.6-sol'
        print('Codex: adaptador existente. Renueva la sesión mediante codex login si devuelve 401.')
    else:
        if not args.endpoint or not args.model:
            raise SystemExit('Indica --endpoint y --model. La API key se configura aparte en el archivo privado de entorno.')
        url = urlparse(args.endpoint)
        if url.scheme not in ['http','https'] or not url.hostname or url.username or url.password or url.query or url.fragment or '\n' in args.endpoint:
            raise SystemExit('Endpoint inválido o con credenciales. Configura la clave en el archivo privado.')
        changes.update(QUIRON_LLM_ENDPOINT_PRIMARY=args.endpoint, QUIRON_LLM_MODEL_PRIMARY=args.model)
    print('Configuración propuesta: '+json.dumps(changes,ensure_ascii=False))
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
