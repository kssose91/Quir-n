#!/usr/bin/env python3
"""Prepara el vectorizador local: llama.cpp (Vulkan/CPU) y su modelo, con SHA-256 fijos.

Sin opciones descarga el runtime y el modelo de serie (Qwen2.5-Coder 1.5B
Q4_K_M). Con `--model NOMBRE` descarga otro modelo del catálogo a la misma
carpeta (y el runtime si faltara). Con `--catalog` imprime el catálogo en JSON,
marcando cuáles están ya en la carpeta; es lo que lee la paleta Agentes.
"""
import argparse
import hashlib
import json
from pathlib import Path
import tarfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT / 'data/worker'
LLAMA = ('llama-b10816-bin-ubuntu-vulkan-x64.tar.gz',
         'https://github.com/ggml-org/llama.cpp/releases/download/b10816/llama-b10816-bin-ubuntu-vulkan-x64.tar.gz',
         '6a880a63a019c0967373f6f8c98adc63c2c40d91234160e86411e9add2d17ff2')
# Catálogo: Qwen2.5-Coder y Qwen3 en GGUF oficial, revisiones y sumas fijadas
# (API de Hugging Face, 6 de septiembre de 2026). El nombre es el que se pasa
# a --model; el archivo es el que queda en la carpeta del worker.
CATALOGO = [
    # min_vram_gb: VRAM de GPU dedicada con la que va bien (0 = pensado para CPU).
    # La regla general: mejor worker, mejores fichas, más tiempo por archivo.
    {'name': 'qwen2.5-coder-0.5b-q4_k_m', 'file': 'qwen2.5-coder-0.5b-instruct-q4_k_m.gguf',
     'repo': 'Qwen/Qwen2.5-Coder-0.5B-Instruct-GGUF', 'revision': 'ebb2015119c907b064c512bf053e945850b5875f',
     'sha256': '1d9614638d18024d0fbb36575a15f1302a3adf044df10345688ec4f6e1c4ff32', 'size_gb': 0.49,
     'min_vram_gb': 0, 'nota': 'sin GPU (CPU): rápido, fichas más pobres'},
    {'name': 'qwen2.5-coder-1.5b-q4_k_m', 'file': 'qwen2.5-coder-1.5b-instruct-q4_k_m.gguf',
     'repo': 'Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF', 'revision': 'f86cb2c1fa58255f8052cc32aeede1b7482d4361',
     'sha256': 'cc324af070c2ecbfd324a30884d2f951a7ff756aba85cb811a6ec436933bb046', 'size_gb': 1.12,
     'min_vram_gb': 2, 'nota': 'de serie: GPU de 4-6 GB (o CPU, lento)'},
    {'name': 'qwen2.5-coder-1.5b-q8_0', 'file': 'qwen2.5-coder-1.5b-instruct-q8_0.gguf',
     'repo': 'Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF', 'revision': 'f86cb2c1fa58255f8052cc32aeede1b7482d4361',
     'sha256': '507de59046601282ba768a9789900e6ccf60ed93ddf346730b7c68eb0715bc47', 'size_gb': 1.89,
     'min_vram_gb': 3, 'nota': 'misma red con menos pérdida; GPU de 4-6 GB'},
    {'name': 'qwen2.5-coder-3b-q4_k_m', 'file': 'qwen2.5-coder-3b-instruct-q4_k_m.gguf',
     'repo': 'Qwen/Qwen2.5-Coder-3B-Instruct-GGUF', 'revision': 'f74adce6aa16316c625447af059dbebe4983757c',
     'sha256': '724fb256bec1ff062b2f65e4569e871ad2e95ab2a3989723d1769c54294730b7', 'size_gb': 2.10,
     'min_vram_gb': 4, 'nota': 'mejores fichas; GPU de 6 GB'},
    {'name': 'qwen3-4b-q4_k_m', 'file': 'Qwen3-4B-Q4_K_M.gguf',
     'repo': 'Qwen/Qwen3-4B-GGUF', 'revision': 'bc640142c66e1fdd12af0bd68f40445458f3869b',
     'sha256': '7485fe6f11af29433bc51cab58009521f205840f5b4ae3a32fa7f92e8534fdf5', 'size_gb': 2.50,
     'min_vram_gb': 4, 'nota': 'Qwen3: entiende mejor la intención; GPU de 6 GB'},
    {'name': 'qwen2.5-coder-7b-q4_k_m', 'file': 'qwen2.5-coder-7b-instruct-q4_k_m.gguf',
     'repo': 'Qwen/Qwen2.5-Coder-7B-Instruct-GGUF', 'revision': '13fb94bfda8c8cf22497dc57b78f391a9acb426a',
     'sha256': '509287f78cb4d4cf6b3843734733b914b2c158e43e22a7f4bf5e963800894d3c', 'size_gb': 4.68,
     'min_vram_gb': 8, 'nota': 'fichas muy buenas; GPU de 8 GB (en 6 GB va justo)'},
    {'name': 'qwen3-8b-q4_k_m', 'file': 'Qwen3-8B-Q4_K_M.gguf',
     'repo': 'Qwen/Qwen3-8B-GGUF', 'revision': '7c41481f57cb95916b40956ab2f0b139b296d974',
     'sha256': 'd98cdcbd03e17ce47681435b5150e34c1417f50b5c0019dd560e4882c5745785', 'size_gb': 5.03,
     'min_vram_gb': 8, 'nota': 'el techo para GPU dedicada: 8-12 GB; las mejores fichas'},
    # Por encima del techo, solo con memoria unificada (DGX Spark, Mac, Strix
    # Halo): mezcla de expertos con 3B activos, rápido para vectorizar.
    {'name': 'qwen3-coder-30b-a3b-q4_k_m', 'file': 'Qwen3-Coder-30B-A3B-Instruct-Q4_K_M.gguf',
     'repo': 'unsloth/Qwen3-Coder-30B-A3B-Instruct-GGUF', 'revision': 'b17cb02dd882d5b6ab62fc777ad2995f19668350',
     'sha256': 'fadc3e5f8d42bf7e894a785b05082e47daee4df26680389817e2093056f088ad', 'size_gb': 18.56,
     'min_vram_gb': 24, 'nota': 'memoria unificada de 32 GB o más (DGX Spark, Mac, Strix Halo)'},
]
DE_SERIE = CATALOGO[1]


