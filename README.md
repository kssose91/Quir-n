# Proyecto TFM: editor con índice de código, grafo y red obrera

**Revisión de cierre, 5 de septiembre de 2026:** consultar
[`docs/CIERRE_2026-09-05.md`](docs/CIERRE_2026-09-05.md) para el estado comprobado,
los defectos corregidos y el plan de los cinco días. Las descripciones de diseño
de este documento no implican que todos los objetivos estén implementados.
El candidato de instalación Linux se construye con `bash scripts/package-linux.sh`;
requisitos y límites en [`deploy/LINUX.md`](deploy/LINUX.md).

Trabajar con repositorios grandes exige saber qué hace cada archivo, de qué
depende, qué cambió y por qué. Quirón construye ese conocimiento como un índice
consultable y lo entrega al modelo de lenguaje como evidencia verificable, en
lugar de volcarle el repositorio entero.

La hipótesis del trabajo es que un índice bien construido reduce el consumo de
contexto y elimina los errores por omisión: el modelo no repite una lógica que ya
existe, porque el índice se la pone delante.

## Componentes

- **`Quirón/llore_editor`** — editor nativo. Interfaz propia en Rust.
- **`Quirón/quiron-brain`** — indexador, grafo, API HTTP y orquestación.
- **`Quirón/semantic-ia-local`** — embeddings y reranker sobre GPU.
- **`Quirón/vertex-gateway`** — salida hacia los modelos de lenguaje, con
  backends intercambiables.

## Los mundos por proyecto y la red obrera

Cada proyecto debe tener su propio mundo. Al abrir la carpeta de Quirón, el
contexto es el de Quirón. Al abrir Templaris, el de Templaris. Ningún fragmento
de un proyecto puede aparecer en las respuestas de otro.

El editor asigna un ULID persistente en `.llore/project.id`. El nuevo worker
mantiene fichas en segundo plano con Qwen2.5-Coder-1.5B Q4_K_M y embeddings
BGE-M3 separados. La firma y los símbolos Rust proceden de Tree-sitter; el modelo
solo propone un resumen. Este experimento no entrena un backbone propio.

Al abrir un proyecto se inicia su monitor. Revisa cambios cada diez segundos,
reutiliza la caché y retira unidades borradas. La interfaz muestra el avance sin
bloquear el chat. La consulta de código filtra dentro de Qdrant por identidad y
modelo y comprueba el hash actual antes de devolver resultados.

## El índice

Qdrant contiene `quiron_code_worker_v1`, separado de la memoria de eventos y del
índice antiguo `quiron_code`. Neo4j recibe archivos y unidades Rust enlazadas
mediante `HAS_UNIT` y `DEFINED_IN`. El chat principal puede recuperar fichas con
ruta, símbolo, firma, rango y hash como pistas para leer el código.

Se probaron dos proyectos, cambios, borrados y exclusión de secretos. Las fichas
pequeñas son útiles para localizar código, pero pueden omitir detalles: no
constituyen pruebas de comportamiento. La expansión AST de dependencias, la
reconstrucción completa desde el ledger y las métricas sobre proyectos grandes
siguen pendientes. La recuperación antigua de eventos necesita su propia
revisión; las pruebas del nuevo índice no certifican esa ruta.

Configuración, API, resultados y límites:
[`docs/WORKER_Y_PROVEEDORES.md`](docs/WORKER_Y_PROVEEDORES.md).

## Contrato de la API HTTP

`quiron-brain` escucha en `127.0.0.1:8766`. Las mutaciones configuradas y todas
las rutas `/index/*` exigen autenticación por *bearer token*; algunos GET antiguos
siguen siendo accesibles sin token. Es la única puerta al índice: el editor no habla con Qdrant ni con Neo4j.

Recuperación:

```
GET  /context                    contexto del proyecto abierto
GET  /search                     búsqueda semántica
GET  /crag                       recuperación aumentada
GET  /recall
GET  /project/:id/timeline       historia del proyecto
```

