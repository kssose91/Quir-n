//! # Paint
//!
//! Canvas para renderizado 2D con tiny-skia.

use crate::Bounds;
use std::collections::HashMap;
use tiny_skia::{
    FillRule, IntSize, Paint, PathBuilder, Pixmap, PixmapPaint, PremultipliedColorU8, Rect, Stroke, Transform,
};
use tiny_skia::{GradientStop, Point, RadialGradient, SpreadMode};

/// Color RGBA
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const WHITE: Color = Color {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    pub const BLACK: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const TRANSPARENT: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };

    // Llore theme colors
    pub const PRIMARY: Color = Color {
        r: 88,
        g: 166,
        b: 255,
        a: 255,
    }; // #58A6FF
    pub const SECONDARY: Color = Color {
        r: 136,
        g: 46,
        b: 224,
        a: 255,
    }; // #882EE0
    pub const BACKGROUND: Color = Color {
        r: 13,
        g: 17,
        b: 23,
        a: 255,
    }; // #0D1117
    pub const SURFACE: Color = Color {
        r: 22,
        g: 27,
        b: 34,
        a: 255,
    }; // #161B22
    pub const TEXT: Color = Color {
        r: 201,
        g: 209,
        b: 217,
        a: 255,
    }; // #C9D1D9
    pub const TEXT_MUTED: Color = Color {
        r: 139,
        g: 148,
        b: 158,
        a: 255,
    }; // #8B949E

    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub fn with_alpha(mut self, alpha: u8) -> Self {
        self.a = alpha;
        self
    }

    pub fn from_hex(hex: u32) -> Self {
        Self {
            r: ((hex >> 16) & 0xFF) as u8,
            g: ((hex >> 8) & 0xFF) as u8,
            b: (hex & 0xFF) as u8,
            a: 255,
        }
    }

    fn to_skia_color(self) -> tiny_skia::Color {
        tiny_skia::Color::from_rgba8(self.r, self.g, self.b, self.a)
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::BLACK
    }
}

/// Canvas para dibujar primitivas 2D
///
/// El lienzo tiene un tamaño en píxeles físicos y un factor de escala. Todo lo
/// que se dibuja se expresa en coordenadas lógicas y el lienzo lo escala: así la
/// interfaz entera crece o encoge —letras y huecos a la vez— sin tocar una sola
/// de las coordenadas del render.
pub struct Canvas {
    pixmap: Pixmap,
    scale: f32,
    /// Anillos de sombra ya rasterizados, por tamaño físico, radio, difuminado,
    /// caída y color. Una sombra se calcula una vez y después se estampa por
    /// copia: medido, la de la tarjeta del editor pasaba de 11 ms a rasterizar
    /// cada fotograma a una fracción de milisegundo estampada.
    shadow_cache: HashMap<ShadowKey, Vec<Tira>>,
}

/// Clave de una sombra cacheada. Todo en píxeles físicos enteros: dos tarjetas
/// del mismo tamaño comparten el anillo aunque estén en sitios distintos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ShadowKey {
    width: u32,
    height: u32,
    radius: u32,
    blur: u32,
    offset_y: i32,
    color: [u8; 4],
}

/// Una tira del anillo ya rasterizada, con su desplazamiento respecto a la
/// esquina del mapa completo.
///
/// Estampar el anillo entero costaba más que rasterizarlo: tiny-skia pasa cada
/// píxel del mapa por su pipeline de mezcla, interior transparente incluido
/// —medido, 4,4 ms en release para la tarjeta del editor—. En tiras solo
/// viajan los píxeles del borde: arriba y abajo con las esquinas, y los dos
/// laterales entre ellas. Unos 57 mil frente a 630 mil.
struct Tira {
    dx: i32,
    dy: i32,
    mapa: Pixmap,
}

/// Tope de la caché: redimensionar una ventana genera un tamaño por fotograma
/// y sin tope la memoria crecería con cada píxel de ancho.
const SHADOW_CACHE_MAX: usize = 24;

