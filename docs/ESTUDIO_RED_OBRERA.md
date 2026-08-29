# Estudio: la matemática de la red obrera de Quirón

**Algoritmos y ecuaciones de los LLM recientes, y cómo reutilizarlos en nuestro worker**
**TFM Quirón · para la tutoría · 10 de julio de 2026**

Este documento extrae **la matemática** de los mecanismos con los que se han
construido los LLM recientes —no compara modelos— y explica, para cada uno:
**(a)** la ecuación esencial, **(b)** por qué funciona, **(c)** la referencia de
arXiv, y **(d)** cómo se aplica a nuestro worker. El objetivo es reutilizar esas
matemáticas en **una sola red pequeña, local, destilable y exportable a ONNX/Rust**.

## Cómo leer las etiquetas de evidencia

Fue un barrido de fuentes primarias con verificación adversarial (tres pasadas,
~75 afirmaciones sobre >40 fuentes). Cada mecanismo lleva una etiqueta:

- **✅ Verificado** — confirmado 3-0 en la verificación adversarial de este barrido.
- **📐 Fundamental** — matemática canónica establecida (referencia original); **no
  re-verificada** adversarialmente aquí, pero es estándar de libro.
- **⚠️ Principio de diseño** — la idea es reutilizable, pero **no la desplegamos
  tal cual** por fricción con ONNX/Rust o por tamaño.
- **🔬 Cuestión abierta** — debate activo en la literatura; se declara como tal.

> **Honestidad metodológica:** las cifras de los *technical reports* (−93,3 % de
> KV-cache, 25 % de FLOPs, «degradación negligible») son **autoreportadas** por
> sus autores. Las de complejidad (O(n·d²), tamaño de estado d×d) son propiedades
> deterministas de la arquitectura. Se distingue donde importa.

---

## 0. Qué es el worker (marco)

Un **arnés** que se pone al asistente al abrir un proyecto: un daemon continuo, un
**«git vitaminado»** que copia del **registro/líder inmutable** (cadena Blake3, la
verdad) y proyecta sin parar dos vistas **desechables** por proyecto —vectores en
Qdrant, grafo en Neo4j—. Cuando se toca un archivo, crea/actualiza un vector con
el nombre del archivo y le extrae las lógicas de qué hace. **Una sola red**.
**Propone; no ejecuta** — su peor error posible es un grafo que hay que
reproyectar, reversible.

Sus tres prioridades ordenan a qué sirve cada matemática de abajo:
**(1) aislar por proyecto, (2) enlazar/recuperar vectores a escala, (3) etiquetar
qué hace cada archivo sin alucinar.**

---

# FAMILIA 1 — Atención eficiente y gestión del KV-cache

El problema: la atención estándar cuesta **O(n²·d)** y su KV-cache crece **lineal**
con la longitud. Tres formas de romperlo.

## 1.1 Multi-head Latent Attention (MLA) — comprimir el KV · ✅ Verificado

**(a) Ecuación.** En vez de cachear K y V por cabeza, se comprimen juntos a un
latente de bajo rango y se reconstruyen al vuelo:

```
c_t^{KV} = W^{DKV} · h_t            con  d_c ≪ d_h · n_h      (down-projection)
k_t^{C}  = W^{UK} · c_t^{KV}        v_t^{C} = W^{UV} · c_t^{KV}  (up-projections)
k_t^{R}  = RoPE(W^{KR} · h_t)       (clave posicional RoPE, desacoplada y compartida)
```

Solo se **cachea `c_t^{KV}` (pequeño) y la clave RoPE `k_t^{R}`**, no K/V completos.

**(b) Por qué funciona.** K y V de un token viven en un subespacio de dimensión
mucho menor que `d_h·n_h`; una proyección de bajo rango los captura casi sin
pérdida. El RoPE se saca aparte porque su rotación **no puede absorberse** en las
up-projections (depende de la posición). Resultado: **−93,3 %** de KV-cache
respecto a DeepSeek-67B con calidad comparable a MHA.

**(c)** DeepSeek-V2 `arXiv:2405.04434`; DeepSeek-V3 `arXiv:2412.19437`.

