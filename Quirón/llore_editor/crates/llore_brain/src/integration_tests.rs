//! # Tests de Integración
//!
//! Tests end-to-end que demuestran el flujo completo:
//! Gate → Validación → Bloqueo/Permitir

use crate::client::QuironClient;
use crate::gates::GateValidator;

/// Test: Flujo completo de no-ghost-editing
///
/// Escenario:
/// 1. Intento editar archivo sin leerlo → BLOQUEADO
/// 2. Leo el archivo
/// 3. Intento editar → PERMITIDO
#[cfg(test)]
mod ghost_editing_tests {
    use super::*;

    #[test]
    fn test_full_ghost_editing_flow() {
        let client = QuironClient::new();
        let mut validator = GateValidator::new(client);

        // === Paso 1: Intento editar sin leer ===
        let result = validator.validate_local("write", Some("/project/src/main.rs"));
        assert!(!result.allowed, "Should block ghost editing");
        assert!(result
            .blocking_gates
            .contains(&"no-ghost-editing".to_string()));
        println!("✓ Bloqueado correctamente: ghost editing detectado");

        // === Paso 2: Registro lectura ===
        validator.register_file_read("/project/src/main.rs");
        assert!(validator.was_file_read("/project/src/main.rs"));
        println!("✓ Lectura registrada");

        // === Paso 3: Ahora puedo editar ===
        let result = validator.validate_local("write", Some("/project/src/main.rs"));
        assert!(result.allowed, "Should allow after reading");
        println!("✓ Edición permitida después de leer");
    }

    #[test]
    fn test_multiple_files() {
        let client = QuironClient::new();
        let mut validator = GateValidator::new(client);

        // Leo solo uno
        validator.register_file_read("src/lib.rs");

        // Puedo editar el que leí
        let result = validator.validate_local("write", Some("src/lib.rs"));
        assert!(result.allowed);

        // No puedo editar otro
        let result = validator.validate_local("write", Some("src/main.rs"));
        assert!(!result.allowed);
        assert!(result
            .blocking_gates
            .contains(&"no-ghost-editing".to_string()));
    }

    #[test]
    fn test_read_allowed_without_prior_read() {
        let client = QuironClient::new();
        let validator = GateValidator::new(client);

        // READ siempre permitido (no es ghost editing)
        let result = validator.validate_local("read", Some("any/file.rs"));
        assert!(result.allowed, "Read should always be allowed");
    }
}

/// Test: Flujo completo de scope-lock
#[cfg(test)]
mod scope_lock_tests {
    use super::*;

    #[test]
    fn test_scope_enforcement() {
        let client = QuironClient::new();
        let mut validator = GateValidator::new(client);

        // Defino scope: solo puedo editar en src/
        validator.set_scope(vec!["src/*".to_string()]);

        // Registro lecturas
        validator.register_file_read("src/lib.rs");
        validator.register_file_read("Cargo.toml");
        validator.register_file_read("tests/integration.rs");

        // === Dentro de scope: permitido ===
        let result = validator.validate_local("write", Some("src/lib.rs"));
        assert!(result.allowed, "Inside scope should be allowed");

        // === Fuera de scope: bloqueado ===
        let result = validator.validate_local("write", Some("Cargo.toml"));
        assert!(!result.allowed, "Outside scope should be blocked");
        assert!(result.blocking_gates.contains(&"scope-lock".to_string()));

        let result = validator.validate_local("write", Some("tests/integration.rs"));
        assert!(!result.allowed, "tests/ is outside src/* scope");
    }

    #[test]
    fn test_nested_scope() {
        let client = QuironClient::new();
        let mut validator = GateValidator::new(client);

        // Scope más específico
        validator.set_scope(vec!["src/api/**".to_string()]);
        validator.register_file_read("src/api/handlers.rs");
        validator.register_file_read("src/lib.rs");

        // src/api/* permitido
        let result = validator.validate_local("write", Some("src/api/handlers.rs"));
        assert!(result.allowed);

        // src/ sin api/ no está en scope
        let result = validator.validate_local("write", Some("src/lib.rs"));
        assert!(!result.allowed);
    }

    #[test]
    fn test_multiple_scope_paths() {
        let client = QuironClient::new();
        let mut validator = GateValidator::new(client);

        // Múltiples paths en scope
        validator.set_scope(vec!["src/*".to_string(), "tests/*".to_string()]);
        validator.register_file_read("src/lib.rs");
        validator.register_file_read("tests/test_main.rs");
        validator.register_file_read("Cargo.toml");

        assert!(
            validator
                .validate_local("write", Some("src/lib.rs"))
                .allowed
        );
        assert!(
            validator
                .validate_local("write", Some("tests/test_main.rs"))
                .allowed
        );
        assert!(
            !validator
                .validate_local("write", Some("Cargo.toml"))
                .allowed
        );
    }
}

/// Test: Combinación de gates
#[cfg(test)]
mod combined_gates_tests {
    use super::*;

    #[test]
    fn test_both_gates_must_pass() {
        let client = QuironClient::new();
        let mut validator = GateValidator::new(client);

        // Defino scope
        validator.set_scope(vec!["src/*".to_string()]);

        // Caso 1: No leí, dentro de scope → bloqueado por ghost-editing
        let result = validator.validate_local("write", Some("src/lib.rs"));
        assert!(!result.allowed);
        assert!(result
            .blocking_gates
            .contains(&"no-ghost-editing".to_string()));

        // Caso 2: Leí, fuera de scope → bloqueado por scope-lock
        validator.register_file_read("Cargo.toml");
        let result = validator.validate_local("write", Some("Cargo.toml"));
        assert!(!result.allowed);
        assert!(result.blocking_gates.contains(&"scope-lock".to_string()));

        // Caso 3: Leí Y dentro de scope → permitido
        validator.register_file_read("src/lib.rs");
        let result = validator.validate_local("write", Some("src/lib.rs"));
        assert!(result.allowed);
    }
}

/// Test: Evidence tracking
#[cfg(test)]
mod evidence_tests {
    use super::*;

    #[test]
    fn test_add_evidence() {
        let client = QuironClient::new();
        let mut validator = GateValidator::new(client);

        // Añadir evidencia
        validator.add_evidence(
            "verification",
            "Tests passed after fix",
            serde_json::json!({
                "tests_run": 42,
                "tests_passed": 42,
                "command": "cargo test"
            }),
        );

        // La evidencia se usará en validate_remote()
        // Por ahora solo verificamos que no falla
    }

    #[test]
    fn test_clear_state() {
        let client = QuironClient::new();
        let mut validator = GateValidator::new(client);

        validator.register_file_read("file.rs");
        validator.set_scope(vec!["src/*".to_string()]);

        assert!(validator.was_file_read("file.rs"));

        validator.clear();

        assert!(!validator.was_file_read("file.rs"));
    }
}

/// Test: Warnings (no blocking)
#[cfg(test)]
mod warning_tests {
    use super::*;

    #[test]
    fn test_stale_read_warning() {
        let client = QuironClient::new();
        let mut validator = GateValidator::new(client);

        // Registrar lectura
        validator.register_file_read("src/lib.rs");

        // Inmediatamente, no hay warning
        let result = validator.validate_local("write", Some("src/lib.rs"));
        assert!(result.allowed);
        assert!(result.warnings.is_empty(), "No warnings for fresh reads");

        // TODO: Simular paso del tiempo para verificar warning de lectura stale
    }
}