def url_de(entrada):
    return 'https://huggingface.co/%s/resolve/%s/%s' % (entrada['repo'], entrada['revision'], entrada['file'])


def sha(path):
    h = hashlib.sha256()
    with path.open('rb') as f:
        for block in iter(lambda: f.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()


def descargar(directory, name, url, expected):
    path = directory / name
    if path.exists() and sha(path) == expected:
        print('Verificado:', name, flush=True)
        return path
    temp = path.with_suffix(path.suffix + '.partial')
    print('Descargando:', name, flush=True)
    with urllib.request.urlopen(url, timeout=120) as source, temp.open('wb') as dest:
        total = 0
        for block in iter(lambda: source.read(1024 * 1024), b''):
            dest.write(block)
            total += len(block)
            if total % (64 * 1024 * 1024) < 1024 * 1024:
                print('  %d MB' % (total // (1024 * 1024)), flush=True)
    if sha(temp) != expected:
        temp.unlink(missing_ok=True)
        raise SystemExit('Checksum incorrecto: ' + name)
    temp.replace(path)
    return path


def servidor_presente(directory):
    """El llama-server ya extraído en la carpeta, si lo hay (puede estar en marcha)."""
    servers = [p for p in directory.rglob('llama-server') if p.is_file()]
    return servers[0] if len(servers) == 1 else None


def preparar_runtime(directory):
    # No se vuelve a extraer sobre un servidor presente: podría estar en marcha.
    presente = servidor_presente(directory)
    if presente:
        return presente
    descargar(directory, *LLAMA)
    runtime = directory / 'llama-b10816'
    runtime.mkdir(exist_ok=True)
    with tarfile.open(directory / LLAMA[0]) as archive:
        archive.extractall(runtime, filter='data')
    server = servidor_presente(directory)
    if not server:
        raise SystemExit('No se encontró un único llama-server')
    return server


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--directory', type=Path, default=CACHE)
    parser.add_argument('--model', choices=[e['name'] for e in CATALOGO], help='modelo del catálogo a descargar')
    parser.add_argument('--catalog', action='store_true', help='imprime el catálogo en JSON y termina')
    args = parser.parse_args()
    directory = args.directory.resolve()
    if args.catalog:
        print(json.dumps([dict(e, url=url_de(e), presente=(directory / e['file']).is_file()) for e in CATALOGO],
                         ensure_ascii=False))
        return
    directory.mkdir(parents=True, exist_ok=True)
    manifest = directory / 'manifest.json'
    server = preparar_runtime(directory)
    entrada = next(e for e in CATALOGO if e['name'] == args.model) if args.model else DE_SERIE
    descargar(directory, entrada['file'], url_de(entrada), entrada['sha256'])
    if not manifest.is_file() or not args.model:
        metadata = {'model': 'Qwen2.5-Coder-1.5B-Instruct', 'revision': DE_SERIE['revision'],
                    'quantization': 'Q4_K_M', 'llama_cpp': 'b10816',
                    'assets': [{'file': LLAMA[0], 'url': LLAMA[1], 'sha256': LLAMA[2]},
                               {'file': DE_SERIE['file'], 'url': url_de(DE_SERIE), 'sha256': DE_SERIE['sha256']}],
                    'manifest_version': 2, 'server': str(server.relative_to(directory)), 'model_path': DE_SERIE['file']}
        manifest.write_text(json.dumps(metadata, indent=2) + '\n')
    if args.model:
        print('Modelo listo: %s. Elígelo en Agentes → Vectorizador → Modelos (o con configure-provider.py worker-model).'
              % entrada['file'], flush=True)
    else:
        print('Worker preparado. Arranque: bash scripts/run-worker.sh', flush=True)


if __name__ == '__main__':
    main()