**(d) Aplicación al worker.** Son solo *matmuls* → **exportable a ONNX**. Un
KV-cache pequeño abarata el daemon en inferencia autoregresiva en Rust. El truco
de absorción de matrices reduce memoria. *(Matiz ⚠️: la variante con absorción y
RoPE desacoplado no es trivial de exportar; el principio low-rank sí.)*

## 1.2 Native Sparse Attention (NSA) — atención dispersa entrenable · ✅ Verificado

**(a) Ecuación.** Tres ramas combinadas por un *gate* aprendido:

```
o_t* = Σ_{c∈{cmp, slc, win}}  g_t^{c} · Attn(q_t, K̃_t^{c}, Ṽ_t^{c})
g_t^{c} = sigmoid(MLP(x_t)) ∈ [0,1]
```

- `cmp`: compresión gruesa de bloques de tokens (contexto global barato).
- `slc`: selección fina de los **top-n bloques** más relevantes.
- `win`: ventana deslizante (contexto local reciente).

Truco clave: los *scores* de selección salen **gratis** de la rama de compresión,
`p_t^{cmp} = softmax(q_tᵀ K̃_t^{cmp})`, y se reutilizan para rankear los bloques.

**(b) Por qué funciona.** Es **entrenable end-to-end** (el *gate* es diferenciable),
no una poda solo-inferencia; y es *hardware-aligned* (carga bloques KV contiguos
para GQA, aprovechando Tensor Cores).

**(c)** `arXiv:2502.11089` (DeepSeek, ACL 2025 best paper).

**(d) Aplicación al worker.** El **gating diferenciable de ramas** es reutilizable:
permite atender selectivamente a los vectores de archivos relacionados dentro de
un proyecto **sin coste cuadrático**. La selección top-n sirve a la prioridad 2.

## 1.3 Atención lineal / lightning + SSE — estado de tamaño fijo · ✅ Verificado

**(a) Ecuación.** El «truco del producto por la derecha» reordena la asociatividad:

```
O = Norm( (Q Kᵀ) V )   →   O = Norm( Q (Kᵀ V) )        O(n²·d) → O(n·d²)
```

En forma recurrente con decaimiento, el estado es una matriz **d×d de tamaño fijo**:

```
kv_t = λ · kv_{t-1} + k_tᵀ v_t          o_t = q_t · kv_t
```

**(b) Por qué funciona.** `Kᵀ V` es un estado d×d que **no crece con la longitud**:
coste **constante por paso** en inferencia. Lo que se pierde es *retrieval*
asociativo (comprimir a estado fijo difumina). Por eso se **hibrida**: 1 bloque de
atención completa cada 7 lineales (7:1). **SSE** (`arXiv:2507.16577`) mitiga la
pérdida con una actualización de estado **top-k dura** (filas dispersas) que reduce
la interferencia entre informaciones y **desacopla la capacidad de estado del nº de
parámetros**.

**(c)** MiniMax-01 `arXiv:2501.08313`; MiniMax-M1 `arXiv:2506.13585`; Ring-linear
`arXiv:2510.19338`; SSE `arXiv:2507.16577`.

**(d) Aplicación al worker — la pieza más importante de esta familia.** El **estado
recurrente d×d es directamente reutilizable y exportable a ONNX**: da un **encoder
de coste constante por token**, ideal para un daemon que procesa archivos **en
streaming** sin re-materializar matrices n×n. Para «enlazar a escala» (prioridad 2)
conviene un backbone **mayormente lineal con pocas capas de atención completa**.
El top-k duro de SSE ayuda a la prioridad 3 (no mezclar lógicas de archivos
distintos → menos alucinación). *(⚠️ el kernel *lightning* I/O-aware con tiling no
se reproduce fácil en Rust; se toma la forma recurrente, no el kernel.)*

---

# FAMILIA 2 — Mezcla de expertos (MoE) y enrutado

## 2.1 DeepSeekMoE: experto compartido + expertos de grano fino · ✅ Verificado

**(a) Ecuación.**

```
h_t' = u_t + Σ_{i=1}^{N_s} FFN_i^{shared}(u_t) + Σ_{i=1}^{N_r} g_{i,t} · FFN_i^{routed}(u_t)
s_{i,t} = sigmoid(u_tᵀ e_i)              (afinidad token–experto)
g_{i,t} = normaliza( s_{i,t} si i ∈ TopK_r, si no 0 )
```

