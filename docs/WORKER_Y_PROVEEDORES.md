# Worker y conexiones: revisión del 10 de septiembre de 2026

Las pruebas del 5 de septiembre se conservan como evidencia histórica. La
evaluación de localización del día 10 y los límites de la auditoría están en
`CIERRE_TFM_2026-09-10.md`. Las correcciones posteriores y sus pruebas están en
`ESTABILIZACION_2026-09-10.md`.

El editor inicia un monitor cuando se abre explícitamente una carpeta, también
con `llore_gui /ruta/al/proyecto`. La identidad es `.quiron/project.id`. El cerebro
revisa cambios cada diez segundos; la interfaz consulta el estado cada cinco
segundos sin bloquear el chat y se actualiza aunque no haya actividad del ratón.
Cerrar la ventana conserva el trabajo en segundo plano. `systemctl --user stop
quiron-brain` detiene el conjunto en la instalación nueva.

## Implementación de esta prueba

1. Recorrido con exclusión de secretos, enlaces, artefactos y archivos ignorados
   por Git cuando la raíz es un repositorio. Máximo 512 KiB por archivo y 64 MiB
   de texto / 10 000 archivos por barrido.
2. Unidades de archivo para las extensiones admitidas. Rust añade unidades de
   lógica mediante Tree-sitter: firma, símbolo y líneas proceden del parser.
   Python, JavaScript y los demás lenguajes tienen por ahora ficha de archivo.
3. Qwen2.5-Coder-1.5B-Instruct Q4_K_M produce un resumen breve en inglés y lo que no puede
   resolver. Solo recibe texto local; no tiene herramientas ni acceso a archivos.
   La ficha v5 pide una frase completa y de una a tres líneas de evidencia,
   cuyos fragmentos copia el programa desde el código. Aporta además constantes
   Rust referenciadas cuando sus definiciones caben en el contexto. Rechaza
   frases incompletas, líneas inexistentes, cifras ausentes del contexto y
   determinadas menciones de lenguajes no respaldadas por la entrada, además
   de salidas inválidas y repeticiones de instrucciones.
   En esos casos conserva una ficha estructural con firma, motivo del rechazo y
   `summary_origin=parser`; las aceptadas llevan `summary_origin=model`.
   Esto permite completar el mapa sin presentar un resumen fallido como válido.
   Un servidor no disponible sigue siendo un error y se reintenta.
   Las entradas de más de 6 000 caracteres se recortan y se marcan `partial`.
4. BGE-M3 vectoriza ruta, símbolo, firma y resumen, compartiendo la instancia CPU del
   cerebro. Qwen usa llama.cpp Vulkan o CPU, una solicitud simultánea y contexto
   predeterminado de 8 192 tokens.
5. Qdrant guarda las fichas en `quiron_code_worker_v1`, con filtro de proyecto
   dentro de la consulta. Neo4j guarda `CodeProject`, `CodeUnit`, `HAS_UNIT` y
   `DEFINED_IN`. Las relaciones `CALLS` se aproximan a partir del parser de Rust
   y de nombres de símbolos; no resuelven por completo módulos y tipos.
6. Los hashes y la caché evitan repetir inferencia; una escritura se confirma
   después de Qdrant y Neo4j. Se retiran funciones y archivos desaparecidos, y sus
   fichas en caché. Un fallo de ficha deja el archivo pendiente y permite seguir
   con los restantes. El monitor reintenta después.
7. La búsqueda comprueba identidad, ruta y hash actual. El chat principal puede
   solicitar fichas del proyecto como pistas, identificadas como texto generado
   que debe verificarse leyendo el código. La búsqueda añade hasta un vecino por
   acierto mediante `CALLS`, con comprobación del hash de su archivo. Cypher
   filtra proyecto y versiones de modelos tanto en la semilla como en el vecino;
   conserva la ficha JSON y su indicador `partial`.

Hasta ocho proyectos pueden permanecer monitorizados. Al reiniciar el cerebro,
el editor vuelve a registrar el proyecto abierto; los demás se reanudan al
abrirlos. Una copia de carpeta con la misma identidad no se acepta como otro
proyecto simultáneo. La separación se probó con dos identidades diferentes.

## Evidencia y límites

- `evidencias/2026-09-10-estabilidad/worker-integracion.json`: 13 comprobaciones
  aprobadas con los cuatro servicios reales. Incluye aristas de prueba entre
  proyectos con archivos idénticos, exclusión de modelos anteriores y conservación
  de metadatos. `fichas-v5.json`: ocho funciones, siete descripciones del modelo
  aceptadas y una ficha estructural; los tres errores concretos de v4 no reaparecen.
  Es una regresión de desarrollo, no una estimación de precisión general.
