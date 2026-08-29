//! # Relojes Lógicos
//!
//! Implementación de relojes Lamport para ordenamiento de eventos
//! en sistemas distribuidos o colaborativos.
//!
//! ## Uso
//!
//! ```rust
//! use llore_core::clock::{Clock, Lamport, ReplicaId};
//!
//! let replica = ReplicaId::new(1);
//! let mut clock = Clock::new(replica);
//!
//! let t1 = clock.tick();
//! let t2 = clock.tick();
//! assert!(t2 > t1);
//! ```

use std::cmp::Ordering;
use std::fmt;

/// Identificador único de réplica/usuario.
///
/// En un sistema colaborativo, cada participante tiene un ReplicaId único.
/// Para Llore (single-user), siempre usamos ReplicaId(0) por defecto.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ReplicaId(pub u16);

impl ReplicaId {
    pub const DEFAULT: Self = Self(0);

    pub fn new(id: u16) -> Self {
        Self(id)
    }
}

impl fmt::Debug for ReplicaId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "R{}", self.0)
    }
}

/// Timestamp lógico Lamport.
///
/// Combina un contador lógico con el ReplicaId para ordenamiento total.
/// Dos eventos con el mismo `value` se desempatan por `replica_id`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Lamport {
    pub value: u32,
    pub replica_id: ReplicaId,
}

impl Lamport {
    pub const MIN: Self = Self {
        value: 0,
        replica_id: ReplicaId(0),
    };

    pub const MAX: Self = Self {
        value: u32::MAX,
        replica_id: ReplicaId(u16::MAX),
    };

    pub fn new(value: u32, replica_id: ReplicaId) -> Self {
        Self { value, replica_id }
    }
}

impl Ord for Lamport {
    fn cmp(&self, other: &Self) -> Ordering {
        self.value
            .cmp(&other.value)
            .then_with(|| self.replica_id.0.cmp(&other.replica_id.0))
    }
}

impl PartialOrd for Lamport {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Debug for Lamport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "T{}@{:?}", self.value, self.replica_id)
    }
}

/// Reloj lógico para generar timestamps ordenados.
///
/// Cada llamada a `tick()` incrementa el contador y devuelve un nuevo timestamp.
#[derive(Clone, Debug)]
pub struct Clock {
    replica_id: ReplicaId,
    value: u32,
}

impl Clock {
    /// Crea un nuevo reloj para la réplica especificada.
    pub fn new(replica_id: ReplicaId) -> Self {
        Self {
            replica_id,
            value: 0,
        }
    }

    /// Crea un reloj con el ReplicaId por defecto (0).
    pub fn default_local() -> Self {
        Self::new(ReplicaId::DEFAULT)
    }

    /// Genera un nuevo timestamp, incrementando el contador.
    pub fn tick(&mut self) -> Lamport {
        self.value = self.value.saturating_add(1);
        Lamport::new(self.value, self.replica_id)
    }

    /// Observa un timestamp externo y actualiza el reloj si es necesario.
    /// Usado para sincronización entre réplicas.
    pub fn observe(&mut self, timestamp: Lamport) {
        self.value = self.value.max(timestamp.value);
    }

    /// Devuelve el ReplicaId de este reloj.
    pub fn replica_id(&self) -> ReplicaId {
        self.replica_id
    }

    /// Devuelve el valor actual sin incrementar.
    pub fn current_value(&self) -> u32 {
        self.value
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::default_local()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lamport_ordering() {
        let t1 = Lamport::new(1, ReplicaId(0));
        let t2 = Lamport::new(1, ReplicaId(1));
        let t3 = Lamport::new(2, ReplicaId(0));

        assert!(t1 < t2); // Same value, different replica
        assert!(t2 < t3); // Different value
        assert!(t1 < t3);
    }

    #[test]
    fn test_clock_tick() {
        let mut clock = Clock::new(ReplicaId(42));

        let t1 = clock.tick();
        let t2 = clock.tick();
        let t3 = clock.tick();

        assert_eq!(t1.value, 1);
        assert_eq!(t2.value, 2);
        assert_eq!(t3.value, 3);
        assert!(t1 < t2);
        assert!(t2 < t3);
    }

    #[test]
    fn test_clock_observe() {
        let mut clock = Clock::new(ReplicaId(0));

        clock.tick(); // value = 1
        clock.observe(Lamport::new(100, ReplicaId(1)));

        let t = clock.tick();
        assert_eq!(t.value, 101);
    }
}
