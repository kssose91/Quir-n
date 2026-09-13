//! Barre un proyecto y proyecta su índice de código.
//!
//! Uso:
//!   index_repo <ruta_raíz> [project_id]
//!
//! El identificador de proyecto se toma del argumento, o de
//! `<raíz>/.llore/project.id` (el ULID que fija el editor al conceder acceso),
//! o del nombre de la carpeta como último recurso.
//!
//! Escribe las unidades al grafo interno en `QUIRON_INDEX_DATA` (por defecto un
//! almacén temporal, para no colisionar con el cerrojo de RocksDB del servicio).
//!
//! Con `QUIRON_INDEX_VECTORIZE=1` vectoriza además las unidades a Qdrant
//! (colección `quiron_code`), usando el servicio semántico configurado. Esto
//! requiere Qdrant en marcha.

use quiron_brain::graph::GraphBuilder;
use quiron_brain::index::Indexer;
use quiron_brain::storage::Storage;
use quiron_brain::types::node::NodeKind;
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
    let do_vectorize = std::env::var("QUIRON_INDEX_VECTORIZE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    println!("Raíz:      {}", root_path.display());
    println!("Proyecto:  {project_id}");
    println!("Almacén:   {data_path}");
    println!("Vectoriza: {}", if do_vectorize { "sí (quiron_code)" } else { "no" });
    println!();

    let storage = Storage::open(&data_path)?;
    let indexer = Indexer::new(storage.clone());

    // 1) Recoger las unidades (determinista) y persistirlas al grafo.
    let units = indexer.collect_units(&root_path, &project_id, max_bytes)?;
    indexer.persist_to_graph(&units)?;
    let stats = units.stats();

    println!("== Barrido ==");
    println!("  Archivos indexados: {}", stats.files);
    println!("  Unidades Lógica:    {}", stats.logic_units);
    println!("  Ficheros omitidos:  {} (lenguaje no soportado)", stats.skipped);

    // 2) Confirmar la persistencia releyendo del grafo, y los invariantes.
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

    // 3) Vectorizar a Qdrant, si se pide.
    #[cfg(feature = "semantic")]
    if do_vectorize {
        use quiron_brain::index::vectorize::CodeVectorizer;
        println!();
        println!("== Vectorización a Qdrant (quiron_code) ==");
        let vectorizer = CodeVectorizer::connect().await?;
        if !vectorizer.health().await {
            eprintln!("  Aviso: el servicio semántico o Qdrant no responden; se omite.");
        } else {
            let n = vectorizer.vectorize(&units.files, &units.logic).await?;
            println!("  Puntos escritos: {n}");
        }
    }
    #[cfg(not(feature = "semantic"))]
    if do_vectorize {
        eprintln!("Vectorización pedida pero el binario se compiló sin la feature 'semantic'.");
    }

    Ok(())
}

fn read_project_id(root: &Path) -> Option<String> {
    let p = if root.join(".quiron/project.id").exists() { root.join(".quiron/project.id") } else { root.join(".llore/project.id") };
    std::fs::read_to_string(p)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
