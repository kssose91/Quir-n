//! Picker metadata from the official Codex CLI cache. No credentials are read.
use serde_json::Value;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct AiModel {
    pub id: String,
    pub label: String,
    pub efforts: Vec<String>,
    pub default_effort: String,
}

pub fn catalog_path() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".codex")))
        .map(|p| p.join("models_cache.json"))
}

pub fn read_catalog() -> Vec<AiModel> {
    catalog_path()
        .and_then(|p| {
            let file = std::fs::File::open(p).ok()?;
            if file.metadata().ok()?.len() > 4_000_000 {
                return None;
            }
            serde_json::from_reader::<_, Value>(file).ok()
        })
        .map(|v| parse_catalog(&v))
        .unwrap_or_default()
}

pub fn parse_catalog(value: &Value) -> Vec<AiModel> {
    let Some(models) = value["models"].as_array() else {
        return Vec::new();
    };
    let mut visible: Vec<_> = models
        .iter()
        .filter(|m| m["visibility"] == "list")
        .collect();
    visible.sort_by_key(|m| m["priority"].as_u64().unwrap_or(u64::MAX));
    let mut seen = std::collections::HashSet::new();
    visible
        .into_iter()
        .filter_map(|model| {
            let id = model["slug"].as_str()?;
            if id.is_empty()
                || id.len() > 100
                || !id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
                || !seen.insert(id.to_string())
            {
                return None;
            }
            let label = model["display_name"]
                .as_str()
                .unwrap_or(id)
                .chars()
                .filter(|c| !c.is_control())
                .take(80)
                .collect();
            let efforts = model["supported_reasoning_levels"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|r| r["effort"].as_str())
                // Ultra delegates automatically; this adapter exposes no native agents.
                .filter(|e| ["low", "medium", "high", "xhigh", "max"].contains(e))
                .map(str::to_string)
                .collect::<Vec<_>>();
            let default = model["default_reasoning_level"]
                .as_str()
                .filter(|s| efforts.iter().any(|e| e == s))
                .unwrap_or_else(|| efforts.first().map(String::as_str).unwrap_or(""));
            Some(AiModel {
                id: id.into(),
                label,
                default_effort: default.into(),
                efforts,
            })
        })
        .collect()
}

pub fn effort_label(effort: &str) -> &str {
    match effort {
        "low" => "Bajo",
        "medium" => "Medio",
        "high" => "Alto",
        "xhigh" => "Extra alto",
        "max" => "Máximo",
        _ => "Predeterminado",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn subscription_catalog_keeps_spark_hides_internal_models_and_filters_efforts() {
        let models = parse_catalog(&serde_json::json!({"models":[
            {"slug":"internal","visibility":"hide","priority":0},
            {"slug":"spark","display_name":"Spark","visibility":"list","priority":2,"supported_in_api":false,"supported_reasoning_levels":[{"effort":"high"}],"default_reasoning_level":"high"},
            {"slug":"astra","visibility":"list","priority":1,"supported_reasoning_levels":[{"effort":"xhigh"},{"effort":"ultra"}],"default_reasoning_level":"ultra"},
            {"slug":"astra","visibility":"list","priority":3}
        ]}));
        assert_eq!(
            models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["astra", "spark"]
        );
        assert_eq!(models[0].efforts, vec!["xhigh"]);
        assert_eq!(models[0].default_effort, "xhigh");
    }
}
