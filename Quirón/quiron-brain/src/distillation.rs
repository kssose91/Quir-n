//! # Memory Distillation
//!
//! Condensar eventos en conocimiento duradero.
//! Los eventos son efímeros, el conocimiento permanece.
//!
//! ## Proceso
//! 1. Agrupar eventos por tema/entidad
//! 2. Extraer patrones recurrentes
//! 3. Generar "memorias destiladas" que representan aprendizajes
//! 4. Podar eventos redundantes, mantener destilados

use crate::ledger::LedgerReader;
use crate::memory::MemoryEnvelopeStore;
use crate::storage::Storage;
use crate::types::{Event, EventId, EventKind};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap}; // FIX 1: BTreeMap for deterministic order

/// Memoria destilada - conocimiento extraído de múltiples eventos
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistilledMemory {
    /// ID único
    pub id: ulid::Ulid,
    /// Tema o entidad principal
    pub topic: String,
    /// Conocimiento esencial (resumen)
    pub essence: String,
    /// Patrones observados
    pub patterns: Vec<Pattern>,
    /// IDs de eventos fuente
    pub source_events: Vec<EventId>,
    /// Importancia calculada
    pub importance: f32,
    /// Primera observación
    pub first_seen: DateTime<Utc>,
    /// Última observación
    pub last_seen: DateTime<Utc>,
    /// Veces reforzado
    pub reinforcement_count: u32,
    /// Tags
    pub tags: Vec<String>,
}

/// Patrón extraído
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pattern {
    /// Nombre del patrón
    pub name: String,
    /// Frecuencia (cuántas veces observado)
    pub frequency: u32,
    /// Confianza (0.0-1.0)
    pub confidence: f32,
    /// Ejemplo representativo
    pub example: Option<String>,
}

/// Configuración del destilador
#[derive(Debug, Clone)]
pub struct DistillerConfig {
    /// Mínimo de eventos para destilar
    pub min_events_to_distill: usize,
    /// Umbral de similitud para agrupar (TODO: implement semantic grouping)
    /// Currently unused - grouping is by exact topic match
    #[allow(dead_code)]
    pub similarity_threshold: f32,
    /// Máximo de patrones por memoria
    pub max_patterns: usize,
    /// Edad mínima de eventos para destilar (segundos)
    pub min_event_age_seconds: u64,
}

impl Default for DistillerConfig {
    fn default() -> Self {
        Self {
            min_events_to_distill: 3,
            similarity_threshold: 0.5,
            max_patterns: 5,
            min_event_age_seconds: 3600, // 1 hora
        }
    }
}

/// Destilador de memoria
pub struct MemoryDistiller<'a> {
    /// Storage for future prune operations (TODO: implement event pruning)
    #[allow(dead_code)]
    storage: &'a Storage,
    reader: &'a LedgerReader,
    config: DistillerConfig,
}

impl<'a> MemoryDistiller<'a> {
    /// Crear nuevo destilador
    pub fn new(storage: &'a Storage, reader: &'a LedgerReader) -> Self {
        Self {
            storage,
            reader,
            config: DistillerConfig::default(),
        }
    }

    /// Crear con configuración personalizada
    pub fn with_config(
        storage: &'a Storage,
        reader: &'a LedgerReader,
        config: DistillerConfig,
    ) -> Self {
        Self {
            storage,
            reader,
            config,
        }
    }

    /// Ejecutar destilación completa
    pub fn distill(&self) -> crate::error::Result<Vec<DistilledMemory>> {
        // 1. Obtener eventos candidatos (suficientemente viejos)
        let now = Utc::now();
        let events = self.reader.recent(500)?;
        let envelopes = MemoryEnvelopeStore::new(self.storage.clone());

        let candidates: Vec<_> = events
            .into_iter()
            .filter(|e| {
                // FIX 2: Safe age calculation (avoid negative causing huge u64)
                let age_seconds = (now - e.ts).num_seconds();
                if age_seconds < 0 {
                    return false; // Future event, skip
                }
                if (age_seconds as u64) < self.config.min_event_age_seconds {
                    return false;
                }

                match envelopes.get_or_default(e) {
                    Ok(envelope) => envelope.eligible_for_distillation(),
                    Err(err) => {
                        tracing::warn!(
                            "Failed to load memory envelope while distilling event {}: {}",
                            e.id,
                            err
                        );
                        false
                    }
                }
            })
            .collect();

        if candidates.len() < self.config.min_events_to_distill {
            return Ok(vec![]);
        }

        // 2. Agrupar por tema
        let groups = self.group_by_topic(&candidates);

        // 3. Destilar cada grupo
        let mut memories = Vec::new();
        for (topic, group_events) in groups {
            if group_events.len() >= self.config.min_events_to_distill {
                if let Some(memory) = self.distill_group(&topic, &group_events) {
                    memories.push(memory);
                }
            }
        }

        Ok(memories)
    }

