//! # Text Element
//!
//! Renderizado de texto usando cosmic-text.

use crate::{
    element::{ElementId, LayoutContext, PaintContext},
    Bounds, Color, Constraints, Element, Size,
};

/// Elemento de texto
pub struct Text {
    content: String,
    color: Color,
    font_size: f32,
    id: Option<ElementId>,
}

impl Text {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            color: Color::TEXT,
            font_size: 14.0,
            id: None,
        }
    }

    /// Establece el color del texto
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Establece el tamaño de fuente
    pub fn size(mut self, size: f32) -> Self {
        self.font_size = size;
        self
    }

    /// Establece el ID
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = Some(id.into());
        self
    }
}

impl Element for Text {
    fn layout(&mut self, constraints: Constraints, _cx: &mut LayoutContext) -> Size {
        // Estimación: ~0.6 * font_size por carácter
        let width = (self.content.len() as f32 * self.font_size * 0.6).min(constraints.max_width);
        let height = self.font_size * 1.2;
        Size::new(width, height)
    }

    fn paint(&mut self, bounds: Bounds, cx: &mut PaintContext) {
        // Crear buffer y renderizar texto real
        let buffer = cx
            .text
            .create_buffer(&self.content, self.font_size, bounds.width);
        cx.text.draw_buffer(
            cx.canvas,
            &buffer,
            bounds.x,
            bounds.y + self.font_size,
            self.color,
        );
    }

    fn id(&self) -> Option<ElementId> {
        self.id.clone()
    }
}

impl<S: Into<String>> From<S> for Text {
    fn from(s: S) -> Self {
        Text::new(s)
    }
}