Inferencia:

```
POST /v1/messages                formato Anthropic Messages
```

`/v1/messages` recupera el contexto limitado al proyecto de la carpeta abierta,
enruta la petición por la vía `primary` o `worker`, y la envía al modelo a través
de `vertex-gateway`. La vía `worker` exige una tarea explícita: no existe
enrutado autónomo.

Escritura:

```
POST /event  /invariants
```

Adoptar el formato de mensajes de Anthropic sobre un modelo de OpenAI demuestra
que el índice no depende del proveedor. Es una propiedad del diseño, no un
accidente.

## La aplicación de escritorio

`llore_editor` es una aplicación nativa: `winit` para la ventana, `tiny-skia` y
`softbuffer` para el rasterizado, `taffy` para el layout y `cosmic-text` para el
texto. La interfaz está escrita desde cero; no hay navegador ni webview.

Las tipografías viajan dentro del binario —Inter para la interfaz, JetBrains Mono
para el código y Lucide para los iconos, todas con licencia libre—, de modo que
el editor se ve igual en cualquier máquina.

Al arrancar sin proyecto se muestra el estado real del índice: si `quiron-brain`
responde, cuántas unidades hay en el almacén vectorial y cuántos nodos en el
grafo. Es la información que decide si una consulta al chat va a servir de algo.

Un fotograma de interfaz cuesta 2,4 ms medidos, de los cuales 1,0 ms es el
modelado del texto y 1,4 ms la composición de glifos. Con repintado por eventos,
no hay motivo para mover el dibujado a la GPU.

Está integrada en el escritorio como cualquier otra aplicación:

```
~/.local/bin/llore_gui                             enlace al binario compilado
~/.local/share/applications/llore.desktop          entrada del menú
~/.local/share/icons/hicolor/*/apps/llore.{svg,png} icono
```

La ventana publica `app_id = "llore"` en Wayland y el mismo valor como `WM_CLASS`
en X11, que coincide con el nombre de la entrada de escritorio. Sin esa
coincidencia el escritorio no asocia el icono a la ventana.

El editor no acepta rutas como argumento, de modo que la entrada no declara tipos
MIME ni se ofrece como aplicación para abrir archivos.

### Confinamiento al proyecto

Llore arranca sin ninguna carpeta abierta. Hasta que se elige un proyecto no hay
raíz, y sin raíz no se lee ningún archivo. El chat queda deshabilitado.

Una vez abierta la carpeta, `crates/llore_ui/src/workspace_guard.rs` clasifica
toda ruta antes de tocar el disco, comparando formas canónicas para que ni `..`
ni un enlace simbólico permitan salir de ella. Las credenciales —`.env`, `*.pem`,
`id_rsa`, `.ssh/`, `.codex/`— se niegan siempre, incluso dentro del proyecto. Los
artefactos generados se ocultan pero pueden consultarse a petición. `.env.example`
se lee: es una plantilla, no una clave.

Antes de esto, el editor tomaba como raíz el directorio de trabajo del proceso.
Lanzado desde el menú de aplicaciones, eso era el directorio personal completo.

Existe un generador de paquete Linux con instalador por usuario, binarios release,
código correspondiente y memoria. Pendiente certificarlo en una máquina limpia;
todavía no es un AppImage ni un Flatpak.

No se adopta Tauri. Tauri sustituiría la interfaz nativa por un webview con
HTML, y obligaría a reescribir las diecisiete mil líneas de `llore_ui`.

## Conexión con el modelo de lenguaje

`vertex-gateway` concentra el tráfico hacia proveedores externos en un único
punto de egreso. Eso es una propiedad de topología, no una restricción de
alcance: el gateway admite `openai_compatible`,
`ollama_native`, `openclaw`, `codex_direct` y `claude_cli` seleccionables por configuración.
Añadir otro proveedor consiste en añadir un backend; no obliga a tocar el índice,
el grafo ni la API.

