//! Neo4j schema management - constraints and indexes.

use super::Neo4jConnector;
use crate::error::Result;

/// Schema definitions for the Quiron Brain graph.
pub struct Schema;

impl Schema {
    /// Apply all constraints and indexes to Neo4j.
    pub async fn ensure(connector: &Neo4jConnector) -> Result<()> {
        tracing::info!("Applying Neo4j schema...");

        // === Unique constraints ===

        // Events
        connector
            .execute(
                "CREATE CONSTRAINT event_id IF NOT EXISTS FOR (e:Event) REQUIRE e.id IS UNIQUE",
            )
            .await?;

        // Files
        connector
            .execute(
                "CREATE CONSTRAINT file_path IF NOT EXISTS FOR (f:File) REQUIRE f.path IS UNIQUE",
            )
            .await?;

        // Patches
        connector
            .execute(
                "CREATE CONSTRAINT patch_id IF NOT EXISTS FOR (p:Patch) REQUIRE p.id IS UNIQUE",
            )
            .await?;

        // Claims
        connector
            .execute(
                "CREATE CONSTRAINT claim_id IF NOT EXISTS FOR (c:Claim) REQUIRE c.id IS UNIQUE",
            )
            .await?;

        // Verifications
        connector.execute(
            "CREATE CONSTRAINT verification_id IF NOT EXISTS FOR (v:Verification) REQUIRE v.id IS UNIQUE"
        ).await?;

        // Evidence
        connector.execute(
            "CREATE CONSTRAINT evidence_id IF NOT EXISTS FOR (ev:Evidence) REQUIRE ev.id IS UNIQUE"
        ).await?;

        // Scopes
        connector
            .execute(
                "CREATE CONSTRAINT scope_id IF NOT EXISTS FOR (s:Scope) REQUIRE s.id IS UNIQUE",
            )
            .await?;

        // Symbols
        connector.execute(
            "CREATE CONSTRAINT symbol_fqn IF NOT EXISTS FOR (sym:Symbol) REQUIRE sym.fqn IS UNIQUE"
        ).await?;

        // Modules
        connector
            .execute(
                "CREATE CONSTRAINT module_id IF NOT EXISTS FOR (m:Module) REQUIRE m.id IS UNIQUE",
            )
            .await?;

        // Tags
        connector
            .execute(
                "CREATE CONSTRAINT tag_name IF NOT EXISTS FOR (t:Tag) REQUIRE t.name IS UNIQUE",
            )
            .await?;

        // === Indexes for common queries ===

        // Event timestamp for timeline queries
        connector
            .execute("CREATE INDEX event_ts IF NOT EXISTS FOR (e:Event) ON (e.ts)")
            .await?;

        // Event kind for filtering
        connector
            .execute("CREATE INDEX event_kind IF NOT EXISTS FOR (e:Event) ON (e.kind)")
            .await?;

        // Patch applied_at for audit queries
        connector
            .execute("CREATE INDEX patch_applied IF NOT EXISTS FOR (p:Patch) ON (p.applied_at)")
            .await?;

        // Verification passed for filtering
        connector
            .execute(
                "CREATE INDEX verification_passed IF NOT EXISTS FOR (v:Verification) ON (v.passed)",
            )
            .await?;

        // Claim has_evidence for orphan detection
        connector
            .execute("CREATE INDEX claim_proven IF NOT EXISTS FOR (c:Claim) ON (c.has_evidence)")
            .await?;

        tracing::info!("Neo4j schema applied successfully");
        Ok(())
    }

    /// Drop all data (for rebuild).
    pub async fn clear_all(connector: &Neo4jConnector) -> Result<()> {
        tracing::warn!("Clearing all Neo4j data...");
        connector.execute("MATCH (n) DETACH DELETE n").await?;
        tracing::info!("Neo4j data cleared");
        Ok(())
    }

    /// Get node counts for health check.
    pub async fn node_counts(connector: &Neo4jConnector) -> Result<String> {
        let rows = connector
            .fetch_all(
                "MATCH (n) RETURN labels(n)[0] as label, count(*) as count ORDER BY count DESC",
            )
            .await?;

        let mut counts = Vec::new();
        for row in rows {
            if let (Ok(label), Ok(count)) = (row.get::<String>("label"), row.get::<i64>("count")) {
                counts.push(format!("{}:{}", label, count));
            }
        }

        Ok(counts.join(", "))
    }
}
