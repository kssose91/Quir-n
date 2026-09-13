# Estabilización posterior a la auditoría — 10 de septiembre de 2026

Este informe continúa el diagnóstico de `CIERRE_TFM_2026-09-10.md`. Documenta
correcciones realizadas sobre el árbol de trabajo, sin publicar ni crear un commit.
Las evidencias nuevas están en `evidencias/2026-09-10-estabilidad/`.

## Fichas y recuperación

La ficha v5 solicita una frase completa, contexto no resuelto y líneas de
evidencia. El programa copia los fragmentos del código y valida las referencias;
el modelo no redacta esas citas. El contexto incorpora definiciones completas de
constantes Rust referenciadas, cuando son de una línea y caben en sus límites.
Su contenido también invalida la caché si cambia.

Se rechazan cifras ausentes del contexto, determinadas menciones de lenguajes
sin respaldo en la entrada, frases incompletas y otras respuestas inválidas.
El rechazo conserva una ficha estructural con firma y motivo. Un fallo de
conexión se reintenta. Las fichas v4 no se reutilizan como v5.

Cypher filtra proyecto y modelos en semilla y vecino y conserva el resumen JSON
y `partial`. Se probaron relaciones entre archivos idénticos de dos proyectos,
un vecino de modelo antiguo y otro parcial. La resolución de `CALLS` sigue
siendo aproximada.

- `fichas-v5.json`: ocho funciones, siete descripciones aceptadas y una ficha
  estructural por frase incompleta. Los errores observados en `stable_id`,
  `collect_calls` y `read_file` no reaparecieron. Citas y hashes corresponden al código.
- `worker-integracion.json`: 13 comprobaciones aprobadas con Qwen, BGE-M3,
  Qdrant y Neo4j reales, incluidos cambios, borrado, caché y aislamiento.
- Se conservan ensayos intermedios. Uno falló porque el servicio de desarrollo
  se apagó por inactividad y detuvo los almacenes. La repetición final mantuvo
  activo ese servicio durante la prueba; solo se limpiaron datos temporales.

Esto es una regresión de desarrollo. Algunas descripciones omiten detalles;
las citas, las reglas y los hashes no prueban fidelidad semántica completa.
No se ha vuelto a medir el Hit@5 del proyecto completo con v5. La identidad de
un GGUF elegido por el usuario todavía depende de su nombre, no de un hash de
sus pesos. No hay detección automática de duplicados ni resumen por carpeta.

## Ledger v2 y transición

El hash v2 utiliza separación de dominio y la serialización completa del evento,
excluido su propio hash. Cubre campos antes omitidos, longitudes y precisión de
fecha. Evento, índices, versión, anclaje y cabecera se confirman en una transacción
Sled con solicitud de persistencia antes de retornar éxito. Los escritores
comparten un bloqueo y rechazan identificadores repetidos, también dentro de un lote.
La verificación falla ante eventos ilegibles o cambios de versión inválidos.
Las envolturas de memoria derivadas permanecen fuera de esa transacción.

El formato de los eventos antiguos no cambia. El primer evento v2 liga una
instantánea de su contenido completo. Esto detecta cambios posteriores respecto
a lo observado en la transición; no acredita la originalidad previa ni impide
que un actor reescriba toda la base y sus hashes. No existe una raíz de confianza externa.

La suite del cerebro pasó 184 pruebas; también se repitieron 321 del editor
(dos ignoradas) y nueve de entrega: 514 aprobadas en esta estabilización.
El gateway conserva las 26 aprobadas en la revisión inicial del mismo día.
Las pruebas del cerebro incluyen mutaciones, delimitación de
cadenas, rechazo atómico de lotes, cuatro escritores concurrentes, corrupción,
migración y recuperación de lotes confirmados después de terminar un proceso.
No es una prueba de corte eléctrico ni de todos los fallos de disco posibles.
Un error de E/S tras el commit deja un resultado incierto; un reintento con el
mismo ID se rechaza si ya estaba guardado.

Antes de migrar el servicio se hizo una copia privada y un ensayo sobre otra
copia. `migracion-ensayo.json` y `migracion-real.json` verifican que los 5 284
eventos anteriores conservaron sus bytes. Se añadió un evento de anclaje:
5 285 eventos, 5 284 históricos y uno v2. La consulta del servicio validó la cadena.
La copia de datos y el binario anterior se conservan en el equipo de desarrollo,
fuera del paquete y del código fuente.