DeepSeek-V3: `N_s=1` compartido, `N_r=256` enrutados, `K_r=8` activos → **671 B
totales, 37 B activados por token**.

**(b) Por qué funciona.** El experto **compartido** captura lo común; los de
**grano fino** se especializan. Así se **desacopla la capacidad (parámetros) del
cómputo por token**.

**(c)** DeepSeekMoE `arXiv:2401.06066`; DeepSeek-V2/V3 `arXiv:2405.04434` / `2412.19437`.

**(d) Aplicación al worker.** ⚠️ **Principio de diseño, no despliegue.** Un MoE real
es impráctico a 48 GB y su *routing* tiene mala fricción con ONNX/Rust. Pero el
patrón **«una cabeza compartida + cabezas especializadas por tipo de lógica»**
inspira la estructura de **nuestra única red** sin necesidad de *routing* disperso.

## 2.2 Balanceo de carga SIN pérdida auxiliar · ✅ Verificado

**(a) Ecuación.** Se añade un **sesgo por experto `b_i`** solo a la **decisión** de
enrutado, nunca al peso de mezcla:

```
enruta i  ⟺  (s_{i,t} + b_i) ∈ TopK          (el peso sigue siendo s_{i,t}, sin sesgo)
b_i ← b_i − γ  si el experto i está sobrecargado
b_i ← b_i + γ  si está infrautilizado          (γ = 0.001)
```

**(b) Por qué funciona.** La pérdida auxiliar clásica inyecta **gradientes de
interferencia** que dañan el objetivo de lenguaje. Aquí el sesgo cambia **qué**
experto se elige sin contaminar **cuánto** pesa, así el balanceo **no altera el
objetivo**. Eleva el techo de rendimiento.

**(c)** Loss-Free Balancing `arXiv:2408.15664`; DeepSeek-V3 `arXiv:2412.19437`.

**(d) Aplicación al worker.** Si la red usa varias cabezas, este **control por
sesgo** es un *feedback loop* barato para repartir carga **sin término de pérdida
extra** — más simple de implementar y de exportar.

---

# FAMILIA 3 — Destilación profesor → estudiante

Hilo conductor: **nuestro profesor se accede por login (Codex/Claude), sin API de
pago y sin logits**. La pregunta matemática es: ¿qué destilación es posible si solo
tenemos el **texto** que devuelve el profesor?

## 3.1 KD clásica (Hinton): *soft targets* con temperatura · 📐 Fundamental

**(a) Ecuación.**

```
L = (1 − α) · CE(y, σ(z_s)) + α · T² · KL( σ(z_t / T) ‖ σ(z_s / T) )
```

**(b) Por qué funciona.** La temperatura `T` **suaviza** el *softmax* del profesor y
expone su «conocimiento oscuro» (qué clases considera parecidas). El factor **`T²`**
compensa que los gradientes del término *soft* escalan como `1/T²`, para que su
magnitud iguale la del término *hard*.

**(c)** Hinton et al. `arXiv:1503.02531`. *(📐 no re-verificado en este barrido;
fórmula canónica.)*

**(d) Aplicación al worker.** ❌ **No aplicable directamente**: exige los **logits**
del profesor `z_t`, que un profesor por login **no expone**.

## 3.2 Sequence-Level KD — la vía viable sin logits · ✅ Verificado

**(a) Ecuación.** El objetivo exacto suma sobre todas las secuencias (intratable);
se aproxima por la **moda** del profesor, hallada con *beam search* `ŷ`:

```
L_{SEQ-KD} = − Σ_{t} q(t|s) · log p(t|s)     ≈     − log p_s( t = ŷ | s )
```

Es decir: **entrenar al estudiante con cross-entropy sobre el TEXTO que genera el
profesor.**

**(b) Por qué funciona.** Reduce la destilación a *maximum likelihood* sobre las
respuestas del profesor. Procedimiento: (1) el profesor genera, (2) recoges su
texto, (3) entrenas al estudiante con CE sobre ese corpus. **Solo necesita texto.**
*(Matiz honesto: la idea de que «la moda captura casi toda la masa» fue **refutada
0-3** en la verificación; el método se sostiene por **tratabilidad**, no porque
esté probado que concentra la probabilidad.)*

