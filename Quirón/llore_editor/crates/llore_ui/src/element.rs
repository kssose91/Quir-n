//! # Element Trait
//!
//! Core trait para elementos de UI. Inspirado en GPUI pero simplificado.
//!
//! ## Ciclo de vida
//!
//! 1. `layout()` - Calcula tamaño dado constraints (via Taffy)
//! 2. `paint()` - Dibuja el elemento en el canvas

use crate::{Bounds, Canvas, Constraints, Size};

/// Core trait para elementos de UI.
///
/// Todo elemento debe implementar layout y paint.
pub trait Element: 'static {
    /// Calcula el tamaño del elemento dado los constraints del padre.
    ///
    /// Esta función es llamada durante la fase de layout para determinar
    /// el tamaño que necesita el elemento.
    fn layout(&mut self, constraints: Constraints, cx: &mut LayoutContext) -> Size;

    /// Dibuja el elemento en el canvas.
    ///
    /// `bounds` contiene la posición y tamaño final después del layout.
    fn paint(&mut self, bounds: Bounds, cx: &mut PaintContext);

    /// Maneja un evento. Retorna true si el evento fue consumido.
    fn event(&mut self, _event: &Event, _cx: &mut EventContext) -> bool {
        false
    }

    /// ID opcional para tracking de estado entre frames.
    fn id(&self) -> Option<ElementId> {
        None
    }
}

/// Conversión a Element para composición fluida.
pub trait IntoElement {
    type Element: Element;

    fn into_element(self) -> Self::Element;
}

impl<E: Element> IntoElement for E {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// ID único para un elemento.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ElementId {
    /// ID numérico
    Integer(u64),
    /// ID string
    Name(String),
}

impl From<&str> for ElementId {
    fn from(s: &str) -> Self {
        ElementId::Name(s.to_string())
    }
}

impl From<u64> for ElementId {
    fn from(id: u64) -> Self {
        ElementId::Integer(id)
    }
}

/// Evento de input
#[derive(Debug, Clone)]
pub enum Event {
    /// Mouse movido
    MouseMove { x: f32, y: f32 },
    /// Click del mouse
    MouseDown { x: f32, y: f32, button: MouseButton },
    /// Mouse release
    MouseUp { x: f32, y: f32, button: MouseButton },
    /// Tecla presionada
    KeyDown { key: Key, modifiers: Modifiers },
    /// Tecla liberada
    KeyUp { key: Key, modifiers: Modifiers },
    /// Texto input
    TextInput { text: String },
    /// Scroll
    Scroll { delta_x: f32, delta_y: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

impl Default for Modifiers {
    fn default() -> Self {
        Self {
            shift: false,
            ctrl: false,
            alt: false,
            meta: false,
        }
    }
}

/// Key codes (subset común)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Num0,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,
    Space,
    Enter,
    Tab,
    Escape,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    Unknown,
}

/// Contexto para la fase de layout
pub struct LayoutContext<'a> {
    /// Referencia al layout engine
    pub(crate) layout: &'a mut crate::layout::LayoutEngine,
}

impl<'a> LayoutContext<'a> {
    /// Acceso explícito al layout engine subyacente.
    pub fn layout_engine(&mut self) -> &mut crate::layout::LayoutEngine {
        self.layout
    }
}

/// Contexto para la fase de paint  
pub struct PaintContext<'a> {
    /// Canvas para dibujar
    pub canvas: &'a mut Canvas,
    /// Sistema de texto
    pub text: &'a mut crate::text::TextSystem,
}

/// Contexto para eventos
pub struct EventContext<'a> {
    /// Si el elemento tiene focus
    pub focused: bool,
    /// Phantom lifetime
    _phantom: std::marker::PhantomData<&'a ()>,
}

impl<'a> EventContext<'a> {
    pub fn new(focused: bool) -> Self {
        Self {
            focused,
            _phantom: std::marker::PhantomData,
        }
    }
}

/// Elemento vacío (no renderiza nada)
pub struct Empty;

impl Element for Empty {
    fn layout(&mut self, _constraints: Constraints, _cx: &mut LayoutContext) -> Size {
        Size::ZERO
    }

    fn paint(&mut self, _bounds: Bounds, _cx: &mut PaintContext) {
        // No-op
    }
}

/// Wrapper para Box<dyn Element>
pub struct AnyElement(Box<dyn Element>);

impl AnyElement {
    pub fn new<E: Element>(element: E) -> Self {
        Self(Box::new(element))
    }
}

impl Element for AnyElement {
    fn layout(&mut self, constraints: Constraints, cx: &mut LayoutContext) -> Size {
        self.0.layout(constraints, cx)
    }

    fn paint(&mut self, bounds: Bounds, cx: &mut PaintContext) {
        self.0.paint(bounds, cx)
    }

    fn event(&mut self, event: &Event, cx: &mut EventContext) -> bool {
        self.0.event(event, cx)
    }

    fn id(&self) -> Option<ElementId> {
        self.0.id()
    }
}
