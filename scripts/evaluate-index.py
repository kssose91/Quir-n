#!/usr/bin/env python3
"""Evaluación exploratoria del índice: localización y volumen de contexto.

Solo consulta el índice salvo --start-index, que registra el monitor del proyecto.
No llama a un asistente remoto, no altera fuentes y no representa ahorro facturado.
Requiere blake3. Tokenización mediante /tokenize del llama-server local configurado.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shlex
import statistics
import subprocess
import time
import urllib.parse
import urllib.request
from datetime import datetime, timezone

import blake3

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=ROOT)
    parser.add_argument('--dataset', type=Path, default=ROOT/'docs/evaluacion/localizacion-v1.json')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--start-index', action='store_true')
    parser.add_argument('--wait-seconds', type=int, default=1200)
    args = parser.parse_args()
    root = args.root.resolve()
    project = (root/'.quiron/project.id').read_text().strip()
    env = {}
    config = Path(os.environ.get('QUIRON_BRAIN_ENV_FILE', Path.home()/'.config/quiron/quiron-brain.env'))
    for line in config.read_text().splitlines():
        if '=' in line and not line.lstrip().startswith('#'):
            key, value = line.split('=', 1)
            env[key] = shlex.split(value)[0] if value.strip() else ''
    base = 'http://127.0.0.1:'+env.get('QUIRON_PORT', '8766')
    worker_url = env.get('QUIRON_LOCAL_WORKER_URL', 'http://127.0.0.1:8092').rstrip('/')
    parsed = urllib.parse.urlsplit(worker_url)
    if parsed.scheme != 'http' or parsed.hostname not in ('127.0.0.1', 'localhost', '::1'):
        raise SystemExit('La evaluación solo usa un tokenizador local')
    dataset_bytes = args.dataset.read_bytes()
    dataset = json.loads(dataset_bytes)
    report = {
        'started_utc': datetime.now(timezone.utc).isoformat(), 'project_id': project,
        'commit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip(),
        'dataset_sha256': hashlib.sha256(dataset_bytes).hexdigest(),
        'protocol': dataset, 'cases': [], 'limits': [
            'Muestra de desarrollo sin holdout: diez objetivos primarios, no todos los símbolos pertinentes.',
            'Hit@5/MRR@5 de localización, no precisión de resúmenes ni éxito de tareas completas.',
            'Volcado de fuentes Rust como referencia de volumen, no como agente competidor.',
            'Tokens del worker local, no facturación ni tokenizador de Claude/Codex.',
            'No incluye historial, prompt del asistente, herramientas, respuesta o lecturas posteriores.',
        ],
    }

    def save():
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2)+'\n')

    def call(url, body=None):
        req = urllib.request.Request(url, data=json.dumps(body).encode() if body is not None else None,
            headers={'Authorization': 'Bearer '+env['QUIRON_API_TOKEN'], 'Content-Type': 'application/json'})
        with urllib.request.urlopen(req, timeout=180) as response:
            return json.load(response)

    def token_count(text):
        return len(call(worker_url+'/tokenize', {'content': text, 'add_special': False})['tokens'])

    try:
        save()  # Protocolo y etiquetas persistidos antes de consultar.
        report['health'] = call(base+'/health')
        if args.start_index:
            call(base+'/index/project', {'root': str(root), 'project_id': project})
            deadline = time.monotonic()+args.wait_seconds
            last_print = 0
            while True:
                status = call(base+'/index/project/'+project)
                if time.monotonic()-last_print > 30:
                    print(json.dumps({'index': status}, ensure_ascii=False), flush=True)
                    last_print = time.monotonic()
                if status['phase'] == 'watching' and status['files_done'] == status['files_total']:
                    report['index_before'] = status
                    break
                if time.monotonic() >= deadline:
                    raise TimeoutError('Índice sin terminar: '+json.dumps(status))
                time.sleep(2)
        names = subprocess.check_output(['git','ls-files','-z','--','Quirón'], cwd=root).decode().split('\0')
        corpus = {}
        chunks = []
        for name in sorted(names):
            p = root/name
            if not name.endswith('.rs') or '/src/' not in name or not p.is_file() or p.is_symlink():
                continue
            raw = p.read_bytes()
            if len(raw) > 512*1024:
                continue
            text = raw.decode('utf-8')
            corpus[name] = {'bytes':len(raw), 'blake3':blake3.blake3(raw).hexdigest()}
            chunks.append(f'\nFILE: {name}\n{text}')
        report['corpus'] = corpus
        props = call(worker_url+'/props')
        report['tokenizer'] = {
            'endpoint': '/tokenize', 'add_special': False,
            'model_file': Path(props.get('model_path', env.get('QUIRON_WORKER_MODEL_FILE', 'unknown'))).name,
            'model_alias': props.get('model_alias', 'quiron-worker'),
        }
        report['full_source_tokens'] = token_count(''.join(chunks))
        report['source_files'] = len(corpus)
        for case in dataset['cases']:
            start = time.monotonic()
            hits = call(base+'/index/search?'+urllib.parse.urlencode({
                'project_id':project, 'q':case['question'], 'limit':5}))['results']
            elapsed = time.monotonic()-start
            def matches(hit):
                return all(hit.get(k) == v for k,v in case['expected'].items())
            direct = [h for h in hits if 'relation' not in h][:5]
            rank = next((i for i,h in enumerate(direct,1) if matches(h)), None)
            checks = []
            for hit in hits:
                relative = Path(hit['path'])
                path = root/relative
                valid_path = not relative.is_absolute() and '..' not in relative.parts and not path.is_symlink() and path.resolve().is_relative_to(root)
                current = valid_path and path.is_file() and blake3.blake3(path.read_bytes()).hexdigest() == hit['content_hash']
                checks.append({'id':hit['id'], 'project_matches':hit['project']==project, 'hash_matches':bool(current)})
            # Mismo formato JSON que se incorpora como conjunto de fichas al chat.
            compact = json.dumps(hits,ensure_ascii=False,separators=(',', ':'))
            item = {'id':case['id'], 'question':case['question'], 'expected':case['expected'],
                'seconds':elapsed, 'primary_rank_at_5':rank, 'hit_with_expansion':any(matches(h) for h in hits),
                'hint_tokens':token_count(compact), 'hits':hits, 'checks':checks}
            report['cases'].append(item)
            save()
            print(json.dumps({k:item[k] for k in ('id','primary_rank_at_5','hit_with_expansion','hint_tokens','seconds')}),flush=True)
        changed = [name for name, item in corpus.items() if blake3.blake3((root/name).read_bytes()).hexdigest()!=item['blake3']]
        report['corpus_changed_during_evaluation'] = changed
        if changed:
            raise RuntimeError('El corpus cambió durante la evaluación')
        cases = report['cases']
        report['metrics'] = {
            'questions':len(cases),
            'hit_at_5':sum(c['primary_rank_at_5'] is not None for c in cases)/len(cases),
            'mrr_at_5':sum(1/c['primary_rank_at_5'] if c['primary_rank_at_5'] else 0 for c in cases)/len(cases),
            'hit_with_expansion':sum(c['hit_with_expansion'] for c in cases)/len(cases),
            'median_search_seconds':statistics.median(c['seconds'] for c in cases),
            'median_hint_tokens':statistics.median(c['hint_tokens'] for c in cases),
            'median_hint_fraction_of_full_source':statistics.median(c['hint_tokens'] for c in cases)/report['full_source_tokens'],
            'all_returned_sources_valid':all(check['project_matches'] and check['hash_matches'] for c in cases for check in c['checks']),
        }
        report['ok'] = True
    except Exception as error:
        report['ok'] = False
        report['error'] = f'{type(error).__name__}: {error}'
        raise
    finally:
        report['finished_utc'] = datetime.now(timezone.utc).isoformat()
        save()
    print(json.dumps(report['metrics'],ensure_ascii=False,indent=2))


if __name__ == '__main__':
    main()