**(c)** Kim & Rush `arXiv:1606.07947`.

**(d) Aplicación al worker.** ✅ **Directamente reutilizable — es nuestra vía.** Es
exactamente el subcaso *off-policy por respuestas* que un profesor por login sí
permite. El estudiante puede ser ~10× más pequeño/rápido con poca pérdida.

## 3.3 Reverse-KL (MiniLLM) y GKD — por qué NO nos sirven · ✅ Verificado

- **MiniLLM** minimiza el **KL inverso** `KL(q_estudiante ‖ p_profesor)`, *mode-seeking*
  (evita que el estudiante ponga masa donde el profesor casi no la tiene). Su
  gradiente es un **policy-gradient tipo REINFORCE**:
  `∇_θ L = E_{x∼q_s}[ ∇_θ log q_s(x) · (log q_s(x) − log p_t(x)) ]`.
  **Es on-policy y necesita `p_t` token a token** → ❌ no viable solo con texto.
  `arXiv:2306.08543`.
- **GKD** generaliza con un objetivo mixto ponderado por `λ`
  (`λ=0` off-policy sobre datos fijos; `λ=1` on-policy sobre lo que genera el
  estudiante), pero **requiere los logits del profesor** sobre esas secuencias.
  ❌ no viable sin logits. El **único subcaso** que sobrevive sin logits es `λ=0`
  con *targets* = secuencias del profesor… que **degenera en Sequence-Level KD**.
  `arXiv:2306.13649`.

## 3.4 Matiz: ¿reverse-KL es realmente mejor? · 🔬 Cuestión abierta

La dicotomía *mode-seeking* (reverse) vs *mode-averaging* (forward) **presupone**
distribuciones continuas y estudiante unimodal. En LLM las distribuciones son
**discretas** (*softmax* sobre vocabulario) y `q` no es unimodal, así que la
motivación de MiniLLM está **en disputa**: forward y reverse KL **comparten el
mismo óptimo** `q_θ = p`. Conclusión práctica: **no sobre-invertir en reverse-KL**;
para un worker destilado solo con respuestas, **Sequence-Level KD (forward
implícito por MLE) es lo defendible**. `arXiv:2404.02657` (COLING 2025).

---

# FAMILIA 4 — Cuantización y representación

## 4.1 AWQ — cuantizar protegiendo canales por activación · ✅ Verificado

**(a) Ecuación.** Reescalado equivalente por canal (sin *mixed precision*):

```
W x  ≈  Q(W · s) · (x / s)             (s > 1 en los canales salientes)
s* = argmin_s ‖ Q(W · s)(x / s) − W x ‖
```

**(b) Por qué funciona.** El error relativo de redondeo de un canal escala como
`1/s`; subir `s` en los canales **salientes** reduce su error. Y los canales
importantes se identifican por la **magnitud de la ACTIVACIÓN**, no del peso
(seleccionar por peso rinde casi como al azar). Ej.: OPT-6.7B, proteger 1 % por
activación baja la perplejidad `23.54 → 11.36`. Es **PTQ**: no reentrena.

**(c)** AWQ `arXiv:2306.00978` (MLSys 2024 best paper).

**(d) Aplicación al worker.** ✅ **Directamente reutilizable** para cuantizar a INT4
el backbone del worker y **caber y correr** en 48 GB / producción.

## 4.2 GPTQ — cuantización PTQ de segundo orden · ✅ Verificado

**(a) Ecuación.** Reconstrucción por capa con información del Hessiano (OBS):

```
argmin_{Ŵ}  ‖ W X − Ŵ X ‖²          H_F = 2 · X_F X_Fᵀ
δ_F = − (w_q − quant(w_q)) / [H_F^{-1}]_{qq} · (H_F^{-1})_{:,q}   (redistribuye el error)
```

**(b) Por qué funciona.** La información de segundo orden **redistribuye el error de
redondeo** a los pesos aún no cuantizados. Como en capas grandes el **orden importa
poco**, se cuantizan todas las filas en el mismo orden → coste
`O(d·col²)` en vez de por peso. Cuantiza GPT-175B a 3-4 bits en ~4 GPU-h.

**(c)** GPTQ `arXiv:2210.17323` (ICLR 2023).