    /// Agrupar eventos por tema (por ahora, por archivo/proyecto)
    /// FIX 1: Using BTreeMap for deterministic iteration order
    fn group_by_topic(&self, events: &[Event]) -> BTreeMap<String, Vec<Event>> {
        let mut groups: BTreeMap<String, Vec<Event>> = BTreeMap::new();

        for event in events {
            // Extraer topic de inputs/outputs o description
            let topic = self.extract_topic(event);
            groups.entry(topic).or_default().push(event.clone());
        }

        groups
    }

    /// Extraer topic de un evento
    /// FIX 4: Better path detection and collision avoidance
    fn extract_topic(&self, event: &Event) -> String {
        // Prioridad: 1) Primer output, 2) Primer input, 3) Primera palabra significativa
        if let Some(output) = event.outputs.first() {
            return self.normalize_path_or_id(output, event.project_id.as_deref());
        }
        if let Some(input) = event.inputs.first() {
            return self.normalize_path_or_id(input, event.project_id.as_deref());
        }

        // Extraer primera palabra significativa de description
        event
            .description
            .split_whitespace()
            .find(|w| w.len() > 3)
            .unwrap_or("general")
            .to_lowercase()
    }

    /// Normalizar path o identificador a topic
    /// FIX 4: Detect if it looks like a path vs an identifier
    fn normalize_path_or_id(&self, value: &str, project_id: Option<&str>) -> String {
        // Check if it looks like a path
        let is_path = value.contains('/')
            || value.contains('\\')
            || value.ends_with(".rs")
            || value.ends_with(".py")
            || value.ends_with(".ts")
            || value.ends_with(".go");

        if is_path {
            self.normalize_path(value, project_id)
        } else {
            // It's an identifier, return as-is (lowercase)
            value.to_lowercase()
        }
    }

    /// Normalizar path a topic
    /// FIX 4: Include project prefix to avoid collisions
    /// FIX A: Use 2 last segments for len >= 2 to preserve context
    /// FIX B: Normalize Windows backslashes before splitting
    fn normalize_path(&self, path: &str, project_id: Option<&str>) -> String {
        // FIX B: Normalize Windows paths
        let normalized_path = path.replace('\\', "/");

        // Extraer componente significativo del path
        // /home/user/project/src/foo.rs -> src/foo.rs o project_id/src/foo.rs
        let parts: Vec<_> = normalized_path
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        // FIX A: For len >= 2, use 2 last segments to preserve context
        let normalized = if parts.len() >= 2 {
            format!("{}/{}", parts[parts.len() - 2], parts[parts.len() - 1])
        } else {
            parts.last().unwrap_or(&"unknown").to_string()
        };

        // Prepend project_id if available to avoid collisions
        match project_id {
            Some(proj) if !proj.is_empty() => format!("{}:{}", proj, normalized),
            _ => normalized,
        }
    }

    /// Destilar un grupo de eventos en una memoria
    fn distill_group(&self, topic: &str, events: &[Event]) -> Option<DistilledMemory> {
        if events.is_empty() {
            return None;
        }

        // Calcular timestamps
        let first_seen = events.iter().map(|e| e.ts).min().unwrap();
        let last_seen = events.iter().map(|e| e.ts).max().unwrap();

        // Calcular importancia promedio con boost por frecuencia
        let base_importance: f32 =
            events.iter().map(|e| e.importance).sum::<f32>() / events.len() as f32;
        let frequency_boost = (events.len() as f32 / 10.0).min(0.3);
        let importance = (base_importance + frequency_boost).min(1.0);

        // Extraer patrones
        let patterns = self.extract_patterns(events);

        // Generar esencia (resumen)
        let essence = self.generate_essence(topic, events, &patterns);

        // Extraer tags comunes
        let tags = self.extract_common_tags(events);

        // IDs de eventos fuente
        let source_events: Vec<_> = events.iter().map(|e| e.id).collect();

        Some(DistilledMemory {
            id: ulid::Ulid::new(),
            topic: topic.to_string(),
            essence,
            patterns,
            source_events,
            importance,
            first_seen,
            last_seen,
            reinforcement_count: events.len() as u32,
            tags,
        })
    }

