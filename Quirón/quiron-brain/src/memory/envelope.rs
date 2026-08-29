//! Versioned semantic envelope for event-backed memories.

use crate::error::Result;
use crate::storage::cf::{CF_MEMORY_ENVELOPES, CF_MEMORY_ENVELOPE_HISTORY};
use crate::storage::Storage;
use crate::types::{Event, EventId, EventKind};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const MEMORY_ENVELOPE_SCHEMA_VERSION: u16 = 2;
pub const MEMORY_CLASSIFICATION_VERSION: u16 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TruthStatus {
    Observed,
    Inferred,
    Summarized,
    Retracted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemorySource {
    Human,
    Agent,
    System,
    Derived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryScope {
    Session,
    Project,
    Global,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromotionStatus {
    WorkingSet,
    Candidate,
    Promoted,
    Suppressed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryTarget {
    Graph,
    Semantic,
    Distilled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    Decision,
    Observation,
    Conversation,
    Insight,
    Preference,
    Problem,
    Task,
    Evidence,
    Execution,
}

impl MemoryEnvelope {
    pub fn projects_to_graph(&self) -> bool {
        self.promotion_targets.contains(&MemoryTarget::Graph)
            && matches!(
                self.promotion_status,
                PromotionStatus::Candidate | PromotionStatus::Promoted
            )
    }

    pub fn indexes_semantic(&self) -> bool {
        self.promotion_targets.contains(&MemoryTarget::Semantic)
            && matches!(
                self.promotion_status,
                PromotionStatus::Candidate | PromotionStatus::Promoted
            )
    }

    pub fn projects_to_neo4j(&self) -> bool {
        self.promotion_targets.contains(&MemoryTarget::Graph)
            && self.promotion_status == PromotionStatus::Promoted
    }

    pub fn eligible_for_distillation(&self) -> bool {
        matches!(
            self.promotion_status,
            PromotionStatus::Candidate | PromotionStatus::Promoted
        )
    }
}

/// Derived metadata attached to an event without mutating the append-only event itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryEnvelope {
    pub schema_version: u16,
    pub classification_version: u16,
    pub revision: u32,
    pub event_id: EventId,
    pub truth_status: TruthStatus,
    pub confidence: f32,
    pub source: MemorySource,
    pub scope: MemoryScope,
    pub actor: String,
    pub asserted_at: DateTime<Utc>,
    pub supersedes: Option<EventId>,
    pub retracted_by: Option<EventId>,
    pub promotion_status: PromotionStatus,
    pub promotion_targets: Vec<MemoryTarget>,
    pub promotion_basis: Vec<String>,
    pub module_id: Option<String>,
    pub file_refs: Vec<String>,
    pub symbol_refs: Vec<String>,
    pub logic_tags: Vec<String>,
    pub memory_kind: MemoryKind,
}

impl MemoryEnvelope {
    pub fn from_event(event: &Event) -> Self {
        let truth_status = classify_truth_status(event.kind);
        let confidence = classify_confidence(event.kind);
        let file_refs = classify_file_refs(event);
        let symbol_refs = classify_symbol_refs(event);
        let logic_tags = classify_logic_tags(event);
        let module_id = classify_module_id(event, &file_refs);
        let memory_kind = classify_memory_kind(event);
        let (promotion_status, promotion_targets, promotion_basis) = classify_promotion(
            event,
            truth_status,
            confidence,
            memory_kind,
            module_id.as_deref(),
            &file_refs,
            &symbol_refs,
            &logic_tags,
        );

        Self {
            schema_version: MEMORY_ENVELOPE_SCHEMA_VERSION,
            classification_version: MEMORY_CLASSIFICATION_VERSION,
            revision: 1,
            event_id: event.id,
            truth_status,
            confidence,
            source: classify_source(event),
            scope: classify_scope(event),
            actor: event.agent_id.clone(),
            asserted_at: event.ts,
            supersedes: None,
            retracted_by: None,
            promotion_status,
            promotion_targets,
            promotion_basis,
            module_id,
            file_refs,
            symbol_refs,
            logic_tags,
            memory_kind,
        }
    }

    pub fn with_revision(mut self, revision: u32) -> Self {
        self.revision = revision.max(1);
        self
    }

    pub fn with_truth_status(mut self, truth_status: TruthStatus) -> Self {
        self.truth_status = truth_status;
        self
    }

    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }

    pub fn with_retracted_by(mut self, event_id: EventId) -> Self {
        self.retracted_by = Some(event_id);
        self
    }

    pub fn with_supersedes(mut self, event_id: EventId) -> Self {
        self.supersedes = Some(event_id);
        self
    }

    pub fn with_promotion(
        mut self,
        promotion_status: PromotionStatus,
        promotion_targets: Vec<MemoryTarget>,
        promotion_basis: Vec<String>,
    ) -> Self {
        self.promotion_status = promotion_status;
        self.promotion_targets = promotion_targets;
        self.promotion_basis = promotion_basis;
        self
    }

    pub fn enrich_from_event(&mut self, event: &Event) {
        if self.file_refs.is_empty() {
            self.file_refs = classify_file_refs(event);
        }
        if self.module_id.is_none() {
            self.module_id = classify_module_id(event, &self.file_refs);
        }
        if self.symbol_refs.is_empty() {
            self.symbol_refs = classify_symbol_refs(event);
        }
        if self.logic_tags.is_empty() {
            self.logic_tags = classify_logic_tags(event);
        }
        if self.classification_version < MEMORY_CLASSIFICATION_VERSION {
            self.memory_kind = classify_memory_kind(event);
            if self.revision == 1 {
                let (promotion_status, promotion_targets, promotion_basis) = classify_promotion(
                    event,
                    self.truth_status,
                    self.confidence,
                    self.memory_kind,
                    self.module_id.as_deref(),
                    &self.file_refs,
                    &self.symbol_refs,
                    &self.logic_tags,
                );
                self.promotion_status = promotion_status;
                self.promotion_targets = promotion_targets;
                self.promotion_basis = promotion_basis;
            }
        }
        self.schema_version = MEMORY_ENVELOPE_SCHEMA_VERSION;
        self.classification_version = MEMORY_CLASSIFICATION_VERSION;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyMemoryEnvelopeV1 {
    pub schema_version: u16,
    pub classification_version: u16,
    pub revision: u32,
    pub event_id: EventId,
    pub truth_status: TruthStatus,
    pub confidence: f32,
    pub source: MemorySource,
    pub scope: MemoryScope,
    pub actor: String,
    pub asserted_at: DateTime<Utc>,
    pub supersedes: Option<EventId>,
    pub retracted_by: Option<EventId>,
    pub promotion_status: PromotionStatus,
    pub promotion_targets: Vec<MemoryTarget>,
    pub promotion_basis: Vec<String>,
}

impl From<LegacyMemoryEnvelopeV1> for MemoryEnvelope {
    fn from(value: LegacyMemoryEnvelopeV1) -> Self {
        Self {
            schema_version: value.schema_version,
            classification_version: value.classification_version,
            revision: value.revision,
            event_id: value.event_id,
            truth_status: value.truth_status,
            confidence: value.confidence,
            source: value.source,
            scope: value.scope,
            actor: value.actor,
            asserted_at: value.asserted_at,
            supersedes: value.supersedes,
            retracted_by: value.retracted_by,
            promotion_status: value.promotion_status,
            promotion_targets: value.promotion_targets,
            promotion_basis: value.promotion_basis,
            module_id: None,
            file_refs: Vec::new(),
            symbol_refs: Vec::new(),
            logic_tags: Vec::new(),
            memory_kind: MemoryKind::Observation,
        }
    }
}

pub struct MemoryEnvelopeStore {
    storage: Storage,
}

impl MemoryEnvelopeStore {
    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }

    pub fn get(&self, event_id: &EventId) -> Result<Option<MemoryEnvelope>> {
        match self
            .storage
            .get(CF_MEMORY_ENVELOPES, &event_id.to_bytes())?
        {
            Some(bytes) => Ok(Some(deserialize_memory_envelope(&bytes)?)),
            None => Ok(None),
        }
    }

    pub fn get_or_default(&self, event: &Event) -> Result<MemoryEnvelope> {
        Ok(match self.get(&event.id)? {
            Some(mut envelope) => {
                envelope.enrich_from_event(event);
                envelope
            }
            None => MemoryEnvelope::from_event(event),
        })
    }

    pub fn save(&self, envelope: &MemoryEnvelope) -> Result<()> {
        let key = envelope.event_id.to_bytes();
        let history_key = history_key(&envelope.event_id, envelope.revision);
        let value = bincode::serialize(envelope)?;

        self.storage.put(CF_MEMORY_ENVELOPES, &key, &value)?;
        self.storage
            .put(CF_MEMORY_ENVELOPE_HISTORY, &history_key, &value)?;
        Ok(())
    }

    pub fn save_default_for_event(&self, event: &Event) -> Result<MemoryEnvelope> {
        if let Some(existing) = self.get(&event.id)? {
            return Ok(existing);
        }

        let envelope = MemoryEnvelope::from_event(event);
        self.save(&envelope)?;
        Ok(envelope)
    }

    pub fn history(&self, event_id: &EventId) -> Result<Vec<MemoryEnvelope>> {
        let prefix = event_id.to_bytes();
        let mut history: Vec<MemoryEnvelope> = Vec::new();

        for (_, value) in self
            .storage
            .prefix_iter(CF_MEMORY_ENVELOPE_HISTORY, &prefix)?
        {
            history.push(deserialize_memory_envelope(&value)?);
        }

        history.sort_by_key(|entry| entry.revision);
        Ok(history)
    }

    /// Creates a new revision without touching the immutable event.
    pub fn revise(
        &self,
        event: &Event,
        mutate: impl FnOnce(&mut MemoryEnvelope),
    ) -> Result<MemoryEnvelope> {
        let current = match self.get(&event.id)? {
            Some(existing) => existing,
            None => {
                let base = MemoryEnvelope::from_event(event);
                self.save(&base)?;
                base
            }
        };
        let next_revision = current.revision + 1;
        let mut next = current.with_revision(next_revision);
        next.asserted_at = Utc::now();
        mutate(&mut next);
        next.confidence = next.confidence.clamp(0.0, 1.0);
        self.save(&next)?;
        Ok(next)
    }
}

fn history_key(event_id: &EventId, revision: u32) -> Vec<u8> {
    let mut key = event_id.to_bytes().to_vec();
    key.extend_from_slice(&revision.to_be_bytes());
    key
}

fn deserialize_memory_envelope(
    bytes: &[u8],
) -> std::result::Result<MemoryEnvelope, bincode::Error> {
    match bincode::deserialize::<MemoryEnvelope>(bytes) {
        Ok(envelope) => Ok(envelope),
        Err(_) => bincode::deserialize::<LegacyMemoryEnvelopeV1>(bytes).map(Into::into),
    }
}

fn classify_truth_status(kind: EventKind) -> TruthStatus {
    match kind {
        EventKind::ClaimMade => TruthStatus::Inferred,
        EventKind::PatternLearned
        | EventKind::CategoryDiscovered
        | EventKind::ConceptLinked
        | EventKind::Insight
        | EventKind::Reflection
        | EventKind::NewKindDiscovered => TruthStatus::Summarized,
        EventKind::ClaimRejected | EventKind::Correction | EventKind::PatchReverted => {
            TruthStatus::Retracted
        }
        _ => TruthStatus::Observed,
    }
}

fn classify_confidence(kind: EventKind) -> f32 {
    match kind {
        EventKind::FileRead
        | EventKind::ToolRunRecorded
        | EventKind::VerificationRecorded
        | EventKind::ClaimVerified
        | EventKind::RepoSnapshotCreated
        | EventKind::ScopeDefined => 0.95,
        EventKind::ClaimRejected | EventKind::Correction | EventKind::PatchReverted => 0.9,
        EventKind::ClaimMade | EventKind::DesignProposed | EventKind::ThoughtExperiment => 0.6,
        EventKind::PatternLearned
        | EventKind::CategoryDiscovered
        | EventKind::ConceptLinked
        | EventKind::Insight
        | EventKind::Reflection
        | EventKind::NewKindDiscovered => 0.72,
        _ => 0.85,
    }
}

fn classify_source(event: &Event) -> MemorySource {
    if matches!(
        event.kind,
        EventKind::PatternLearned
            | EventKind::CategoryDiscovered
            | EventKind::ConceptLinked
            | EventKind::Insight
            | EventKind::Reflection
            | EventKind::NewKindDiscovered
    ) {
        return MemorySource::Derived;
    }

    if event.agent_id.eq_ignore_ascii_case("system") || event.agent_id.starts_with("system:") {
        return MemorySource::System;
    }

    if event.agent_id.contains("mobile")
        || event.agent_id.contains("user")
        || event.agent_id.starts_with("human")
    {
        return MemorySource::Human;
    }

    MemorySource::Agent
}

fn classify_scope(event: &Event) -> MemoryScope {
    if event.project_id.is_some() {
        return MemoryScope::Project;
    }

    if is_session_kind(event.kind) {
        return MemoryScope::Session;
    }

    MemoryScope::Global
}

fn classify_promotion(
    event: &Event,
    truth_status: TruthStatus,
    confidence: f32,
    memory_kind: MemoryKind,
    module_id: Option<&str>,
    file_refs: &[String],
    symbol_refs: &[String],
    logic_tags: &[String],
) -> (PromotionStatus, Vec<MemoryTarget>, Vec<String>) {
    let has_structural_context = module_id.is_some()
        || !file_refs.is_empty()
        || !symbol_refs.is_empty()
        || !logic_tags.is_empty();
    let has_code_anchor = module_id.is_some() || !file_refs.is_empty() || !symbol_refs.is_empty();

    if truth_status == TruthStatus::Retracted {
        return (
            PromotionStatus::Suppressed,
            Vec::new(),
            vec!["retracted memory should not be promoted".to_string()],
        );
    }

    if is_session_kind(event.kind) {
        return (
            PromotionStatus::WorkingSet,
            Vec::new(),
            vec!["session-scoped conversational memory".to_string()],
        );
    }

    if matches!(
        event.kind,
        EventKind::Decision
            | EventKind::PreferenceLearned
            | EventKind::ClaimVerified
            | EventKind::ScopeDefined
            | EventKind::PromiseKept
            | EventKind::BehaviorCorrected
    ) {
        return (
            PromotionStatus::Promoted,
            vec![MemoryTarget::Graph, MemoryTarget::Semantic],
            vec!["stable decision or verified preference".to_string()],
        );
    }

    if matches!(
        event.kind,
        EventKind::PatternLearned
            | EventKind::CategoryDiscovered
            | EventKind::ConceptLinked
            | EventKind::Insight
            | EventKind::Reflection
            | EventKind::NewKindDiscovered
    ) {
        return (
            PromotionStatus::Promoted,
            vec![
                MemoryTarget::Graph,
                MemoryTarget::Semantic,
                MemoryTarget::Distilled,
            ],
            vec!["derived knowledge worth consolidation".to_string()],
        );
    }

    if has_structural_context
        && matches!(
            memory_kind,
            MemoryKind::Decision
                | MemoryKind::Evidence
                | MemoryKind::Problem
                | MemoryKind::Insight
                | MemoryKind::Preference
        )
    {
        let mut targets = vec![MemoryTarget::Graph];
        if confidence >= 0.8 || event.importance >= 0.65 {
            targets.push(MemoryTarget::Semantic);
        }

        return (
            PromotionStatus::Candidate,
            targets,
            vec!["structured memory with durable semantic anchors".to_string()],
        );
    }

    if matches!(
        event.kind,
        EventKind::VerificationRecorded
            | EventKind::RepoSnapshotCreated
            | EventKind::FileRead
            | EventKind::PatchApplied
            | EventKind::PatchProposed
            | EventKind::PatchReverted
            | EventKind::ToolRunRecorded
    ) {
        return (
            PromotionStatus::Candidate,
            vec![MemoryTarget::Graph],
            vec!["traceable engineering event".to_string()],
        );
    }

    if has_code_anchor
        && matches!(
            memory_kind,
            MemoryKind::Execution | MemoryKind::Observation | MemoryKind::Task
        )
    {
        let mut targets = vec![MemoryTarget::Graph];
        if confidence >= 0.9 || event.importance >= 0.7 || !logic_tags.is_empty() {
            targets.push(MemoryTarget::Semantic);
        }

        return (
            PromotionStatus::Candidate,
            targets,
            vec!["structured episodic memory tied to code entities".to_string()],
        );
    }

    if confidence >= 0.8 || event.importance >= 0.75 {
        return (
            PromotionStatus::Candidate,
            vec![MemoryTarget::Semantic],
            vec!["high confidence or salient event".to_string()],
        );
    }

    (
        PromotionStatus::WorkingSet,
        Vec::new(),
        vec!["kept in ledger until reinforced".to_string()],
    )
}

fn classify_memory_kind(event: &Event) -> MemoryKind {
    if let Some(kind) = tag_values(&event.tags, "memory_kind:")
        .into_iter()
        .next()
        .and_then(parse_memory_kind)
    {
        return kind;
    }

    match event.kind {
        EventKind::Decision
        | EventKind::ScopeDefined
        | EventKind::DesignProposed
        | EventKind::PromiseMade => MemoryKind::Decision,
        EventKind::Question | EventKind::AdviceSought => MemoryKind::Task,
        EventKind::Alert
        | EventKind::ClaimRejected
        | EventKind::ScopeViolation
        | EventKind::Frustration
        | EventKind::Concern => MemoryKind::Problem,
        EventKind::FileRead
        | EventKind::VerificationRecorded
        | EventKind::ClaimVerified
        | EventKind::RepoSnapshotCreated => MemoryKind::Evidence,
        EventKind::Run
        | EventKind::Action
        | EventKind::PatchApplied
        | EventKind::PatchProposed
        | EventKind::PatchReverted
        | EventKind::ToolRunRecorded
        | EventKind::Query => MemoryKind::Execution,
        EventKind::Conversation
        | EventKind::Emotion
        | EventKind::Humor
        | EventKind::Gratitude
        | EventKind::Celebration
        | EventKind::Agreement
        | EventKind::Disagreement
        | EventKind::Encouragement
        | EventKind::Apology
        | EventKind::Empathy
        | EventKind::PersonalStory
        | EventKind::AdviceGiven => MemoryKind::Conversation,
        EventKind::PreferenceLearned => MemoryKind::Preference,
        EventKind::PatternLearned
        | EventKind::CategoryDiscovered
        | EventKind::ConceptLinked
        | EventKind::Insight
        | EventKind::Reflection
        | EventKind::NewKindDiscovered
        | EventKind::ThoughtExperiment
        | EventKind::Philosophy
        | EventKind::Metaphor => MemoryKind::Insight,
        _ => MemoryKind::Observation,
    }
}

fn parse_memory_kind(value: String) -> Option<MemoryKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "decision" => Some(MemoryKind::Decision),
        "observation" => Some(MemoryKind::Observation),
        "conversation" => Some(MemoryKind::Conversation),
        "insight" => Some(MemoryKind::Insight),
        "preference" => Some(MemoryKind::Preference),
        "problem" => Some(MemoryKind::Problem),
        "task" => Some(MemoryKind::Task),
        "evidence" => Some(MemoryKind::Evidence),
        "execution" => Some(MemoryKind::Execution),
        _ => None,
    }
}

fn classify_module_id(event: &Event, file_refs: &[String]) -> Option<String> {
    tag_values(&event.tags, "module:")
        .into_iter()
        .next()
        .or_else(|| file_refs.iter().find_map(|value| module_from_path(value)))
}

fn classify_file_refs(event: &Event) -> Vec<String> {
    let mut refs = tag_values(&event.tags, "file:");
    refs.extend(
        event
            .inputs
            .iter()
            .chain(event.outputs.iter())
            .filter(|value| looks_like_file_ref(value))
            .cloned(),
    );
    unique_strings(refs)
}

fn classify_symbol_refs(event: &Event) -> Vec<String> {
    let mut refs = tag_values(&event.tags, "symbol:");
    refs.extend(
        event
            .inputs
            .iter()
            .chain(event.outputs.iter())
            .filter(|value| looks_like_symbol_ref(value))
            .cloned(),
    );
    unique_strings(refs)
}

fn classify_logic_tags(event: &Event) -> Vec<String> {
    unique_strings(tag_values(&event.tags, "logic:"))
}

fn tag_values(tags: &[String], prefix: &str) -> Vec<String> {
    tags.iter()
        .filter_map(|tag| tag.strip_prefix(prefix))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn unique_strings(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values.dedup();
    values
}

fn module_from_path(path: &str) -> Option<String> {
    let normalized = path.replace('\\', "/");
    let trimmed = normalized.trim_matches('/');
    let mut parts: Vec<_> = trimmed.split('/').filter(|part| !part.is_empty()).collect();
    if parts.len() < 2 {
        return None;
    }
    parts.pop();
    Some(parts.join("/"))
}

fn looks_like_file_ref(value: &str) -> bool {
    value.contains('/')
        || value.contains('\\')
        || value.ends_with(".rs")
        || value.ends_with(".py")
        || value.ends_with(".ts")
        || value.ends_with(".tsx")
        || value.ends_with(".js")
        || value.ends_with(".go")
        || value.ends_with(".java")
}

fn looks_like_symbol_ref(value: &str) -> bool {
    value.contains("::") && !looks_like_file_ref(value)
}

fn is_session_kind(kind: EventKind) -> bool {
    matches!(
        kind,
        EventKind::Conversation
            | EventKind::Emotion
            | EventKind::Humor
            | EventKind::Gratitude
            | EventKind::Frustration
            | EventKind::Celebration
            | EventKind::Question
            | EventKind::Correction
            | EventKind::Encouragement
            | EventKind::Apology
            | EventKind::Confusion
            | EventKind::Agreement
            | EventKind::Disagreement
            | EventKind::PersonalStory
            | EventKind::AdviceSought
            | EventKind::Empathy
            | EventKind::Concern
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::EventKind;
    use tempfile::TempDir;

    #[test]
    fn save_default_creates_first_revision() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let store = MemoryEnvelopeStore::new(storage);
        let event =
            Event::new(EventKind::ClaimMade, "Claim without verification").with_project("proj-a");

        let saved = store.save_default_for_event(&event).unwrap();

        assert_eq!(saved.revision, 1);
        assert_eq!(saved.truth_status, TruthStatus::Inferred);
        assert_eq!(saved.confidence, 0.6);
        assert_eq!(saved.promotion_status, PromotionStatus::WorkingSet);
        assert_eq!(saved.promotion_targets, Vec::<MemoryTarget>::new());
    }

    #[test]
    fn revise_persists_history() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let store = MemoryEnvelopeStore::new(storage);
        let event = Event::new(EventKind::Observation, "Observed fact");

        store.save_default_for_event(&event).unwrap();
        let revised = store
            .revise(&event, |envelope| {
                envelope.truth_status = TruthStatus::Retracted;
                envelope.confidence = 0.92;
            })
            .unwrap();

        let history = store.history(&event.id).unwrap();
        assert_eq!(revised.revision, 2);
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].revision, 1);
        assert_eq!(history[1].truth_status, TruthStatus::Retracted);
    }

    #[test]
    fn verified_decision_is_promoted() {
        let event = Event::new(EventKind::Decision, "Architectural decision");
        let envelope = MemoryEnvelope::from_event(&event);

        assert_eq!(envelope.promotion_status, PromotionStatus::Promoted);
        assert!(envelope.promotion_targets.contains(&MemoryTarget::Graph));
        assert!(envelope.promotion_targets.contains(&MemoryTarget::Semantic));
    }

    #[test]
    fn derives_taxonomy_from_tags_and_io() {
        let event = Event::new(EventKind::Observation, "Observed handler behavior")
            .with_tags(vec![
                "module:api/server".to_string(),
                "symbol:build_context::resolve".to_string(),
                "logic:hybrid_recall".to_string(),
                "memory_kind:evidence".to_string(),
                "file:src/api/server.rs".to_string(),
            ])
            .with_inputs(vec!["src/vct.rs".to_string()]);

        let envelope = MemoryEnvelope::from_event(&event);

        assert_eq!(envelope.module_id.as_deref(), Some("api/server"));
        assert!(envelope
            .file_refs
            .contains(&"src/api/server.rs".to_string()));
        assert!(envelope.file_refs.contains(&"src/vct.rs".to_string()));
        assert_eq!(
            envelope.symbol_refs,
            vec!["build_context::resolve".to_string()]
        );
        assert_eq!(envelope.logic_tags, vec!["hybrid_recall".to_string()]);
        assert_eq!(envelope.memory_kind, MemoryKind::Evidence);
        assert_eq!(envelope.promotion_status, PromotionStatus::Candidate);
        assert!(envelope.promotion_targets.contains(&MemoryTarget::Graph));
    }

    #[test]
    fn structured_execution_memory_gains_graph_projection() {
        let event = Event::new(EventKind::Action, "Edited retrieval pipeline")
            .with_project("proj-a")
            .with_tags(vec![
                "file:src/vct.rs".to_string(),
                "symbol:VirtualContextTools::recall_async".to_string(),
                "logic:hybrid_recall".to_string(),
            ]);

        let envelope = MemoryEnvelope::from_event(&event);

        assert_eq!(envelope.memory_kind, MemoryKind::Execution);
        assert_eq!(envelope.promotion_status, PromotionStatus::Candidate);
        assert!(envelope.promotion_targets.contains(&MemoryTarget::Graph));
    }

    #[test]
    fn upgrading_classification_recomputes_default_promotion_for_revision_one() {
        let event =
            Event::new(EventKind::Observation, "Observed handler behavior").with_tags(vec![
                "module:api/server".to_string(),
                "file:src/api/server.rs".to_string(),
                "memory_kind:evidence".to_string(),
            ]);
        let mut envelope: MemoryEnvelope = LegacyMemoryEnvelopeV1 {
            schema_version: 1,
            classification_version: 2,
            revision: 1,
            event_id: event.id,
            truth_status: TruthStatus::Observed,
            confidence: 0.85,
            source: MemorySource::Agent,
            scope: MemoryScope::Project,
            actor: "legacy".to_string(),
            asserted_at: event.ts,
            supersedes: None,
            retracted_by: None,
            promotion_status: PromotionStatus::Candidate,
            promotion_targets: vec![MemoryTarget::Semantic],
            promotion_basis: vec!["high confidence or salient event".to_string()],
        }
        .into();

        envelope.enrich_from_event(&event);

        assert_eq!(
            envelope.classification_version,
            MEMORY_CLASSIFICATION_VERSION
        );
        assert_eq!(envelope.memory_kind, MemoryKind::Evidence);
        assert!(envelope.promotion_targets.contains(&MemoryTarget::Graph));
    }

    #[test]
    fn deserialize_legacy_envelope_v1_falls_back_cleanly() {
        let legacy = LegacyMemoryEnvelopeV1 {
            schema_version: 1,
            classification_version: 1,
            revision: 1,
            event_id: EventId::new(),
            truth_status: TruthStatus::Observed,
            confidence: 0.85,
            source: MemorySource::Agent,
            scope: MemoryScope::Project,
            actor: "legacy".to_string(),
            asserted_at: Utc::now(),
            supersedes: None,
            retracted_by: None,
            promotion_status: PromotionStatus::Candidate,
            promotion_targets: vec![MemoryTarget::Semantic],
            promotion_basis: vec!["legacy".to_string()],
        };

        let bytes = bincode::serialize(&legacy).unwrap();
        let decoded = deserialize_memory_envelope(&bytes).unwrap();

        assert_eq!(decoded.revision, 1);
        assert_eq!(decoded.truth_status, TruthStatus::Observed);
        assert_eq!(decoded.schema_version, 1);
        assert_eq!(decoded.classification_version, 1);
        assert!(decoded.file_refs.is_empty());
        assert_eq!(decoded.memory_kind, MemoryKind::Observation);
    }
}
