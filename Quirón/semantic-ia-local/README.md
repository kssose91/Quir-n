# Servicio semántico local

Servicio Rust aislado para calcular embeddings y reranking del índice vectorial.

La mitad de embeddings es transitoria: la red obrera se diseña para producir los
vectores de código de 1024 dimensiones que pueblan Qdrant (Anexo A, §4.3–4.5).
El reranker, en cambio, permanece: es el paso 4 del worker de recuperación.

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
