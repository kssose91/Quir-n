#!/usr/bin/env python3
"""Servidor simulado compatible con OpenAI y con el descubrimiento de Ollama.

Sirve para probar el camino completo paleta → configure-provider → cerebro →
gateway → modelo sin clave real ni Ollama instalado:

- `POST /v1/chat/completions`: si se pasó `--key`, exige `Authorization:
  Bearer <clave>` (401 si no coincide). Con herramientas ofrecidas y sin
  resultado previo, pide `search_text` (o la primera herramienta); con un
  resultado de herramienta en la conversación, contesta citándolo.
- `GET /api/tags` y `GET /v1/models`: anuncia `--model` como si fuera Ollama.
- `--trace RUTA`: deja un JSON con cada petición (sin la clave: solo si
  coincidía) para que la prueba compruebe qué llegó.
"""
import argparse, json, sys, time
from http.server import BaseHTTPRequestHandler, HTTPServer

parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
parser.add_argument('--port', type=int, required=True)
parser.add_argument('--model', default='mock-coder')
parser.add_argument('--key', help='clave exigida en Authorization (Bearer)')
parser.add_argument('--trace', help='archivo JSON con las peticiones recibidas')
args = parser.parse_args()
trace = []

def guardar(entrada):
    trace.append(entrada)
    if args.trace:
        with open(args.trace, 'w') as f:
            json.dump(trace, f, ensure_ascii=False, indent=1)

class Handler(BaseHTTPRequestHandler):
    def log_message(self, fmt, *a):
        sys.stderr.write('[mock-openai] ' + fmt % a + '\n')

    def _json(self, code, body):
        data = json.dumps(body).encode()
        self.send_response(code)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.path.startswith('/api/tags'):
            return self._json(200, {'models': [{'name': args.model, 'model': args.model}]})
        if self.path.startswith('/v1/models'):
            return self._json(200, {'object': 'list', 'data': [{'id': args.model, 'object': 'model'}]})
        self._json(404, {'error': 'no such path'})

    def do_POST(self):
        largo = int(self.headers.get('Content-Length', 0))
        cuerpo = json.loads(self.rfile.read(largo) or b'{}')
        auth = self.headers.get('Authorization', '')
        clave_ok = (auth == 'Bearer ' + args.key) if args.key else None
        mensajes = cuerpo.get('messages', [])
        herramientas = [t.get('function', {}).get('name') for t in cuerpo.get('tools', [])]
        hay_resultado = any(m.get('role') == 'tool' for m in mensajes)
        guardar({'hora': time.strftime('%H:%M:%S'), 'ruta': self.path, 'clave_correcta': clave_ok,
                 'modelo': cuerpo.get('model'), 'roles': [m.get('role') for m in mensajes],
                 'herramientas_ofrecidas': herramientas, 'con_resultado_de_herramienta': hay_resultado})
        if args.key and not clave_ok:
            return self._json(401, {'error': {'message': 'clave de API incorrecta o ausente', 'type': 'invalid_request_error'}})
        if not self.path.startswith('/v1/chat/completions'):
            return self._json(404, {'error': {'message': 'no such path'}})
        if herramientas and not hay_resultado:
            nombre = 'search_text' if 'search_text' in herramientas else herramientas[0]
            argumentos = json.dumps({'query': 'ensure_current'}) if nombre == 'search_text' else '{}'
            mensaje = {'role': 'assistant', 'content': None, 'tool_calls': [
                {'id': 'call_mock_1', 'type': 'function', 'function': {'name': nombre, 'arguments': argumentos}}]}
            finish = 'tool_calls'
        else:
            salida = next((m.get('content', '') for m in reversed(mensajes) if m.get('role') == 'tool'), '')
            texto = ('Respuesta simulada del servidor ' + args.model + '. '
                     + ('La herramienta devolvió ' + str(len(salida)) + ' caracteres; primera línea: '
                        + salida.strip().splitlines()[0][:120] if salida.strip() else 'No hubo herramienta.'))
            mensaje = {'role': 'assistant', 'content': texto}
            finish = 'stop'
        self._json(200, {'id': 'chatcmpl-mock', 'object': 'chat.completion', 'model': args.model,
                         'choices': [{'index': 0, 'message': mensaje, 'finish_reason': finish}],
                         'usage': {'prompt_tokens': 42, 'completion_tokens': 17, 'total_tokens': 59}})

HTTPServer(('127.0.0.1', args.port), Handler).serve_forever()
