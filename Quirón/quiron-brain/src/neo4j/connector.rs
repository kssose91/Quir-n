//! Neo4j connector with connection pooling.

use crate::error::Result;
use neo4rs::{Graph, Query};
use std::sync::Arc;

/// Neo4j connection wrapper with built-in pooling.
#[derive(Clone)]
pub struct Neo4jConnector {
    graph: Arc<Graph>,
}

impl Neo4jConnector {
    /// Connect to Neo4j with the given credentials.
    pub async fn connect(uri: &str, user: &str, password: &str) -> Result<Self> {
        let graph = Graph::new(uri, user, password)
            .await
            .map_err(|e| anyhow::anyhow!("Neo4j connection failed: {}", e))?;

        tracing::info!(uri = uri, "Connected to Neo4j");

        Ok(Self {
            graph: Arc::new(graph),
        })
    }

    /// Execute a Cypher query without returning results.
    pub async fn execute(&self, cypher: &str) -> Result<()> {
        self.graph
            .run(Query::new(cypher.to_string()))
            .await
            .map_err(|e| anyhow::anyhow!("Neo4j execute error: {}", e))?;
        Ok(())
    }

    /// Execute a Cypher query with a pre-built Query object.
    pub async fn run(&self, query: Query) -> Result<()> {
        self.graph
            .run(query)
            .await
            .map_err(|e| anyhow::anyhow!("Neo4j run error: {}", e))?;
        Ok(())
    }

    /// Execute a query and return results as a vector.
    /// Uses an immediate transaction to fetch all rows.
    /// NOTE: Propagates errors instead of silently ignoring them.
    pub async fn fetch_all(&self, cypher: &str) -> Result<Vec<neo4rs::Row>> {
        let mut result = self
            .graph
            .execute(Query::new(cypher.to_string()))
            .await
            .map_err(|e| anyhow::anyhow!("Neo4j query error: {}", e))?;

        let mut rows = Vec::new();
        loop {
            match result.next().await {
                Ok(Some(row)) => rows.push(row),
                Ok(None) => break, // No more rows
                Err(e) => return Err(anyhow::anyhow!("Neo4j fetch error: {}", e).into()),
            }
        }

        Ok(rows)
    }

    /// Execute a query with a pre-built Query object and return results.
    /// NOTE: Propagates errors instead of silently ignoring them.
    pub async fn fetch_all_query(&self, query: Query) -> Result<Vec<neo4rs::Row>> {
        let mut result = self
            .graph
            .execute(query)
            .await
            .map_err(|e| anyhow::anyhow!("Neo4j query error: {}", e))?;

        let mut rows = Vec::new();
        loop {
            match result.next().await {
                Ok(Some(row)) => rows.push(row),
                Ok(None) => break,
                Err(e) => return Err(anyhow::anyhow!("Neo4j fetch error: {}", e).into()),
            }
        }

        Ok(rows)
    }

    /// Get the underlying graph for advanced operations.
    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    /// Check if connection is alive.
    pub async fn health_check(&self) -> Result<bool> {
        match self.execute("RETURN 1").await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    // Tests require a running Neo4j instance
    // Run with: NEO4J_URI=bolt://127.0.0.1:7687 cargo test --features neo4j
}
