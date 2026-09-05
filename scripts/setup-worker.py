#!/usr/bin/env python3
"""Descarga solo Qwen 1.5B Q4_K_M y llama.cpp Vulkan/CPU, con versiones y SHA256 fijos."""
import argparse
import hashlib
import json
from pathlib import Path
import tarfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT / 'data/worker'
MODEL = 'qwen2.5-coder-1.5b-instruct-q4_k_m.gguf'
REVISION = 'f86cb2c1fa58255f8052cc32aeede1b7482d4361'
ASSETS = [
    ('llama-b10816-bin-ubuntu-vulkan-x64.tar.gz',
     'https://github.com/ggml-org/llama.cpp/releases/download/b10816/llama-b10816-bin-ubuntu-vulkan-x64.tar.gz',
     '6a880a63a019c0967373f6f8c98adc63c2c40d91234160e86411e9add2d17ff2'),
    (MODEL, f'https://huggingface.co/Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF/resolve/{REVISION}/{MODEL}',
     'cc324af070c2ecbfd324a30884d2f951a7ff756aba85cb811a6ec436933bb046'),
]

def sha(path):
    h = hashlib.sha256()
    with path.open('rb') as f:
        for block in iter(lambda: f.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()


def main():
    global CACHE
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--directory', type=Path, default=CACHE)
    CACHE = parser.parse_args().directory.resolve()
    CACHE.mkdir(parents=True, exist_ok=True)
    for name, url, expected in ASSETS:
        path = CACHE / name
        if path.exists() and sha(path) == expected:
            print('Verificado:', name, flush=True)
            continue
        temp = path.with_suffix(path.suffix + '.partial')
        print('Descargando:', name, flush=True)
        with urllib.request.urlopen(url, timeout=120) as source, temp.open('wb') as dest:
            for block in iter(lambda: source.read(1024 * 1024), b''):
                dest.write(block)
        if sha(temp) != expected:
            raise SystemExit('Checksum incorrecto: ' + name)
        temp.replace(path)
    runtime = CACHE / 'llama-b10816'
    runtime.mkdir(exist_ok=True)
    with tarfile.open(CACHE / ASSETS[0][0]) as archive:
        archive.extractall(runtime, filter='data')
    servers = list(runtime.rglob('llama-server'))
    if len(servers) != 1:
        raise SystemExit('No se encontró un único llama-server')
    metadata = {'model': 'Qwen2.5-Coder-1.5B-Instruct', 'revision': REVISION,
                'quantization': 'Q4_K_M', 'llama_cpp': 'b10816',
                'assets': [{'file': a[0], 'url': a[1], 'sha256': a[2]} for a in ASSETS],
                'manifest_version': 2, 'server': str(servers[0].relative_to(CACHE)), 'model_path': MODEL}
    (CACHE / 'manifest.json').write_text(json.dumps(metadata, indent=2) + '\n')
    print('Worker preparado. Arranque: bash scripts/run-worker.sh', flush=True)


if __name__ == '__main__':
    main()
