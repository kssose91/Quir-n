//! # Llore Editor
//!
//! Editor de texto privado y local para Quirón.
//!
//! ## Filosofía
//!
//! - **Privacidad Total**: Nada sale sin permiso
//! - **Local-First**: Todo vive en tu máquina
//! - **Honestidad Estructural**: Código que refleja intención
//! - **Sin Restricciones Artificiales**: Libertad de pensamiento

use llore_editor::Editor;
use llore_language::LanguageRegistry;
use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║                    LLORE EDITOR v0.1.0                       ║");
    println!("║           Tu casa privada para código y pensamiento          ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║  🔒 Privacidad Total    📁 Local-First    🧠 Tu Cerebro      ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    // Inicializar registro de lenguajes
    let registry = LanguageRegistry::with_builtin_languages();
    println!("✓ {} lenguajes cargados", registry.count());

    // Procesar argumentos
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 {
        let path = PathBuf::from(&args[1]);
        match fs::read_to_string(&path) {
            Ok(content) => {
                let editor = Editor::open_file(path.clone(), &content, &registry);
                print_editor_status(&editor, &path);
            }
            Err(e) => {
                eprintln!("Error abriendo archivo: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        let editor = Editor::new();
        println!("📝 Nuevo buffer vacío");
        println!("   Modo: {:?}", editor.mode());
        println!("   Líneas: {}", editor.line_count());
    }

    println!();
    println!("══════════════════════════════════════════════════════════════");
    println!("Llore está listo. Esta es tu casa.");
    println!();
}

fn print_editor_status(editor: &Editor, path: &PathBuf) {
    println!();
    println!("📄 Archivo: {}", path.display());
    println!("   Nombre: {}", editor.buffer_name());
    println!("   Modo: {:?}", editor.mode());
    println!("   Líneas: {}", editor.line_count());
    println!(
        "   Modificado: {}",
        if editor.is_modified() { "Sí" } else { "No" }
    );

    // Mostrar primeras líneas
    let text = editor.text();
    let preview: Vec<&str> = text.lines().take(5).collect();
    if !preview.is_empty() {
        println!();
        println!("   Vista previa:");
        for (i, line) in preview.iter().enumerate() {
            let truncated = if line.len() > 60 {
                format!("{}...", &line[..60])
            } else {
                line.to_string()
            };
            println!("   {:>3} │ {}", i + 1, truncated);
        }
        if text.lines().count() > 5 {
            println!("   ... ({} líneas más)", text.lines().count() - 5);
        }
    }
}
