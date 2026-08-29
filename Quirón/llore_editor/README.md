# Llore Editor

Editor de código local con chat integrado, construido para trabajar con
repositorios grandes y con un índice semántico del código.

## Objetivo

- Editar y buscar código desde una interfaz propia.
- Enviar al chat únicamente contexto relevante del proyecto.
- Consultar responsabilidades, símbolos, dependencias y cambios de archivos.
- Señalar candidatos a lógica duplicada con evidencia revisable.

La conexión con el modelo se realiza mediante el gateway local configurado. El
editor no contiene credenciales ni llama directamente a proveedores externos.

## Crates principales

- `llore_core`: estructuras de datos fundamentales (rope, reloj).
- `llore_util`: utilidades compartidas (rutas, cadenas, resultados).
- `llore_buffer`: buffer de edición, historial y deshacer.
- `llore_editor`: edición de texto, cursor y selección.
- `llore_language`: registro de lenguajes y resaltado sintáctico.
- `llore_ui`: estado, componentes, render de interfaz y guardia de rutas.
- `llore_brain`: cliente de la API local, registro y recuperación de contexto.

## Construcción y pruebas

```bash
cargo build --release
cargo test --locked
```

La red obrera no vive en el editor y no se incorporará a él: es un daemon
continuo dentro de `quiron-brain`. El editor la alcanza, como a todo lo demás,
por la API local en `127.0.0.1:8766` — nunca habla con Qdrant ni con Neo4j.
