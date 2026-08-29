//! Projector - Projects ledger events to Neo4j graph.
//!
//! Principio: proyección idempotente con checkpoint.

use super::Neo4jConnector;
use crate::error::Result;
use crate::memory::{MemoryEnvelope, MemoryEnvelopeStore};
use crate::storage::Storage;
use crate::types::{Event, EventId, EventKind};
use neo4rs::Query;

/// Column family key for the Neo4j checkpoint.
const NEO4J_CHECKPOINT_KEY: &[u8] = b"neo4j_last_projected";

/// Projects events from the ledger to Neo4j.
pub struct Projector {
    storage: Storage,
    neo4j: Neo4jConnector,
}

impl Projector {
    /// Create a new projector.
    pub fn new(storage: Storage, neo4j: Neo4jConnector) -> Self {
        Self { storage, neo4j }
    }

    /// Project a single event to Neo4j (idempotent via MERGE).
    pub async fn project_event(&self, event: &Event) -> Result<()> {
        let envelope = MemoryEnvelopeStore::new(self.storage.clone()).get_or_default(event)?;
        if !envelope.projects_to_neo4j() {
            self.save_checkpoint(&event.id)?;
            tracing::debug!(
                event_id = %event.id,
                promotion_status = ?envelope.promotion_status,
                "Skipping Neo4j projection due to memory promotion policy"
            );
            return Ok(());
        }

        // Always create/update the Event node
        self.project_event_node(event, &envelope).await?;

        // Project based on event kind
        match event.kind {
            EventKind::FileRead => self.project_file_read(event).await?,
            EventKind::PatchProposed => self.project_patch_proposed(event).await?,
            EventKind::PatchApplied => self.project_patch_applied(event).await?,
            EventKind::PatchReverted => self.project_patch_reverted(event).await?,
            EventKind::VerificationRecorded => self.project_verification(event).await?,
            EventKind::ClaimMade => self.project_claim(event).await?,
            EventKind::ScopeDefined => self.project_scope(event).await?,
            EventKind::RepoSnapshotCreated => self.project_snapshot(event).await?,
            _ => {
                // Core events don't need special projection
            }
        }

        // Generic structural projection for promoted memories. This is what lets
        // conversational/cognitive memories participate in the graph instead of
        // remaining isolated Event nodes.
        self.project_context_links(event, &envelope).await?;

        // Update checkpoint
        self.save_checkpoint(&event.id)?;

        Ok(())
    }

    /// Create/update the Event node.
    async fn project_event_node(&self, event: &Event, envelope: &MemoryEnvelope) -> Result<()> {
        let query = Query::new(
            r#"
            MERGE (e:Event {id: $id})
            SET e.ts = datetime($ts),
                e.kind = $kind,
                e.agent_id = $agent_id,
                e.description = $description,
                e.project_id = $project_id,
                e.module_id = $module_id,
                e.truth_status = $truth_status,
                e.confidence = $confidence,
                e.memory_source = $memory_source,
                e.memory_scope = $memory_scope,
                e.memory_kind = $memory_kind,
                e.file_refs = $file_refs,
                e.symbol_refs = $symbol_refs,
                e.logic_tags = $logic_tags,
                e.promotion_status = $promotion_status
        "#
            .to_string(),
        )
        .param("id", event.id.0.to_string())
        .param("ts", event.ts.to_rfc3339())
        .param("kind", format!("{:?}", event.kind))
        .param("agent_id", event.agent_id.clone())
        .param("description", event.description.clone())
        .param("project_id", event.project_id.clone().unwrap_or_default())
        .param("module_id", envelope.module_id.clone().unwrap_or_default())
        .param(
            "truth_status",
            format!("{:?}", envelope.truth_status).to_lowercase(),
        )
        .param("confidence", envelope.confidence as f64)
        .param(
            "memory_source",
            format!("{:?}", envelope.source).to_lowercase(),
        )
        .param(
            "memory_scope",
            format!("{:?}", envelope.scope).to_lowercase(),
        )
        .param(
            "memory_kind",
            format!("{:?}", envelope.memory_kind).to_lowercase(),
        )
        .param("file_refs", envelope.file_refs.join(","))
        .param("symbol_refs", envelope.symbol_refs.join(","))
        .param("logic_tags", envelope.logic_tags.join(","))
        .param(
            "promotion_status",
            format!("{:?}", envelope.promotion_status).to_lowercase(),
        );

        self.neo4j.run(query).await
    }

