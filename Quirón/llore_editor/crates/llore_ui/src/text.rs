//! # Text
//!
//! Renderizado de texto usando cosmic-text.

use crate::{Canvas, Color};
use cosmic_text::{
    Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, SwashContent, Weight, Wrap,
};

/// Interfaz: Archivo, SIL Open Font License 1.1.
///
/// Sustituye a Inter (4-sep-2026). El rediseño «Modernist» apoya su carácter en
/// una grotesca con más personalidad que Inter y, sobre todo, en el peso 800 de
/// los titulares: Inter solo viajaba en Regular y Medium, de modo que no había
/// con qué escribirlos. Los ficheros de Inter siguen en `assets/fonts/` por si
/// hay que volver, pero ya no se empotran.
const ARCHIVO_REGULAR: &[u8] = include_bytes!("../assets/fonts/Archivo-Regular.ttf");
const ARCHIVO_MEDIUM: &[u8] = include_bytes!("../assets/fonts/Archivo-Medium.ttf");
const ARCHIVO_EXTRABOLD: &[u8] = include_bytes!("../assets/fonts/Archivo-ExtraBold.ttf");
/// Código: JetBrains Mono, SIL Open Font License 1.1.
const INTER_REGULAR: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
const INTER_MEDIUM: &[u8] = include_bytes!("../assets/fonts/Inter-Medium.ttf");
const JETBRAINS_MONO_REGULAR: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");
/// Iconos: Lucide, licencia ISC.
const LUCIDE: &[u8] = include_bytes!("../assets/fonts/Lucide.ttf");

/// Familia de los títulos (la marca): Archivo en negrita.
pub const HEADING_FAMILY: &str = "Archivo";

/// Familia de la fuente de iconos, para `Family::Name`.
pub const ICON_FAMILY: &str = "lucide";

/// Sistema de texto global
pub struct TextSystem {
    font_system: FontSystem,
    swash_cache: SwashCache,
}

impl TextSystem {
    pub fn new() -> Self {
        let mut font_system = FontSystem::new();

        // Las tipografías viajan dentro del binario: el editor se ve igual en
        // cualquier máquina, sin depender de lo que haya instalado.
        {
            let database = font_system.db_mut();
            database.load_font_data(ARCHIVO_REGULAR.to_vec());
            database.load_font_data(ARCHIVO_MEDIUM.to_vec());
            database.load_font_data(ARCHIVO_EXTRABOLD.to_vec());
            database.load_font_data(INTER_REGULAR.to_vec());
            database.load_font_data(INTER_MEDIUM.to_vec());
            database.load_font_data(JETBRAINS_MONO_REGULAR.to_vec());
            database.load_font_data(LUCIDE.to_vec());

            // Al redefinir las familias genéricas, todo el código que ya pedía
            // `SansSerif` o `Monospace` adopta las nuevas sin cambiar. El
            // texto va en Inter, hecha para pantalla a tamaños pequeños;
            // Archivo se queda para los títulos, que es donde luce.
            database.set_sans_serif_family("Inter");
            database.set_monospace_family("JetBrains Mono");
        }

        let swash_cache = SwashCache::new();

        Self {
            font_system,
            swash_cache,
        }
    }

    /// Crea un buffer de texto para la UI (SansSerif)
    pub fn create_buffer(&mut self, text: &str, font_size: f32, max_width: f32) -> Buffer {
        // Párrafos (chat, manual): interlineado holgado, que descansa la vista.
        let metrics = Metrics::new(font_size, font_size * 1.4);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);

        buffer.set_size(&mut self.font_system, Some(max_width), None);
        // By default UI elements use regular sans-serif for better readability
        buffer.set_text(
            &mut self.font_system,
            text,
            Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
        );
        buffer.shape_until_scroll(&mut self.font_system, false);

