#!/usr/bin/env python3
"""Prueba de evaluación: cada agente de la paleta, de punta a punta.

Hace lo que haría quien evalúa la aplicación: elegir un agente en la paleta
Agentes y preguntar en el chat. La elección se aplica con el mismo script y los
mismos argumentos que usa la paleta (`configure-provider.py … --apply`) y la
pregunta se teclea en el editor real con el arnés gráfico (`smoke-gui-x11.py`).

Escenarios (`--only` para elegir):

  compatible  OpenAI/compatible con clave, contra un servidor simulado que exige
              la clave y pide una herramienta (scripts/tests/mock-openai.py).
  red-local   servidor compatible en la red: el llama-server del worker (:8092),
              con su clave (el token del cerebro), como un servidor de la LAN.
  ollama      Ollama simulado en :11434 (descubrimiento por /api/tags, tarjeta
              en la paleta y chat por su API compatible).
  codex       suscripción de ChatGPT vía Codex (real, si hay sesión).
  claude      suscripción de Claude por su CLI (real, si hay sesión).

Guarda una copia del archivo privado antes de empezar y lo restaura al final
(reiniciando el cerebro), pase lo que pase. Deja capturas y `resumen.json` en
`--output-dir`. Nunca escribe claves: en el resumen solo consta si llegaron.
"""
import argparse, json, os, pathlib, secrets, shutil, socket, subprocess, sys, time, urllib.request

RAIZ = pathlib.Path(__file__).resolve().parents[1]
PREGUNTA = 'que hace la funcion ensure current y en que archivo esta'


def leer_env(path):
    valores = {}
    for linea in path.read_text().splitlines():
        if '=' in linea and not linea.lstrip().startswith('#'):
            k, v = linea.split('=', 1)
            valores[k.strip()] = v.strip().strip('"')
    return valores


def puerto_libre(puerto):
    with socket.socket() as s:
        return s.connect_ex(('127.0.0.1', puerto)) != 0


def esperar(condicion, segundos, que):
    t0 = time.monotonic()
    while time.monotonic() - t0 < segundos:
        if condicion():
            return
        time.sleep(1)
    raise SystemExit('tiempo agotado esperando ' + que)


def cerebro_sano(url, token):
    def ok():
        try:
            req = urllib.request.Request(url + '/health', headers={'Authorization': 'Bearer ' + token})
            with urllib.request.urlopen(req, timeout=3) as r:
                return r.status == 200
        except Exception:
            return False
    return ok


def configurar(args_script, clave):
    orden = ['python3', str(RAIZ / 'scripts/configure-provider.py'), *args_script]
    if clave is not None:
        orden.append('--api-key-from-stdin')
    orden.append('--apply')
    r = subprocess.run(orden, input=(clave + '\n') if clave is not None else None, capture_output=True, text=True)
    salida = (r.stdout + r.stderr).strip()
    if r.returncode != 0:
        raise RuntimeError('configure-provider falló: ' + salida[-400:])
    return salida.splitlines()[-1]


def lineas_gateway(desde):
    out = subprocess.run(['journalctl', '--user', '-u', 'quiron-brain', '--since', desde, '-o', 'cat'],
                         capture_output=True, text=True).stdout
    return [l[l.index('[gateway]'):] for l in out.splitlines() if '[gateway]' in l]