    /// Extraer patrones de eventos
    /// FIX 5: Sort patterns by frequency before taking top N
    /// FIX: Use quotas (3 kinds + 2 keywords) to ensure keywords aren't truncated
    fn extract_patterns(&self, events: &[Event]) -> Vec<Pattern> {
        let mut patterns = Vec::new();

        // Cupo: reservar espacio para kinds y keywords
        let max_kinds = self.config.max_patterns.saturating_sub(2);
        let max_keywords = 2.min(self.config.max_patterns);

        // Patrón: tipos de evento comunes (using BTreeMap for determinism)
        let mut kind_counts: BTreeMap<String, u32> = BTreeMap::new();
        for event in events {
            let kind_str = format!("{:?}", event.kind);
            *kind_counts.entry(kind_str).or_default() += 1;
        }

        // FIX 5: Sort by frequency desc, then name asc for determinism
        let mut kind_vec: Vec<_> = kind_counts.into_iter().collect();
        kind_vec.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        for (kind, count) in kind_vec.into_iter().take(max_kinds) {
            if count >= 2 {
                patterns.push(Pattern {
                    name: format!("frequent_{}", kind.to_lowercase()),
                    frequency: count,
                    confidence: count as f32 / events.len() as f32,
                    example: None,
                });
            }
        }

        // Patrón: palabras frecuentes en descriptions
        let mut word_counts: HashMap<String, u32> = HashMap::new();
        for event in events {
            for word in event.description.split_whitespace() {
                let word = word.to_lowercase();
                if word.len() > 4 {
                    *word_counts.entry(word).or_default() += 1;
                }
            }
        }

        let mut word_vec: Vec<_> = word_counts.into_iter().collect();
        // FIX: Sort with tie-breaker for determinism
        word_vec.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        for (word, count) in word_vec.into_iter().take(max_keywords) {
            if count >= 2 {
                patterns.push(Pattern {
                    name: format!("keyword_{}", word),
                    frequency: count,
                    confidence: count as f32 / events.len() as f32,
                    example: None,
                });
            }
        }

        patterns
    }

    /// Generar esencia (resumen) del grupo
    fn generate_essence(&self, topic: &str, events: &[Event], patterns: &[Pattern]) -> String {
        let action_types: Vec<_> = events
            .iter()
            .map(|e| match e.kind {
                EventKind::Action => "action".to_string(),
                EventKind::Observation => "observed".to_string(),
                EventKind::Decision => "decided".to_string(),
                EventKind::Alert => "alert".to_string(),
                EventKind::Run => "run".to_string(),
                EventKind::FileRead => "file_read".to_string(),
                EventKind::PatchApplied => "patch_applied".to_string(),
                _ => "worked_on".to_string(),
            })
            .collect();

        // FIX 6: Use BTreeSet for deterministic order instead of HashSet
        let unique_actions: std::collections::BTreeSet<_> = action_types.iter().cloned().collect();
        let actions_str = unique_actions
            .into_iter()
            .take(3)
            .collect::<Vec<_>>()
            .join(", ");

        let pattern_summary = if patterns.is_empty() {
            String::new()
        } else {
            let pattern_names: Vec<_> = patterns
                .iter()
                .take(2)
                .map(|p| p.name.replace("frequent_", "").replace("keyword_", ""))
                .collect();
            format!(". Patterns: {}", pattern_names.join(", "))
        };

        format!(
            "{} ({} events): {}{}",
            topic,
            events.len(),
            actions_str,
            pattern_summary
        )
    }

    /// Extraer tags comunes
    fn extract_common_tags(&self, events: &[Event]) -> Vec<String> {
        let mut tag_counts: HashMap<String, u32> = HashMap::new();

        for event in events {
            for tag in &event.tags {
                *tag_counts.entry(tag.clone()).or_default() += 1;
            }
        }

        let threshold = (events.len() / 2).max(1);
        let mut common: Vec<_> = tag_counts
            .into_iter()
            .filter(|(_, count)| *count >= threshold as u32)
            .map(|(tag, _)| tag)
            .collect();

        common.sort();
        common.truncate(5);
        common
    }
}

/// Store para memorias destiladas
pub struct DistilledMemoryStore {
    storage: Storage,
}

