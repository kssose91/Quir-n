#!/usr/bin/env python3
"""Inspect the real gateway/CLI contract against localhost; no external inference."""
import argparse
import hashlib
import http.server
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading


def main():
    root = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--gateway', type=Path, default=root/'Quirón/vertex-gateway/target/debug/vertex-gateway')
    parser.add_argument('--codex', default=shutil.which('codex'))
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if not args.codex:
        parser.error('Codex CLI is required')
    requests = []

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_GET(self):
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            self.wfile.write(b'{"models":[]}')

        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            requests.append(body)
            text = json.dumps({'content': '', 'tool_calls': [
                {'call_id': 'read-1', 'name': 'read_file', 'arguments': '{"path":"math.rs"}'}]})
            item = {'id': 'msg_probe', 'type': 'message', 'role': 'assistant',
                    'content': [{'type': 'output_text', 'text': text}]}
            events = [{'type': 'response.output_item.done', 'item': item},
                      {'type': 'response.completed', 'response': {'id': 'resp_probe', 'status': 'completed',
                       'model': 'gpt-6-astra', 'output': [item],
                       'usage': {'input_tokens': 10, 'output_tokens': 10, 'total_tokens': 20}}}]
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.end_headers()
            self.wfile.write(''.join('data: '+json.dumps(e)+'\n\n' for e in events).encode())

    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix='quiron-codex-contract-') as folder:
            wrapper = Path(folder)/'codex-probe'
            overrides = ['-c', 'model_provider="probe"', '-c', 'model_providers.probe.name="Probe"',
                         '-c', 'model_providers.probe.wire_api="responses"',
                         '-c', f'model_providers.probe.base_url="http://127.0.0.1:{server.server_port}/v1"',
                         '-c', 'model_providers.probe.requires_openai_auth=false']
            wrapper.write_text('#!/usr/bin/env python3\nimport os,sys\n'
                               'assert "QUIRON_API_TOKEN" not in os.environ\n'
                               f'os.execv({args.codex!r}, [{args.codex!r}]+sys.argv[1:-1]+{overrides!r}+[sys.argv[-1]])\n')
            wrapper.chmod(0o755)
            env = dict(os.environ, QUIRON_GATEWAY_BACKEND='claude_cli', QUIRON_CODEX_CLI=str(wrapper),
                       QUIRON_API_TOKEN='synthetic-do-not-forward', QUIRON_LLM_TIMEOUT_PRIMARY_SECS='45')
            request = {'provider': 'codex_cli', 'model': 'gpt-6-astra', 'reasoning_effort': 'xhigh', 'prompt': 'Read math.rs.',
                       'tools': [{'name': 'read_file', 'description': 'Read a project file',
                                  'parameters': {'type': 'object', 'properties': {'path': {'type': 'string'}},
                                                 'required': ['path'], 'additionalProperties': False}}]}
            process = subprocess.run([str(args.gateway.resolve())], input=json.dumps(request)+'\n',
                                     env=env, text=True, capture_output=True, timeout=60)
            response = json.loads(process.stdout)
            assert process.returncode == 0 and response.get('ok'), response
            assert len(requests) == 1, len(requests)
            wire = requests[0]
            assert wire['model'] == 'gpt-6-astra'
            assert wire['reasoning']['effort'] == 'xhigh'
            # CLI 0.153 may advertise the Plan-only question tool with fallback
            # model metadata. It cannot access the project and exec cannot wait
            # for interactive input. File, shell, image, web and MCP tools must
            # all be absent, including when model discovery returns no catalog.
            native_names = [t.get('name', t.get('type')) for t in wire['tools']]
            assert set(native_names) <= {'request_user_input'}, native_names
            assert response['tool_calls'][0]['name'] == 'read_file'
            assert json.loads(response['tool_calls'][0]['arguments']) == {'path': 'math.rs'}
            report = {'cli_version': subprocess.check_output([args.codex, '--version'], text=True).strip(),
                      'gateway_sha256': hashlib.sha256(args.gateway.read_bytes()).hexdigest(),
                      'checks': {'selected_model_reaches_cli': True, 'effort_reaches_cli': True,
                                 'explicit_provider_overrides_default': True,
                                 'no_native_project_tools': True, 'quiron_tool_contract_roundtrip': True,
                                 'brain_credential_removed': True}, 'response': response,
                      'advertised_native_tools': native_names,
                      'configured_provider': 'claude_cli', 'requested_provider': 'codex_cli',
                      'scope': 'Real gateway and Codex CLI; simulated Responses server on localhost. No model quality measurement.',
                      'ok': True}
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2)+'\n')
            print(json.dumps(report, ensure_ascii=False, indent=2))
    finally:
        server.shutdown()
        server.server_close()


if __name__ == '__main__':
    main()
