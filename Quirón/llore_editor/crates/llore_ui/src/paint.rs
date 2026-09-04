//! # Paint
//!
//! Canvas para renderizado 2D con tiny-skia.

use crate::Bounds;
use tiny_skia::{
    FillRule, Paint, PathBuilder, Pixmap, PremultipliedColorU8, Rect, Stroke, Transform,
};

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
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Self {
        let pixmap = Pixmap::new(width, height).expect("Failed to create pixmap");
        Self { pixmap, scale: 1.0 }
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
        // El alfa se reparte entre las capas. Como se superponen, el centro
        // acumula casi el color entero y el borde se apaga: eso es la sombra.
        let alfa = (color.a as f32 / CAPAS as f32).max(1.0) as u8;
        for capa in (0..CAPAS).rev() {
            let crece = blur * (capa as f32 + 1.0) / CAPAS as f32;
            let silueta = Bounds::new(
                bounds.x - crece,
                bounds.y - crece + offset_y,
                bounds.width + crece * 2.0,
                bounds.height + crece * 2.0,
            );
            self.fill_rounded_rect(silueta, radius + crece, color.with_alpha(alfa));
        }
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
}