impl Canvas {
    pub fn new(width: u32, height: u32) -> Self {
        let pixmap = Pixmap::new(width, height).expect("Failed to create pixmap");
        Self {
            pixmap,
            scale: 1.0,
            shadow_cache: HashMap::new(),
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if self.pixmap.width() != width || self.pixmap.height() != height {
            self.pixmap = Pixmap::new(width, height).expect("Failed to resize pixmap");
        }
    }

    /// Factor por el que se multiplica cada coordenada lógica.
    pub fn scale(&self) -> f32 {
        self.scale
    }

    pub fn set_scale(&mut self, scale: f32) {
        if scale.is_finite() && scale > 0.0 {
            self.scale = scale;
        }
    }

    /// Transformación aplicada a cada primitiva.
    fn transform(&self) -> Transform {
        Transform::from_scale(self.scale, self.scale)
    }

    /// Anchura en píxeles físicos.
    pub fn width(&self) -> u32 {
        self.pixmap.width()
    }

    /// Altura en píxeles físicos.
    pub fn height(&self) -> u32 {
        self.pixmap.height()
    }

    /// Limpia el canvas con un color
    pub fn clear(&mut self, color: Color) {
        self.pixmap.fill(color.to_skia_color());
    }

    /// Compone una máscara de cobertura de 8 bits sobre el lienzo.
    ///
    /// Es la operación que necesita el texto: cada byte de `mask` indica cuánto
    /// cubre el glifo ese píxel. Se mezcla con `source-over` conservando el
    /// antialiasing completo. Dibujar en su lugar un rectángulo por píxel, y
    /// descartar las coberturas bajas, recorta los bordes de las letras y las
    /// vuelve ilegibles en tamaños pequeños.
    pub fn blend_mask(&mut self, x: i32, y: i32, width: u32, height: u32, mask: &[u8], color: Color) {
        if width == 0 || height == 0 || color.a == 0 {
            return;
        }

        let canvas_w = self.pixmap.width() as i32;
        let canvas_h = self.pixmap.height() as i32;
        let pixels = self.pixmap.pixels_mut();

        for row in 0..height as i32 {
            let target_y = y + row;
            if target_y < 0 || target_y >= canvas_h {
                continue;
            }
            for column in 0..width as i32 {
                let target_x = x + column;
                if target_x < 0 || target_x >= canvas_w {
                    continue;
                }

                let coverage = mask[(row as u32 * width + column as u32) as usize] as u32;
                if coverage == 0 {
                    continue;
                }

                let source_alpha = coverage * color.a as u32 / 255;
                if source_alpha == 0 {
                    continue;
                }

                let index = (target_y * canvas_w + target_x) as usize;
                let destination = pixels[index];
                let inverse = 255 - source_alpha;

                // Componentes de origen premultiplicados por su propio alfa.
                let red = color.r as u32 * source_alpha / 255
                    + destination.red() as u32 * inverse / 255;
                let green = color.g as u32 * source_alpha / 255
                    + destination.green() as u32 * inverse / 255;
                let blue = color.b as u32 * source_alpha / 255
                    + destination.blue() as u32 * inverse / 255;
                let alpha = source_alpha + destination.alpha() as u32 * inverse / 255;

                if let Some(blended) = PremultipliedColorU8::from_rgba(
                    red.min(alpha) as u8,
                    green.min(alpha) as u8,
                    blue.min(alpha) as u8,
                    alpha as u8,
                ) {
                    pixels[index] = blended;
                }
            }
        }
    }

    /// Dibuja un rectángulo relleno
    pub fn fill_rect(&mut self, bounds: Bounds, color: Color) {
        let Some(rect) = Rect::from_xywh(bounds.x, bounds.y, bounds.width, bounds.height) else {
            return;
        };
        let path = {
            let mut pb = PathBuilder::new();
            pb.push_rect(rect);
            pb.finish().unwrap()
        };

        let mut paint = Paint::default();
        paint.set_color(color.to_skia_color());

        self.pixmap.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            self.transform(),
            None,
        );
    }