- `evidencias/2026-09-05/worker-integracion.json`: prueba real con Qwen, BGE-M3,
  Qdrant y Neo4j. Autenticación obligatoria, dos proyectos, secretos/enlaces,
  consulta, modificación, eliminación y ausencia de inferencia al repetir un
  barrido sin cambios.
- `worker-ficha-v2-evaluacion.json`: experimento anterior conservado como
  evidencia negativa. El modelo omitía parámetros y confundía cálculos con
  efectos externos. Por eso la ficha final simplifica la salida y conserva la
  firma exacta del parser.
- En la prueba final, dos fichas de una función pequeña tardaron 3,47 y 1,10 s
  por proyecto en la prueba v3, incluyendo persistencia y búsqueda. En v4 fueron
  5,44 y 3,12 s para el mismo recorrido; las fichas en inglés fueron más largas. Son muestras sintéticas con
  modelos cargados; no representan el rendimiento de un repositorio grande.
- El proceso de Qwen cargado ocupó entre 1 117 y 1 130 MiB en las observaciones realizadas en la RTX 3060 de 6 GB. No es una
  medición del pico máximo. BGE-M3 permanece en CPU; no se entrenó ningún modelo.
- El resumen final identificó correctamente el cálculo con descuento, pero
  omitió explicar su rama de error en v3. En un archivo real apareció además
  una repetición de instrucciones, ahora rechazada en v4. La prueba en inglés
  mejoró una descripción de función, pero confundió longitud en bytes con
  caracteres; no demuestra precisión semántica completa. El esquema JSON y el hash no demuestran
  fidelidad semántica. Falta evaluar cobertura y recuperación sobre tareas reales.
- El manifiesto incremental vive en Sled. Todavía no existe un historial completo
  de cambios de código que permita reconstruir estas proyecciones desde el ledger.
  La recuperación sí amplía por `CALLS`, pero no representa todas las dependencias
  AST. La consulta de vecinos ya filtra proyecto y modelos y conserva los
  metadatos; las pruebas de aislamiento siguen siendo acotadas.
- Se rechazan enlaces, pero sigue pendiente eliminar las carreras entre la
  inspección de una ruta y su apertura. Es una prueba para el escritorio local.

### Memoria del host

En el barrido completo del 5 de septiembre el proceso `llama-server` alcanzó
7,8 GB de RAM del host: su caché de prompts (`--cache-ram`) admite 8192 MiB por
defecto en b10816 y se llenó con las fichas de 179 archivos. Ese pico, sumado a
los 3,4 GB del cerebro con BGE-M3 en proceso, bloqueó el portátil de 15 GiB.
`run-worker.sh` limita ahora la caché a 256 MiB (`QUIRON_WORKER_CACHE_RAM_MIB`)
y la unidad del worker lleva `MemoryHigh=2G`/`MemoryMax=3G`; el modelo sigue en
la GPU y la calidad de las fichas no depende de esa caché.

## Preparación y API

En este equipo los servicios `quiron-brain` y `quiron-worker` ya están preparados.
Para otra instalación usar `python3 install.py --with-worker`; descarga unos
1,15 GB adicionales. Runtime y modelo están fijados por revisión y SHA256 en
`scripts/setup-worker.py`, con procedencia en `data/worker/manifest.json`.
El modelo no se incluye dentro del archivo del instalador ni requiere un corpus.

Si se instala sin `--with-worker`, se puede preparar después:

```sh
python3 ~/.local/share/quiron/scripts/setup-worker.py --directory ~/.local/share/quiron-data/worker
systemctl --user start quiron-worker
```

El worker escucha solo en `127.0.0.1:8092` y usa el token local del cerebro. La
selección automática prefiere NVIDIA; sin ella usa CPU. `QUIRON_WORKER_DEVICE`
permite seleccionar otro dispositivo mostrado por `llama-server --list-devices`.

Todas estas rutas requieren bearer token, incluso con la autenticación antigua
desactivada. No introducir tokens en capturas, documentación o argumentos CLI.

| Método y ruta | Uso |
|---|---|
| `POST /index/project` | `{root: ruta_absoluta, project_id: ULID}`; iniciar/reanudar y obtener estado |
| `GET /index/project/:id` | Consultar estado y errores |
| `DELETE /index/project/:id` | Solicitar parada del monitor; conserva el índice |
| `GET /index/search?project_id=…&q=…&limit=4` | Fichas vigentes y puntuación |

