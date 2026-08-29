//! # Llore Core
//!
//! Estructuras de datos fundamentales para Llore Editor.
//!
//! Este crate proporciona:
//! - `Clock` - Relojes lógicos para ordenamiento de eventos
//! - `Rope` - Buffer de texto eficiente para edición
//! - Colecciones optimizadas para editor de texto
//!
//! ## Filosofía
//!
//! Todo el código aquí es escrito desde cero, inspirado en patrones
//! de Zed pero implementado para las necesidades específicas de Quirón.

pub mod clock;
pub mod rope;

// Re-exports para conveniencia
pub use clock::{Clock, Lamport, ReplicaId};
pub use rope::Rope;
