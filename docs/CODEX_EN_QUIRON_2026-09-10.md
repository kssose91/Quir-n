# Codex dentro de Quirón — 10 de septiembre de 2026

Actualización posterior: el menú reúne ahora Claude/Anthropic y Codex, y Claude
Sonnet conserva el valor predeterminado del autor. La selección se transmite por
conversación sin reiniciar servicios. Las comprobaciones de esa revisión están
en [Claude y Codex en el mismo selector](evidencias/2026-09-10-proveedores/README.md).
Las evidencias de este informe corresponden a la primera incorporación de Codex.

El selector anterior tenía una lista fija y las conversaciones con herramientas
sustituían la elección por GPT-5.5. La interfaz ahora carga los modelos visibles
de la sesión de Codex, ofrece su actualización y conserva el modelo y el esfuerzo
de razonamiento al reabrir el proyecto. Al cambiar de proveedor, una selección
guardada de otro proveedor deja de sobrescribir la configuración nueva.

El catálogo consultado con Codex CLI 0.153.0 anuncia GPT-6 Astra, GPT-5.6 Sol,
GPT-5.6 Terra, GPT-5.6 Luna, GPT-5.5 y GPT-5.3 Codex Spark. Son los modelos visibles
en esta cuenta y fecha; no se presenta esa lista como disponibilidad universal.
Los niveles admitidos por cada modelo se muestran en español. `xhigh` corresponde
a **Extra alto**. Se excluye `ultra`, asociado a delegación automática: este
adaptador utiliza las herramientas y los controles propios de Quirón.

## Conexión

La conexión nueva `codex_cli` usa la CLI oficial autenticada y un contrato JSON de
respuesta y solicitudes de herramientas. El editor ejecuta estas solicitudes con
su guardia habitual y aporta los resultados al siguiente turno. El adaptador
histórico `codex_direct`, que leía el archivo de sesión de Codex, se retiró
en la revisión final.

La CLI se inicia en una carpeta temporal, sin cargar la configuración de ejecución
del usuario, sin persistir su hilo y con las herramientas nativas de archivos,
comandos, imágenes, agentes, aplicaciones y servicios desactivadas. Quirón elimina
del entorno del subproceso sus credenciales de cerebro, almacenes y proveedores.
La propia CLI administra su sesión; el sondeo de la interfaz usa `login status` y
no muestra su salida ni abre el archivo de autenticación.

El contrato limita las llamadas, exige nombres ofrecidos, identificadores
distintos y argumentos JSON válidos. El adaptador exige la confirmación de fin de
turno y propaga errores; no cambia a otro modelo cuando falla el elegido. Tiene
tiempo de espera y límites de tamaño de entrada y salida. `max_tokens` es una
instrucción al modelo, no un tope duro de generación de la CLI.

Esta integración inicia conversaciones propias de Quirón. No transfiere el hilo,
la memoria ni las herramientas de la sesión abierta en el IDE. El worker local
Qwen/BGE-M3 mantiene su función de fichas y embeddings.

## Evidencias

En `docs/evidencias/2026-09-10-codex/`:

- `catalogo.json`: metadatos visibles de la cuenta, sin credenciales ni plantillas
  de instrucciones de los modelos.
- `protocolo-local.json`: cinco comprobaciones con gateway y CLI reales frente a
  un servidor Responses simulado. Verifica el modelo, `xhigh`, el contrato de
  lectura, la retirada de una credencial sintética y la ausencia de herramientas
  nativas con acceso al proyecto. Incluso sin catálogo, solo se anuncia
  `request_user_input`, que pertenece al modo Plan y no permite entrada interactiva
  en este uso de `exec`. No es una medición de calidad de un modelo.
- `codex-astra-conexion.json`: respuesta real de Astra a una función sintética que
  duplica siete; devuelve catorce. El registro confirma la selección `xhigh`.
  La CLI informó 9 402 tokens de entrada y 77 de salida; esta prueba no demuestra
  ahorro de contexto, pues incluye las instrucciones de la CLI.
- `gui-sintetica/gui-chat.json` y `gui-chat.png`: prueba real desde la ventana
  final del editor con un proyecto sintético de cuatro líneas. Qwen generó dos
  fichas, de archivo y función, que se recuperaron con su hash y referencias.
  Astra solicitó `read_file(math.rs)` y devolvió `Some(14)` y `None` para los casos
  normal y de desbordamiento. Las dos llamadas confirman `gpt-6-astra` y `xhigh`.
  El editor registró 49,0 s para la ronda; el archivo quedó intacto y la ventana
  cerró con código 0. El compositor mantuvo 1910 × 1032, aunque el arnés solicitó
  tamaños menores. No se acredita esta ronda como prueba a 900 píxeles.
- `tests.json`: 544 pruebas aprobadas —323 del editor, 184 del cerebro, 28 del
  gateway y nueve de entrega—; dos pruebas del editor permanecen ignoradas.
- `runtime-ubuntu24.json` y `runtime-debian13.json`: carga de los cinco binarios
  finales, ventana Xvfb y staging del instalador en Ubuntu 24.04; carga de
  bibliotecas y ejecución de `ledger_admin` en la base Debian 13. Máximo
  GLIBC_2.39. Los hashes permiten identificar los binarios comprobados.

La primera ronda sintética respondió correctamente, pero falló el guardado del
arnés por duplicar un argumento `question`. Se conserva el diagnóstico inicial y
se corrigió el arnés antes de la ronda completa anterior. No se ejecutó la
consulta propuesta sobre los dos archivos privados del ledger: la revisión
automática de aprobación solicitó autorización específica para enviarlos a Codex.

## Límites de la entrega

La prueba de inferencia no certifica todos los modelos del catálogo ni el
comportamiento de versiones futuras de Codex CLI. La integración no incorpora el
modo Ultra, transferencia de hilos del IDE ni una LLM propia entrenada desde cero.
La evaluación independiente de fidelidad de fichas, la comparación de tareas con
y sin índice, la instalación completa en una VM limpia y los datos de autoría
pendientes de la memoria siguen formando parte del cierre del TFM.

La implementación sigue el contrato de ejecución documentado en
[Codex no interactivo](https://learn.chatgpt.com/docs/non-interactive-mode) y las
opciones de la [referencia de configuración](https://learn.chatgpt.com/docs/config-file/config-reference).
