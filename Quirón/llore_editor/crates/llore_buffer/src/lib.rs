//! # Llore Buffer
//!
//! Buffer de edición con soporte para:
//! - Undo/Redo ilimitado
//! - Timestamps lógicos para ordenamiento
//! - Estado modificado (dirty flag)
//!
//! ## Ejemplo
//!
//! ```rust
//! use llore_buffer::Buffer;
//!
//! let mut buffer = Buffer::new();
//! buffer.insert(0, "Hello");
//! buffer.insert(5, " World");
//! assert_eq!(buffer.text(), "Hello World");
//!
//! buffer.undo();
//! assert_eq!(buffer.text(), "Hello");
//!
//! buffer.redo();
//! assert_eq!(buffer.text(), "Hello World");
//! ```

mod buffer;
mod edit;
mod history;

pub use buffer::Buffer;
pub use edit::{Edit, EditKind};
pub use history::History;
