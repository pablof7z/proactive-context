use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::{Args, ValueEnum};
use serde::Serialize;

use super::extract::{self, DumpOptions, DumpRecord, DumpSource};

#[derive(Debug, Clone, Args)]
pub struct DumpArgs {
    /// Project directory to match by recorded transcript cwd. Defaults to the current directory.
    #[arg(long)]
    pub cwd: Option<PathBuf>,

    /// Dump every local project instead of filtering to --cwd/current directory.
    #[arg(long)]
    pub all_projects: bool,

    /// Also include path-prefix matches below --cwd for non-git directories.
    #[arg(long)]
    pub include_subdirs: bool,

    /// Provider to read.
    #[arg(long, value_enum, default_value = "both")]
    pub provider: ProviderFilter,

    /// Output format.
    #[arg(long, value_enum, default_value = "json")]
    pub format: DumpFormat,

    /// Write to a file instead of stdout.
    #[arg(long, short)]
    pub output: Option<PathBuf>,

    /// Keep original human message text instead of cleaning pasted code, diffs, and wrappers.
    #[arg(long)]
    pub raw_text: bool,

    /// Skip ~/.codex/archived_sessions.
    #[arg(long)]
    pub no_archived_codex: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ProviderFilter {
    Both,
    Claude,
    Codex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DumpFormat {
    Json,
    Markdown,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct TranscriptDump {
    schema_version: u8,
    text_mode: String,
    projects: Vec<DumpProject>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct DumpProject {
    name: String,
    root: String,
    sessions: Vec<DumpSession>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct DumpSession {
    provider: String,
    session_id: String,
    cwd: String,
    transcript_path: String,
    started_at: String,
    ended_at: String,
    messages: Vec<DumpMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct DumpMessage {
    role: String,
    text: String,
    timestamp: String,
    line: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_reason: Option<String>,
}

pub fn run(args: DumpArgs) -> Result<()> {
    if args.all_projects && args.cwd.is_some() {
        bail!("--cwd cannot be combined with --all-projects");
    }

    let cwd = if args.all_projects {
        None
    } else {
        Some(match args.cwd {
            Some(cwd) => cwd,
            None => std::env::current_dir().context("read current directory")?,
        })
    };

    let options = DumpOptions {
        source: match args.provider {
            ProviderFilter::Both => DumpSource::Both,
            ProviderFilter::Claude => DumpSource::Claude,
            ProviderFilter::Codex => DumpSource::Codex,
        },
        cwd,
        include_subdirs: args.include_subdirs,
        include_archived_codex: !args.no_archived_codex,
        clean: !args.raw_text,
    };

    let records = extract::dump_records(&options)?;
    let dump = build_dump(records, options.clean);
    let rendered = render(args.format, &dump)?;
    let (session_count, message_count) = dump_counts(&dump);

    match args.output {
        Some(path) if path.as_os_str() == "-" => {
            print!("{rendered}");
        }
        Some(path) => {
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("create {}", parent.display()))?;
            }
            std::fs::write(&path, rendered).with_context(|| format!("write {}", path.display()))?;
            eprintln!(
                "pc recall dump: wrote {session_count} session(s), {message_count} message(s) to {}",
                path.display()
            );
        }
        None => {
            print!("{rendered}");
        }
    }

    Ok(())
}

fn project_identity(cwd: &str, fallback: &str) -> (String, String) {
    if cwd.is_empty() {
        return (fallback.to_string(), String::new());
    }

    let root = crate::config::resolve_project_root(Path::new(cwd));
    let root = std::fs::canonicalize(&root).unwrap_or(root);
    let name = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(fallback)
        .to_string();
    (name, root.to_string_lossy().to_string())
}

fn update_bounds(session: &mut DumpSession, timestamp: &str) {
    if timestamp.is_empty() {
        return;
    }
    if session.started_at.is_empty() || timestamp < session.started_at.as_str() {
        session.started_at = timestamp.to_string();
    }
    if session.ended_at.is_empty() || timestamp > session.ended_at.as_str() {
        session.ended_at = timestamp.to_string();
    }
}

fn build_dump(records: Vec<DumpRecord>, clean: bool) -> TranscriptDump {
    type SessionKey = (String, String, String, String, String);

    let mut project_cache: HashMap<String, (String, String)> = HashMap::new();
    let mut sessions: BTreeMap<SessionKey, DumpSession> = BTreeMap::new();

    for record in records {
        let (project_name, project_root) = project_cache
            .entry(record.cwd.clone())
            .or_insert_with(|| project_identity(&record.cwd, &record.provider))
            .clone();
        let key = (
            project_root,
            project_name,
            record.provider.clone(),
            record.session_id.clone(),
            record.transcript_path.clone(),
        );
        let session = sessions.entry(key).or_insert_with(|| DumpSession {
            provider: record.provider.clone(),
            session_id: record.session_id.clone(),
            cwd: record.cwd.clone(),
            transcript_path: record.transcript_path.clone(),
            started_at: String::new(),
            ended_at: String::new(),
            messages: vec![],
        });
        update_bounds(session, &record.timestamp);
        session.messages.push(DumpMessage {
            role: record.role,
            text: record.text,
            timestamp: record.timestamp,
            line: record.line,
            phase: record.phase,
            stop_reason: record.stop_reason,
        });
    }

    let mut projects: BTreeMap<(String, String), Vec<DumpSession>> = BTreeMap::new();
    for ((root, name, _, _, _), mut session) in sessions {
        session.messages.sort_by(|a, b| {
            a.line
                .cmp(&b.line)
                .then_with(|| a.timestamp.cmp(&b.timestamp))
                .then_with(|| a.role.cmp(&b.role))
        });
        projects.entry((root, name)).or_default().push(session);
    }

    let mut projects: Vec<DumpProject> = projects
        .into_iter()
        .map(|((root, name), mut sessions)| {
            sessions.sort_by(|a, b| {
                (a.started_at.is_empty(), &a.started_at)
                    .cmp(&(b.started_at.is_empty(), &b.started_at))
                    .then_with(|| a.provider.cmp(&b.provider))
                    .then_with(|| a.session_id.cmp(&b.session_id))
                    .then_with(|| a.transcript_path.cmp(&b.transcript_path))
            });
            DumpProject {
                name,
                root,
                sessions,
            }
        })
        .collect();
    projects.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.root.cmp(&b.root)));

    TranscriptDump {
        schema_version: 1,
        text_mode: if clean { "clean" } else { "raw" }.to_string(),
        projects,
    }
}

fn dump_counts(dump: &TranscriptDump) -> (usize, usize) {
    let sessions = dump
        .projects
        .iter()
        .map(|project| project.sessions.len())
        .sum();
    let messages = dump
        .projects
        .iter()
        .flat_map(|project| &project.sessions)
        .map(|session| session.messages.len())
        .sum();
    (sessions, messages)
}

fn render(format: DumpFormat, dump: &TranscriptDump) -> Result<String> {
    match format {
        DumpFormat::Json => {
            let mut output = serde_json::to_string_pretty(dump)?;
            output.push('\n');
            Ok(output)
        }
        DumpFormat::Markdown => Ok(render_markdown(dump)),
        DumpFormat::Text => Ok(render_text(dump)),
    }
}

fn message_label(message: &DumpMessage) -> String {
    let mut label = message.role.clone();
    if let Some(phase) = &message.phase {
        label.push_str(&format!(" phase={phase}"));
    }
    if let Some(stop_reason) = &message.stop_reason {
        label.push_str(&format!(" stop_reason={stop_reason}"));
    }
    label
}

fn render_text(dump: &TranscriptDump) -> String {
    let mut output = String::new();
    for project in &dump.projects {
        for session in &project.sessions {
            output.push_str(&format!(
                "=== {} | {} | {} ===\nroot={}\ncwd={}\ntranscript={}\n\n",
                project.name,
                session.provider,
                session.session_id,
                project.root,
                session.cwd,
                session.transcript_path,
            ));
            for message in &session.messages {
                let timestamp = if message.timestamp.is_empty() {
                    "unknown-time"
                } else {
                    &message.timestamp
                };
                output.push_str(&format!(
                    "[{timestamp}] {} line={}\n{}\n\n",
                    message_label(message),
                    message.line,
                    message.text.trim(),
                ));
            }
        }
    }
    output
}

fn render_markdown(dump: &TranscriptDump) -> String {
    let mut output = String::new();
    let (session_count, message_count) = dump_counts(dump);
    output.push_str("# Transcript Dump\n\n");
    output.push_str(&format!(
        "{session_count} session(s), {message_count} message(s)\n\n"
    ));

    for project in &dump.projects {
        output.push_str(&format!("## {}\n\n", project.name));
        output.push_str(&format!("- Root: `{}`\n\n", project.root));
        for session in &project.sessions {
            output.push_str(&format!(
                "### {} - {}\n\n- Transcript: `{}`\n- CWD: `{}`\n- Start: `{}`\n- End: `{}`\n\n",
                session.provider,
                session.session_id,
                session.transcript_path,
                session.cwd,
                session.started_at,
                session.ended_at,
            ));
            for message in &session.messages {
                let timestamp = if message.timestamp.is_empty() {
                    "unknown-time"
                } else {
                    &message.timestamp
                };
                output.push_str(&format!(
                    "#### {} - {} - line {}\n\n",
                    message_label(message),
                    timestamp,
                    message.line,
                ));
                let fence = markdown_fence(&message.text);
                output.push_str(&fence);
                output.push('\n');
                output.push_str(message.text.trim());
                output.push('\n');
                output.push_str(&fence);
                output.push_str("\n\n");
            }
        }
    }

    output
}

fn markdown_fence(text: &str) -> String {
    let mut max_run = 0usize;
    let mut current = 0usize;
    for character in text.chars() {
        if character == '`' {
            current += 1;
            max_run = max_run.max(current);
        } else {
            current = 0;
        }
    }
    "`".repeat(max_run.max(3) + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn records() -> Vec<DumpRecord> {
        vec![
            DumpRecord {
                provider: "codex".into(),
                cwd: "/tmp/proj".into(),
                session_id: "s1".into(),
                timestamp: "2026-01-01T00:00:00Z".into(),
                line: 2,
                transcript_path: "/tmp/rollout.jsonl".into(),
                role: "user".into(),
                text: "hello\nworld".into(),
                phase: None,
                stop_reason: None,
            },
            DumpRecord {
                provider: "codex".into(),
                cwd: "/tmp/proj".into(),
                session_id: "s1".into(),
                timestamp: "2026-01-01T00:00:01Z".into(),
                line: 3,
                transcript_path: "/tmp/rollout.jsonl".into(),
                role: "assistant".into(),
                text: "response".into(),
                phase: Some("final_answer".into()),
                stop_reason: None,
            },
        ]
    }

    #[test]
    fn json_groups_messages_by_project_and_session() {
        let dump = build_dump(records(), true);
        let rendered = render(DumpFormat::Json, &dump).unwrap();
        let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();

        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["text_mode"], "clean");
        assert_eq!(value["projects"].as_array().unwrap().len(), 1);
        let session = &value["projects"][0]["sessions"][0];
        assert_eq!(session["messages"].as_array().unwrap().len(), 2);
        assert_eq!(session["messages"][0]["role"], "user");
        assert_eq!(session["messages"][1]["role"], "assistant");
        assert_eq!(session["messages"][1]["phase"], "final_answer");
        assert!(session["messages"][0].get("phase").is_none());
    }

    #[test]
    fn markdown_fence_is_longer_than_message_backticks() {
        assert_eq!(markdown_fence("```code```"), "````");
        assert_eq!(markdown_fence("````code````"), "`````");
    }

    #[test]
    fn human_readable_formats_include_both_roles() {
        let dump = build_dump(records(), true);
        let markdown = render(DumpFormat::Markdown, &dump).unwrap();
        let text = render(DumpFormat::Text, &dump).unwrap();

        assert!(markdown.contains("#### user"));
        assert!(markdown.contains("#### assistant phase=final_answer"));
        assert!(text.contains("user line=2"));
        assert!(text.contains("assistant phase=final_answer line=3"));
    }

    #[test]
    fn projects_with_the_same_basename_remain_separate() {
        let mut records = records();
        let mut other = records[0].clone();
        other.cwd = "/var/tmp/proj".into();
        other.session_id = "s2".into();
        other.transcript_path = "/var/tmp/rollout.jsonl".into();
        records.push(other);

        let dump = build_dump(records, true);

        assert_eq!(dump.projects.len(), 2);
        assert_eq!(dump.projects[0].name, "proj");
        assert_eq!(dump.projects[1].name, "proj");
        assert_ne!(dump.projects[0].root, dump.projects[1].root);
    }
}
