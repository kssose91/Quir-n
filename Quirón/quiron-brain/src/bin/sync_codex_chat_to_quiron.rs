use anyhow::{bail, Context, Result};
use clap::Parser;
use regex::Regex;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::Duration;
use unicode_normalization::UnicodeNormalization;
use walkdir::WalkDir;

#[derive(Parser, Debug)]
#[command(about = "Sync visible Codex chat messages into Quiron ledger.")]
struct Args {
    #[arg(long)]
    session_file: Option<PathBuf>,

    #[arg(long)]
    project_root_filter: Option<PathBuf>,

    #[arg(long, default_value_os_t = default_session_root())]
    session_root: PathBuf,

    #[arg(long, default_value_os_t = default_session_index())]
    session_index: PathBuf,

    #[arg(long, default_value_os_t = default_env_file())]
    env_file: PathBuf,

    #[arg(long, default_value_os_t = default_codex_config())]
    codex_config: PathBuf,

    #[arg(long, default_value_os_t = default_state_file())]
    state_file: PathBuf,

    #[arg(long, env = "QUIRON_PROJECT_ID")]
    project_id: Option<String>,

    #[arg(long)]
    include_commentary: bool,

    #[arg(long)]
    from_start: bool,

    #[arg(long)]
    max_events: Option<usize>,

    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SyncState {
    #[serde(default)]
    sessions: HashMap<String, SessionProgress>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SessionProgress {
    #[serde(default)]
    last_line: usize,
}

#[derive(Debug, Clone)]
struct SessionMeta {
    cwd: Option<String>,
    originator: Option<String>,
    source: Option<String>,
    session_id: Option<String>,
}

#[derive(Debug, Clone)]
struct VisibleMessage {
    line_no: usize,
    role: String,
    phase: Option<String>,
    message: String,
    timestamp: Option<String>,
}

#[derive(Debug, Serialize)]
struct CreateEventRequest {
    kind: String,
    description: String,
    project_id: String,
    tags: Vec<String>,
    module_id: String,
    logic_tags: Vec<String>,
    memory_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    inputs: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    outputs: Option<Vec<String>>,
}

fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn default_session_root() -> PathBuf {
    home_dir().join(".codex").join("sessions")
}

fn default_session_index() -> PathBuf {
    home_dir().join(".codex").join("session_index.jsonl")
}

fn default_codex_config() -> PathBuf {
    home_dir().join(".codex").join("config.toml")
}

fn default_env_file() -> PathBuf {
    home_dir()
        .join(".config")
        .join("quiron")
        .join("quiron-brain.env")
}

fn default_state_file() -> PathBuf {
    home_dir()
        .join(".config")
        .join("quiron")
        .join("codex-chat-sync-state.json")
}

fn expand_user_path(raw: &str) -> PathBuf {
    if raw == "~" {
        return home_dir();
    }
    if let Some(stripped) = raw.strip_prefix("~/") {
        return home_dir().join(stripped);
    }
    PathBuf::from(raw)
}

fn load_env_file(path: &Path) -> HashMap<String, String> {
    let Ok(content) = fs::read_to_string(path) else {
        return HashMap::new();
    };
    let mut env_map = HashMap::new();
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        env_map.insert(key.trim().to_string(), value.trim().to_string());
    }
    env_map
}

fn load_project_root_markers(path: &Path) -> Vec<String> {
    let Ok(content) = fs::read_to_string(path) else {
        return vec![".git".to_string()];
    };
    let Ok(value) = content.parse::<toml::Value>() else {
        return vec![".git".to_string()];
    };
    let Some(markers) = value
        .get("project_root_markers")
        .and_then(|value| value.as_array())
    else {
        return vec![".git".to_string()];
    };
    let markers: Vec<String> = markers
        .iter()
        .filter_map(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    if markers.is_empty() {
        vec![".git".to_string()]
    } else {
        markers
    }
}

fn resolve_project_root_from_path(path: &Path, markers: &[String]) -> PathBuf {
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !resolved.exists() {
        return resolved;
    }
    for candidate in resolved.ancestors() {
        if markers.iter().any(|marker| candidate.join(marker).exists()) {
            return candidate.to_path_buf();
        }
    }
    resolved
}

fn resolve_session_file(
    explicit: Option<&Path>,
    session_root: &Path,
    project_root_filter: Option<&Path>,
    markers: &[String],
) -> Result<PathBuf> {
    if let Some(explicit) = explicit {
        if !explicit.is_file() {
            bail!("session file not found: {}", explicit.display());
        }
        return Ok(explicit.to_path_buf());
    }

    let project_root_filter =
        project_root_filter.map(|path| resolve_project_root_from_path(path, markers));
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in WalkDir::new(session_root)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
    {
        if entry.path().extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };

        if let Some(target_root) = project_root_filter.as_ref() {
            let Ok(meta) = load_session_meta(entry.path()) else {
                continue;
            };
            let Some(session_root) = find_project_root(meta.cwd.as_deref(), markers) else {
                continue;
            };
            if session_root != *target_root {
                continue;
            }
        }

        match &newest {
            Some((current, _)) if modified <= *current => {}
            _ => newest = Some((modified, entry.into_path())),
        }
    }

    newest
        .map(|(_, path)| path)
        .with_context(|| match project_root_filter {
            Some(filter_root) => format!(
                "no Codex session files found under {} matching project root {}",
                session_root.display(),
                filter_root.display()
            ),
            None => format!(
                "no Codex session files found under {}",
                session_root.display()
            ),
        })
}

fn extract_thread_id(session_file: &Path) -> Option<String> {
    let regex =
        Regex::new(r"([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})\.jsonl$")
            .ok()?;
    let file_name = session_file.file_name()?.to_string_lossy();
    regex
        .captures(&file_name)
        .and_then(|captures| captures.get(1).map(|m| m.as_str().to_string()))
}

fn load_thread_name(session_index: &Path, thread_id: Option<&str>) -> Option<String> {
    let thread_id = thread_id?;
    let file = fs::File::open(session_index).ok()?;
    let reader = BufReader::new(file);
    for line in reader.lines().map_while(std::result::Result::ok) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(item) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if item.get("id").and_then(Value::as_str) != Some(thread_id) {
            continue;
        }
        let name = item.get("thread_name").and_then(Value::as_str)?.trim();
        if name.is_empty() {
            return None;
        }
        return Some(name.to_string());
    }
    None
}

