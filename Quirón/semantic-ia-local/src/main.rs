#[tokio::main]
async fn main() {
    if let Err(err) = semantic_ia_local::run().await {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
