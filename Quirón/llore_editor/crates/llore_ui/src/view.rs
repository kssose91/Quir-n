//! # View
//!
//! Trait para renderizar vistas con estado.

use crate::element::AnyElement;

/// Trait para views con estado que generan elementos.
///
/// Similar al Render trait de GPUI. Cada frame, `render()` es llamado
/// para generar el árbol de elementos.
pub trait Render: 'static + Sized {
    /// Renderiza la vista en un elemento.
    fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement;
}

/// Wrapper trait para poder tener views dinámicas
pub trait RenderDyn: 'static {
    fn render_dyn(&mut self, cx: &mut DynViewContext) -> AnyElement;
}

/// Contexto para renderizar una vista
pub struct ViewContext<'a, V> {
    /// Estado de la aplicación
    pub(crate) app: &'a mut crate::app::AppState,
    /// Phantom para el tipo de vista
    _phantom: std::marker::PhantomData<V>,
}

impl<'a, V> ViewContext<'a, V> {
    pub fn new(app: &'a mut crate::app::AppState) -> Self {
        Self {
            app,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Notifica que la vista necesita re-render
    pub fn notify(&mut self) {
        self.app.needs_render = true;
    }

    /// Ejecuta una acción en el próximo frame
    pub fn defer<F: FnOnce(&mut V) + 'static>(&mut self, _action: F) {
        // TODO: implementar cola de acciones diferidas
    }
}

/// Contexto dinámico para views sin tipo específico
pub struct DynViewContext<'a> {
    pub(crate) app: &'a mut crate::app::AppState,
}

impl<'a> DynViewContext<'a> {
    pub fn new(app: &'a mut crate::app::AppState) -> Self {
        Self { app }
    }

    /// Acceso explícito al estado global de la app.
    pub fn app_state(&mut self) -> &mut crate::app::AppState {
        self.app
    }
}

/// Convierte algo en un elemento
pub trait IntoElement {
    fn into_element(self) -> AnyElement;
}

impl<E: crate::Element> IntoElement for E {
    fn into_element(self) -> AnyElement {
        AnyElement::new(self)
    }
}