fn load_session_meta(session_file: &Path) -> Result<SessionMeta> {
    let file = fs::File::open(session_file)
        .with_context(|| format!("failed to open session file {}", session_file.display()))?;
    let reader = BufReader::new(file);
    let mut meta = SessionMeta {
        cwd: None,
        originator: None,
        source: None,
        session_id: None,
    };

    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(item) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let item_type = item.get("type").and_then(Value::as_str);
        let Some(payload) = item.get("payload").and_then(Value::as_object) else {
            continue;
        };

        if item_type == Some("session_meta") {
            if meta.cwd.is_none() {
                meta.cwd = payload
                    .get("cwd")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
            }
            if meta.originator.is_none() {
                meta.originator = payload
                    .get("originator")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
            }
            if meta.source.is_none() {
                meta.source = payload
                    .get("source")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
            }
            if meta.session_id.is_none() {
                meta.session_id = payload
                    .get("id")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
            }
            if meta.cwd.is_some() {
                return Ok(meta);
            }
        }

        if item_type == Some("turn_context") && meta.cwd.is_none() {
            meta.cwd = payload
                .get("cwd")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
        }
    }

    Ok(meta)
}

fn load_state(path: &Path) -> SyncState {
    let Ok(content) = fs::read_to_string(path) else {
        return SyncState::default();
    };
    serde_json::from_str::<SyncState>(&content).unwrap_or_default()
}

fn save_state(path: &Path, state: &SyncState) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create state dir {}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(state)?;
    fs::write(path, format!("{content}\n"))
        .with_context(|| format!("failed to write state file {}", path.display()))
}

fn sanitize_tag_value(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.trim().chars().flat_map(|ch| ch.to_lowercase()) {
        let keep = ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-');
        if keep {
            out.push(ch);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        trimmed
    }
}

fn normalize_project_id(value: &str) -> String {
    let ascii: String = value.nfkd().filter(|ch| ch.is_ascii()).collect();
    sanitize_tag_value(&ascii)
}

fn find_project_root(cwd: Option<&str>, markers: &[String]) -> Option<PathBuf> {
    let cwd = cwd?;
    let current = expand_user_path(cwd);
    Some(resolve_project_root_from_path(&current, markers))
}

fn iter_visible_messages(
    session_file: &Path,
    start_line: usize,
    include_commentary: bool,
) -> Result<Vec<VisibleMessage>> {
    let file = fs::File::open(session_file)
        .with_context(|| format!("failed to open session file {}", session_file.display()))?;
    let reader = BufReader::new(file);
    let mut messages = Vec::new();

    for (index, line) in reader.lines().enumerate() {
        let line_no = index + 1;
        if line_no <= start_line {
            continue;
        }
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(item) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if item.get("type").and_then(Value::as_str) != Some("event_msg") {
            continue;
        }
        let Some(payload) = item.get("payload").and_then(Value::as_object) else {
            continue;
        };
        let payload_type = payload.get("type").and_then(Value::as_str);
        let timestamp = item
            .get("timestamp")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);

        match payload_type {
            Some("user_message") => {
                let Some(message) = payload.get("message").and_then(Value::as_str) else {
                    continue;
                };
                let message = message.trim();
                if message.is_empty() {
                    continue;
                }
                messages.push(VisibleMessage {
                    line_no,
                    role: "user".to_string(),
                    phase: None,
                    message: message.to_string(),
                    timestamp,
                });
            }
            Some("agent_message") => {
                let phase = payload
                    .get("phase")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                if !include_commentary && phase.as_deref() != Some("final_answer") {
                    continue;
                }
                let Some(message) = payload.get("message").and_then(Value::as_str) else {
                    continue;
                };
                let message = message.trim();
                if message.is_empty() {
                    continue;
                }
                messages.push(VisibleMessage {
                    line_no,
                    role: "assistant".to_string(),
                    phase,
                    message: message.to_string(),
                    timestamp,
                });
            }
            _ => {}
        }
    }

    Ok(messages)
}

