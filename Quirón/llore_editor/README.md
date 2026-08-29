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

- `llore_core`: estructuras de datos fundamentales.
- `llore_util`: utilidades compartidas.
- `llore_ui`: estado, componentes y render de interfaz.
- `llore_editor`: edición de texto.
- `llore_workspace`: gestión de proyectos.

## Construcción y pruebas

```bash
cargo build --release
cargo test --locked
```

La red neuronal de mantenimiento del índice no forma parte todavía del editor.
Se incorporará después de estabilizar la interfaz, el chat y el contrato del
índice vectorial.