Para repetir la integración sin mezclar datos de usuario:
`python3 scripts/smoke-worker.py --output /tmp/worker-prueba.json`.

## Proveedores y suscripciones

Una suscripción no proporciona una clave universal ni basta con su identificador.
Cada proveedor determina cómo se inicia sesión y qué uso cubren sus límites.

| Conexión | Autenticación y estado |
|---|---|
| `claude_cli` | CLI oficial autenticada mediante `claude auth login`; probado con sesión de claude.ai |
| `codex_cli` | CLI oficial autenticada; modelo y esfuerzo explícitos, contrato JSON y herramientas ejecutadas por Quirón |
| `codex_direct` | Adaptador histórico de HTTP que lee la sesión; se conserva para configuraciones existentes, pero «Usar ChatGPT» selecciona ahora `codex_cli` |
| `openai_compatible` | Endpoint, modelo y credencial de API propia (OpenAI, un servidor compatible en la red local u Ollama por su `/v1`); transporta la conversación estructurada y las herramientas de Quirón (`tools` de OpenAI; las llamadas escritas como JSON por modelos pequeños también se aceptan) |
| `ollama_native` | API nativa de Ollama (`/api/chat`), sin herramientas; la paleta usa la compatible |

Cada llamada deja en el registro del cerebro una línea común
`[gateway] backend=… model=… ok=… tool_calls=N content_chars=…`, sea cual sea
el adaptador. La ronda de evaluación de los cinco agentes (`scripts/demo-agentes.py`,
con `scripts/tests/mock-openai.py` como servidor simulado que exige clave y pide
una herramienta) está en `evidencias/2026-09-05/agentes/`; guía para quien
evalúa: `GUIA_EVALUACION.md`.

Todo eso se elige desde el editor en la paleta **Agentes** (chip «● Agentes»
de la barra superior o paleta de órdenes): tarjetas con el estado real de cada
proveedor en el equipo, «Iniciar sesión» (abre una terminal con el flujo de la
propia CLI), «Configurar…» (pide
endpoint, modelo y, si procede, clave; la clave va por la entrada estándar del
script, enmascarada en pantalla) y «Usar». La misma paleta enseña el modelo
del worker local y permite cambiarlo por otro `.gguf` de su carpeta.
Claude y Codex se ejecutan mediante sus CLI oficiales. La conexión nueva
`codex_cli` usa `codex exec --ignore-user-config --ephemeral` con salida
estructurada, en una carpeta temporal y con las herramientas nativas de archivos,
comandos, imágenes, agentes y servicios desactivadas.
La CLI administra su autenticación. Se probó la versión 0.153.0; el catálogo y
los esfuerzos se leen de sus metadatos y pueden actualizarse desde el selector.
No existe un identificador universal de suscripción.

El selector conserva juntas las opciones de Claude (Sonnet, Opus y Haiku, alias
de su CLI) y las del catálogo de Codex. La petición de chat incluye `provider`
(`claude_cli` o `codex_cli`), de modo que cambiar el modelo no reescribe la
configuración privada ni reinicia los servicios. El gateway rechaza otros valores
en ese campo y cualquier intento de usarlo para cambiar la ruta del worker.

El modelo elegido llega sin sustitución al gateway. `reasoning_effort` atraviesa
el cliente, la API del cerebro y el adaptador; la selección no afecta al worker
Qwen/BGE-M3. Ultra queda fuera del adaptador porque implica delegación nativa.
Las llamadas a herramientas se devuelven como contrato JSON para que las ejecute
la guardia del editor; cada respuesta del modelo recibe el historial necesario.
El presupuesto de salida se envía como instrucción, no como límite duro de la CLI.

```sh
python3 scripts/configure-provider.py codex-cli --model gpt-6-astra --reasoning-effort xhigh --apply
```