fn build_event_body(
    project_id: &str,
    project_root: Option<&Path>,
    workspace_cwd: Option<&Path>,
    thread_id: Option<&str>,
    thread_name: Option<&str>,
    session_id: Option<&str>,
    session_originator: Option<&str>,
    session_source: Option<&str>,
    session_file: &Path,
    message: &VisibleMessage,
) -> CreateEventRequest {
    let mut label = if message.role == "user" {
        "Codex user message".to_string()
    } else {
        "Codex assistant message".to_string()
    };
    if let Some(phase) = &message.phase {
        label = format!("{label} [{phase}]");
    }
    if let Some(thread_name) = thread_name {
        label = format!("{label} in \"{thread_name}\"");
    }
    if let Some(source_timestamp) = &message.timestamp {
        label = format!("{label} @ {source_timestamp}");
    }

    let mut tags = vec![
        "channel:codex".to_string(),
        "source:codex_session".to_string(),
        format!("project:{project_id}"),
        format!("codex_role:{}", message.role),
        format!("codex_line:{}", message.line_no),
        format!(
            "codex_file:{}",
            sanitize_tag_value(
                &session_file
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "unknown".to_string()),
            )
        ),
    ];
    if let Some(phase) = &message.phase {
        tags.push(format!("codex_phase:{}", sanitize_tag_value(phase)));
    }
    if let Some(thread_id) = thread_id {
        tags.push(format!("codex_thread:{thread_id}"));
    }
    if let Some(session_id) = session_id {
        tags.push(format!("codex_session:{session_id}"));
    }
    if let Some(session_originator) = session_originator {
        tags.push(format!(
            "codex_originator:{}",
            sanitize_tag_value(session_originator)
        ));
    }
    if let Some(session_source) = session_source {
        tags.push(format!(
            "codex_source:{}",
            sanitize_tag_value(session_source)
        ));
    }
    if let Some(project_root) = project_root.and_then(|path| path.file_name()) {
        tags.push(format!(
            "project_root:{}",
            normalize_project_id(&project_root.to_string_lossy())
        ));
    }
    if let Some(workspace_cwd) = workspace_cwd.and_then(|path| path.file_name()) {
        tags.push(format!(
            "workspace:{}",
            normalize_project_id(&workspace_cwd.to_string_lossy())
        ));
    }

    let (inputs, outputs) = if message.role == "user" {
        (Some(vec![message.message.clone()]), None)
    } else {
        (None, Some(vec![message.message.clone()]))
    };

    CreateEventRequest {
        kind: "OBSERVATION".to_string(),
        description: label,
        project_id: project_id.to_string(),
        tags,
        module_id: "codex_chat".to_string(),
        logic_tags: vec!["codex_chat_sync".to_string()],
        memory_kind: "conversation".to_string(),
        inputs,
        outputs,
    }
}

async fn post_event(
    client: &reqwest::Client,
    quiron_url: &str,
    api_token: &str,
    body: &CreateEventRequest,
) -> Result<Value> {
    let response = client
        .post(format!("{}/event", quiron_url.trim_end_matches('/')))
        .header(CONTENT_TYPE, "application/json")
        .header(AUTHORIZATION, format!("Bearer {api_token}"))
        .json(body)
        .send()
        .await
        .context("failed to send event to Quiron")?;

    let status = response.status();
    let payload = response
        .text()
        .await
        .context("failed to read Quiron response body")?;
    if !status.is_success() {
        bail!("HTTP {} importing event: {}", status.as_u16(), payload);
    }
    let value = serde_json::from_str(&payload).context("failed to parse Quiron response JSON")?;
    Ok(value)
}

