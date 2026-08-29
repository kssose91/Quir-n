//! # Container Element
//!
//! Container con layout automático.

use crate::{
    element::{AnyElement, ElementId, LayoutContext, PaintContext},
    Bounds, Constraints, Element, Size, Style,
};

/// Container que organiza children vertical u horizontalmente.
pub struct Container {
    children: Vec<AnyElement>,
    style: Style,
    id: Option<ElementId>,
    child_layouts: Vec<Bounds>,
}

impl Container {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            style: Style::default(),
            id: None,
            child_layouts: Vec::new(),
        }
    }

    /// Container vertical (column)
    pub fn column() -> Self {
        let mut c = Self::new();
        c.style.flex_direction = crate::style::FlexDirection::Column;
        c
    }

    /// Container horizontal (row)
    pub fn row() -> Self {
        let mut c = Self::new();
        c.style.flex_direction = crate::style::FlexDirection::Row;
        c
    }

    /// Añade un hijo
    pub fn child(mut self, child: impl Element) -> Self {
        self.children.push(AnyElement::new(child));
        self
    }

    /// Establece gap entre elementos
    pub fn gap(mut self, gap: f32) -> Self {
        self.style.gap = gap;
        self
    }

    /// Establece padding
    pub fn padding(mut self, padding: f32) -> Self {
        self.style.padding = crate::style::Edges::all(padding);
        self
    }

    /// Establece ID
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = Some(id.into());
        self
    }
}

impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}

impl Element for Container {
    fn layout(&mut self, constraints: Constraints, cx: &mut LayoutContext) -> Size {
        let fixed_width = resolve_dimension(self.style.width, constraints.max_width);
        let fixed_height = resolve_dimension(self.style.height, constraints.max_height);
        let padding = self.style.padding;

        let available_width = fixed_width.unwrap_or(constraints.max_width);
        let available_height = fixed_height.unwrap_or(constraints.max_height);
        let inner_width = (available_width - padding.left - padding.right).max(0.0);
        let inner_height = (available_height - padding.top - padding.bottom).max(0.0);
        let is_row = matches!(
            self.style.flex_direction,
            crate::style::FlexDirection::Row | crate::style::FlexDirection::RowReverse
        );

        let mut child_layouts = Vec::with_capacity(self.children.len());
        let mut main_cursor = 0.0f32;
        let mut cross_extent = 0.0f32;

        for child in &mut self.children {
            let child_size = child.layout(
                Constraints {
                    min_width: 0.0,
                    max_width: inner_width,
                    min_height: 0.0,
                    max_height: inner_height,
                },
                cx,
            );
            let child_width = child_size.width.min(inner_width);
            let child_height = child_size.height.min(inner_height);

            let (x, y, main_size, cross_size) = if is_row {
                (
                    padding.left + main_cursor,
                    padding.top,
                    child_width,
                    child_height,
                )
            } else {
                (
                    padding.left,
                    padding.top + main_cursor,
                    child_height,
                    child_width,
                )
            };

            child_layouts.push(Bounds::new(x, y, child_width, child_height));
            main_cursor += main_size + self.style.gap;
            cross_extent = cross_extent.max(cross_size);
        }

        let content_main = if self.children.is_empty() {
            0.0
        } else {
            main_cursor - self.style.gap
        };

        let (content_width, content_height) = if is_row {
            (content_main, cross_extent)
        } else {
            (cross_extent, content_main)
        };

        if matches!(
            self.style.flex_direction,
            crate::style::FlexDirection::RowReverse
        ) {
            for child in &mut child_layouts {
                child.x = padding.left + (content_width - (child.x - padding.left) - child.width);
            }
        }
        if matches!(
            self.style.flex_direction,
            crate::style::FlexDirection::ColumnReverse
        ) {
            for child in &mut child_layouts {
                child.y = padding.top + (content_height - (child.y - padding.top) - child.height);
            }
        }

        self.child_layouts = child_layouts;

        let auto_width = content_width + padding.left + padding.right;
        let auto_height = content_height + padding.top + padding.bottom;
        let width = fixed_width.unwrap_or(auto_width);
        let height = fixed_height.unwrap_or(auto_height);

        Size::new(
            width.clamp(constraints.min_width, constraints.max_width),
            height.clamp(constraints.min_height, constraints.max_height),
        )
    }

    fn paint(&mut self, bounds: Bounds, cx: &mut PaintContext) {
        // Dibuja fondo si existe
        if let Some(bg) = self.style.background {
            cx.canvas.fill_rect(bounds, bg);
        }

        for (child, child_bounds) in self.children.iter_mut().zip(self.child_layouts.iter()) {
            child.paint(
                Bounds::new(
                    bounds.x + child_bounds.x,
                    bounds.y + child_bounds.y,
                    child_bounds.width,
                    child_bounds.height,
                ),
                cx,
            );
        }
    }

    fn id(&self) -> Option<ElementId> {
        self.id.clone()
    }
}

fn resolve_dimension(dimension: crate::style::Dimension, available: f32) -> Option<f32> {
    match dimension {
        crate::style::Dimension::Auto => None,
        crate::style::Dimension::Length(value) => Some(value.min(available)),
        crate::style::Dimension::Percent(percent) => {
            Some((available * percent / 100.0).min(available))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::LayoutEngine;

    struct Fixed {
        size: Size,
    }

    impl Fixed {
        fn new(width: f32, height: f32) -> Self {
            Self {
                size: Size::new(width, height),
            }
        }
    }

    impl Element for Fixed {
        fn layout(&mut self, _constraints: Constraints, _cx: &mut LayoutContext) -> Size {
            self.size
        }

        fn paint(&mut self, _bounds: Bounds, _cx: &mut PaintContext) {}
    }

    #[test]
    fn test_container_row_layout_children() {
        let mut container = Container::row()
            .padding(3.0)
            .gap(1.0)
            .child(Fixed::new(10.0, 5.0))
            .child(Fixed::new(8.0, 7.0));
        let mut engine = LayoutEngine::new();
        let mut cx = LayoutContext {
            layout: &mut engine,
        };

        let size = container.layout(Constraints::loose(Size::new(100.0, 100.0)), &mut cx);

        assert_eq!(size, Size::new(25.0, 13.0));
        assert_eq!(container.child_layouts.len(), 2);
        assert_eq!(container.child_layouts[0], Bounds::new(3.0, 3.0, 10.0, 5.0));
        assert_eq!(container.child_layouts[1], Bounds::new(14.0, 3.0, 8.0, 7.0));
    }
}