**(d) Aplicación al worker.** ✅ Alternativa/complemento de AWQ para PTQ del backbone.

## 4.3 InfoNCE — la pérdida que da buenos embeddings · 📐 Fundamental

**(a) Ecuación.**

```
L = − log [ exp(sim(q, k⁺)/τ) / Σ_i exp(sim(q, k_i)/τ) ]
```

**(b) Por qué funciona.** Acerca el ancla `q` a su positivo `k⁺` y la aleja de los
negativos `k_i` (los demás del *batch*). La temperatura `τ` controla cuánto se
castiga a los negativos difíciles. Es aprendizaje contrastivo estándar.

**(c)** van den Oord et al. (CPC) `arXiv:1807.03748`. *(📐 canónico, no re-verificado.)*

**(d) Aplicación al worker.** ✅ Base para entrenar los **embeddings de código
1024-dim** que pueblan Qdrant (prioridad 2). Positivos = pares código↔resumen o
código↔código equivalente.

## 4.4 Last-token pooling — de decoder causal a embedding · 📐 Fundamental

**(a) Idea.** En un *decoder* causal, el **estado del último token** `e = h_L`
resume la secuencia, porque la atención causal deja que ese token «vea» todos los
anteriores. Es cómo modelos como e5-mistral / gte-Qwen / jina-code-embeddings
producen un vector desde un backbone autoregresivo.

**(c)** jina-code-embeddings `arXiv:2508.21290`; e5-mistral `arXiv:2401.00368`.
*(📐 no re-verificado.)*

**(d) Aplicación al worker.** Permite **reutilizar el mismo backbone destilado**
tanto para etiquetar (generar) como para vectorizar (embedding), alineando ambas
representaciones. Cuando tocamos un archivo, su vector = `h_L` de su resumen.

## 4.5 Matryoshka (MRL) — un vector, muchas dimensiones · 📐 Fundamental

**(a) Ecuación.** Se entrena con la suma de la pérdida sobre **prefijos anidados**
del vector:

```
L = Σ_{m ∈ {8,16,32,…,1024}}  c_m · L_task( z_{1:m} )
```

**(b) Por qué funciona.** Al forzar que **cada prefijo** `z_{1:m}` sea por sí solo
un buen embedding, la información se ordena por importancia en las primeras
dimensiones. Así el **mismo vector se puede truncar** a 256 o 64 dim **sin
reentrenar**, perdiendo poca calidad.

**(c)** MRL `arXiv:2205.13147`. *(📐 no re-verificado.)*

**(d) Aplicación al worker.** ✅ Vectores **1024-dim truncables**: búsqueda barata
en dimensión baja + reranking en dimensión alta. Escala la prioridad 2 sin duplicar
índices.

---

# Síntesis — la red obrera propuesta

Combinando **solo lo reutilizable**, una **única red pequeña** con esta forma:

1. **Backbone híbrido mayormente lineal** (estado recurrente `d×d`, §1.3) con
   **pocas capas de atención completa** (ratio ~7:1) → **encoder de coste constante
   por token**, exportable a ONNX, que procesa archivos en *streaming* en el daemon.
2. **Una cabeza compartida + cabezas por tipo de lógica** (inspiración del experto
   compartido de §2.1, **sin** *routing* MoE real) para etiquetar.
3. **Embeddings de código** con **InfoNCE (§4.3) + last-token pooling (§4.4) +
   Matryoshka a 1024-dim (§4.5)** → compatibles con Qdrant/`bge-m3` y truncables.
4. **Entrenamiento por Sequence-Level KD (§3.2)** desde el profesor por login (solo
   texto): la **única** destilación viable sin logits.
5. **Despliegue cuantizado con AWQ INT4 (§4.1)** para caber en 48 GB y correr en el
   mismo `ort` de Rust que ya usa `semantic-ia-local`.

## Qué es reutilizable y qué es solo principio de diseño