    /// Project FileRead event.
    async fn project_file_read(&self, event: &Event) -> Result<()> {
        if let Some(path) = event.inputs.first() {
            let query = Query::new(
                r#"
                MATCH (e:Event {id: $event_id})
                MERGE (f:File {path: $path})
                MERGE (e)-[:READS]->(f)
            "#
                .to_string(),
            )
            .param("event_id", event.id.0.to_string())
            .param("path", path.clone());

            self.neo4j.run(query).await?;
        }
        Ok(())
    }

    /// Project PatchProposed event.
    async fn project_patch_proposed(&self, event: &Event) -> Result<()> {
        let patch_id = format!("patch_{}", event.id.0);
        // NOTE: Using MERGE for idempotency (same event projected twice = same result)
        let query = Query::new(
            r#"
            MATCH (e:Event {id: $event_id})
            MERGE (p:Patch {id: $patch_id})
            ON CREATE SET p.status = 'proposed', p.proposed_at = datetime($ts)
            MERGE (e)-[:PROPOSES]->(p)
        "#
            .to_string(),
        )
        .param("event_id", event.id.0.to_string())
        .param("patch_id", patch_id)
        .param("ts", event.ts.to_rfc3339());

        self.neo4j.run(query).await
    }

    /// Project PatchApplied event.
    async fn project_patch_applied(&self, event: &Event) -> Result<()> {
        let patch_id = format!("patch_{}", event.id.0);

        // Create/update patch node
        let query = Query::new(
            r#"
            MATCH (e:Event {id: $event_id})
            MERGE (p:Patch {id: $patch_id})
            SET p.status = 'applied', p.applied_at = datetime($ts)
            MERGE (e)-[:APPLIES]->(p)
        "#
            .to_string(),
        )
        .param("event_id", event.id.0.to_string())
        .param("patch_id", patch_id.clone())
        .param("ts", event.ts.to_rfc3339());

        self.neo4j.run(query).await?;

        // Link to modified files
        for path in &event.outputs {
            let query = Query::new(
                r#"
                MATCH (p:Patch {id: $patch_id})
                MERGE (f:File {path: $path})
                MERGE (p)-[:MODIFIES]->(f)
            "#
                .to_string(),
            )
            .param("patch_id", patch_id.clone())
            .param("path", path.clone());

            self.neo4j.run(query).await?;
        }

        Ok(())
    }

    /// Project PatchReverted event.
    async fn project_patch_reverted(&self, event: &Event) -> Result<()> {
        // The input should be the patch_id or event_id of the original patch
        let raw_id = event.inputs.first().cloned().unwrap_or_default();
        // Normalize to patch_id format if it's just an event id
        let original_patch_id = if raw_id.starts_with("patch_") {
            raw_id
        } else {
            format!("patch_{}", raw_id)
        };
        let revert_id = format!("revert_{}", event.id.0);

        // Use OPTIONAL MATCH + MERGE for robustness (original may not exist in graph yet)
        let query = Query::new(
            r#"
            MATCH (e:Event {id: $event_id})
            MERGE (original:Patch {id: $original_patch_id})
            SET original.status = 'reverted', original.reverted_at = datetime($ts)
            MERGE (revert:Patch {id: $revert_id})
            ON CREATE SET revert.status = 'applied', revert.applied_at = datetime($ts)
            MERGE (e)-[:APPLIES]->(revert)
            MERGE (revert)-[:REVERTS]->(original)
        "#
            .to_string(),
        )
        .param("event_id", event.id.0.to_string())
        .param("original_patch_id", original_patch_id)
        .param("revert_id", revert_id)
        .param("ts", event.ts.to_rfc3339());

        self.neo4j.run(query).await
    }

    /// Project VerificationRecorded event.
    async fn project_verification(&self, event: &Event) -> Result<()> {
        // Improved passed detection: check for positive OR absence of negative
        let desc_lower = event.description.to_lowercase();
        let has_positive = desc_lower.contains("passed")
            || desc_lower.contains("success")
            || desc_lower.contains("ok")
            || desc_lower.contains("✓");
        let has_negative = desc_lower.contains("failed")
            || desc_lower.contains("error")
            || desc_lower.contains("✗");
        let passed = has_positive && !has_negative;

        let verification_id = format!("verify_{}", event.id.0);

        let query = Query::new(
            r#"
            MATCH (e:Event {id: $event_id})
            CREATE (v:Verification {
                id: $verification_id,
                passed: $passed,
                ts: datetime($ts),
                description: $description
            })
            CREATE (e)-[:RECORDS]->(v)
        "#
            .to_string(),
        )
        .param("event_id", event.id.0.to_string())
        .param("verification_id", verification_id.clone())
        .param("passed", passed)
        .param("ts", event.ts.to_rfc3339())
        .param("description", event.description.clone());

        self.neo4j.run(query).await?;

        // Link verification to recent patches
        let link_query = Query::new(
            r#"
            MATCH (v:Verification {id: $verification_id})
            MATCH (p:Patch)
            WHERE p.applied_at < v.ts 
              AND p.applied_at > datetime($ts) - duration('PT10M')
              AND NOT EXISTS { (v)-[:VERIFIES]->(p) }
            CREATE (v)-[:VERIFIES]->(p)
        "#
            .to_string(),
        )
        .param("verification_id", verification_id)
        .param("ts", event.ts.to_rfc3339());

        self.neo4j.run(link_query).await
    }

