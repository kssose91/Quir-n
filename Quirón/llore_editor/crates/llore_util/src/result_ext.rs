//! # Extensiones de Result y Option
//!
//! Métodos de conveniencia para trabajar con Result y Option.

/// Extensiones para Result<T, E>.
pub trait ResultExt<T, E> {
    /// Loguea el error y devuelve None.
    fn log_err(self) -> Option<T>;

    /// Loguea el error con contexto y devuelve None.
    fn log_err_with(self, context: &str) -> Option<T>;
}

impl<T, E: std::fmt::Display> ResultExt<T, E> for Result<T, E> {
    fn log_err(self) -> Option<T> {
        match self {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("[ERROR] {}", e);
                None
            }
        }
    }

    fn log_err_with(self, context: &str) -> Option<T> {
        match self {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("[ERROR] {}: {}", context, e);
                None
            }
        }
    }
}

/// Extensiones para Option<T>.
pub trait OptionExt<T> {
    /// Devuelve el valor o loguea un mensaje y devuelve el default.
    fn unwrap_or_log(self, default: T, msg: &str) -> T;
}

impl<T> OptionExt<T> for Option<T> {
    fn unwrap_or_log(self, default: T, msg: &str) -> T {
        match self {
            Some(v) => v,
            None => {
                eprintln!("[WARN] {}", msg);
                default
            }
        }
    }
}

/// Macro para propagar errores con contexto más limpio.
///
/// # Ejemplo
/// ```ignore
/// let file = try_with!(std::fs::read_to_string("config.json"), "reading config");
/// ```
#[macro_export]
macro_rules! try_with {
    ($expr:expr, $ctx:expr) => {
        match $expr {
            Ok(v) => v,
            Err(e) => {
                return Err(anyhow::anyhow!("{}: {}", $ctx, e));
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_err() {
        let ok: Result<i32, &str> = Ok(42);
        assert_eq!(ok.log_err(), Some(42));

        let err: Result<i32, &str> = Err("something bad");
        assert_eq!(err.log_err(), None);
    }

    #[test]
    fn test_option_ext() {
        let some = Some(42);
        assert_eq!(some.unwrap_or_log(0, "missing"), 42);

        let none: Option<i32> = None;
        assert_eq!(none.unwrap_or_log(99, "using default"), 99);
    }
}
