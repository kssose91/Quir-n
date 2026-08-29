//! Graph builder for materializing nodes and edges.

use crate::error::{BrainError, Result};
use crate::memory::MemoryEnvelopeStore;
use crate::storage::cf::*;
use crate::storage::keys::*;
use crate::storage::Storage;
use crate::types::{Edge, EdgeKind, Event, EventKind, Node, NodeId, NodeKind};

/// Builds and queries the cognitive graph.
pub struct GraphBuilder {
    storage: Storage,
}

impl GraphBuilder {
    /// Create a new graph builder.
    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }

    /// Project an event into the graph as nodes and edges.
    /// This is called after an event is appended to the ledger.
    pub fn project_event(&self, event: &Event) -> Result<()> {
        let envelope = MemoryEnvelopeStore::new(self.storage.clone()).get_or_default(event)?;
        if !envelope.projects_to_graph() {
            tracing::debug!(
                event_id = %event.id,
                promotion_status = ?envelope.promotion_status,
                "Skipping local graph projection due to memory promotion policy"
            );
            return Ok(());
        }

        // Create a node for the event based on its kind
        let node_kind = match event.kind {
            EventKind::Decision => NodeKind::Decision,
            EventKind::Action => NodeKind::Action,
            EventKind::Observation => NodeKind::Observation,
            EventKind::Run => NodeKind::Run,
            EventKind::Artifact => NodeKind::Artifact,
            EventKind::Alert => NodeKind::Alert,
            EventKind::Invariant => NodeKind::Invariant,
            EventKind::Query => NodeKind::Decision, // Query is like a decision to investigate
            // Senior Supervisor Protocol kinds map to their closest equivalents
            EventKind::FileRead | EventKind::RepoSnapshotCreated => NodeKind::Observation,
            EventKind::PatchProposed | EventKind::PatchApplied | EventKind::PatchReverted => {
                NodeKind::Action
            }
            EventKind::ToolRunRecorded | EventKind::VerificationRecorded => NodeKind::Run,
            EventKind::ClaimMade | EventKind::ClaimVerified | EventKind::ClaimRejected => {
                NodeKind::Decision
            }
            EventKind::ScopeDefined | EventKind::ScopeViolation => NodeKind::Invariant,

            // === Conversaciones Humanas ===
            EventKind::Conversation => NodeKind::Conversation,
            EventKind::Emotion | EventKind::Gratitude | EventKind::Frustration => NodeKind::Emotion,
            EventKind::Humor | EventKind::Celebration => NodeKind::Humor,
            EventKind::Philosophy | EventKind::Reflection => NodeKind::Philosophy,
            EventKind::Metaphor | EventKind::Insight => NodeKind::Metaphor,
            EventKind::Dream => NodeKind::Dream,
            EventKind::Memory => NodeKind::Memory,
            EventKind::Teaching | EventKind::Correction | EventKind::Encouragement => {
                NodeKind::Teaching
            }
            EventKind::Question | EventKind::Confusion => NodeKind::Question,
            EventKind::Apology | EventKind::Agreement | EventKind::Disagreement => {
                NodeKind::Conversation
            }

            // === Relaciones Interpersonales ===
            EventKind::PersonalStory => NodeKind::PersonalStory,
            EventKind::AdviceSought | EventKind::AdviceGiven => NodeKind::Teaching,
            EventKind::PromiseMade | EventKind::PromiseKept => NodeKind::Promise,
            EventKind::Concern | EventKind::Empathy => NodeKind::Empathy,

            // === Creatividad ===
            EventKind::CreativeIdea | EventKind::NameCreated | EventKind::DesignProposed => {
                NodeKind::Creative
            }
            EventKind::ThoughtExperiment => NodeKind::ThoughtExperiment,

            // === Evolución y Aprendizaje ===
            EventKind::CategoryDiscovered | EventKind::PatternLearned => NodeKind::Pattern,
            EventKind::ConceptLinked => NodeKind::Concept,
            EventKind::DateRemembered => NodeKind::DateMemory,
            EventKind::PreferenceLearned => NodeKind::Preference,
            EventKind::BehaviorCorrected => NodeKind::Action,
            EventKind::NewKindDiscovered => NodeKind::Emergent,
        };

        // Create the event node
        let mut node = Node::new(node_kind, &event.description, event.id)
            .with_description(&event.description)
            .with_confidence(envelope.confidence);
        node.id = NodeId::from_content(&event.id.to_bytes());
        if let Some(project_id) = &event.project_id {
            node = node.with_project(project_id.clone());
        }

        self.add_node(&node)?;

        // Create edges for inputs (files read, dependencies)
        for (_i, input) in event.inputs.iter().enumerate() {
            // Create a simple FILE node for each input
            let input_node = Node::new(NodeKind::Artifact, input, event.id)
                .with_description(format!("Input file: {}", input));

            // Use a deterministic ID based on the path
            let input_node_id = NodeId::from_content(input.as_bytes());
            let mut input_node_with_id = input_node;
            input_node_with_id.id = input_node_id;

            // Only add if it doesn't exist (MERGE behavior)
            if self.get_node(&input_node_id)?.is_none() {
                self.add_node(&input_node_with_id)?;
            }

            // Edge: event READS input
            let edge = Edge::deterministic(EdgeKind::Reads, node.id, input_node_id, event.id)
                .with_confidence(envelope.confidence);
            self.add_edge(&edge)?;
        }

        // Create edges for outputs (files written, artifacts produced)
        for output in &event.outputs {
            let output_node_id = NodeId::from_content(output.as_bytes());

            // Create output node if not exists
            if self.get_node(&output_node_id)?.is_none() {
                let output_node = Node::new(NodeKind::Artifact, output, event.id)
                    .with_description(format!("Output: {}", output));
                let mut output_node_with_id = output_node;
                output_node_with_id.id = output_node_id;
                self.add_node(&output_node_with_id)?;
            }

            // Edge: event PRODUCES output
            let edge = Edge::deterministic(EdgeKind::Produces, node.id, output_node_id, event.id)
                .with_confidence(envelope.confidence);
            self.add_edge(&edge)?;
        }

        // Link to parent event if exists
        if let Some(parent_id) = &event.parent_event_id {
            // Create edge to parent
            let parent_node_id = NodeId::from_content(parent_id.to_bytes().as_slice());
            if self.get_node(&parent_node_id)?.is_some() {
                let edge =
                    Edge::deterministic(EdgeKind::Precedes, parent_node_id, node.id, event.id)
                        .with_confidence(envelope.confidence);
                self.add_edge(&edge)?;
            }
        }

        tracing::debug!("Projected event {} as node {}", event.id, node.id);
        Ok(())
    }

    /// Add a node to the graph.
    pub fn add_node(&self, node: &Node) -> Result<()> {
        let key = encode_node_key(&node.id);
        let value = bincode::serialize(node)?;
        self.storage.put(CF_NODES, &key, &value)?;
        tracing::debug!("Added node {} ({:?})", node.id, node.kind);
        Ok(())
    }

    /// Get a node by ID.
    pub fn get_node(&self, id: &NodeId) -> Result<Option<Node>> {
        let key = encode_node_key(id);
        match self.storage.get(CF_NODES, &key)? {
            Some(bytes) => Ok(Some(bincode::deserialize(&bytes)?)),
            None => Ok(None),
        }
    }

    /// Get a node by ID, returning an error if not found.
    pub fn get_node_or_error(&self, id: &NodeId) -> Result<Node> {
        self.get_node(id)?
            .ok_or_else(|| BrainError::NodeNotFound(id.to_string()))
    }

    /// Add an edge to the graph.
    pub fn add_edge(&self, edge: &Edge) -> Result<()> {
        let edge_bytes = bincode::serialize(edge)?;
        let kind_byte = edge.kind.as_u8();

        // edges_out: src|kind|dst|edge_id -> edge
        let out_key = encode_edge_out_key(&edge.source, kind_byte, &edge.target, &edge.id);
        self.storage.put(CF_EDGES_OUT, &out_key, &edge_bytes)?;

        // edges_in: dst|kind|src|edge_id -> edge_bytes (duplicated for fast reads)
        let in_key = encode_edge_in_key(&edge.target, kind_byte, &edge.source, &edge.id);
        self.storage.put(CF_EDGES_IN, &in_key, &edge_bytes)?;

        tracing::debug!(
            "Added edge {:?}: {} -> {} (id: {:?})",
            edge.kind,
            edge.source,
            edge.target,
            edge.id
        );
        Ok(())
    }

    /// Get outgoing edges from a node.
    pub fn edges_from(&self, node_id: &NodeId) -> Result<Vec<Edge>> {
        let prefix = edge_out_prefix(node_id);
        let mut edges = Vec::new();

        for (_, value) in self.storage.prefix_iter(CF_EDGES_OUT, &prefix)? {
            let edge: Edge = bincode::deserialize(&value)?;
            edges.push(edge);
        }

        Ok(edges)
    }

    /// Get outgoing edges of a specific kind.
    pub fn edges_from_of_kind(&self, node_id: &NodeId, kind: EdgeKind) -> Result<Vec<Edge>> {
        let mut prefix = node_id.to_bytes().to_vec();
        prefix.push(kind.as_u8());

        let mut edges = Vec::new();

        for (_, value) in self.storage.prefix_iter(CF_EDGES_OUT, &prefix)? {
            let edge: Edge = bincode::deserialize(&value)?;
            edges.push(edge);
        }

        Ok(edges)
    }

    /// Get incoming edges to a node.
    pub fn edges_to(&self, node_id: &NodeId) -> Result<Vec<Edge>> {
        let prefix = edge_in_prefix(node_id);
        let mut edges = Vec::new();

        // Key format: dst|kind|src|edge_id -> edge_bytes
        // We iterate by dst prefix and deserialize directly (no double lookup)
        for (_, value) in self.storage.prefix_iter(CF_EDGES_IN, &prefix)? {
            let edge: Edge = bincode::deserialize(&value)?;
            edges.push(edge);
        }

        Ok(edges)
    }

    /// Count total nodes.
    pub fn node_count(&self) -> Result<u64> {
        self.storage.count(CF_NODES)
    }

    /// Get all nodes (use with caution).
    pub fn all_nodes(&self) -> Result<Vec<Node>> {
        let mut nodes = Vec::new();

        for (_, value) in self.storage.iter_tree(CF_NODES)? {
            let node: Node = bincode::deserialize(&value)?;
            nodes.push(node);
        }

        Ok(nodes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Event, EventId, EventKind, NodeKind};
    use tempfile::TempDir;

    #[test]
    fn test_add_and_get_node() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let builder = GraphBuilder::new(storage);

        let event_id = EventId::new();
        let node = Node::new(NodeKind::Decision, "Test decision", event_id);
        let node_id = node.id;

        builder.add_node(&node).unwrap();

        let loaded = builder.get_node(&node_id).unwrap().unwrap();
        assert_eq!(loaded.name, "Test decision");
    }

    #[test]
    fn test_add_and_query_edges() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let builder = GraphBuilder::new(storage);

        let event_id = EventId::new();
        let node_a = Node::new(NodeKind::Action, "Action A", event_id);
        let node_b = Node::new(NodeKind::Artifact, "Artifact B", event_id);

        builder.add_node(&node_a).unwrap();
        builder.add_node(&node_b).unwrap();

        let edge = Edge::new(EdgeKind::Produces, node_a.id, node_b.id, event_id);
        builder.add_edge(&edge).unwrap();

        // Query outgoing
        let edges_out = builder.edges_from(&node_a.id).unwrap();
        assert_eq!(edges_out.len(), 1);
        assert_eq!(edges_out[0].target, node_b.id);

        // Query incoming
        let edges_in = builder.edges_to(&node_b.id).unwrap();
        assert_eq!(edges_in.len(), 1);
        assert_eq!(edges_in[0].source, node_a.id);
    }

    #[test]
    fn test_project_event_respects_promotion_policy() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let builder = GraphBuilder::new(storage);

        let event = Event::new(EventKind::ClaimMade, "Tentative claim without verification");
        builder.project_event(&event).unwrap();

        assert_eq!(builder.node_count().unwrap(), 0);
    }

    #[test]
    fn test_project_event_uses_deterministic_parent_link() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let builder = GraphBuilder::new(storage);

        let parent = Event::new(EventKind::Decision, "Parent decision");
        let child = Event::new(EventKind::Decision, "Child decision").with_parent(parent.id);

        builder.project_event(&parent).unwrap();
        builder.project_event(&child).unwrap();

        let parent_node_id = NodeId::from_content(&parent.id.to_bytes());
        let edges = builder.edges_from(&parent_node_id).unwrap();
        assert!(edges.iter().any(|edge| edge.kind == EdgeKind::Precedes));
    }
}
