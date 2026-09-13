# Cierre de revisión del TFM — 10 de septiembre de 2026

Este documento conserva el diagnóstico inicial, incluida la prueba negativa de
GLIBC_2.43 y las fichas v4. Las correcciones y comprobaciones posteriores se
documentan en [Estabilización](ESTABILIZACION_2026-09-10.md).

Quirón dispone de un prototipo funcional de editor e índice local. Este cierre
actualiza la memoria y prepara un candidato de entrega coherente con el árbol
revisado, cuya base es el commit `9a28d02`. No declara cumplidos todos los
objetivos iniciales ni certifica una instalación limpia completa.

## Alcance implementado y cambio de objetivos

- Al abrir un proyecto se registra su identidad y se inicia un monitor de cambios.
- Se generan fichas de archivos admitidos y de unidades sintácticas Rust. Qwen
  redacta descripciones; BGE-M3 genera embeddings. Son funciones distintas.
- Qdrant almacena fichas y vectores. Neo4j representa pertenencia y llamadas
  aproximadas. La recuperación comprueba ruta y hash del archivo vigente.
- La interfaz permite descargar y seleccionar modelos compatibles y elegir
  conexiones con asistentes. El acceso por suscripción depende del proveedor;
  no existe una clave universal basada en el número de suscripción.
- No se entrenó una red propia. El autor declara dos torres adicionales por SSH
  y dos RTX 3090; no se accedió a ellas ni se midió entrenamiento en este cierre.
  La decisión es de alcance, corpus, recursos y plazo, no una imposibilidad de
  realizar cualquier ajuste de un modelo preentrenado.
- No hay detector automático de lógica duplicada ni resumen consolidado de
  responsabilidades por carpeta. Quedan como trabajo futuro.

## Hallazgos que limitan las garantías

| Prioridad | Evidencia en el código | Consecuencia y criterio de cierre |
| --- | --- | --- |
| Alta | `Quirón/quiron-brain/src/ledger/writer.rs`, `hash_event`, líneas 41–76 | No cubre todos los campos del evento ni delimita todos los campos variables. La cadena no acredita integridad de todo el registro. Requiere formato versionado, migración y regresiones. |
| Alta | Mismo archivo, `append`, líneas 85–118 | Eventos, índices y cabeza se escriben en operaciones separadas; el comentario de atomicidad excede el comportamiento. No hay confirmación de durabilidad por evento. Requiere transacción/recuperación y pruebas de interrupción. |
| Alta | `index/worker.rs`, recuperación, líneas 547–605 | La expansión por `CALLS` no repite el filtro de proyecto en Cypher y rellena metadatos incompletos. Debe validar proyecto, modelo y estado de cada vecino. |
| Media | `index/extract.rs`, `callee_name`, líneas 162–184; `index/worker.rs`, `link_calls`, desde 906 | Se pierde parte de la cualificación de módulos y se enlaza por nombres aproximados. No equivale a un grafo completo ni semánticamente exacto. |
| Media | `index/worker.rs`, manifiesto y caché; `index/unit.rs`, `ChangeUnit` | El índice se mantiene desde fuentes y caché. No se reconstruye una historia completa del código desde el ledger. |
| Media | `index/worker.rs`, `summary_model_tag` | La identidad del generador usa el nombre del GGUF. Pesos distintos con el mismo nombre no invalidan por sí solos la caché. |
| Media | `localizacion.json` y `fidelidad-observaciones.json` | Hay fichas con errores y frases incompletas aunque pasan el esquema. La vigencia por hash no valida el significado del resumen. |
| Alta para distribución | Instalador, metadatos de compilación y `compatibilidad.json` | El editor exige `GLIBC_2.43`; no carga en la base Debian 13 con glibc 2.41 utilizada en la prueba. Requiere otra base de compilación o destino compatible. El staging no sustituye una instalación completa. |

No se cambió el formato del ledger ni se migraron datos para corregir estos
hallazgos durante la revisión documental. Los límites se trasladaron a la memoria,
al manual y a la guía de evaluación. El paquete anterior no correspondía al
árbol actual; el script de empaquetado recompila los binarios y comprueba la
vigencia de las exportaciones antes de preparar otro candidato.

## Validación del 10 de septiembre

`evidencias/2026-09-10/verificacion.json` registra 533 pruebas aprobadas:
321 del editor, 177 del cerebro con `full`, 26 del gateway y 9 de entrega.
Hay 2 pruebas del editor ignoradas por defecto. Los logs se conservan junto al
JSON. Las pruebas del editor que utilizan un servidor simulado en localhost
necesitaron ejecutarse fuera de la restricción de sockets del sandbox. Las de
entrega emplean Docker simulado.