def arnes(extra, salida):
    orden = ['python3', str(RAIZ / 'scripts/smoke-gui-x11.py'), '--project', str(RAIZ), '--output-dir', str(salida), *extra]
    r = subprocess.run(orden, capture_output=True, text=True, timeout=420)
    return {'exit': r.returncode, 'ultimas_lineas': (r.stdout + r.stderr).strip().splitlines()[-4:]}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--env', type=pathlib.Path, default=pathlib.Path.home() / '.config/quiron/quiron-brain.env')
    parser.add_argument('--brain-url', default='http://127.0.0.1:8766')
    parser.add_argument('--only', nargs='*', choices=['compatible', 'red-local', 'ollama', 'codex', 'claude'])
    parser.add_argument('--output-dir', type=pathlib.Path, required=True)
    args = parser.parse_args()
    escenarios = args.only or ['compatible', 'red-local', 'ollama', 'codex', 'claude']
    args.output_dir.mkdir(parents=True, exist_ok=True)
    env = leer_env(args.env)
    token = env['QUIRON_API_TOKEN']
    sano = cerebro_sano(args.brain_url, token)

    copia = args.env.with_name(args.env.name + '.bak-demo-agentes')
    shutil.copy2(args.env, copia)
    os.chmod(copia, 0o600)
    resumen = {'fecha': time.strftime('%Y-%m-%dT%H:%M:%S%z'), 'pregunta': PREGUNTA,
               'proveedor_original': env.get('QUIRON_GATEWAY_BACKEND'), 'escenarios': {}}
    # Repetir solo algunos escenarios (--only) conserva los demás del resumen anterior.
    anterior = args.output_dir / 'resumen.json'
    if args.only and anterior.exists():
        resumen['escenarios'] = json.loads(anterior.read_text()).get('escenarios', {})
    procesos = []
    try:
        for nombre in escenarios:
            print('==', nombre, flush=True)
            salida = args.output_dir / nombre
            salida.mkdir(exist_ok=True)
            registro = {'inicio': time.strftime('%H:%M:%S')}
            clave = None
            traza = None
            if nombre == 'compatible':
                puerto = 18081
                clave = secrets.token_hex(16)
                traza = salida / 'mock-trace.json'
                procesos.append(subprocess.Popen(['python3', str(RAIZ / 'scripts/tests/mock-openai.py'), '--port', str(puerto),
                                                  '--model', 'mock-gpt', '--key', clave, '--trace', str(traza)],
                                                 stderr=open(salida / 'mock.log', 'w')))
                esperar(lambda: not puerto_libre(puerto), 10, 'el servidor simulado')
                config = ['openai-compatible', '--endpoint', 'http://127.0.0.1:%d' % puerto, '--model', 'mock-gpt']
            elif nombre == 'red-local':
                clave = token  # llama-server del worker exige el token del cerebro como clave
                config = ['openai-compatible', '--endpoint', 'http://127.0.0.1:8092', '--model', 'quiron-worker']
            elif nombre == 'ollama':
                if not puerto_libre(11434):
                    registro['omitido'] = 'hay algo escuchando en :11434 (¿Ollama real?)'
                    resumen['escenarios'][nombre] = registro
                    continue
                traza = salida / 'mock-trace.json'
                procesos.append(subprocess.Popen(['python3', str(RAIZ / 'scripts/tests/mock-openai.py'), '--port', '11434',
                                                  '--model', 'mock-coder:7b', '--trace', str(traza)],
                                                 stderr=open(salida / 'mock.log', 'w')))
                esperar(lambda: not puerto_libre(11434), 10, 'el Ollama simulado')
                config = ['openai-compatible', '--endpoint', 'http://127.0.0.1:11434', '--model', 'mock-coder:7b']
            elif nombre == 'codex':
                config = ['codex-direct', '--model', 'gpt-5.5']
            else:
                config = ['claude-cli', '--model', 'sonnet']
            registro['configuracion'] = config + (['--api-key-from-stdin'] if clave else []) + ['--apply']
            try:
                registro['configure_provider'] = configurar(config, clave)
            except RuntimeError as e:
                registro['error'] = str(e)
                resumen['escenarios'][nombre] = registro
                continue
            esperar(sano, 120, 'el cerebro tras el reinicio')
            desde = time.strftime('%Y-%m-%d %H:%M:%S')
            if nombre == 'ollama':
                registro['paleta'] = arnes(['--panel', 'agentes'], salida / 'paleta')
            registro['chat'] = arnes(['--chat', PREGUNTA], salida)
            evidencia = salida / 'gui-chat.json'
            if evidencia.exists():
                chat = json.loads(evidencia.read_text()).get('chat', {})
                registro['chat'].update(segundos=chat.get('seconds'), llamadas_al_modelo=chat.get('llamadas'))
            registro['gateway'] = lineas_gateway(desde)
            if traza and traza.exists():
                peticiones = json.loads(traza.read_text())
                registro['servidor_simulado'] = {
                    'peticiones': len(peticiones),
                    'clave_correcta_en_todas': all(p.get('clave_correcta') in (True, None) for p in peticiones),
                    'pidio_herramienta_y_recibio_resultado': any(p.get('con_resultado_de_herramienta') for p in peticiones)}
            # Bien solo si el editor cerró limpio y la última llamada del gateway
            # fue una respuesta (no una petición de herramienta ni un error).
            ultima = registro['gateway'][-1] if registro['gateway'] else ''
            registro['ok'] = registro['chat']['exit'] == 0 and 'ok=true' in ultima and 'tool_calls=0' in ultima
            resumen['escenarios'][nombre] = registro
            for p in procesos:
                p.terminate()
            procesos.clear()
    finally:
        for p in procesos:
            p.terminate()
        # Restaurar el archivo privado tal como estaba y reiniciar el cerebro.
        shutil.copy2(copia, args.env)
        os.chmod(args.env, 0o600)
        copia.unlink()
        subprocess.run(['systemctl', '--user', 'restart', 'quiron-brain.service'], check=False)
        try:
            esperar(sano, 120, 'el cerebro restaurado')
            resumen['restaurado'] = True
        except SystemExit:
            resumen['restaurado'] = False
    (args.output_dir / 'resumen.json').write_text(json.dumps(resumen, ensure_ascii=False, indent=2) + '\n')
    todos = all(e.get('ok') for e in resumen['escenarios'].values() if 'omitido' not in e)
    print(('OK' if todos else 'REVISAR') + ': resumen en ' + str(args.output_dir / 'resumen.json'))
    return 0 if todos else 1


if __name__ == '__main__':
    sys.exit(main())
