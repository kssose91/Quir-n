#!/usr/bin/env python3
"""Prueba real, aislada por IDs aleatorios: fichas, vectores, grafo, cambios y borrados."""
import argparse
import base64
import json
import os
from pathlib import Path
import secrets
import shlex
import shutil
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request


def load_env():
    values = {}
    path = Path(os.environ.get('QUIRON_BRAIN_ENV_FILE', Path.home()/'.config/quiron/quiron-brain.env'))
    for line in path.read_text().splitlines():
        if line.strip() and not line.lstrip().startswith('#') and '=' in line:
            k, v = line.split('=', 1)
            values[k] = shlex.split(v)[0] if v.strip() else ''
    return values


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    env = load_env()
    token = env['QUIRON_API_TOKEN']
    base = 'http://127.0.0.1:' + env.get('QUIRON_PORT', '8766')

    def request(url, body=None, method=None, headers=None):
        data = None if body is None else json.dumps(body).encode()
        req = urllib.request.Request(url, data=data, method=method,
            headers={'Content-Type':'application/json', **(headers or {})})
        with urllib.request.urlopen(req, timeout=120) as r:
            data = r.read()
            return json.loads(data) if data else None

    def api(path, body=None, method=None):
        return request(base+path, body, method, {'Authorization':'Bearer '+token})

    def graph_query(statement, parameters):
        auth = base64.b64encode((env.get('NEO4J_USER','neo4j')+':'+env['NEO4J_PASSWORD']).encode()).decode()
        value = request('http://127.0.0.1:7474/db/neo4j/tx/commit', {'statements':[{
            'statement':statement, 'parameters':parameters}]},
            headers={'Authorization':'Basic '+auth})
        assert not value['errors'], value['errors']
        return [item['row'] for item in value['results'][0]['data']]

    def graph_count(project):
        row = graph_query('MATCH (u:CodeUnit {project:$p}) RETURN count(u), collect(DISTINCT u.summary_origin)', {'p':project})[0]
        if row[0]: assert row[1] and all(v in ['model','parser'] for v in row[1]), row
        return row[0]

    def wait_index(project, predicate=lambda p: True, timeout=240):
        deadline = time.monotonic()+timeout
        while time.monotonic()<deadline:
            p = api('/index/project/'+project)
            if p['phase']=='error': raise AssertionError(p['error'])
            if p['phase']=='watching' and predicate(p): return p
            time.sleep(1)
        raise AssertionError('Tiempo agotado: '+json.dumps(p))

    alphabet='0123456789ABCDEFGHJKMNPQRSTVWXYZ'
    def identity():
        return '0'+''.join(secrets.choice(alphabet) for _ in range(25))

    # systemd Type=simple puede estar activo mientras BGE-M3 aún se carga.
    deadline=time.monotonic()+120
    for endpoint in [base+'/health','http://127.0.0.1:8092/health']:
        while True:
            try:
                request(endpoint);break
            except urllib.error.URLError:
                if time.monotonic()>deadline:raise
                time.sleep(1)

    results={'model':'Qwen2.5-Coder-1.5B-Instruct Q4_K_M','embeddings':'BGE-M3','checks':{}}
    projects=[]
    try:
        # Sin token ni siquiera se acepta la raíz.
        try:
            request(base+'/index/project', {'root':'/','project_id':identity()})
            raise AssertionError('Índice accesible sin token')
        except urllib.error.HTTPError as e:
            assert e.code==401
        results['checks']['requires_auth']=True
        try:
            request('http://127.0.0.1:8092/v1/chat/completions', {'model':'quiron-worker','messages':[]})
            raise AssertionError('Modelo accesible sin token')
        except urllib.error.HTTPError as e:
            assert e.code==401
        results['checks']['requires_worker_auth']=True
        for n in range(2):
            root=Path(tempfile.mkdtemp(prefix='quiron-worker-smoke-'))
            project=identity();projects.append((root,project))
            (root/'.quiron').mkdir();(root/'.quiron/project.id').write_text(project)
            (root/'math.rs').write_text('pub fn total(prices: &[f64], discount: f64) -> Result<f64, &\'static str> {\n    if !(0.0..=1.0).contains(&discount) { return Err("invalid discount"); }\n    Ok(prices.iter().sum::<f64>() * (1.0 - discount))\n}\n')
            (root/'.env').write_text('TEST_SECRET=must_not_be_indexed')
            (root/'linked.rs').symlink_to('/etc/hostname')
            t=time.monotonic();api('/index/project',{'root':str(root),'project_id':project})
            progress=wait_index(project)
            assert progress['files_total']==1 and progress['units_written']==2, progress
            assert graph_count(project)==2
            hits=api('/index/search?'+urllib.parse.urlencode({'project_id':project,'q':'calcular total con descuento'}))['results']
            assert hits and all(h['project']==project and h['path']=='math.rs' for h in hits)
            assert any(h['summary_origin']=='model' for h in hits), 'Ninguna ficha neuronal aceptada en la muestra simple'
            results.setdefault('initial',[]).append({'seconds':round(time.monotonic()-t,2),'progress':progress,'hits':hits})
        results['checks'].update(project_isolation=True, qdrant_retrieval=True, neo4j_projection=True, secrets_and_symlinks_excluded=True)
        # Both projects have identical paths/content. A hash check alone cannot
        # reject an incorrect edge into the other project or an obsolete model.
        first, second = projects[0][1], projects[1][1]
        second_ids = {h['id'] for h in results['initial'][1]['hits']}
        graph_query('MATCH (a:CodeUnit {project:$a}), (b:CodeUnit {project:$b}) MERGE (a)-[:CALLS]->(b)', {'a':first,'b':second})
        hits = api('/index/search?'+urllib.parse.urlencode({'project_id':first,'q':'calcular total con descuento','limit':1}))['results']
        assert not any(h['id'] in second_ids for h in hits), 'Expansión fuera del proyecto'
        results['checks']['graph_neighbor_project_filter']=True
        original_record = results['initial'][0]['hits'][0]
        probe = dict(original_record, id='probe-'+first, partial=True,
                     summary_model='obsolete-model', summary_json=json.dumps(original_record['summary']))
        probe.pop('summary',None);probe.pop('score',None)
        graph_query('CREATE (v:CodeUnit) SET v=$props WITH v MATCH (u:CodeUnit {project:$p}) WHERE u.id<>v.id MERGE (u)-[:CALLS]->(v)', {'props':probe,'p':first})
        try:
            hits=api('/index/search?'+urllib.parse.urlencode({'project_id':first,'q':'calcular total con descuento','limit':1}))['results']
            assert not any(h['id']==probe['id'] for h in hits), 'Expansión con modelo obsoleto'
            results['checks']['graph_neighbor_model_filter']=True
            graph_query('MATCH (v:CodeUnit {id:$id}) SET v.summary_model=$model', {'id':probe['id'],'model':original_record['summary_model']})
            hits=api('/index/search?'+urllib.parse.urlencode({'project_id':first,'q':'calcular total con descuento','limit':1}))['results']
            expanded=next(h for h in hits if h['id']==probe['id'])
            assert expanded['partial'] is True and isinstance(expanded['summary'],dict)
            results['checks']['graph_preserves_partial_and_summary_metadata']=True
        finally:
            graph_query('MATCH (v:CodeUnit {id:$id, project:$p}) DETACH DELETE v',{'id':probe['id'],'p':first})
        root,project=projects[0]
        original=api('/index/project/'+project)['summaries_generated']
        time.sleep(12)
        assert api('/index/project/'+project)['summaries_generated']==original
        results['checks']['unchanged_skips_inference']=True
        # Un resultado antiguo no es retornable mientras se recalcula el archivo.
        (root/'math.rs').write_text('pub fn count_items(prices: &[f64]) -> usize { prices.len() }\n')
        hits=api('/index/search?'+urllib.parse.urlencode({'project_id':project,'q':'descuento'}))['results']
        assert not any(h['symbol']=='total' for h in hits)
        progress=wait_index(project,lambda p:p['summaries_generated']>original)
        hits=api('/index/search?'+urllib.parse.urlencode({'project_id':project,'q':'contar precios'}))['results']
        assert any(h['symbol']=='count_items' for h in hits) and all(h['symbol']!='total' for h in hits)
        assert graph_count(project)==2 and graph_count(projects[1][1])==2
        results['checks'].update(stale_results_rejected=True, changed_logic_replaced=True)
        (root/'math.rs').unlink()
        wait_index(project,lambda p:p['files_total']==0)
        assert graph_count(project)==0 and graph_count(projects[1][1])==2
        assert not api('/index/search?'+urllib.parse.urlencode({'project_id':project,'q':'precios'}))['results']
        results['checks']['deletion_scoped_to_project']=True
        results['ok']=True
    finally:
        # Retirar solo las unidades de nuestras carpetas; nunca borrar colecciones.
        for root,project in projects:
            (root/'math.rs').unlink(missing_ok=True)
            try:
                wait_index(project,lambda p:p['files_total']==0,timeout=30)
                api('/index/project/'+project,method='DELETE')
                shutil.rmtree(root)
            except Exception as e:
                results.setdefault('cleanup_errors',[]).append(str(e))
        args.output.parent.mkdir(parents=True,exist_ok=True)
        args.output.write_text(json.dumps(results,ensure_ascii=False,indent=2)+'\n')
    print(json.dumps({'ok':results.get('ok',False),'checks':results['checks'],'evidence':str(args.output)},ensure_ascii=False))


if __name__=='__main__':
    main()