    /// Project ClaimMade event.
    async fn project_claim(&self, event: &Event) -> Result<()> {
        let claim_id = format!("claim_{}", event.id.0);

        let query = Query::new(
            r#"
            MATCH (e:Event {id: $event_id})
            CREATE (c:Claim {
                id: $claim_id,
                content: $content,
                ts: datetime($ts),
                has_evidence: false
            })
            CREATE (e)-[:CLAIMS]->(c)
        "#
            .to_string(),
        )
        .param("event_id", event.id.0.to_string())
        .param("claim_id", claim_id.clone())
        .param("content", event.description.clone())
        .param("ts", event.ts.to_rfc3339());

        self.neo4j.run(query).await?;

        // Check for recent verification
        let link_query = Query::new(
            r#"
            MATCH (c:Claim {id: $claim_id})
            MATCH (v:Verification {passed: true})
            WHERE v.ts >= datetime($ts) - duration('PT5M')
              AND v.ts <= datetime($ts) + duration('PT5M')
            SET c.has_evidence = true
            CREATE (v)-[:PROVES]->(c)
        "#
            .to_string(),
        )
        .param("claim_id", claim_id)
        .param("ts", event.ts.to_rfc3339());

        self.neo4j.run(link_query).await
    }

    /// Project ScopeDefined event.
    async fn project_scope(&self, event: &Event) -> Result<()> {
        let scope_id = format!("scope_{}", event.id.0);

        let query = Query::new(
            r#"
            MATCH (e:Event {id: $event_id})
            CREATE (s:Scope {
                id: $scope_id,
                description: $description,
                ts: datetime($ts)
            })
            CREATE (e)-[:DEFINES]->(s)
        "#
            .to_string(),
        )
        .param("event_id", event.id.0.to_string())
        .param("scope_id", scope_id.clone())
        .param("description", event.description.clone())
        .param("ts", event.ts.to_rfc3339());

        self.neo4j.run(query).await?;

        // Link scope to allowed files
        for path in &event.outputs {
            let link_query = Query::new(
                r#"
                MATCH (s:Scope {id: $scope_id})
                MERGE (f:File {path: $path})
                CREATE (s)-[:ALLOWS]->(f)
            "#
                .to_string(),
            )
            .param("scope_id", scope_id.clone())
            .param("path", path.clone());

            self.neo4j.run(link_query).await?;
        }

        Ok(())
    }

    /// Project RepoSnapshotCreated event.
    async fn project_snapshot(&self, event: &Event) -> Result<()> {
        let snapshot_id = format!("snap_{}", event.id.0);

        let query = Query::new(
            r#"
            MATCH (e:Event {id: $event_id})
            MERGE (snap:Snapshot {id: $snapshot_id})
            ON CREATE SET snap.ts = datetime($ts),
                          snap.description = $description
            MERGE (e)-[:CREATES]->(snap)
        "#
            .to_string(),
        )
        .param("event_id", event.id.0.to_string())
        .param("snapshot_id", snapshot_id)
        .param("ts", event.ts.to_rfc3339())
        .param("description", event.description.clone());

        self.neo4j.run(query).await
    }

    async fn project_context_links(&self, event: &Event, envelope: &MemoryEnvelope) -> Result<()> {
        self.project_parent_link(event).await?;
        self.project_module_link(event, envelope).await?;
        self.project_logic_tags(event, envelope).await?;
        self.project_symbol_refs(event, envelope).await?;
        self.project_file_refs(event, envelope).await?;
        Ok(())
    }