        buffer
    }

    /// Crea un buffer de una sola línea, recortado con puntos suspensivos.
    ///
    /// Las etiquetas de la interfaz —nombres de archivo, pestañas, rutas— nunca
    /// deben envolverse: la fila que las contiene tiene altura fija, de modo que
    /// una segunda línea se pinta encima de la fila siguiente.
    ///
    /// El recorte se calcula midiendo, no estimando: una tipografía proporcional
    /// no permite deducir cuántos caracteres caben a partir de su número.
    pub fn create_line_buffer(&mut self, text: &str, font_size: f32, max_width: f32) -> Buffer {
        let completo = self.line_buffer_raw(text, font_size, max_width);
        if Self::buffer_width(&completo) <= max_width {
            return completo;
        }

        let caracteres: Vec<char> = text.chars().collect();
        let (mut bajo, mut alto) = (0usize, caracteres.len());

        // El mayor prefijo que, con la elipsis, sigue cabiendo.
        while bajo < alto {
            let medio = (bajo + alto).div_ceil(2);
            let candidato: String = caracteres[..medio].iter().collect::<String>() + "…";
            let ancho = Self::buffer_width(&self.line_buffer_raw(&candidato, font_size, max_width));
            if ancho <= max_width {
                bajo = medio;
            } else {
                alto = medio - 1;
            }
        }

        let recortado: String = caracteres[..bajo].iter().collect::<String>() + "…";
        self.line_buffer_raw(&recortado, font_size, max_width)
    }

    /// Buffer sin envolver, del ancho que necesite.
    fn line_buffer_raw(&mut self, text: &str, font_size: f32, max_width: f32) -> Buffer {
        let metrics = Metrics::new(font_size, font_size * 1.2);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);

        buffer.set_wrap(&mut self.font_system, Wrap::None);
        buffer.set_size(&mut self.font_system, Some(max_width.max(1.0)), None);
        buffer.set_text(
            &mut self.font_system,
            text,
            Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
        );
        buffer.shape_until_scroll(&mut self.font_system, false);

        buffer
    }

    fn buffer_width(buffer: &Buffer) -> f32 {
        buffer
            .layout_runs()
            .map(|run| run.line_w)
            .fold(0.0f32, f32::max)
    }

    /// Crea un buffer de titular: misma familia, peso 800.
    ///
    /// Las demás llamadas de dibujo no piden peso y heredan el regular; esta es
    /// la única que sube a ExtraBold. El interlineado de 1,12 es el que fija la
    /// maqueta para los encabezados, más apretado que el 1,2 del cuerpo: un
    /// titular grande con interlineado de cuerpo se deshilacha.
    pub fn create_heading_buffer(&mut self, text: &str, font_size: f32, max_width: f32) -> Buffer {
        let metrics = Metrics::new(font_size, font_size * 1.12);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);

        buffer.set_size(&mut self.font_system, Some(max_width), None);
        buffer.set_text(
            &mut self.font_system,
            text,
            Attrs::new()
                .family(Family::Name(HEADING_FAMILY))
                .weight(Weight::EXTRA_BOLD),
            Shaping::Advanced,
        );
        buffer.shape_until_scroll(&mut self.font_system, false);

        buffer
    }

    /// Crea un buffer de etiqueta: una línea en peso medio.
    ///
    /// Los rótulos de sección y de rol de la maqueta —SESIONES, ARCHIVOS,
    /// TÚ, QUIRÓN— van a 10,5 px en peso 600 y mayúsculas. Archivo viaja en
    /// Regular, Medium y ExtraBold: Medium es el más cercano. La maqueta les
    /// da además un tracking de 0,14 em que cosmic-text 0.12 no expone; se
    /// queda sin él, y el que quiera notarlo tendrá que poner los dos lado a
    /// lado.
    pub fn create_label_buffer(&mut self, text: &str, font_size: f32, max_width: f32) -> Buffer {
        let metrics = Metrics::new(font_size, font_size * 1.2);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);

        buffer.set_wrap(&mut self.font_system, Wrap::None);
        buffer.set_size(&mut self.font_system, Some(max_width.max(1.0)), None);
        buffer.set_text(
            &mut self.font_system,
            text,
            Attrs::new().family(Family::SansSerif).weight(Weight::MEDIUM),
            Shaping::Advanced,
        );
        buffer.shape_until_scroll(&mut self.font_system, false);

        buffer
    }

    /// Crea un buffer con un glifo de la fuente de iconos.
    ///
    /// Los iconos no se envuelven ni se justifican: se piden por su punto de
    /// código y se dibujan como cualquier otro texto.
    pub fn create_icon_buffer(&mut self, icon: char, font_size: f32) -> Buffer {
        let metrics = Metrics::new(font_size, font_size);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);

        buffer.set_size(&mut self.font_system, Some(font_size * 2.0), None);
        buffer.set_text(
            &mut self.font_system,
            &icon.to_string(),
            Attrs::new().family(Family::Name(ICON_FAMILY)),
            Shaping::Advanced,
        );
        buffer.shape_until_scroll(&mut self.font_system, false);

        buffer
    }

    /// Crea un buffer de texto para código (Monospace)
    pub fn create_code_buffer(&mut self, text: &str, font_size: f32, max_width: f32) -> Buffer {
        let metrics = Metrics::new(font_size, font_size * 1.2);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);

        buffer.set_size(&mut self.font_system, Some(max_width), None);
        buffer.set_text(
            &mut self.font_system,
            text,
            Attrs::new().family(Family::Monospace),
            Shaping::Advanced,
        );
        buffer.shape_until_scroll(&mut self.font_system, false);

        buffer
    }

    /// Dibuja un buffer de texto en el canvas
    pub fn draw_buffer(
        &mut self,
        canvas: &mut Canvas,
        buffer: &Buffer,
        x: f32,
        y: f32,
        color: Color,
    ) {
        self.draw_buffer_range(canvas, buffer, x, y, color, None);
    }

    /// Como `draw_buffer`, pero solo pinta las líneas cuya base cae dentro de
    /// `[y_min, y_max]`. El lienzo no recorta, y un hilo desplazado necesita
    /// que las líneas fuera de la banda visible no se dibujen.
    pub fn draw_buffer_within(
        &mut self,
        canvas: &mut Canvas,
        buffer: &Buffer,
        x: f32,
        y: f32,
        color: Color,
        y_min: f32,
        y_max: f32,
    ) {
        self.draw_buffer_range(canvas, buffer, x, y, color, Some((y_min, y_max)));
    }

    fn draw_buffer_range(
        &mut self,
        canvas: &mut Canvas,
        buffer: &Buffer,
        x: f32,
        y: f32,
        color: Color,
        range: Option<(f32, f32)>,
    ) {
        // Collect glyph info first to avoid borrow issues
        let mut glyphs_to_draw = Vec::new();

        // `glyph.physical` no incorpora la posición vertical de su línea: sin
        // ella, un texto envuelto pinta todas sus líneas sobre la misma base.
        // Se desplaza cada línea respecto a la primera, de modo que `y` sigue
        // siendo la base de la línea inicial y el texto de una sola línea no se
        // mueve.
        let first_line_y = buffer
            .layout_runs()
            .next()
            .map(|run| run.line_y)
            .unwrap_or(0.0);

        // El lienzo trabaja en coordenadas lógicas. `physical` escala la posición
        // del glifo dentro de su línea y el tamaño al que se rasteriza, pero
        // **no** el desplazamiento que se le pasa: ese hay que escalarlo aquí.
        //
        // Rasterizar al tamaño real, en lugar de estirar un mapa de bits, es lo
        // que permite agrandar la interfaz sin emborronar las letras.
        let scale = canvas.scale();

        for run in buffer.layout_runs() {
            let line_offset = run.line_y - first_line_y;
            if let Some((y_min, y_max)) = range {
                let base = y + line_offset;
                if base < y_min || base > y_max {
                    continue;
                }
            }
            let origin = (x * scale, (y + line_offset) * scale);
            for glyph in run.glyphs.iter() {
                let physical_glyph = glyph.physical(origin, scale);
                glyphs_to_draw.push(physical_glyph);
            }
        }

        // Now draw each glyph
        for physical_glyph in glyphs_to_draw {
            let Some(image) = self
                .swash_cache
                .get_image(&mut self.font_system, physical_glyph.cache_key)
            else {
                continue;
            };

            let gx = physical_glyph.x + image.placement.left;
            let gy = physical_glyph.y - image.placement.top;
            let w = image.placement.width;
            let h = image.placement.height;
            if w == 0 || h == 0 {
                continue;
            }

            match image.content {
                SwashContent::Mask | SwashContent::SubpixelMask => {
                    let expected = (w * h) as usize;
                    if image.data.len() >= expected {
                        canvas.blend_mask(gx, gy, w, h, &image.data[..expected], color);
                    }
                }
                // Los glifos en color —emoji— traen RGBA por píxel y llevan su
                // propio color; sólo se toma su cobertura para no teñirlos.
                SwashContent::Color => {
                    let expected = (w * h * 4) as usize;
                    if image.data.len() >= expected {
                        let coverage: Vec<u8> =
                            image.data[..expected].chunks_exact(4).map(|px| px[3]).collect();
                        canvas.blend_mask(gx, gy, w, h, &coverage, color);
                    }
                }
            }
        }
    }

    /// Mide el tamaño de un texto (SansSerif)
    pub fn measure(&mut self, text: &str, font_size: f32, max_width: f32) -> (f32, f32) {
        let buffer = self.create_buffer(text, font_size, max_width);

        let mut width = 0.0f32;
        let mut height = 0.0f32;

        for run in buffer.layout_runs() {
            width = width.max(run.line_w);
            height += run.line_height;
        }

        (width, height)
    }

    /// Mide el tamaño de un texto (Monospace)
    pub fn measure_code(&mut self, text: &str, font_size: f32, max_width: f32) -> (f32, f32) {
        let buffer = self.create_code_buffer(text, font_size, max_width);

        let mut width = 0.0f32;
        let mut height = 0.0f32;

        for run in buffer.layout_runs() {
            width = width.max(run.line_w);
            height += run.line_height;
        }

        (width, height)
    }
}