fn preview_text(message: &str, max_chars: usize) -> String {
    message.replace('\n', " ").chars().take(max_chars).collect()
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let env_map = load_env_file(&args.env_file);
    let project_root_markers = load_project_root_markers(&args.codex_config);
    let quiron_url = env_map
        .get("QUIRON_URL")
        .cloned()
        .unwrap_or_else(|| "http://127.0.0.1:8766".to_string());
    let api_token = env_map.get("QUIRON_API_TOKEN").cloned();

    if api_token.is_none() && !args.dry_run {
        eprintln!(
            "QUIRON_API_TOKEN not found in {}; use --dry-run or configure auth.",
            args.env_file.display()
        );
        std::process::exit(1);
    }

    let session_file = match resolve_session_file(
        args.session_file.as_deref(),
        &args.session_root,
        args.project_root_filter.as_deref(),
        &project_root_markers,
    ) {
        Ok(path) => path,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };

    let thread_id = extract_thread_id(&session_file);
    let thread_name = load_thread_name(&args.session_index, thread_id.as_deref());
    let session_meta = load_session_meta(&session_file)?;
    let workspace_cwd = session_meta
        .cwd
        .as_deref()
        .map(expand_user_path)
        .map(|path| path.canonicalize().unwrap_or(path));
    let project_root = find_project_root(session_meta.cwd.as_deref(), &project_root_markers);

    let project_id = args.project_id.unwrap_or_else(|| {
        project_root
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| normalize_project_id(&name.to_string_lossy()))
            .or_else(|| {
                workspace_cwd
                    .as_ref()
                    .and_then(|path| path.file_name())
                    .map(|name| normalize_project_id(&name.to_string_lossy()))
            })
            .unwrap_or_else(|| "unknown-project".to_string())
    });

    let mut state = load_state(&args.state_file);
    let session_key = session_file
        .canonicalize()
        .unwrap_or_else(|_| session_file.clone())
        .display()
        .to_string();
    let last_line = if args.from_start {
        0
    } else {
        state
            .sessions
            .get(&session_key)
            .map(|entry| entry.last_line)
            .unwrap_or(0)
    };

    let messages = iter_visible_messages(&session_file, last_line, args.include_commentary)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(5))
        .build()
        .context("failed to build HTTP client")?;

    let mut imported = 0usize;
    let mut newest_line = last_line;

    for message in messages {
        newest_line = message.line_no;
        let body = build_event_body(
            &project_id,
            project_root.as_deref(),
            workspace_cwd.as_deref(),
            thread_id.as_deref(),
            thread_name.as_deref(),
            session_meta.session_id.as_deref(),
            session_meta.originator.as_deref(),
            session_meta.source.as_deref(),
            &session_file,
            &message,
        );

        if args.dry_run {
            let payload = json!({
                "project_id": project_id.as_str(),
                "project_root": project_root.as_ref().map(|path| path.display().to_string()),
                "workspace_cwd": workspace_cwd.as_ref().map(|path| path.display().to_string()),
                "line": message.line_no,
                "role": message.role,
                "phase": message.phase,
                "preview": preview_text(&message.message, 120),
                "description": body.description,
            });
            println!("{}", serde_json::to_string(&payload)?);
        } else {
            let response = match post_event(
                &client,
                &quiron_url,
                api_token.as_deref().unwrap_or_default(),
                &body,
            )
            .await
            {
                Ok(response) => response,
                Err(err) => {
                    eprintln!("importing line {} failed: {}", message.line_no, err);
                    std::process::exit(1);
                }
            };

            let payload = json!({
                "line": message.line_no,
                "role": message.role,
                "phase": message.phase,
                "event_id": response.get("id"),
                "ok": response.get("ok"),
            });
            println!("{}", serde_json::to_string(&payload)?);
        }

        imported += 1;
        if let Some(max_events) = args.max_events {
            if imported >= max_events {
                break;
            }
        }
    }

    if !args.dry_run {
        state.sessions.insert(
            session_key,
            SessionProgress {
                last_line: newest_line,
            },
        );
        save_state(&args.state_file, &state)?;
    }

    let payload = json!({
        "session_file": session_file.display().to_string(),
        "thread_id": thread_id,
        "thread_name": thread_name,
        "project_id": project_id.as_str(),
        "project_root": project_root.as_ref().map(|path| path.display().to_string()),
        "workspace_cwd": workspace_cwd.as_ref().map(|path| path.display().to_string()),
        "imported": imported,
        "last_line": newest_line,
        "dry_run": args.dry_run,
    });
    println!("{}", serde_json::to_string(&payload)?);

    Ok(())
}