impl DistilledMemoryStore {
    /// Crear nuevo store
    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }

    /// Guardar memoria
    pub fn save(&self, memory: &DistilledMemory) -> crate::error::Result<()> {
        let key = memory.id.to_bytes();
        let value = bincode::serialize(memory)?;
        self.storage
            .put(crate::storage::cf::CF_MEMORIES, &key, &value)?;
        Ok(())
    }

    /// Obtener memoria por ID
    pub fn get(&self, id: ulid::Ulid) -> crate::error::Result<Option<DistilledMemory>> {
        let key = id.to_bytes();
        match self.storage.get(crate::storage::cf::CF_MEMORIES, &key)? {
            Some(bytes) => Ok(Some(bincode::deserialize(&bytes)?)),
            None => Ok(None),
        }
    }

    /// Listar todas las memorias
    pub fn list(&self) -> crate::error::Result<Vec<DistilledMemory>> {
        let mut memories = Vec::new();
        for (_, value) in self.storage.iter_tree(crate::storage::cf::CF_MEMORIES)? {
            let memory: DistilledMemory = bincode::deserialize(&value)?;
            memories.push(memory);
        }
        // FIX 7: Stable sort with 3 criteria for determinism
        // FIX E: Compare Ulid directly instead of converting to String (O(1) vs O(N))
        memories.sort_by(|a, b| {
            b.importance
                .partial_cmp(&a.importance)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.last_seen.cmp(&a.last_seen))
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(memories)
    }

    /// Buscar memorias por topic
    pub fn find_by_topic(&self, topic: &str) -> crate::error::Result<Vec<DistilledMemory>> {
        let all = self.list()?;
        Ok(all
            .into_iter()
            .filter(|m| m.topic.contains(topic))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::{LedgerReader, LedgerWriter};
    use crate::storage::Storage;
    use tempfile::TempDir;

    fn setup() -> (TempDir, Storage, LedgerReader) {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let reader = LedgerReader::new(storage.clone());
        (dir, storage, reader)
    }

    #[test]
    fn test_distiller_empty() {
        let (_dir, storage, reader) = setup();
        let distiller = MemoryDistiller::new(&storage, &reader);

        let memories = distiller.distill().unwrap();
        assert!(
            memories.is_empty(),
            "Should produce no memories from empty ledger"
        );
    }

    #[test]
    fn test_distiller_config() {
        let config = DistillerConfig {
            min_events_to_distill: 5,
            similarity_threshold: 0.7,
            max_patterns: 3,
            min_event_age_seconds: 60,
        };

        assert_eq!(config.min_events_to_distill, 5);
        assert_eq!(config.max_patterns, 3);
    }

    #[test]
    fn test_pattern_extraction() {
        // Test pattern struct
        let pattern = Pattern {
            name: "frequent_action".to_string(),
            frequency: 5,
            confidence: 0.8,
            example: Some("example text".to_string()),
        };

        assert_eq!(pattern.name, "frequent_action");
        assert_eq!(pattern.frequency, 5);
    }

    #[test]
    fn test_distill_skips_working_set_session_events() {
        let (_dir, storage, reader) = setup();
        let writer = LedgerWriter::new(storage.clone());
        for idx in 0..3 {
            writer
                .append(Event::new(
                    EventKind::Conversation,
                    format!("session chatter {}", idx),
                ))
                .unwrap();
        }

        let distiller = MemoryDistiller::with_config(
            &storage,
            &reader,
            DistillerConfig {
                min_event_age_seconds: 0,
                ..DistillerConfig::default()
            },
        );

        let memories = distiller.distill().unwrap();
        assert!(memories.is_empty(), "working-set chat should not distill");
    }

    #[test]
    fn test_distill_keeps_candidate_and_promoted_events() {
        let (_dir, storage, reader) = setup();
        let writer = LedgerWriter::new(storage.clone());
        for idx in 0..3 {
            writer
                .append(Event::new(
                    EventKind::Observation,
                    format!("alpha implementation {}", idx),
                ))
                .unwrap();
        }

        let distiller = MemoryDistiller::with_config(
            &storage,
            &reader,
            DistillerConfig {
                min_event_age_seconds: 0,
                ..DistillerConfig::default()
            },
        );

        let memories = distiller.distill().unwrap();
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].topic, "alpha");
        assert_eq!(memories[0].source_events.len(), 3);
    }

    #[test]
    fn test_normalize_path() {
        let (_dir, storage, reader) = setup();
        let distiller = MemoryDistiller::new(&storage, &reader);

        let path = "/home/user/project/src/lib.rs";
        // FIX 4: normalize_path now takes optional project_id
        let normalized = distiller.normalize_path(path, None);

        assert_eq!(normalized, "src/lib.rs");
    }
}