Una prueba local con proveedor simulado inspecciona la petición de la CLI y
comprueba que no ofrece herramientas nativas con acceso al proyecto. Sin catálogo,
CLI 0.153.0 puede anunciar `request_user_input`, restringida al modo Plan e
indisponible como entrada interactiva en `exec`; se registra esta salvedad.
Las comprobaciones
reales y de interfaz se conservan en `evidencias/2026-09-10-codex/`.
La referencia del transporte es [Codex no interactivo](https://learn.chatgpt.com/docs/non-interactive-mode).

Claude CLI 2.1.251 se probó con `--safe-mode`, `--restricted`, `--tools ""`,
`--strict-mcp-config` y `--no-session-persistence`, en una carpeta temporal. No
se extraen sus tokens OAuth. Las herramientas de Quirón viajan como peticiones
JSON y se ejecutan en el arnés del editor. Se verificó una lectura solicitada y
su respuesta: `evidencias/2026-09-05/claude-cli.json`. El adaptador captura uso de
tokens, errores y tiempo límite. `max_tokens` se transmite como instrucción;
la CLI controla sus límites de salida y puede exceder ese presupuesto solicitado.

Para revisar la selección, sin cambiar nada:

```sh
python3 scripts/configure-provider.py claude-cli --model sonnet
```

Añadir `--apply` guarda esa elección en el entorno privado y reinicia el cerebro.
Modelos del vectorizador: `scripts/setup-worker.py --catalog` lista el catálogo
(Qwen2.5-Coder 0.5B/1.5B/3B/7B, Qwen3 4B y 8B en GGUF oficial, y Qwen3-Coder
30B-A3B para memoria unificada; revisión, SHA-256 y VRAM orientativa fijos)
y `--model NOMBRE` descarga uno a la carpeta del worker; la paleta Agentes hace
lo mismo desde la tarjeta «Vectorizador» (Modelos → catálogo, o «Añadir .gguf…»
para un archivo propio).
Otras formas: `openai-compatible --endpoint URL --model M [--api-key-from-stdin]`,
`ollama-native --model M` (endpoint `http://127.0.0.1:11434` por defecto) y
`worker-model --worker-model ruta.gguf` (fija `QUIRON_WORKER_MODEL_FILE`, que
lee `scripts/run-worker.sh`; el indexador etiqueta las fichas con el nombre del
archivo del modelo y las regenera si cambia ese nombre). Sustituir los pesos
conservando el mismo nombre no invalida por sí solo esa caché.
Reabrir el editor actualiza el selector a Sonnet/Opus/Haiku. La configuración
principal existente se conserva hasta elegir otra. En este portátil quedó
seleccionado `claude_cli` con `sonnet` el 5 de septiembre por la tarde; la
consulta de demostración está en `evidencias/2026-09-05/consulta-codigo.json`.

Corrección del adaptador ese mismo día: `modelUsage` de la CLI lista también un
modelo auxiliar (`claude-haiku-4-5`, ≈900 tokens por llamada) y el adaptador
atribuía la respuesta al primero de la lista; ahora la atribuye al modelo pedido
y, si no aparece, al que más salida generó. Cada llamada deja en el registro del
cerebro una línea `[claude_cli]` con modelo, turnos, tokens y longitud del
contenido; si el contenido es menor de 40 caracteres, añade el resultado crudo.

Manos con Claude CLI: la CLI corre con `--tools ""`, así que el modelo no tiene
herramientas nativas y debe pedir las de Quirón devolviendo `tool_calls` en el
contrato JSON. Por el API lo hace (`stop_reason: tool_use`, `read_file`); en la
interfaz se observó un intento nativo de `read_file` rechazado por la CLI, tras
el cual respondió desde las fichas. El prompt de sistema lo prohíbe ahora de
forma explícita; el ciclo completo herramienta → resultado → respuesta en la
interfaz sigue sin evidencia repetida. Las pruebas no habilitaron
pagos adicionales ni garantizan uso ilimitado de una suscripción.

Fuentes primarias: [modelo Qwen](https://huggingface.co/Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF),
[servidor llama.cpp](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md),
[Claude programático](https://code.claude.com/docs/en/headless),
[CLI de Claude](https://code.claude.com/docs/en/cli-reference),
[autenticación de Codex](https://developers.openai.com/codex/auth/),
[Codex no interactivo](https://developers.openai.com/codex/noninteractive/).

El código de integración registra el origen de cada ficha. Las evidencias
`worker-caso-real.json` y `worker-english-evaluacion.json` conservan resultados
negativos; un JSON terminado correctamente no equivale a un resumen correcto.
`structural_fallbacks` cuenta escrituras de fichas estructurales en la sesión del
monitor. Su uso no significa que Qwen haya explicado esa lógica.

En la muestra final v4, Qwen identificó también el resultado o error de la función
de descuento. El límite de longitud cerró alguna última frase de forma incompleta:
es otra limitación observada, no una prueba de análisis semántico exhaustivo.