impl Default for TextSystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE_HEIGHT_12: f32 = 12.0 * 1.2;

    /// Filas del lienzo que contienen al menos un píxel pintado.
    fn filas_con_tinta(canvas: &Canvas) -> Vec<u32> {
        let width = canvas.width() as usize;
        canvas
            .data()
            .chunks_exact(4)
            .enumerate()
            .filter(|(_, pixel)| pixel[3] > 0)
            .map(|(index, _)| (index / width) as u32)
            .collect()
    }

    #[test]
    fn las_tipografias_empotradas_estan_registradas() {
        let text_system = TextSystem::new();
        let familias: Vec<String> = text_system
            .font_system
            .db()
            .faces()
            .flat_map(|face| face.families.iter().map(|(name, _)| name.clone()))
            .collect();

        for esperada in ["Archivo", "JetBrains Mono", ICON_FAMILY] {
            assert!(
                familias.iter().any(|nombre| nombre == esperada),
                "falta la familia empotrada {esperada}"
            );
        }
    }

    /// Mide el coste de un fotograma de interfaz. No es una aserción: se ejecuta
    /// a mano con `cargo test -- --ignored --nocapture medir_coste_del_fotograma`
    /// para decidir dónde optimizar antes de cambiar de motor de dibujo.
    #[test]
    #[ignore]
    fn medir_coste_del_fotograma() {
        use std::time::Instant;

        let mut text_system = TextSystem::new();
        let etiquetas: Vec<String> = (0..71).map(|i| format!("Etiqueta de interfaz {i}")).collect();

        // Fase 1: modelado del texto (shaping), lo que hoy se repite cada fotograma.
        let inicio = Instant::now();
        let buffers: Vec<Buffer> = etiquetas
            .iter()
            .map(|texto| text_system.create_buffer(texto, 12.0, 240.0))
            .collect();
        let shaping = inicio.elapsed();

        // Fase 2: composición de glifos sobre el lienzo.
        let mut canvas = Canvas::new(1280, 720);
        let inicio = Instant::now();
        for (index, buffer) in buffers.iter().enumerate() {
            let y = 12.0 + (index % 40) as f32 * 16.0;
            text_system.draw_buffer(&mut canvas, buffer, 8.0, y, Color::new(220, 220, 220));
        }
        let composicion = inicio.elapsed();

        // Fase 3: el mismo modelado, con los glifos ya en la caché de swash.
        let inicio = Instant::now();
        let _: Vec<Buffer> = etiquetas
            .iter()
            .map(|texto| text_system.create_buffer(texto, 12.0, 240.0))
            .collect();
        let shaping_caliente = inicio.elapsed();

        println!("\n  modelado (71 etiquetas, frío):    {shaping:?}");
        println!("  composición de glifos:            {composicion:?}");
        println!("  modelado (71 etiquetas, caliente): {shaping_caliente:?}");
        println!(
            "  total por fotograma:              {:?}\n",
            shaping_caliente + composicion
        );
    }

    #[test]
    fn un_icono_de_lucide_se_rasteriza() {
        let mut text_system = TextSystem::new();

        let mut canvas = Canvas::new(40, 40);
        let buffer = text_system.create_icon_buffer(crate::icons::EXPLORER, 18.0);
        text_system.draw_buffer(&mut canvas, &buffer, 8.0, 26.0, Color::new(255, 255, 255));

        let pintados = filas_con_tinta(&canvas).len();
        assert!(
            pintados > 4,
            "el glifo de icono no se dibujó: {pintados} filas con tinta"
        );
    }

    #[test]
    fn el_texto_envuelto_reparte_sus_lineas_en_vertical() {
        let mut text_system = TextSystem::new();
        let texto = "palabra ".repeat(12);
        let ancho = 90.0;

        let (_, alto) = text_system.measure(&texto, 12.0, ancho);
        assert!(alto > 2.0 * LINE_HEIGHT_12, "el texto debe envolver: {alto}");

        let mut canvas = Canvas::new(120, 140);
        let buffer = text_system.create_buffer(&texto, 12.0, ancho);
        text_system.draw_buffer(&mut canvas, &buffer, 4.0, 14.0, Color::new(255, 255, 255));

        let filas = filas_con_tinta(&canvas);
        assert!(!filas.is_empty(), "no se pintó ningún glifo");

        let primera = *filas.iter().min().expect("hay tinta");
        let ultima = *filas.iter().max().expect("hay tinta");
        let reparto = (ultima - primera) as f32;

        // Apiladas sobre la misma base, todas las líneas cabrían en la altura
        // de un solo renglón. Ese era el defecto: el chat se pintaba encima de
        // sí mismo.
        assert!(
            reparto > 2.0 * LINE_HEIGHT_12,
            "las líneas se solapan: tinta repartida en {reparto} px, entre {primera} y {ultima}"
        );
    }

    #[test]
    fn una_etiqueta_larga_se_recorta_en_una_sola_linea() {
        let mut text_system = TextSystem::new();
        let nombre = "neostore.labeltokenstore.db.names.id";
        let ancho = 90.0;

        let buffer = text_system.create_line_buffer(nombre, 12.0, ancho);

        let lineas = buffer.layout_runs().count();
        assert_eq!(lineas, 1, "una etiqueta no debe envolver: {lineas} líneas");

        let ancho_real = TextSystem::buffer_width(&buffer);
        assert!(ancho_real <= ancho, "se sale del hueco: {ancho_real} > {ancho}");
    }

    #[test]
    fn una_etiqueta_que_cabe_no_se_toca() {
        let mut text_system = TextSystem::new();
        let buffer = text_system.create_line_buffer("main.rs", 12.0, 200.0);

        let texto: String = buffer
            .layout_runs()
            .flat_map(|run| run.glyphs.iter().map(|g| &run.text[g.start..g.end]))
            .collect();
        assert_eq!(texto, "main.rs", "no debe recortarse lo que cabe");
    }

    #[test]
    fn el_recorte_no_desborda_aunque_el_hueco_sea_diminuto() {
        let mut text_system = TextSystem::new();
        let buffer = text_system.create_line_buffer("neostore.labeltokenstore", 12.0, 8.0);

        assert_eq!(buffer.layout_runs().count(), 1);
    }

    #[test]
    fn la_escala_del_lienzo_agranda_el_texto_y_lo_reposiciona() {
        let mut text_system = TextSystem::new();

        // Mismo texto y mismas coordenadas lógicas, dos escalas.
        let mut natural = Canvas::new(200, 200);
        let buffer = text_system.create_buffer("Hola", 12.0, 100.0);
        text_system.draw_buffer(&mut natural, &buffer, 10.0, 40.0, Color::new(255, 255, 255));

        let mut doble = Canvas::new(200, 200);
        doble.set_scale(2.0);
        let buffer = text_system.create_buffer("Hola", 12.0, 100.0);
        text_system.draw_buffer(&mut doble, &buffer, 10.0, 40.0, Color::new(255, 255, 255));

        let base_natural = *filas_con_tinta(&natural).iter().max().expect("tinta");
        let base_doble = *filas_con_tinta(&doble).iter().max().expect("tinta");

        // La base de la línea lógica y=40 cae cerca de y=80 en el lienzo doble.
        assert!(
            (base_doble as i32 - 2 * base_natural as i32).abs() <= 4,
            "la posición no escaló: {base_natural} -> {base_doble}"
        );

        // Y el glifo se rasteriza más grande, no se estira: ocupa más filas.
        let alto_natural = filas_con_tinta(&natural).len();
        let alto_doble = filas_con_tinta(&doble).len();
        assert!(
            alto_doble > alto_natural + 2,
            "el glifo no creció: {alto_natural} -> {alto_doble} filas"
        );
    }

    #[test]
    fn una_sola_linea_conserva_su_base() {
        let mut text_system = TextSystem::new();

        let mut canvas = Canvas::new(120, 80);
        let buffer = text_system.create_buffer("Hola", 12.0, 100.0);
        text_system.draw_buffer(&mut canvas, &buffer, 4.0, 40.0, Color::new(255, 255, 255));

        let filas = filas_con_tinta(&canvas);
        assert!(!filas.is_empty(), "no se pintó ningún glifo");

        let primera = *filas.iter().min().expect("hay tinta");
        let ultima = *filas.iter().max().expect("hay tinta");

        // `y` es la base de la primera línea. Corregir el reparto vertical no
        // debe desplazar los rótulos de una línea ya alineados en la interfaz.
        assert!(primera >= 26, "el texto subió demasiado: {primera}");
        assert!(ultima <= 42, "el texto bajó respecto a su base: {ultima}");
    }
}
