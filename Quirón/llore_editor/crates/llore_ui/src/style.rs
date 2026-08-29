//! # Style
//!
//! Estilos para elementos, inspirados en CSS/Flexbox.

use crate::Color;

/// Estilo de un elemento
#[derive(Debug, Clone)]
pub struct Style {
    // Layout
    pub display: Display,
    pub flex_direction: FlexDirection,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub flex_basis: Dimension,
    pub align_items: AlignItems,
    pub justify_content: JustifyContent,
    pub gap: f32,

    // Size
    pub width: Dimension,
    pub height: Dimension,
    pub min_width: Dimension,
    pub min_height: Dimension,
    pub max_width: Dimension,
    pub max_height: Dimension,

    // Spacing
    pub padding: Edges,
    pub margin: Edges,

    // Appearance
    pub background: Option<Color>,
    pub border_color: Option<Color>,
    pub border_width: f32,
    pub border_radius: f32,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis: Dimension::Auto,
            align_items: AlignItems::Stretch,
            justify_content: JustifyContent::Start,
            gap: 0.0,

            width: Dimension::Auto,
            height: Dimension::Auto,
            min_width: Dimension::Auto,
            min_height: Dimension::Auto,
            max_width: Dimension::Auto,
            max_height: Dimension::Auto,

            padding: Edges::default(),
            margin: Edges::default(),

            background: None,
            border_color: None,
            border_width: 0.0,
            border_radius: 0.0,
        }
    }
}

impl Style {
    /// Convierte a taffy::Style
    pub fn to_taffy(&self) -> taffy::Style {
        taffy::Style {
            display: match self.display {
                Display::Flex => taffy::Display::Flex,
                Display::None => taffy::Display::None,
            },
            flex_direction: match self.flex_direction {
                FlexDirection::Row => taffy::FlexDirection::Row,
                FlexDirection::Column => taffy::FlexDirection::Column,
                FlexDirection::RowReverse => taffy::FlexDirection::RowReverse,
                FlexDirection::ColumnReverse => taffy::FlexDirection::ColumnReverse,
            },
            flex_grow: self.flex_grow,
            flex_shrink: self.flex_shrink,
            flex_basis: self.flex_basis.to_taffy(),
            align_items: Some(match self.align_items {
                AlignItems::Start => taffy::AlignItems::Start,
                AlignItems::End => taffy::AlignItems::End,
                AlignItems::Center => taffy::AlignItems::Center,
                AlignItems::Stretch => taffy::AlignItems::Stretch,
            }),
            justify_content: Some(match self.justify_content {
                JustifyContent::Start => taffy::JustifyContent::Start,
                JustifyContent::End => taffy::JustifyContent::End,
                JustifyContent::Center => taffy::JustifyContent::Center,
                JustifyContent::SpaceBetween => taffy::JustifyContent::SpaceBetween,
                JustifyContent::SpaceAround => taffy::JustifyContent::SpaceAround,
            }),
            gap: taffy::Size {
                width: taffy::LengthPercentage::Length(self.gap),
                height: taffy::LengthPercentage::Length(self.gap),
            },
            size: taffy::Size {
                width: self.width.to_taffy(),
                height: self.height.to_taffy(),
            },
            min_size: taffy::Size {
                width: self.min_width.to_taffy(),
                height: self.min_height.to_taffy(),
            },
            max_size: taffy::Size {
                width: self.max_width.to_taffy(),
                height: self.max_height.to_taffy(),
            },
            padding: self.padding.to_taffy(),
            margin: self.margin.to_taffy_auto(),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub enum Display {
    #[default]
    Flex,
    None,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum FlexDirection {
    #[default]
    Row,
    Column,
    RowReverse,
    ColumnReverse,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum AlignItems {
    Start,
    End,
    Center,
    #[default]
    Stretch,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum JustifyContent {
    #[default]
    Start,
    End,
    Center,
    SpaceBetween,
    SpaceAround,
}

#[derive(Debug, Clone, Copy)]
pub enum Dimension {
    Auto,
    Length(f32),
    Percent(f32),
}

impl Default for Dimension {
    fn default() -> Self {
        Dimension::Auto
    }
}

impl Dimension {
    pub fn to_taffy(self) -> taffy::Dimension {
        match self {
            Dimension::Auto => taffy::Dimension::Auto,
            Dimension::Length(l) => taffy::Dimension::Length(l),
            Dimension::Percent(p) => taffy::Dimension::Percent(p / 100.0),
        }
    }
}

/// Spacing en los 4 lados
#[derive(Debug, Clone, Copy, Default)]
pub struct Edges {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Edges {
    pub fn all(value: f32) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    pub fn xy(x: f32, y: f32) -> Self {
        Self {
            top: y,
            right: x,
            bottom: y,
            left: x,
        }
    }

    pub fn to_taffy(self) -> taffy::Rect<taffy::LengthPercentage> {
        taffy::Rect {
            top: taffy::LengthPercentage::Length(self.top),
            right: taffy::LengthPercentage::Length(self.right),
            bottom: taffy::LengthPercentage::Length(self.bottom),
            left: taffy::LengthPercentage::Length(self.left),
        }
    }

    pub fn to_taffy_auto(self) -> taffy::Rect<taffy::LengthPercentageAuto> {
        taffy::Rect {
            top: taffy::LengthPercentageAuto::Length(self.top),
            right: taffy::LengthPercentageAuto::Length(self.right),
            bottom: taffy::LengthPercentageAuto::Length(self.bottom),
            left: taffy::LengthPercentageAuto::Length(self.left),
        }
    }
}