    /// Dibuja un círculo relleno, con antialiasing: para puntos pequeños (2–6 px)
    /// un rectángulo se ve como un píxel cuadrado; esto se ve redondo.
    pub fn fill_circle(&mut self, cx: f32, cy: f32, radius: f32, color: Color) {
        let Some(path) = PathBuilder::from_circle(cx, cy, radius) else {
            return;
        };
        let mut paint = Paint::default();
        paint.set_color(color.to_skia_color());
        self.pixmap.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            self.transform(),
            None,
        );
    }

    /// Dibuja una bola: un círculo con degradado radial —brillo desplazado
    /// arriba a la izquierda y sombra hacia el borde— que a 3–8 px se lee con
    /// volumen, no como un disco plano. Conserva el alfa del color.
    pub fn fill_sphere(&mut self, cx: f32, cy: f32, radius: f32, color: Color) {
        let Some(path) = PathBuilder::from_circle(cx, cy, radius) else {
            return;
        };
        let mezcla = |hacia: u8, k: f32| -> tiny_skia::Color {
            let m = |v: u8| (v as f32 + (hacia as f32 - v as f32) * k).round().clamp(0.0, 255.0) as u8;
            tiny_skia::Color::from_rgba8(m(color.r), m(color.g), m(color.b), color.a)
        };
        let sombreado = RadialGradient::new(
            Point::from_xy(cx - radius * 0.42, cy - radius * 0.42),
            Point::from_xy(cx, cy),
            radius * 1.15,
            vec![
                GradientStop::new(0.0, mezcla(255, 0.62)),
                GradientStop::new(0.45, color.to_skia_color()),
                GradientStop::new(1.0, mezcla(0, 0.45)),
            ],
            SpreadMode::Pad,
            self.transform(),
        );
        let mut paint = Paint::default();
        match sombreado {
            Some(shader) => paint.shader = shader,
            None => paint.set_color(color.to_skia_color()),
        }
        self.pixmap.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            self.transform(),
            None,
        );
    }

    /// Dibuja un rectángulo con bordes redondeados
    pub fn fill_rounded_rect(&mut self, bounds: Bounds, radius: f32, color: Color) {
        if radius <= 0.0 {
            self.fill_rect(bounds, color);
            return;
        }

        let path = {
            let mut pb = PathBuilder::new();
            let r = radius.min(bounds.width / 2.0).min(bounds.height / 2.0);
            let x = bounds.x;
            let y = bounds.y;
            let w = bounds.width;
            let h = bounds.height;

            pb.move_to(x + r, y);
            pb.line_to(x + w - r, y);
            pb.quad_to(x + w, y, x + w, y + r);
            pb.line_to(x + w, y + h - r);
            pb.quad_to(x + w, y + h, x + w - r, y + h);
            pb.line_to(x + r, y + h);
            pb.quad_to(x, y + h, x, y + h - r);
            pb.line_to(x, y + r);
            pb.quad_to(x, y, x + r, y);
            pb.close();

            pb.finish().unwrap()
        };

        let mut paint = Paint::default();
        paint.set_color(color.to_skia_color());

        self.pixmap.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            self.transform(),
            None,
        );
    }

    /// Dibuja el borde de un rectángulo
    /// Sombra suave bajo un rectángulo redondeado.
    ///
    /// El rediseño «Modernist» apoya su jerarquía en tarjetas que flotan: la
    /// bandeja de escritura, el editor, los paneles. Sin sombra, «flotar» no se
    /// distingue de «estar pegado», y todo vuelve a leerse plano.
    ///
    /// tiny-skia no trae desenfoque, así que se aproxima apilando capas: varias
    /// siluetas concéntricas, cada una un poco mayor y muy transparente. Con
    /// ocho capas el degradado ya no se ve escalonado a los tamaños que usa la
    /// interfaz, y sale mucho más barato que desenfocar un mapa de bits.
    ///
    /// `blur` es el radio de difuminado y `offset_y` cuánto cae la sombra, en
    /// las mismas unidades que CSS: una sombra `0 3px 10px` se pide con
    /// `offset_y = 3.0` y `blur = 10.0`.
    pub fn drop_shadow(
        &mut self,
        bounds: Bounds,
        radius: f32,
        offset_y: f32,
        blur: f32,
        color: Color,
    ) {
        const CAPAS: usize = 8;
        if blur <= 0.0 || color.a == 0 {
            return;
        }
        // El alfa se reparte entre las capas. Como se superponen, el borde
        // exterior queda casi transparente y el interior acumula el color:
        // eso es la sombra.
        //
        // Cada capa es un anillo, no una losa: la silueta exterior menos la
        // propia tarjeta, con relleno par-impar. La tarjeta se pinta opaca
        // encima, así que rellenar su interior ocho veces con mezcla alfa era
        // trabajo tirado — medido, 17 ms por fotograma en la tarjeta del
        // editor, 130 veces su propio relleno. El anillo solo toca el borde.
        // Todo en píxeles físicos: el anillo se rasteriza a la escala real y
        // se estampa sin transformar, así que no se emborrona al ampliar.
        let k = self.scale;
        let key = ShadowKey {
            width: (bounds.width * k).round().max(1.0) as u32,
            height: (bounds.height * k).round().max(1.0) as u32,
            radius: (radius * k).round() as u32,
            blur: (blur * k).round().max(1.0) as u32,
            offset_y: (offset_y * k).round() as i32,
            color: [color.r, color.g, color.b, color.a],
        };
        if !self.shadow_cache.contains_key(&key) {
            if self.shadow_cache.len() >= SHADOW_CACHE_MAX {
                self.shadow_cache.clear();
            }
            if let Some(anillo) = Self::rasterizar_anillo(key, CAPAS) {
                self.shadow_cache.insert(key, Self::trocear_en_tiras(&anillo, key));
            }
        }
        let Some(tiras) = self.shadow_cache.get(&key) else {
            return;
        };
        // El mapa lleva un margen de `blur` alrededor de la tarjeta (más la
        // caída abajo): su esquina va `blur` píxeles arriba y a la izquierda.
        let x = (bounds.x * k).round() as i32 - key.blur as i32;
        let y = (bounds.y * k).round() as i32 - key.blur as i32;
        let paint = PixmapPaint::default();
        for tira in tiras {
            self.pixmap.draw_pixmap(
                x + tira.dx,
                y + tira.dy,
                tira.mapa.as_ref(),
                &paint,
                Transform::identity(),
                None,
            );
        }
    }

    /// Recorta del anillo completo las cuatro tiras que contienen sombra. Las
    /// horizontales llevan las esquinas (el anillo entra `radius` en la caja de
    /// la tarjeta por ellas); las verticales van entre ambas.
    fn trocear_en_tiras(anillo: &Pixmap, key: ShadowKey) -> Vec<Tira> {
        let m = key.blur;
        let (w, h) = (key.width, key.height);
        let r = key.radius.min(w / 2).min(h / 2);
        let ancho = anillo.width();
        let alto = anillo.height();
        let alto_arriba = (m + r).min(alto);
        let y_abajo = (m + h).saturating_sub(r).min(alto);
        let mut tiras = Vec::with_capacity(4);
        let mut meter = |x0: u32, y0: u32, tw: u32, th: u32| {
            if tw == 0 || th == 0 {
                return;
            }
            if let Some(mapa) = Self::recortar(anillo, x0, y0, tw, th) {
                tiras.push(Tira {
                    dx: x0 as i32,
                    dy: y0 as i32,
                    mapa,
                });
            }
        };
        meter(0, 0, ancho, alto_arriba);
        meter(0, y_abajo, ancho, alto.saturating_sub(y_abajo));
        let alto_lateral = y_abajo.saturating_sub(alto_arriba);
        meter(0, alto_arriba, m.min(ancho), alto_lateral);
        meter(ancho.saturating_sub(m), alto_arriba, m.min(ancho), alto_lateral);
        tiras
    }

    /// Copia un rectángulo de un mapa a otro nuevo, byte a byte: el formato es
    /// el mismo (RGBA premultiplicado), así que no hay conversión.
    fn recortar(mapa: &Pixmap, x0: u32, y0: u32, w: u32, h: u32) -> Option<Pixmap> {
        let stride = mapa.width() as usize * 4;
        let src = mapa.data();
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for fila in y0..y0 + h {
            let ini = fila as usize * stride + x0 as usize * 4;
            out.extend_from_slice(&src[ini..ini + w as usize * 4]);
        }
        Pixmap::from_vec(out, IntSize::from_wh(w, h)?)
    }

    /// Rasteriza el anillo de una sombra en un mapa propio, con la tarjeta en
    /// (blur, blur) y sitio para la caída por abajo.
    fn rasterizar_anillo(key: ShadowKey, capas: usize) -> Option<Pixmap> {
        let blur = key.blur as f32;
        let ancho = key.width + key.blur * 2;
        let alto = key.height + key.blur * 2 + key.offset_y.unsigned_abs();
        let mut mapa = Pixmap::new(ancho, alto)?;
        let tarjeta = Bounds::new(blur, blur, key.width as f32, key.height as f32);
        let alfa = (key.color[3] as f32 / capas as f32).max(1.0) as u8;
        let mut paint = Paint::default();
        paint.set_color(
            Color::new(key.color[0], key.color[1], key.color[2])
                .with_alpha(alfa)
                .to_skia_color(),
        );
        for capa in (0..capas).rev() {
            let crece = blur * (capa as f32 + 1.0) / capas as f32;
            let exterior = Bounds::new(
                tarjeta.x - crece,
                tarjeta.y - crece + key.offset_y as f32,
                tarjeta.width + crece * 2.0,
                tarjeta.height + crece * 2.0,
            );
            let mut pb = PathBuilder::new();
            Self::rounded_rect_subpath(&mut pb, exterior, key.radius as f32 + crece);
            Self::rounded_rect_subpath(&mut pb, tarjeta, key.radius as f32);
            if let Some(path) = pb.finish() {
                mapa.fill_path(&path, &paint, FillRule::EvenOdd, Transform::identity(), None);
            }
        }
        Some(mapa)
    }

    /// Añade a `pb` el contorno de un rectángulo redondeado como subruta
    /// cerrada, sin rellenar nada. Con dos de ellas y relleno par-impar sale
    /// un anillo.
    fn rounded_rect_subpath(pb: &mut PathBuilder, bounds: Bounds, radius: f32) {
        let r = radius.min(bounds.width / 2.0).min(bounds.height / 2.0).max(0.0);
        let (x, y, w, h) = (bounds.x, bounds.y, bounds.width, bounds.height);
        pb.move_to(x + r, y);
        pb.line_to(x + w - r, y);
        pb.quad_to(x + w, y, x + w, y + r);
        pb.line_to(x + w, y + h - r);
        pb.quad_to(x + w, y + h, x + w - r, y + h);
        pb.line_to(x + r, y + h);
        pb.quad_to(x, y + h, x, y + h - r);
        pb.line_to(x, y + r);
        pb.quad_to(x, y, x + r, y);
        pb.close();
    }

    pub fn stroke_rect(&mut self, bounds: Bounds, color: Color, width: f32) {
        let Some(rect) = Rect::from_xywh(bounds.x, bounds.y, bounds.width, bounds.height) else {
            return;
        };
        let path = {
            let mut pb = PathBuilder::new();
            pb.push_rect(rect);
            pb.finish().unwrap()
        };

        let mut paint = Paint::default();
        paint.set_color(color.to_skia_color());

        let stroke = Stroke {
            width,
            ..Default::default()
        };

        self.pixmap
            .stroke_path(&path, &paint, &stroke, self.transform(), None);
    }

    /// Dibuja una línea
    pub fn draw_line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, color: Color, width: f32) {
        let path = {
            let mut pb = PathBuilder::new();
            pb.move_to(x1, y1);
            pb.line_to(x2, y2);
            pb.finish().unwrap()
        };

        let mut paint = Paint::default();
        paint.set_color(color.to_skia_color());

        let stroke = Stroke {
            width,
            ..Default::default()
        };

        self.pixmap
            .stroke_path(&path, &paint, &stroke, self.transform(), None);
    }

    /// Obtiene los bytes del pixmap para blitting a la pantalla
    pub fn data(&self) -> &[u8] {
        self.pixmap.data()
    }

    /// Obtiene los pixels como u32 ARGB para softbuffer
    pub fn as_argb(&self) -> Vec<u32> {
        self.pixmap
            .data()
            .chunks(4)
            .map(|c| {
                // tiny-skia usa RGBA, softbuffer espera ARGB (o XRGB)
                let r = c[0] as u32;
                let g = c[1] as u32;
                let b = c[2] as u32;
                let a = c[3] as u32;
                (a << 24) | (r << 16) | (g << 8) | b
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canvas_basic() {
        let mut canvas = Canvas::new(100, 100);
        canvas.clear(Color::WHITE);
        canvas.fill_rect(Bounds::new(10.0, 10.0, 50.0, 50.0), Color::BLACK);

        assert_eq!(canvas.width(), 100);
        assert_eq!(canvas.height(), 100);
    }

    /// La sombra es un anillo: no toca el interior de la tarjeta y sí oscurece
    /// justo fuera de su borde. Si alguien la vuelve a hacer losa, el interior
    /// cambia y este test cae — y con él vuelven los 17 ms por fotograma.
    #[test]
    fn la_sombra_no_pinta_el_interior_de_la_tarjeta() {
        let mut canvas = Canvas::new(200, 200);
        canvas.clear(Color::from_hex(0xF3F2F2));
        let tarjeta = Bounds::new(50.0, 50.0, 100.0, 100.0);

        let pixel = |c: &Canvas, x: usize, y: usize| -> [u8; 4] {
            let i = (y * 200 + x) * 4;
            let d = c.data();
            [d[i], d[i + 1], d[i + 2], d[i + 3]]
        };
        let centro_antes = pixel(&canvas, 100, 100);
        let borde_antes = pixel(&canvas, 100, 155);

        canvas.drop_shadow(tarjeta, 8.0, 3.0, 10.0, Color::from_hex(0x2D2B2B).with_alpha(41));

        assert_eq!(
            pixel(&canvas, 100, 100),
            centro_antes,
            "el centro de la tarjeta debe quedar intacto"
        );
        assert_ne!(
            pixel(&canvas, 100, 155),
            borde_antes,
            "justo bajo el borde inferior debe haber sombra"
        );
    }

    /// Mide lo que cuesta la sombra frente al relleno que la acompaña. No es
    /// una aserción: `cargo test -p llore_ui --release -- --ignored --nocapture medir_coste_de_la_sombra`.
    #[test]
    #[ignore]
    fn medir_coste_de_la_sombra() {
        use std::time::Instant;
        let mut canvas = Canvas::new(1920, 1080);
        let tarjeta = Bounds::new(900.0, 60.0, 720.0, 820.0);
        let bandeja = Bounds::new(330.0, 900.0, 600.0, 76.0);
        let sombra = Color::from_hex(0x2D2B2B).with_alpha(41);
        let blanco = Color::new(255, 255, 255);
        let n = 20;

        let t = Instant::now();
        for _ in 0..n { canvas.clear(Color::from_hex(0xF3F2F2)); }
        let clear = t.elapsed() / n;

        let t = Instant::now();
        for _ in 0..n { canvas.fill_rounded_rect(tarjeta, 16.0, blanco); }
        let relleno = t.elapsed() / n;

        let t = Instant::now();
        for _ in 0..n { canvas.drop_shadow(tarjeta, 16.0, 3.0, 10.0, sombra); }
        let sombra_tarjeta = t.elapsed() / n;

        let t = Instant::now();
        for _ in 0..n { canvas.drop_shadow(bandeja, 16.0, 3.0, 10.0, sombra); }
        let sombra_bandeja = t.elapsed() / n;

        println!("  limpiar el lienzo 1920x1080:     {clear:?}");
        println!("  relleno de la tarjeta del editor: {relleno:?}");
        println!("  sombra de la tarjeta (8 capas):   {sombra_tarjeta:?}");
        println!("  sombra de la bandeja (8 capas):   {sombra_bandeja:?}");
    }
}