| Mecanismo | Veredicto para el worker |
| --- | --- |
| Estado lineal recurrente `d×d` (§1.3) | ✅ Directamente reutilizable (encoder ONNX) |
| Selección top-n / gating de NSA (§1.2) | ✅ Reutilizable (atender a vectores relacionados) |
| Sequence-Level KD (§3.2) | ✅ **Nuestra vía de destilación** |
| AWQ INT4 (§4.1) / GPTQ (§4.2) | ✅ Reutilizable (caber y correr) |
| InfoNCE + last-token + Matryoshka (§4.3–4.5) | ✅ Reutilizable (embeddings 1024-dim) |
| MLA / compresión de KV (§1.1) | ⚠️ Idea sí; export ONNX con fricción |
| MoE real / *routing* (§2.1) | ⚠️ Solo el patrón de cabezas; no desplegar |
| Kernel *lightning* I/O-aware (§1.3) | ⚠️ Forma recurrente sí; kernel no |
| Reverse-KL / GKD (§3.3) | ❌ Necesitan logits del profesor |
| KD clásica con temperatura (§3.1) | ❌ Necesita logits del profesor |

---

# Cuestiones abiertas y lagunas honestas

- **Sin respaldo primario en este barrido** (fórmulas fundamentales, incluidas por
  canónicas pero **no re-verificadas** aquí): KD con temperatura `T²` (§3.1),
  InfoNCE (§4.3), last-token pooling (§4.4), Matryoshka (§4.5). Conviene citarlas
  como establecidas, no como «verificadas por nosotros».
- **`forward` vs `reverse` KL** es **debate activo** (§3.4): no cerrar la tesis de
  MiniLLM como hecho.
- **Fricción real ONNX/Rust** de pesos cuantizados por canal (AWQ INT4) y de
  MLA/MoE **no está medida** en fuentes primarias: hay que probarlo nosotros.
- **Fidelidad anti-alucinación** de resúmenes *code-to-text* pequeños: no hay
  evidencia primaria; se medirá con un conjunto etiquetado propio.

---

# Preguntas para el tutor

**Sobre el diseño (de la matemática):**

1. **Destilación viable:** confirmamos que, sin logits del profesor, **solo
   Sequence-Level KD** es aplicable (reverse-KL/GKD quedan fuera). ¿Acepta esta
   restricción como marco, o valora conseguir un profesor local que sí exponga
   logits para poder usar KD con temperatura?
2. **Arquitectura:** ¿le convence un **backbone híbrido lineal + atención completa
   esporádica** como columna del worker, frente a un *encoder* clásico pequeño?
3. **Embeddings:** ¿validamos **Matryoshka a 1024-dim** para no duplicar índices en
   Qdrant, midiendo la pérdida al truncar?

**Sobre la entrega** (dato: el editor `llore` es un **binario Rust nativo** —`winit`
+ `softbuffer` + `cosmic-text`, ni web ni Tauri):

4. ¿Basta entregar el **repositorio que se compila** (`cargo build --release
   --features full`), o el TFM espera un **instalable de Windows** (`.exe`/`.msi`,
   con *cross-compile*) para el tribunal?

---

# Referencias (todas verificadas como fuentes reales)

**Atención y KV:** MLA — DeepSeek-V2 `2405.04434`, DeepSeek-V3 `2412.19437` · NSA
`2502.11089` · Lightning/lineal — MiniMax-01 `2501.08313`, MiniMax-M1 `2506.13585`,
Ring-linear `2510.19338` · SSE `2507.16577`.
**MoE:** DeepSeekMoE `2401.06066` · Loss-Free Balancing `2408.15664`.
**Destilación:** Hinton `1503.02531` · Sequence-Level KD `1606.07947` · MiniLLM
`2306.08543` · GKD `2306.13649` · Forward/Reverse KL `2404.02657`.
**Cuantización y representación:** AWQ `2306.00978` · GPTQ `2210.17323` · InfoNCE
`1807.03748` · e5-mistral `2401.00368` · jina-code-embeddings `2508.21290` ·
Matryoshka `2205.13147`.

---

## Resumen en una frase

Reutilizamos, con su matemática, **cinco piezas** verificadas: **estado lineal
recurrente `d×d`** para un encoder barato en *streaming*, **Sequence-Level KD** como
única destilación posible sin logits, **AWQ INT4** para desplegar, e **InfoNCE +
last-token pooling + Matryoshka** para embeddings de código 1024-dim truncables —
todo en **una sola red**, sobre el `ort` de Rust. MLA, MoE y reverse-KL quedan como
principios de diseño o descartes por fricción con nuestro stack o por el profesor
sin logits.
