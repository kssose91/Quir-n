//! # Llore UI
//!
//! Framework de interfaz gráfica propio para Llore Editor.
//!
//! Inspirado en los patrones de GPUI pero implementado desde cero usando:
//! - **tiny-skia** - Renderizado 2D (CPU)
//! - **winit** - Windowing multiplataforma
//! - **taffy** - Layout Flexbox/Grid
//! - **cosmic-text** - Renderizado de texto
//!
//! ## Arquitectura
//!
//! ```text
//! ┌─────────────────────────────────────┐
//! │    View (impl Render)               │
//! ├─────────────────────────────────────┤
//! │    Element (layout + paint)         │
//! ├─────────────────────────────────────┤
//! │    Taffy (layout engine)            │
//! ├─────────────────────────────────────┤
//! │    tiny-skia (rendering) + winit    │
//! └─────────────────────────────────────┘
//! ```
//!
//! ## Privacidad
//!
//! - **CERO conexiones externas**
//! - **Solo archivos locales**
//! - **Sin telemetría**

pub mod app;
pub mod element;
pub mod layout;
pub mod paint;
pub mod style;
pub mod syntax_highlight;
pub mod text;
pub mod view;
pub mod window;

// Elements
pub mod elements;

// Theme
pub mod theme;

// Sistema de diseño: escala tipográfica, rejilla y factor de escala
pub mod design;

// Iconos de la fuente Lucide empotrada
pub mod icons;

// Proyectos abiertos recientemente
pub mod recents;

// Identidad del proyecto abierto
pub mod project_id;

// Herramientas que el modelo ejecuta a través del arnés
pub mod chat_tools;

// Chat
pub mod chat_panel;

// Confinamiento del editor a la carpeta del proyecto
pub mod workspace_guard;

// Re-exports
pub use app::App;
pub use element::{Element, IntoElement};
pub use layout::{Bounds, Constraints, LayoutEngine, Size};
pub use paint::{Canvas, Color};
pub use style::Style;
pub use view::Render;
pub use window::Window;

// Prelude for convenient imports
pub mod prelude {
    pub use crate::{
        elements::*, App, Bounds, Canvas, Color, Constraints, Element, IntoElement, Render, Size,
        Style, Window,
    };
}
