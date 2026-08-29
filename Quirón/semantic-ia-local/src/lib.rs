pub mod api;
pub mod config;
pub mod engine;
pub mod error;

use std::sync::Arc;

pub use config::Config;
pub use engine::{SemanticEngines, ServiceHealth};
pub use error::{Result, ServiceError};

pub async fn run() -> Result<()> {
    let config = Arc::new(Config::from_env()?);
    let engines = Arc::new(SemanticEngines::new(Arc::clone(&config)));
    api::serve(config, engines).await
}
