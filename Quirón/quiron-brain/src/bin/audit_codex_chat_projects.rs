use clap::Parser;
use quiron_brain::storage::Storage;
use quiron_brain::types::Event;
use std::collections::BTreeMap;
use unicode_normalization::UnicodeNormalization;

#[derive(Parser, Debug)]
#[command(about = "Audit Codex chat events for project labeling mismatches.")]
struct Args {
    #[arg(long, env = "QUIRON_URL")]
    quiron_url: Option<String>,

    #[arg(long, default_value_t = 200)]
    recent_limit: usize,

    #[arg(long, env = "QUIRON_DATA")]
    data_path: Option<String>,
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

fn first_tag_value<'a>(event: &'a Event, prefix: &str) -> Option<&'a str> {
    event.tags.iter().find_map(|tag| tag.strip_prefix(prefix))
}

fn is_codex_chat_event(event: &Event) -> bool {
    event.tags.iter().any(|tag| tag == "logic:codex_chat_sync")
        || event.tags.iter().any(|tag| tag == "module:codex_chat")
}

fn mismatch_reasons(event: &Event) -> Vec<String> {
    let mut reasons = Vec::new();
    let Some(project_id) = event.project_id.as_deref() else {
        reasons.push("missing project_id".to_string());
        return reasons;
    };

    if let Some(project_root) = first_tag_value(event, "project_root:") {
        let normalized = normalize_project_id(project_root);
        if normalized != project_id {
            reasons.push(format!(
                "project_root mismatch: project_id={} project_root={}",
                project_id, normalized
            ));
        }
    }

    if let Some(workspace) = first_tag_value(event, "workspace:") {
        let normalized = normalize_project_id(workspace);
        if normalized != project_id {
            reasons.push(format!(
                "workspace mismatch: project_id={} workspace={}",
                project_id, normalized
            ));
        }
    }

    reasons
}

async fn load_events(args: &Args) -> anyhow::Result<Vec<Event>> {
    if let Some(quiron_url) = args.quiron_url.as_deref() {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        let url = format!(
            "{}/events?limit={}",
            quiron_url.trim_end_matches('/'),
            args.recent_limit
        );
        let response = client.get(url).send().await?;
        let response = response.error_for_status()?;
        let events = response.json::<Vec<Event>>().await?;
        return Ok(events);
    }

    let data_path = args
        .data_path
        .clone()
        .unwrap_or_else(|| "./data".to_string());
    let storage = Storage::open(&data_path)?;
    let reader = quiron_brain::ledger::LedgerReader::new(storage);
    reader.all_events().map_err(Into::into)
}

fn print_report(events: &[Event]) {
    let codex_events = events.iter().filter(|event| is_codex_chat_event(event)).count();
    let mut mismatches = Vec::new();
    let mut counts_by_project = BTreeMap::<String, usize>::new();

    for event in events.iter().filter(|event| is_codex_chat_event(event)) {
        let reasons = mismatch_reasons(event);
        if reasons.is_empty() {
            continue;
        }

        let project = event
            .project_id
            .clone()
            .unwrap_or_else(|| "missing".to_string());
        *counts_by_project.entry(project).or_default() += 1;

        mismatches.push((event, reasons));
    }

    println!("codex_chat_events_audited={}", codex_events);
    println!("mismatch_count={}", mismatches.len());

    if mismatches.is_empty() {
        println!("status=ok");
        return;
    }

    println!("counts_by_project:");
    for (project, count) in counts_by_project {
        println!("  {}={}", project, count);
    }

    println!("examples:");
    for (event, reasons) in mismatches.into_iter().take(20) {
        println!("  id={}", event.id);
        println!("  ts={}", event.ts);
        println!(
            "  project_id={}",
            event.project_id.as_deref().unwrap_or("missing")
        );
        if let Some(root) = first_tag_value(event, "project_root:") {
            println!("  project_root={}", root);
        }
        if let Some(workspace) = first_tag_value(event, "workspace:") {
            println!("  workspace={}", workspace);
        }
        println!("  reasons={}", reasons.join(" | "));
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let events = load_events(&args).await?;
    print_report(&events);
    Ok(())
}
