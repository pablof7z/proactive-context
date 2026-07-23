//! Durable, privacy-conscious traces for one hook-injection run.
//!
//! Each run is written under the canonical project's local logs directory. The
//! hook prompt is represented by size and digest only; the exact hook stdout is
//! retained because it is the artifact this trace exists to audit.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use uuid::Uuid;

use crate::events::now_rfc3339;
use crate::project_store::ProjectStore;
use crate::query::QueryResult;

pub const TRACE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunTrace {
    pub schema_version: u32,
    pub run_id: String,
    pub started_at: String,
    pub project: ProjectIdentity,
    pub hook: HookMetadata,
    #[serde(default)]
    pub retrieval_candidates: Vec<RetrievalCandidate>,
    #[serde(default)]
    pub decisions: Vec<Decision>,
    #[serde(default)]
    pub events: Vec<TraceEvent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery: Option<Delivery>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ledger: Option<LedgerLink>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_outcome: Option<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectIdentity {
    pub project_id: String,
    pub project_uuid: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HookMetadata {
    pub harness: String,
    pub session_id: String,
    pub prompt_chars: usize,
    pub prompt_sha256: String,
    pub transcript_supplied: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RetrievalCandidate {
    pub path: String,
    pub chunk_index: i64,
    pub score: f64,
    pub content_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Decision {
    pub ts: String,
    pub stage: String,
    pub value: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TraceEvent {
    pub ts: String,
    pub event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lat_ms: Option<u64>,
    pub payload: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Delivery {
    pub dialect: String,
    pub stdout: String,
    pub stdout_chars: usize,
    pub stdout_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LedgerLink {
    pub path: String,
    pub title: String,
    pub body_chars: usize,
    pub body_sha256: String,
}

struct ActiveTrace {
    path: PathBuf,
    trace: RunTrace,
}

static ACTIVE: OnceLock<Mutex<Option<ActiveTrace>>> = OnceLock::new();

fn active() -> &'static Mutex<Option<ActiveTrace>> {
    ACTIVE.get_or_init(|| Mutex::new(None))
}

pub fn sha256_text(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

/// Start a trace and return the ID used by events, sidecars, ledger, and CLI lookup.
pub fn begin(
    store: &ProjectStore,
    harness: &str,
    session_id: &str,
    prompt: &str,
    transcript_supplied: bool,
) -> String {
    let run_id = Uuid::new_v4().simple().to_string();
    if !crate::events::logging_enabled() {
        if let Ok(mut guard) = active().lock() {
            *guard = None;
        }
        return run_id;
    }
    let path = trace_path(store, &run_id);
    let trace = RunTrace {
        schema_version: TRACE_SCHEMA_VERSION,
        run_id: run_id.clone(),
        started_at: now_rfc3339(),
        project: ProjectIdentity {
            project_id: store.manifest.project_id.clone(),
            project_uuid: store.manifest.project_uuid.clone(),
        },
        hook: HookMetadata {
            harness: harness.to_string(),
            session_id: session_id.to_string(),
            prompt_chars: prompt.len(),
            prompt_sha256: sha256_text(prompt),
            transcript_supplied,
        },
        retrieval_candidates: Vec::new(),
        decisions: Vec::new(),
        events: Vec::new(),
        delivery: None,
        ledger: None,
        final_outcome: None,
    };
    if let Ok(mut guard) = active().lock() {
        *guard = Some(ActiveTrace { path, trace });
        persist_locked(guard.as_ref());
    }
    run_id
}

pub fn trace_path(store: &ProjectStore, run_id: &str) -> PathBuf {
    store
        .logs_dir()
        .join("inject-runs")
        .join(format!("{run_id}.json"))
}

pub fn record_retrieval(hits: &[QueryResult]) {
    mutate(|trace| {
        trace.retrieval_candidates = hits
            .iter()
            .map(|hit| RetrievalCandidate {
                path: hit.path.clone(),
                chunk_index: hit.chunk_index,
                score: hit.score,
                content_sha256: if hit.content_hash.is_empty() {
                    sha256_text(&hit.content)
                } else {
                    hit.content_hash.clone()
                },
            })
            .collect();
    });
}

pub fn record_decision(stage: &str, value: Value) {
    mutate(|trace| {
        trace.decisions.push(Decision {
            ts: now_rfc3339(),
            stage: stage.to_string(),
            value: sanitize_value(value),
        });
    });
}

pub fn record_event(event: &str, lat_ms: Option<u64>, payload: &Value) {
    let safe_payload = sanitize_value(payload.clone());
    mutate(|trace| {
        trace.events.push(TraceEvent {
            ts: now_rfc3339(),
            event: event.to_string(),
            lat_ms,
            payload: safe_payload.clone(),
        });
        if event == "inject.done" {
            trace.final_outcome = Some(safe_payload);
        }
    });
}

pub fn record_delivery(dialect: &str, stdout: &str, context: Option<&str>) {
    mutate(|trace| {
        trace.delivery = Some(Delivery {
            dialect: dialect.to_string(),
            stdout: stdout.to_string(),
            stdout_chars: stdout.len(),
            stdout_sha256: sha256_text(stdout),
            context: context.map(str::to_string),
        });
    });
}

pub fn record_ledger(path: &Path, title: Option<&str>, body: &str) {
    mutate(|trace| {
        trace.ledger = Some(LedgerLink {
            path: path.to_string_lossy().to_string(),
            title: title.unwrap_or("").to_string(),
            body_chars: body.trim().len(),
            body_sha256: sha256_text(body.trim()),
        });
    });
}

fn mutate(f: impl FnOnce(&mut RunTrace)) {
    if !crate::events::logging_enabled() {
        return;
    }
    let Ok(mut guard) = active().lock() else {
        return;
    };
    let Some(active) = guard.as_mut() else {
        return;
    };
    f(&mut active.trace);
    persist_locked(Some(active));
}

fn persist_locked(active: Option<&ActiveTrace>) {
    let Some(active) = active else {
        return;
    };
    let Ok(bytes) = serde_json::to_vec_pretty(&active.trace) else {
        return;
    };
    let Some(parent) = active.path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
    }
    let tmp = parent.join(format!(".{}.tmp", Uuid::new_v4()));
    if fs::write(&tmp, bytes).is_err() {
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600));
    }
    if fs::rename(&tmp, &active.path).is_err() {
        let _ = fs::remove_file(tmp);
    }
}

fn sanitize_value(mut value: Value) -> Value {
    const CONTENT_KEYS: &[&str] = &[
        "prompt",
        "prompt_preview",
        "text",
        "snippet",
        "raw",
        "resolved",
        "briefing_text",
        "summary",
        "system",
        "user",
        "messages",
        "content",
        "response_preview",
        "message",
        "error",
        "query",
    ];
    fn visit(value: &mut Value) {
        match value {
            Value::Array(values) => values.iter_mut().for_each(visit),
            Value::Object(map) => {
                for (key, value) in map.iter_mut() {
                    if CONTENT_KEYS.contains(&key.as_str()) {
                        if let Some(text) = value.as_str() {
                            *value = serde_json::json!({
                                "chars": text.len(),
                                "sha256": sha256_text(text)
                            });
                        } else {
                            let encoded = serde_json::to_string(value).unwrap_or_default();
                            *value = serde_json::json!({
                                "chars": encoded.len(),
                                "sha256": sha256_text(&encoded)
                            });
                        }
                    } else {
                        visit(value);
                    }
                }
            }
            _ => {}
        }
    }
    visit(&mut value);
    value
}

pub fn inspect(store: &ProjectStore, run_id: &str) -> Result<()> {
    if run_id.is_empty()
        || run_id
            .chars()
            .any(|c| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
    {
        anyhow::bail!("invalid injection run ID");
    }
    let path = trace_path(store, run_id);
    let bytes = fs::read(&path)
        .with_context(|| format!("injection trace not found: {}", path.display()))?;
    let trace: RunTrace = serde_json::from_slice(&bytes)
        .with_context(|| format!("invalid injection trace: {}", path.display()))?;
    println!("{}", serde_json::to_string_pretty(&trace)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizer_hashes_content_but_keeps_operational_fields() {
        let safe = sanitize_value(serde_json::json!({
            "path": "docs/auth.md",
            "score": 0.91,
            "prompt_preview": "private request",
            "nested": {"response_preview": "private answer"}
        }));
        assert_eq!(safe["path"], "docs/auth.md");
        assert_eq!(safe["score"], 0.91);
        assert_eq!(safe["prompt_preview"]["chars"], 15);
        assert_eq!(
            safe["prompt_preview"]["sha256"],
            sha256_text("private request")
        );
        assert_eq!(safe["nested"]["response_preview"]["chars"], 14);
        assert!(!safe.to_string().contains("private request"));
        assert!(!safe.to_string().contains("private answer"));
    }

    #[test]
    fn trace_path_is_scoped_to_project_logs() {
        let store = ProjectStore {
            subject: crate::project_store::GitRepo {
                worktree_root: PathBuf::from("/tmp/subject"),
                common_dir: PathBuf::from("/tmp/subject/.git"),
            },
            manifest: crate::project_store::StoreManifest {
                schema_version: 1,
                project_uuid: "uuid".into(),
                project_id: "project".into(),
            },
            repo_dir: PathBuf::from("/tmp/projects/project"),
            state_dir: PathBuf::from("/tmp/state/uuid"),
        };
        assert_eq!(
            trace_path(&store, "run"),
            PathBuf::from("/tmp/state/uuid/logs/inject-runs/run.json")
        );
    }

    #[test]
    fn trace_records_exact_delivery_and_correlated_ledger_artifacts() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore {
            subject: crate::project_store::GitRepo {
                worktree_root: temp.path().join("subject"),
                common_dir: temp.path().join("subject/.git"),
            },
            manifest: crate::project_store::StoreManifest {
                schema_version: 1,
                project_uuid: "uuid-exact".into(),
                project_id: "project-exact".into(),
            },
            repo_dir: temp.path().join("projects/project-exact"),
            state_dir: temp.path().join("state/uuid-exact"),
        };
        let run_id = begin(&store, "codex", "session-exact", "private prompt", false);
        crate::events::init_context_with_request(
            &store.manifest.project_id,
            "session-exact",
            run_id.clone(),
        );

        let ledger_path = temp.path().join("ledger/session-exact.jsonl");
        record_ledger(
            &ledger_path,
            Some("Auth route"),
            "Use POST /v2/session. (./docs/auth.md:14)",
        );
        let stdout =
            "<relevant-context from=\"pc skill\">Use POST /v2/session.</relevant-context>";
        record_delivery("raw-text", stdout, Some("Use POST /v2/session."));

        let trace: RunTrace =
            serde_json::from_slice(&fs::read(trace_path(&store, &run_id)).unwrap()).unwrap();
        assert_eq!(trace.run_id, run_id);
        assert_eq!(trace.project.project_id, "project-exact");
        assert_eq!(trace.delivery.as_ref().unwrap().stdout, stdout);
        assert_eq!(
            trace.delivery.as_ref().unwrap().context.as_deref(),
            Some("Use POST /v2/session.")
        );
        assert_eq!(
            trace.ledger.as_ref().unwrap().path,
            ledger_path.to_string_lossy()
        );
        assert_eq!(trace.ledger.as_ref().unwrap().title, "Auth route");
        assert_eq!(
            trace.ledger.as_ref().unwrap().body_sha256,
            sha256_text("Use POST /v2/session. (./docs/auth.md:14)")
        );
    }
}