    async fn project_parent_link(&self, event: &Event) -> Result<()> {
        let Some(parent_id) = event.parent_event_id else {
            return Ok(());
        };

        let query = Query::new(
            r#"
            MERGE (parent:Event {id: $parent_event_id})
            MATCH (e:Event {id: $event_id})
            MERGE (parent)-[:PRECEDES]->(e)
        "#
            .to_string(),
        )
        .param("parent_event_id", parent_id.0.to_string())
        .param("event_id", event.id.0.to_string());

        self.neo4j.run(query).await
    }

    async fn project_module_link(&self, event: &Event, envelope: &MemoryEnvelope) -> Result<()> {
        let Some(module_id) = envelope
            .module_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return Ok(());
        };

        let query = Query::new(
            r#"
            MATCH (e:Event {id: $event_id})
            MERGE (m:Module {id: $module_id})
            ON CREATE SET m.name = $module_id
            MERGE (e)-[:IN_MODULE]->(m)
        "#
            .to_string(),
        )
        .param("event_id", event.id.0.to_string())
        .param("module_id", module_id.to_string());

        self.neo4j.run(query).await
    }

    async fn project_logic_tags(&self, event: &Event, envelope: &MemoryEnvelope) -> Result<()> {
        for tag in envelope.logic_tags.iter().filter(|value| is_plausible_tag(value)) {
            let query = Query::new(
                r#"
                MATCH (e:Event {id: $event_id})
                MERGE (t:Tag {name: $name})
                MERGE (e)-[:TAGGED]->(t)
            "#
                .to_string(),
            )
            .param("event_id", event.id.0.to_string())
            .param("name", tag.clone());

            self.neo4j.run(query).await?;
        }

        Ok(())
    }

    async fn project_symbol_refs(&self, event: &Event, envelope: &MemoryEnvelope) -> Result<()> {
        for fqn in envelope
            .symbol_refs
            .iter()
            .filter(|value| is_plausible_symbol_ref(value))
        {
            let symbol_name = fqn.rsplit("::").next().unwrap_or(fqn.as_str()).to_string();
            let query = Query::new(
                r#"
                MATCH (e:Event {id: $event_id})
                MERGE (sym:Symbol {fqn: $fqn})
                ON CREATE SET sym.name = $name,
                              sym.description = $fqn
                MERGE (e)-[:MENTIONS]->(sym)
            "#
                .to_string(),
            )
            .param("event_id", event.id.0.to_string())
            .param("fqn", fqn.clone())
            .param("name", symbol_name);

            self.neo4j.run(query).await?;
        }

        Ok(())
    }

    async fn project_file_refs(&self, event: &Event, envelope: &MemoryEnvelope) -> Result<()> {
        for path in envelope
            .file_refs
            .iter()
            .filter(|value| is_plausible_file_ref(value))
        {
            let query = Query::new(
                r#"
                MATCH (e:Event {id: $event_id})
                MERGE (f:File {path: $path})
                MERGE (e)-[:MENTIONS]->(f)
            "#
                .to_string(),
            )
            .param("event_id", event.id.0.to_string())
            .param("path", path.clone());

            self.neo4j.run(query).await?;
        }

        Ok(())
    }

    /// Save checkpoint (last projected event ID).
    pub fn save_checkpoint(&self, event_id: &EventId) -> Result<()> {
        self.storage
            .put("kv", NEO4J_CHECKPOINT_KEY, &event_id.0.to_bytes())
    }

    /// Get the last projected event ID.
    pub fn get_checkpoint(&self) -> Result<Option<EventId>> {
        match self.storage.get("kv", NEO4J_CHECKPOINT_KEY)? {
            Some(bytes) => {
                if bytes.len() == 16 {
                    let mut arr = [0u8; 16];
                    arr.copy_from_slice(&bytes);
                    Ok(Some(EventId(ulid::Ulid::from_bytes(arr))))
                } else {
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }

    /// Clear checkpoint (for rebuild).
    pub fn clear_checkpoint(&self) -> Result<()> {
        self.storage.delete("kv", NEO4J_CHECKPOINT_KEY)
    }

    /// Get the Neo4j connector.
    pub fn neo4j(&self) -> &Neo4jConnector {
        &self.neo4j
    }
}

fn is_plausible_file_ref(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && !trimmed.contains('\n')
        && trimmed.len() <= 512
        && (trimmed.contains('/') || trimmed.contains('\\'))
}

fn is_plausible_symbol_ref(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty() && !trimmed.contains('\n') && trimmed.len() <= 256 && trimmed.contains("::")
}

fn is_plausible_tag(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty() && !trimmed.contains('\n') && trimmed.len() <= 128
}
