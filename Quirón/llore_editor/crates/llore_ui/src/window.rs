//! # Window
//!
//! Ventana de la aplicación usando winit.

use softbuffer::Surface;
use std::num::NonZeroU32;
use std::sync::Arc;
use winit::dpi::LogicalSize;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window as WinitWindow, WindowAttributes};

use crate::{Bounds, Canvas, Size};

/// Identificador de la aplicación ante el compositor.
///
/// Debe coincidir con el nombre del fichero `llore.desktop` para que el
/// escritorio asocie icono y ventana. Wayland lo lee como `app_id`; X11, como
/// la parte general de `WM_CLASS`.
const APP_ID: &str = "llore";

/// Ventana de la aplicación
pub struct Window {
    window: Arc<WinitWindow>,
    surface: Surface<Arc<WinitWindow>, Arc<WinitWindow>>,
    canvas: Canvas,
    size: Size,
}

impl Window {
    /// Crea una nueva ventana (llamar desde el event loop)
    pub fn new(event_loop: &ActiveEventLoop, title: &str, width: u32, height: u32) -> Self {
        #[allow(unused_mut)]
        let mut attrs = WindowAttributes::default()
            .with_title(title)
            .with_inner_size(LogicalSize::new(width, height));

        #[cfg(all(unix, not(target_os = "macos")))]
        {
            use winit::platform::wayland::WindowAttributesExtWayland;
            use winit::platform::x11::WindowAttributesExtX11;
            attrs = WindowAttributesExtWayland::with_name(attrs, APP_ID, APP_ID);
            attrs = WindowAttributesExtX11::with_name(attrs, APP_ID, APP_ID);
        }

        let window = event_loop
            .create_window(attrs)
            .expect("Failed to create window");
        let window = Arc::new(window);

        let context = softbuffer::Context::new(window.clone()).expect("Failed to create context");
        let surface = Surface::new(&context, window.clone()).expect("Failed to create surface");

        let canvas = Canvas::new(width, height);

        Self {
            window,
            surface,
            canvas,
            size: Size::new(width as f32, height as f32),
        }
    }

    /// Obtiene el tamaño actual
    pub fn size(&self) -> Size {
        self.size
    }

    /// Obtiene los bounds de la ventana
    pub fn bounds(&self) -> Bounds {
        Bounds::new(0.0, 0.0, self.size.width, self.size.height)
    }

    /// Redimensiona la ventana
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.size = Size::new(width as f32, height as f32);
        self.canvas.resize(width, height);

        let _ = self.surface.resize(
            NonZeroU32::new(width).unwrap(),
            NonZeroU32::new(height).unwrap(),
        );
    }

    /// Acceso al canvas para dibujar
    pub fn canvas(&mut self) -> &mut Canvas {
        &mut self.canvas
    }

    /// Presenta el contenido del canvas en la ventana
    pub fn present(&mut self) {
        let width = self.size.width as u32;
        let height = self.size.height as u32;

        if width == 0 || height == 0 {
            return;
        }

        let mut buffer = self.surface.buffer_mut().expect("Failed to get buffer");
        let pixels = self.canvas.as_argb();

        for (i, pixel) in pixels.iter().enumerate() {
            if i < buffer.len() {
                buffer[i] = *pixel;
            }
        }

        buffer.present().expect("Failed to present buffer");
    }

    /// Solicita un redibujado
    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    /// Obtiene referencia al window de winit
    pub fn raw(&self) -> &WinitWindow {
        &self.window
    }
}
