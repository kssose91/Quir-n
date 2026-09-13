# Claude y Codex en el mismo selector

La primera conexión de Codex filtraba el menú por el proveedor activo, ocultando
las opciones de Anthropic. Esta revisión conserva ambos grupos: Claude Sonnet,
Opus y Haiku —alias de su CLI— y los seis modelos visibles del catálogo de Codex
consultado en esta cuenta. Se restaura Claude Sonnet como opción predeterminada
del servicio, tal como estaba antes de incorporar Codex.

La selección de CLI pertenece a cada conversación. `provider` atraviesa el
editor, la API y el gateway junto con el modelo; elegir otra opción no reescribe
la configuración privada ni reinicia el cerebro o el worker. El gateway solo
admite `claude_cli` y `codex_cli` como elecciones explícitas y rechaza usarlas
para cambiar la ruta del worker. Sin ese campo se conserva la configuración
habitual. Las preferencias de Claude y Codex se restauran al reabrir el proyecto.

Las pruebas automatizadas de esta revisión suman 546 aprobadas: 324 del editor,
184 del cerebro, 29 del gateway y las nueve de entrega verificadas en la ronda
anterior. Dos pruebas del editor continúan ignoradas. Los compiladores de la
primera ejecución recibieron SIGTERM; el enlace posterior detectó símbolos
ausentes en la caché incremental. Tras retirar extracciones temporales de nuestras
pruebas y recompilar el editor sin esa caché, la repetición terminó
con los resultados registrados en `tests.json`.

Las evidencias de `../2026-09-10-codex/` corresponden a la revisión anterior.
Las pruebas de esta revisión utilizan solo una función Rust sintética de cuatro
líneas; no se presentan como una auditoría de un repositorio privado completo.

## Resultado de la integración

- `gui-modelos/` y `gui-modelos-codex/`: los dos grupos permanecen visibles tanto
  con Claude como con Codex seleccionado. Se comprueba el binario identificado
  por su SHA-256; los tamaños reales de captura dependen del compositor.
- `gui-claude/gui-chat.json`: el alias `sonnet` fue resuelto por la CLI como
  `claude-sonnet-5`. Realizó dos llamadas, recuperó dos fichas y leyó `math.rs`.
  El editor registró 12,8 s en esta ronda concreta.
- `gui-codex/gui-chat.json`: `gpt-6-astra`, esfuerzo `xhigh`, dos llamadas, las
  mismas dos fichas y lectura del archivo. El editor registró 32,8 s. Estos dos
  tiempos no constituyen una comparación de rendimiento entre modelos.
- Ambos respondieron `Some(14)` y `None` para las entradas siete y el máximo
  `u32`; el código quedó intacto y las ventanas cerraron normalmente.
- `cambio-proveedor.json`: el cerebro y el worker conservaron sus PID al pasar
  de Claude a Codex, y el archivo privado de configuración no cambió. Claude
  Sonnet siguió como predeterminado del servicio.
- `protocolo-local.json`: el gateway portable envía una petición explícita a
  Codex aunque su configuración predeterminada sea Claude; se inspeccionan
  modelo, esfuerzo, herramientas y retirada de la credencial sintética.
- `runtime-ubuntu24.json` y `runtime-debian13.json`: comprobación de los cinco
  binarios de esta revisión. La ventana Xvfb y el staging en Ubuntu completan la
  verificación de ABI, con máximo GLIBC_2.39. Continúa pendiente la instalación
  completa en una VM limpia.
