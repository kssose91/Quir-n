# Servicio semántico local

Servicio Rust aislado para calcular embeddings y reranking del índice vectorial.
No debe confundirse con la red obrera: el reranker solo ordena resultados, y la
red obrera no lo sustituye.

## API

- `GET /health`
- `POST /v1/embed`
- `POST /v1/rerank`

Los modelos se cargan bajo demanda. El almacenamiento vectorial queda fuera de
este proceso.

## Valores actuales

- Embeddings: `BAAI/bge-m3`.
- Reranking: `rozgo/bge-reranker-v2-m3`.
- Dirección: `127.0.0.1:8091`.
- Dispositivo CUDA: configurable mediante `SEMANTIC_IA_CUDA_DEVICE_ID`.

## Ejecución

```bash
CUDA_VISIBLE_DEVICES=0 cargo run --release
```

El servicio acepta lotes de texto, pero el indexador es responsable de excluir
archivos generados, dependencias, secretos y fragmentos sin valor semántico.
