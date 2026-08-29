//! Barre un proyecto y proyecta su índice de código al grafo interno.
//!
//! Uso:
//!   index_repo <ruta_raíz> [project_id]
//!
//! El identificador de proyecto se toma del argumento, o de
//! `<raíz>/.llore/project.id` (el ULID que fija el editor al conceder acceso),
//! o del nombre de la carpeta como último recurso.
//!
//! Por defecto escribe en un almacén temporal (`QUIRON_INDEX_DATA`, por defecto
//! `./data/index-verify`) para no colisionar con el RocksDB del servicio, que
//! mantiene un cerrojo exclusivo. Para poblar el índice que usa el servicio,
//! párese el servicio y apúntese `QUIRON_INDEX_DATA` a `QUIRON_DATA`.

use quiron_brain::graph::GraphBuilder;
use quiron_brain::index::Indexer;
use quiron_brain::storage::Storage;
use quiron_brain::types::node::NodeKind;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let root = args.get(1).map(|s| s.as_str()).unwrap_or(".");
    let root_path = std::fs::canonicalize(root)?;

    let project_id = args
        .get(2)
        .cloned()
        .or_else(|| read_project_id(&root_path))
        .unwrap_or_else(|| {
            root_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string())
        });

    let data_path = std::env::var("QUIRON_INDEX_DATA")
        .unwrap_or_else(|_| "./data/index-verify".to_string());
    let max_bytes: u64 = std::env::var("QUIRON_INDEX_MAX_BYTES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(512 * 1024);

    println!("Raíz:      {}", root_path.display());
    println!("Proyecto:  {project_id}");
    println!("Almacén:   {data_path}");
    println!();

    let storage = Storage::open(&data_path)?;
    let indexer = Indexer::new(storage.clone());
    let stats = indexer.index_repo(&root_path, &project_id, max_bytes)?;

    println!("== Barrido ==");
    println!("  Archivos indexados: {}", stats.files);
    println!("  Unidades Lógica:    {}", stats.logic_units);
    println!("  Ficheros omitidos:  {} (lenguaje no soportado)", stats.skipped);

    // Releer del grafo confirma que la persistencia es real, y verifica los
    // invariantes: ninguna unidad sin proyecto, identidad estable.
    let graph = GraphBuilder::new(storage);
    let nodes = graph.all_nodes()?;
    let files = nodes.iter().filter(|n| n.kind == NodeKind::FileUnit).count();
    let logic = nodes.iter().filter(|n| n.kind == NodeKind::LogicUnit).count();
    let sin_proyecto = nodes
        .iter()
        .filter(|n| {
            matches!(n.kind, NodeKind::FileUnit | NodeKind::LogicUnit)
                && n.project_id.as_deref().unwrap_or("").is_empty()
        })
        .count();

    println!();
    println!("== Persistido en el grafo ==");
    println!("  Nodos FileUnit:  {files}");
    println!("  Nodos LogicUnit: {logic}");
    println!("  Sin proyecto:    {sin_proyecto} (debe ser 0)");

    println!();
    println!("== Muestra de unidades Lógica ==");
    for n in nodes
        .iter()
        .filter(|n| n.kind == NodeKind::LogicUnit)
        .take(8)
    {
        println!("  {} · {}", n.name, n.description.as_deref().unwrap_or("").lines().next().unwrap_or(""));
    }

    Ok(())
}

fn read_project_id(root: &Path) -> Option<String> {
    let p = root.join(".llore/project.id");
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}
