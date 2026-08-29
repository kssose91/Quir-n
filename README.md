# Proyecto TFM: editor con índice de código, grafo y red obrera

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

El mecanismo existe y está a medias. Cada unidad lleva un campo `project`, y la
recuperación filtra por él —aunque tarde: la búsqueda trae los candidatos más
parecidos de toda la colección y descarta después los de otro proyecto, en
memoria, en lugar de filtrar dentro de Qdrant.

Hay además dos defectos medidos que nada tienen que ver con el campo `project`:

- El identificador que envía el editor es la **ruta canónica** del proyecto
  (`/home/kssose/Quirón`), mientras que los eventos lo guardan como **nombre**
  (`quiron`). Dos vocabularios para la misma cosa.
- La colección mezcla el índice con **memoria personal del agente y restos de
  pruebas**, de ámbito `global`. No pertenecen al índice de código y deben vivir
  aparte.

Un mundo contaminado no es un mundo.

Antes de la red obrera hay dos correcciones deterministas que no admiten espera:
separar la memoria personal a su propia colección, y filtrar por proyecto dentro
de Qdrant en lugar de después, con un índice de carga útil sobre `project`.

La red obrera es la pieza que mantiene el orden una vez restablecido. Es una red
neuronal pequeña, propia, entrenada sobre las unidades del índice, y su trabajo
es continuo:

1. **Asignar y mantener el proyecto** de cada unidad. Ninguna unidad sin mundo.
2. **Clasificar unidades** por tipo y responsabilidad.
3. **Producir el resumen estructurado** de cada archivo y cada lógica.
4. **Priorizar candidatos** a lógica duplicada para revisión.

Restricciones que no se negocian: toda salida cumple un esquema, se refiere al
hash vigente de la unidad, y pasa validaciones deterministas antes de escribirse.
La red obrera no controla el sistema de archivos, ni Qdrant, ni el grafo. Propone;
no ejecuta.

Su tamaño y arquitectura se deciden midiendo sobre un corpus de proyectos
reales, no por estimación previa.

Es un vectorizador con sentido: el mismo backbone etiqueta y vectoriza, de modo
que el vector de una unidad es el estado del último token del resumen que ella
misma ha escrito. Ficha y embedding salen alineados de una sola pasada.

No confundirla con el reranker (`bge-reranker-v2-m3`), que solo ordena
resultados, ni con el worker de recuperación, que es determinista.

## El índice

Tres tipos de unidad, definidos en la memoria (§4.2.2): **Archivo**, **Lógica**
y **Cambio**. Cada unidad tiene identificador estable, hash y proyecto.

- **Qdrant** almacena los vectores y el texto semántico. Responde a «qué se
  parece a esto».
- **Neo4j** almacena las relaciones estructurales —`IMPORTS`, `CALLS`,
  `DEPENDS_ON`— extraídas por análisis estático. Responde a «de qué depende esto
  y qué se rompe si lo cambio».

Los dos almacenes no se integran entre sí. El **worker de recuperación** es el
cable que falta: filtra por proyecto, busca por similitud en Qdrant, expande cada
candidato por el grafo en Neo4j, reordena con el reranker y recorta al
presupuesto de contexto. Escribirlo es parte del trabajo.

Cada fragmento entregado al modelo incluye ruta, símbolo, rango y hash, de modo
que toda afirmación pueda verificarse contra el código de origen.

## Contrato de la API HTTP

`quiron-brain` escucha en `127.0.0.1:8766` y exige autenticación por *bearer
token*. Es la única puerta al índice: el editor no habla con Qdrant ni con Neo4j.

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
POST /event  /events/batch  /invariants
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

Queda pendiente el empaquetado distribuible (AppImage o Flatpak). Lo instalado
sirve solo a este usuario.

No se adopta Tauri. Tauri sustituiría la interfaz nativa por un webview con
HTML, y obligaría a reescribir las diecisiete mil líneas de `llore_ui`.

## Conexión con el modelo de lenguaje

`vertex-gateway` concentra el tráfico hacia proveedores externos en un único
punto de egreso. Eso es una propiedad de topología, no una restricción de
alcance: el gateway admite hoy cuatro backends —`openai_compatible`,
`ollama_native`, `openclaw` y `codex_direct`— seleccionables por configuración.
Añadir otro proveedor consiste en añadir un backend; no obliga a tocar el índice,
el grafo ni la API.

Precisamente por concentrar la salida en un punto, el sistema puede crecer hacia
otros modelos sin reescribirse.

La configuración vigente es `codex_direct`: lee las credenciales de sesión que
mantiene Codex en `~/.codex/auth.json`, extrae el testigo de acceso y el
identificador de cuenta, y envía la petición a la API de respuestas de Codex.
Modelo: `gpt-5.6-sol`.

No se emplea ninguna clave de API. El coste queda cubierto por la suscripción.

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
en `ExecStartPre` y `... down` en `ExecStopPost`. Ese script descubre los
contenedores por puerto, los arranca (o los crea con `--restart no`) y espera a
que respondan antes de ceder el paso al cerebro.

Docker es *rootful* en esta máquina: el usuario debe pertenecer al grupo
`docker` (`sudo usermod -aG docker $USER` y reiniciar sesión) para que el
servicio gestione los contenedores sin `sudo`. Si un contenedor heredó una
política de reinicio automático, se corrige una sola vez con
`docker update --restart no <contenedor>` para que no reviva al arrancar la
máquina.

Qdrant y Neo4j escuchan hoy en `0.0.0.0`, y Qdrant carece de autenticación;
conviene restringirlos a la interfaz local antes de exponer la máquina a una red
no confiable.

## Documentos

- `memoria/borrador-memoria-tfm.md` — la memoria del TFM. Es la única fuente
  de verdad del proyecto: arquitectura, objetivos, plan y resultados. Léase
  primero.
- `docs/ESTUDIO_RED_OBRERA.md` — Anexo A: la matemática de la red obrera,
  con etiquetas de evidencia y verificación adversarial.
- `docs/ciclo-worker.html` — recorrido visual del ciclo: de conceder acceso a
  una carpeta a la rotación continua del worker.