Para otra base, detener primero el servicio, copiar íntegramente su directorio
Sled y verificar la copia. La herramienta requiere una base existente:

```sh
ledger_admin verify --data /ruta/a/la/copia
ledger_admin anchor --data /ruta/a/la/copia
ledger_admin verify --data /ruta/a/la/copia
```

Tras validar el ensayo, aplicar `anchor` a la base original detenida con su
respaldo disponible y arrancar el cerebro nuevo. Repetir `anchor` no añade otro
evento si ya hay v2. Para revertir, base y binario deben corresponder a la misma
revisión; no sustituir solo el binario por uno antiguo sobre datos v2. Si existen
escrituras posteriores, conservarlas antes de plantear una restauración.

## Paquete y alcance de compatibilidad

El primer candidato, compilado en el host, exigía GLIBC_2.43. La compilación de
entrega pasa a Ubuntu 24.04 (glibc 2.39), con imagen base fijada por digest y Rust
1.93.0. Ubuntu 22.04 se descartó porque ONNX Runtime necesita funciones C23 que
su glibc no proporciona. El empaquetador rechaza símbolos superiores a 2.39.

```sh
bash scripts/package-linux-portable.sh
```

El proceso usa dependencias locales de Cargo y ONNX Runtime y compila sin red
después de preparar la imagen. No es una compilación desde un equipo sin cachés
ni se afirma reproducibilidad bit a bit. Incluye cinco binarios, entre ellos
`ledger_admin`, fuentes, memoria y evidencias. `BUILD.json` registra la imagen
de compilación y los requisitos reales de cada binario.

Las comprobaciones de carga de bibliotecas, ventana y staging se registran por
separado. El instalador sigue requiriendo las descargas, Docker y systemd de
usuario; una prueba en contenedor no certifica el ciclo completo en una VM limpia.

| Comprobación | Resultado y evidencia |
| --- | --- |
| ABI de los cinco binarios | Máximo GLIBC_2.39; `ledger_admin` requiere 2.38. `BUILD.json` registra cada binario |
| Ubuntu 24.04, glibc 2.39 | Cinco binarios con bibliotecas resueltas; ventana Xvfb renderizada a 1280 × 720 y cierre normal con código 0; `runtime-ubuntu24.json` |
| Debian 13, glibc 2.41 | Cinco binarios con bibliotecas resueltas y ejecución de `ledger_admin --help`; `runtime-debian13.json` |
| Instalador en staging Ubuntu | Copia de los cinco binarios, configuración 0600 y rechazo de reinstalación conservando la configuración; `runtime-ubuntu24.json` |
| Copia histórica con binario portátil | 5 284 eventos históricos y uno v2 válidos; `ledger-portable.json` |
| Runtime de Qwen | `llama-server --version` carga en Ubuntu; sin inferencia GPU dentro de ese contenedor; `worker-runtime-ubuntu24.json` |

La primera apertura en la imagen mínima falló por `libXcursor`, una dependencia
cargada dinámicamente que no aparece en `ldd`. El instalador comprueba ahora las
bibliotecas del backend gráfico antes de escribir la instalación. Se conservan
el fallo de ventana y la detección de `libXcursor`/`libXi` ausentes en
`gui-fallo-biblioteca-dinamica.log` y `preflight-biblioteca-ausente.log`.
Tras añadirlas, la ventana abrió y cerró correctamente. La captura
`bienvenida-ubuntu24.png` corresponde únicamente a la ventana de prueba.

Para repetir la prueba de ventana, extraer el paquete, preparar la imagen con
`deploy/Dockerfile.build-linux` y ejecutar desde la raíz del repositorio:

```sh
mkdir -p /tmp/quiron-runtime-evidence
docker run --rm --network none --read-only --user "$(id -u):$(id -g)" \
  --cap-drop ALL --security-opt no-new-privileges --memory 2g \
  --tmpfs /tmp:rw,size=1g \
  --mount type=bind,source=/ruta/quiron-linux-x86_64,target=/package,readonly \
  --mount type=bind,source=/tmp/quiron-runtime-evidence,target=/evidence \
  --mount "type=bind,source=$PWD/scripts/tests/probe_linux_runtime.py,target=/probe.py,readonly" \
  --mount "type=bind,source=$PWD/scripts/smoke-gui-x11.py,target=/gui-harness.py,readonly" \
  --mount "type=bind,source=$PWD/deploy/install-linux.py,target=/installer.py,readonly" \
  quiron-build:ubuntu24.04 python3 /probe.py --gui
```