Precisamente por concentrar la salida en un punto, el sistema puede crecer hacia
otros modelos sin reescribirse.

El proveedor se elige desde el editor, en la paleta **Agentes** (suscripción de
Claude por su CLI, suscripción de ChatGPT vía Codex, OpenAI o un servidor
compatible con clave, servidor en la red local, Ollama), o con
`scripts/configure-provider.py`. Para evaluar la aplicación en un equipo nuevo:
[docs/GUIA_EVALUACION.md](docs/GUIA_EVALUACION.md). El manual de uso está dentro
de la aplicación (F1) y es [docs/MANUAL.md](docs/MANUAL.md). Con `codex_direct` el gateway lee las
credenciales de sesión que mantiene Codex en `~/.codex/auth.json`, extrae el
testigo de acceso y el identificador de cuenta, y envía la petición a la API de
respuestas de Codex.

La sesión de este equipo no usa una clave de API. La disponibilidad y los
límites dependen del proveedor. Claude se conecta mediante su CLI oficial; no
se reutiliza su OAuth como clave de API ni basta un ID de suscripción.

Configuración en una única fuente de verdad:

```
~/.config/quiron/quiron-brain.env
    ├── leído por systemd (EnvironmentFile)
    └── enlazado desde Quirón/quiron-brain/.env.local (script de arranque)
```

### Limitación conocida

El gateway lee el testigo de acceso sin renovarlo, aunque el fichero de
credenciales contiene un testigo de refresco que no se utiliza. Cuando el
testigo caduca, las peticiones fallan hasta iniciar sesión de nuevo con Codex.

## Arranque

Los almacenes viven con la aplicación, no antes. Qdrant y Neo4j no arrancan al
encender la máquina: los levanta el servicio del cerebro justo antes de sí mismo,
y los para justo después. El editor, al abrirse, pide ese arranque.

```sh
# Instalar el servicio (una vez):
cp deploy/quiron-brain.service ~/.config/systemd/user/
systemctl --user daemon-reload
# No se habilita al boot (unidad `static`): se activa a demanda.

systemctl --user start quiron-brain      # levanta almacenes + cerebro
systemctl --user status quiron-brain     # estado
systemctl --user stop  quiron-brain      # para cerebro + almacenes
```

El servicio (`deploy/quiron-brain.service`) invoca `scripts/quiron-stores.sh up`
en `ExecStartPre` y `... down` en `ExecStopPost`. Ese script gestiona solo los
nombres configurados, los arranca (o los crea con `--restart no`, imágenes fijadas
por digest y puertos en loopback) y exige que Qdrant y una consulta Bolt
autenticada respondan antes de ceder el paso al cerebro. Un timeout es un error.

Docker es *rootful* en esta máquina: el usuario debe pertenecer al grupo
`docker` (`sudo usermod -aG docker $USER` y reiniciar sesión) para que el
servicio gestione los contenedores sin `sudo`. Si un contenedor heredó una
política de reinicio automático, se corrige una sola vez con
`docker update --restart no <contenedor>` para que no reviva al arrancar la
máquina.

Los contenedores antiguos conservan sus puertos y volúmenes; actualizar el script
no los migra. En el equipo auditado todavía publican en todas las interfaces.
La instalación nueva solo publica en `127.0.0.1`. Cerrar la ventana del editor
todavía no para systemd: para cerrar también los almacenes, usar
`systemctl --user stop quiron-brain`.

## Documentos

- `memoria/borrador-memoria-tfm.md` — la memoria del TFM. Es la única fuente
  de verdad del proyecto: arquitectura, objetivos, plan y resultados. Léase
  primero.
- `docs/ESTUDIO_RED_OBRERA.md` — Anexo A: la matemática de la red obrera,
  con etiquetas de evidencia y verificación adversarial.
- `docs/ciclo-worker.html` — recorrido visual del ciclo: de conceder acceso a
  una carpeta a la rotación continua del worker.