También pasó la integración real del worker con Qdrant y Neo4j sobre dos
proyectos temporales (`worker-integracion.json`). La prueba de ventana
(`gui/gui-edit.json`) comprobó edición, guardado, deshacer/rehacer con estados
intermedios, cierre, reapertura e indexación. El primer intento del arnés
suponía Ctrl+End al final del documento; se corrigió para utilizar navegación
por líneas admitida por el editor. No se cambió ese comportamiento del producto.

La comprobación aislada de bibliotecas en Debian 13 falló porque el editor
requiere `acosf@GLIBC_2.43` y la imagen tiene glibc 2.41. No se arrancó Neo4j
en ese contenedor ni se montaron sus datos. `compatibilidad.json` conserva
imagen, hash del binario y salida de `ldd`. El primer empaquetado offline también
falló por el enlace a ONNX Runtime; se repitió con acceso a su caché/descarga.

`evidencias/2026-09-10/localizacion.json` conserva diez preguntas y objetivos
fijados antes de la consulta, sus resultados y los hashes del corpus:

| Medida exploratoria | Resultado |
| --- | --- |
| Hit@5 | 10/10 |
| MRR@5 | 0,75 |
| Hit con expansión por llamadas | 10/10; sin incremento en esta muestra |
| Mediana de petición de búsqueda con modelos cargados | 0,111 s |
| Apariciones de fichas devueltas | 75; proyecto y hash coincidentes |
| Referencia de volumen | 140 archivos Rust, 420 881 tokens |
| Mediana de fichas por pregunta | 2 491,5 tokens, 0,592 % de la referencia |

El tokenizador es el del Qwen local, sin tokens especiales. La referencia es
un volcado de fuentes; no es un asistente competidor ni coste facturado. Las
preguntas pertenecen al desarrollo y no son un conjunto independiente. No se
midió el éxito de tareas completas ni se evaluó cada afirmación generada.

La inspección posterior encontró tres errores concretos: TypeScript en la
descripción de un extractor integrado para Rust, 1 000 en lugar de la constante
de 24 000 en `read_file`, y separadores mal situados en la explicación de
`stable_id`. También hay frases incompletas. Se conservan las fichas y fuentes
en `fidelidad-observaciones.json`; no se presenta una tasa general de error.

## Reproducir y preparar los documentos

Desde el repositorio, con las fuentes y modelos locales disponibles:

```sh
python3 -m venv /tmp/quiron-tfm-tools
/tmp/quiron-tfm-tools/bin/pip install -r scripts/requirements-evaluacion.txt
/tmp/quiron-tfm-tools/bin/python scripts/evaluate-index.py --start-index --output /tmp/localizacion.json
CARGO_BUILD_JOBS=1 bash scripts/package-linux.sh
```

La evaluación requiere el cerebro y el worker configurados, identidad `.quiron/`
y acceso al token local; no lo imprime ni lo incluye en evidencias. La exportación
necesita las fuentes Liberation Sans/Mono. Los resultados nuevos deben guardarse
en otra ruta para conservar la evidencia de esta revisión.

La memoria del TFM, su fuente Markdown y el exportador que la convierte en DOCX
y PDF se entregan aparte de este repositorio; el exportador incorpora
`docs/ESTUDIO_RED_OBRERA.md` como Anexo A y su informe `exportacion.json`
identifica los hashes. El paquete incluye fuentes, binarios, manuales y
evidencias.

La verificación del archivo de distribución, el staging y la prueba de la
ventana se registra por separado junto al paquete, después de construirlo.
`BUILD.json` y `SHA256SUMS` identifican su contenido. Si cambia el texto, hay
que volver a exportar y empaquetar.

## Pendientes antes de presentar o publicar

1. Completar tutor, repositorio de entrega, horas/costes y reflexión personal del
   autor; revisar y aprobar la redacción. Los campos siguen marcados `[COMPLETAR]`.
2. Ejecutar instalación y reinicio completos en un segundo equipo limpio con
   los almacenes y el worker, conservando evidencia de persistencia y ABI.
3. Completar revisión de licencias de distribución y dependencias transitivas.
4. Corregir o mantener explícitos los límites técnicos de la tabla anterior.
   Para sostener ahorro y calidad, añadir comparación de tareas, referencia
   léxica y evaluación independiente de fidelidad en varios repositorios.

Las evidencias históricas del 5 de septiembre se conservan con su fecha; no
se reetiquetan como pruebas de este cierre. No se ha publicado ni subido el
candidato a un repositorio remoto.
