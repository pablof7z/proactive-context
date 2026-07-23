//! Deterministic suppression of context the host already exposed to the model.
//!
//! This module deliberately does not use embeddings or fuzzy/semantic thresholds. It only
//! suppresses exact source identities, exact content fingerprints, and text proven present by
//! case-insensitive whitespace-normalized containment. The hook prompt is always available.
//! Conversation, harness instructions, and prior PC payloads are included only when the harness
//! exposes them through a readable transcript surface.

use crate::query::QueryResult;
use regex::Regex;
use serde_json::Value;
use std::collections::{BTreeSet, HashSet};
use std::sync::OnceLock;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SuppressionStats {
    pub dropped_hits: usize,
    pub fingerprint_matches: usize,
    pub source_identity_matches: usize,
    pub containment_matches: usize,
    pub partially_masked_hits: usize,
    pub removed_lines: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextSuppression {
    pub text: String,
    pub removed_lines: usize,
    pub fully_suppressed: bool,
}

#[derive(Debug, Clone)]
pub struct ContextCoverage {
    transcript_status: &'static str,
    fragment_count: usize,
    recent_message_window: usize,
    roles: BTreeSet<String>,
    instruction_fragments: usize,
    pc_context_fragments: usize,
    limitations: BTreeSet<&'static str>,
}

impl ContextCoverage {
    pub fn telemetry(&self) -> Value {
        serde_json::json!({
            "transcript_status": self.transcript_status,
            "context_fragments": self.fragment_count,
            "recent_message_window": self.recent_message_window,
            "roles": self.roles.iter().collect::<Vec<_>>(),
            "instruction_fragments": self.instruction_fragments,
            "pc_context_fragments": self.pc_context_fragments,
            "limitations": self.limitations.iter().collect::<Vec<_>>(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct ContextOverlap {
    normalized_fragments: Vec<String>,
    raw_fingerprints: HashSet<String>,
    source_chunks: HashSet<(String, i64)>,
    coverage: ContextCoverage,
}

#[derive(Debug)]
struct ContextMessage {
    role: String,
    text: String,
    is_instruction: bool,
    is_persistent_instruction: bool,
}

impl ContextOverlap {
    /// Build the exact-match surface available to this hook execution.
    ///
    /// `model_context_path` must point at the least-lossy transcript the harness exposes. For
    /// Codex this is the original rollout rather than PC's user/assistant-only normalized copy.
    pub fn from_hook(
        prompt: &str,
        model_context_path: Option<&str>,
        harness: &str,
        recent_message_window: usize,
    ) -> Self {
        let mut normalized_fragments = Vec::new();
        let mut raw_fingerprints = HashSet::new();
        let mut source_chunks = HashSet::new();
        let mut normalized_seen = HashSet::new();
        add_fragment(
            prompt,
            &mut normalized_fragments,
            &mut raw_fingerprints,
            &mut source_chunks,
            &mut normalized_seen,
        );

        let mut roles = BTreeSet::new();
        let mut instruction_fragments = 0usize;
        let mut pc_context_fragments = 0usize;
        let mut limitations = BTreeSet::new();

        let transcript_status = match model_context_path {
            None => {
                limitations.insert("conversation_context_not_exposed");
                limitations.insert("harness_instructions_not_exposed");
                limitations.insert("loaded_pc_context_not_exposed");
                "not_provided"
            }
            Some(path) => match std::fs::read_to_string(path) {
                Err(_) => {
                    limitations.insert("conversation_context_unreadable");
                    limitations.insert("harness_instructions_unreadable");
                    limitations.insert("loaded_pc_context_unreadable");
                    "unreadable"
                }
                Ok(raw) => {
                    let messages: Vec<ContextMessage> = raw
                        .lines()
                        .filter_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
                        .filter_map(|value| context_message(&value))
                        .filter(|message| !message.text.trim().is_empty())
                        .collect();
                    let message_count = messages.len();
                    let ordinary_count = messages
                        .iter()
                        .filter(|message| !message.is_persistent_instruction)
                        .count();
                    let ordinary_to_skip = ordinary_count.saturating_sub(recent_message_window);
                    let mut ordinary_seen = 0usize;
                    for message in messages {
                        let include = if message.is_persistent_instruction {
                            true
                        } else {
                            let include = ordinary_seen >= ordinary_to_skip;
                            ordinary_seen += 1;
                            include
                        };
                        if !include {
                            continue;
                        }
                        roles.insert(message.role);
                        if message.is_instruction {
                            instruction_fragments += 1;
                        }
                        if looks_like_pc_context(&message.text) {
                            pc_context_fragments += 1;
                        }
                        add_fragment(
                            &message.text,
                            &mut normalized_fragments,
                            &mut raw_fingerprints,
                            &mut source_chunks,
                            &mut normalized_seen,
                        );
                    }
                    if message_count == 0 {
                        limitations.insert("transcript_has_no_message_context");
                        limitations.insert("harness_instructions_not_exposed");
                        limitations.insert("loaded_pc_context_not_exposed");
                        "no_messages"
                    } else {
                        if instruction_fragments == 0 {
                            limitations.insert("harness_instruction_role_not_exposed");
                        }
                        // The opencode adapter intentionally removes `_pcInjected` parts before it
                        // writes the transcript. Make this known boundary observable instead of
                        // interpreting absence as proof that no prior PC context exists.
                        if harness == "opencode" {
                            limitations.insert("loaded_pc_context_filtered_by_opencode_adapter");
                        }
                        "loaded"
                    }
                }
            },
        };

        let fragment_count = normalized_fragments.len();
        Self {
            normalized_fragments,
            raw_fingerprints,
            source_chunks,
            coverage: ContextCoverage {
                transcript_status,
                fragment_count,
                recent_message_window,
                roles,
                instruction_fragments,
                pc_context_fragments,
                limitations,
            },
        }
    }

    pub fn coverage(&self) -> &ContextCoverage {
        &self.coverage
    }

    /// Remove retrieved chunks already present in model context. Partially overlapping chunks keep
    /// their new lines and lose only paragraphs/lines whose complete normalized text is present.
    pub fn suppress_hits(&self, hits: Vec<QueryResult>) -> (Vec<QueryResult>, SuppressionStats) {
        let mut kept = Vec::with_capacity(hits.len());
        let mut stats = SuppressionStats::default();

        for mut hit in hits {
            let source_id = (normalize_source_path(&hit.path), hit.chunk_index);
            if self.source_chunks.contains(&source_id) {
                stats.dropped_hits += 1;
                stats.source_identity_matches += 1;
                continue;
            }
            if self.raw_fingerprints.contains(&hit.content_hash)
                || self
                    .raw_fingerprints
                    .contains(&crate::db::content_hash(hit.content.trim()))
            {
                stats.dropped_hits += 1;
                stats.fingerprint_matches += 1;
                continue;
            }
            if self.contains_text(&hit.content) {
                stats.dropped_hits += 1;
                stats.containment_matches += 1;
                continue;
            }

            let masked = self.mask_source_preserving_lines(&hit.content);
            stats.removed_lines += masked.removed_lines;
            if masked.fully_suppressed {
                stats.dropped_hits += 1;
                stats.containment_matches += 1;
                continue;
            }
            if masked.removed_lines > 0 {
                stats.partially_masked_hits += 1;
                hit.content = masked.text;
            }
            kept.push(hit);
        }

        (kept, stats)
    }

    /// Mask exact overlapping source paragraphs/lines while preserving the original line count.
    /// Keeping line positions stable preserves the compiler's source citation contract.
    pub fn mask_source_preserving_lines(&self, text: &str) -> TextSuppression {
        let lines: Vec<&str> = text.split('\n').collect();
        let mut masked = vec![false; lines.len()];

        let mut start = 0usize;
        while start < lines.len() {
            if lines[start].trim().is_empty() {
                start += 1;
                continue;
            }
            let mut end = start + 1;
            while end < lines.len() && !lines[end].trim().is_empty() {
                end += 1;
            }
            let paragraph = lines[start..end].join("\n");
            if self.contains_text(&paragraph) {
                masked[start..end].fill(true);
            }
            start = end;
        }

        for (index, line) in lines.iter().enumerate() {
            if !masked[index] && !line.trim().is_empty() && self.contains_text(line) {
                masked[index] = true;
            }
        }

        let removed_lines = masked.iter().filter(|masked| **masked).count();
        let remaining_has_text = lines
            .iter()
            .zip(masked.iter())
            .any(|(line, masked)| !*masked && !line.trim().is_empty());
        let text = lines
            .iter()
            .zip(masked.iter())
            .map(|(line, masked)| if *masked { "" } else { *line })
            .collect::<Vec<_>>()
            .join("\n");

        TextSuppression {
            text,
            removed_lines,
            fully_suppressed: !remaining_has_text,
        }
    }

    /// Remove compiled output lines already present in context. Inline source citations are ignored
    /// only for the comparison, allowing `Known fact. (path:12)` to match an existing `Known fact.`
    /// without changing a surviving output line.
    pub fn suppress_compiled(&self, compiled: &str) -> TextSuppression {
        let mut kept = Vec::new();
        let mut removed_lines = 0usize;
        let mut title: Option<&str> = None;

        for (index, line) in compiled.lines().enumerate() {
            if index == 0 && line.trim_start().to_ascii_uppercase().starts_with("TITLE:") {
                title = Some(line);
                continue;
            }
            let comparable = strip_citations_for_comparison(line);
            if !comparable.is_empty() && self.contains_text(&comparable) {
                removed_lines += 1;
            } else {
                kept.push(line);
            }
        }

        let has_body = kept.iter().any(|line| !line.trim().is_empty());
        if !has_body {
            return TextSuppression {
                text: "TITLE: none".to_string(),
                removed_lines,
                fully_suppressed: true,
            };
        }

        let mut output = String::new();
        if let Some(title) = title {
            output.push_str(title);
            output.push('\n');
        }
        output.push_str(kept.join("\n").trim_end());
        TextSuppression {
            text: output,
            removed_lines,
            fully_suppressed: false,
        }
    }

    pub fn contains_text(&self, text: &str) -> bool {
        let candidate = normalize_text(text);
        !candidate.is_empty()
            && self
                .normalized_fragments
                .iter()
                .any(|fragment| fragment.contains(&candidate))
    }
}

/// Extract message content without applying capture's user/assistant or XML-removal policy.
///
/// This is intentionally limited to recognized message envelopes and text content blocks, so tool
/// call arguments/results and unrelated JSON metadata cannot suppress retrieved project context.
fn context_message(value: &Value) -> Option<ContextMessage> {
    if value.get("type").and_then(Value::as_str) == Some("response_item") {
        let payload = value.get("payload")?;
        if payload.get("type").and_then(Value::as_str) != Some("message") {
            return None;
        }
        let role = payload.get("role").and_then(Value::as_str)?.to_string();
        let text = context_text(payload.get("content")?);
        let is_instruction = matches!(role.as_str(), "system" | "developer");
        let is_persistent_instruction = is_instruction && !looks_like_pc_context(&text);
        return Some(ContextMessage {
            role,
            text,
            is_instruction,
            is_persistent_instruction,
        });
    }

    if let Some(message) = value.get("message") {
        let top_role = value.get("type").and_then(Value::as_str).unwrap_or("");
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or(top_role);
        if role.is_empty() {
            return None;
        }
        let text = context_text(message.get("content")?);
        let prompt_source = value.get("promptSource").and_then(Value::as_str);
        let is_instruction = matches!(role, "system" | "developer")
            || prompt_source == Some("system")
            || value
                .get("isMeta")
                .and_then(Value::as_bool)
                .unwrap_or(false);
        let is_persistent_instruction =
            matches!(role, "system" | "developer") && !looks_like_pc_context(&text);
        return Some(ContextMessage {
            role: role.to_string(),
            text,
            is_instruction,
            is_persistent_instruction,
        });
    }

    let role = value.get("role").and_then(Value::as_str)?;
    let text = context_text(value.get("content")?);
    let is_instruction = matches!(role, "system" | "developer");
    let is_persistent_instruction = is_instruction && !looks_like_pc_context(&text);
    Some(ContextMessage {
        role: role.to_string(),
        text,
        is_instruction,
        is_persistent_instruction,
    })
}

/// Preserve XML-wrapped context and instruction messages for the overlap-only transcript surface.
pub(crate) fn context_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| {
                let kind = block.get("type").and_then(Value::as_str).unwrap_or("");
                if !matches!(kind, "text" | "input_text" | "output_text") {
                    return None;
                }
                block
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn add_fragment(
    text: &str,
    normalized_fragments: &mut Vec<String>,
    raw_fingerprints: &mut HashSet<String>,
    source_chunks: &mut HashSet<(String, i64)>,
    normalized_seen: &mut HashSet<String>,
) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    raw_fingerprints.insert(crate::db::content_hash(text));
    raw_fingerprints.insert(crate::db::content_hash(trimmed));

    let normalized = normalize_text(trimmed);
    if !normalized.is_empty() && normalized_seen.insert(normalized.clone()) {
        normalized_fragments.push(normalized);
    }
    collect_source_chunks(trimmed, source_chunks);
}

fn collect_source_chunks(text: &str, out: &mut HashSet<(String, i64)>) {
    for line in text.lines() {
        let line = line.trim();
        let Some(header) = line
            .strip_prefix("--- ")
            .and_then(|line| line.strip_suffix(" ---"))
        else {
            continue;
        };
        let Some((path, chunk_tail)) = header.rsplit_once(" (chunk ") else {
            continue;
        };
        let Some(index) = chunk_tail
            .split(',')
            .next()
            .and_then(|value| value.parse().ok())
        else {
            continue;
        };
        out.insert((normalize_source_path(path), index));
    }
}

fn normalize_source_path(path: &str) -> String {
    let path = path.trim().replace('\\', "/");
    path.strip_prefix("./").unwrap_or(&path).to_string()
}

fn normalize_text(text: &str) -> String {
    text.split_whitespace()
        .map(|part| part.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

fn citation_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"\((?:\./)?[^()\n]+:\d+(?:-\d+)?\)").expect("valid citation regex")
    })
}

fn strip_citations_for_comparison(line: &str) -> String {
    citation_regex()
        .replace_all(line, "")
        .trim()
        .trim_start_matches(['-', '*', '•', ' '])
        .trim()
        .to_string()
}

fn looks_like_pc_context(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    (lower.contains("<system-reminder") && lower.contains("relevant project context"))
        || (lower.contains("<relevant-context") && lower.contains("from=\"pc"))
        || lower.lines().any(|line| {
            line.trim_start().starts_with("--- ")
                && line.contains(" (chunk ")
                && line.trim_end().ends_with(" ---")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(path: &str, chunk: i64, content: &str) -> QueryResult {
        QueryResult {
            path: path.to_string(),
            chunk_index: chunk,
            content: content.to_string(),
            content_hash: crate::db::content_hash(content),
            score: 0.9,
        }
    }

    #[test]
    fn drops_exact_normalized_containment_without_semantic_matching() {
        let overlap = ContextOverlap::from_hook(
            "The manifest lives at .pc/manifest.json.\nPlease verify it.",
            None,
            "claude",
            32,
        );
        let (kept, stats) = overlap.suppress_hits(vec![
            hit("docs/a.md", 0, "the manifest   lives at .pc/manifest.json."),
            hit("docs/b.md", 0, "A different fact remains."),
        ]);

        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].path, "docs/b.md");
        assert_eq!(stats.dropped_hits, 1);
        assert_eq!(stats.containment_matches, 1);
    }

    #[test]
    fn exact_source_identity_suppresses_raw_fallback_chunk() {
        let transcript = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            transcript.path(),
            serde_json::json!({
                "role": "user",
                "content": "<relevant-context from=\"pc skill\">\n--- docs/spec.md (chunk 4, score 0.91) ---\nEarlier rendering\n</relevant-context>"
            })
            .to_string(),
        )
        .unwrap();
        let overlap =
            ContextOverlap::from_hook("continue", transcript.path().to_str(), "claude", 32);
        let (kept, stats) =
            overlap.suppress_hits(vec![hit("docs/spec.md", 4, "Content changed in storage.")]);

        assert!(kept.is_empty());
        assert_eq!(stats.source_identity_matches, 1);
        assert_eq!(
            overlap.coverage().telemetry()["pc_context_fragments"],
            serde_json::json!(1)
        );
    }

    #[test]
    fn partial_hit_masks_only_proven_repeated_lines() {
        let overlap =
            ContextOverlap::from_hook("Known exact line.\nDo the next step.", None, "claude", 32);
        let (kept, stats) = overlap.suppress_hits(vec![hit(
            "docs/spec.md",
            1,
            "Known exact line.\nNew project-specific line.",
        )]);

        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].content, "\nNew project-specific line.");
        assert_eq!(stats.partially_masked_hits, 1);
        assert_eq!(stats.removed_lines, 1);
    }

    #[test]
    fn reads_developer_xml_and_user_context_but_ignores_tool_output() {
        let mut transcript = tempfile::NamedTempFile::new().unwrap();
        use std::io::Write;
        writeln!(
            transcript,
            "{}",
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"Harness rule alpha."}]}})
        )
        .unwrap();
        writeln!(
            transcript,
            "{}",
            serde_json::json!({"type":"user","promptSource":"system","message":{"role":"user","content":"<system-reminder>Relevant project context (demo): Prior PC fact.</system-reminder>"}})
        )
        .unwrap();
        writeln!(
            transcript,
            "{}",
            serde_json::json!({"type":"response_item","payload":{"type":"function_call_output","output":"Tool-only secret fact."}})
        )
        .unwrap();

        let overlap =
            ContextOverlap::from_hook("Current prompt.", transcript.path().to_str(), "codex", 32);
        assert!(overlap.contains_text("Harness rule alpha."));
        assert!(overlap.contains_text("Prior PC fact."));
        assert!(!overlap.contains_text("Tool-only secret fact."));
        let telemetry = overlap.coverage().telemetry();
        assert_eq!(telemetry["instruction_fragments"], serde_json::json!(2));
        assert_eq!(telemetry["pc_context_fragments"], serde_json::json!(1));
        assert_eq!(telemetry["transcript_status"], serde_json::json!("loaded"));
    }

    #[test]
    fn missing_transcript_reports_unavailable_surfaces() {
        let overlap = ContextOverlap::from_hook("Prompt remains available.", None, "hermes", 32);
        let telemetry = overlap.coverage().telemetry();
        assert_eq!(
            telemetry["transcript_status"],
            serde_json::json!("not_provided")
        );
        assert!(telemetry["limitations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == "harness_instructions_not_exposed"));
        assert!(overlap.contains_text("Prompt remains available."));
    }

    #[test]
    fn compiled_line_matches_existing_fact_without_its_citation() {
        let overlap = ContextOverlap::from_hook(
            "The manifest lives at .pc/manifest.json.",
            None,
            "codex",
            32,
        );
        let suppressed = overlap.suppress_compiled(
            "TITLE: Manifest location\nThe manifest lives at .pc/manifest.json. (docs/spec.md:12)",
        );
        assert!(suppressed.fully_suppressed);
        assert_eq!(suppressed.text, "TITLE: none");
        assert_eq!(suppressed.removed_lines, 1);
    }

    #[test]
    fn source_masking_preserves_line_numbers() {
        let overlap = ContextOverlap::from_hook("Second line already known.", None, "claude", 32);
        let source = "First line.\nSecond line already known.\nThird line.";
        let masked = overlap.mask_source_preserving_lines(source);
        assert_eq!(masked.text.lines().count(), source.lines().count());
        assert_eq!(masked.text, "First line.\n\nThird line.");
        assert_eq!(masked.removed_lines, 1);
    }

    #[test]
    fn compaction_window_drops_old_conversation_but_keeps_persistent_instructions() {
        let mut transcript = tempfile::NamedTempFile::new().unwrap();
        use std::io::Write;
        writeln!(
            transcript,
            "{}",
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"Persistent harness rule."}]}})
        )
        .unwrap();
        writeln!(
            transcript,
            "{}",
            serde_json::json!({"role":"user","content":"Old conversation fact."})
        )
        .unwrap();
        writeln!(
            transcript,
            "{}",
            serde_json::json!({"role":"assistant","content":"Middle turn."})
        )
        .unwrap();
        writeln!(
            transcript,
            "{}",
            serde_json::json!({"role":"user","content":"Recent conversation fact."})
        )
        .unwrap();

        let overlap =
            ContextOverlap::from_hook("Current prompt.", transcript.path().to_str(), "codex", 1);
        assert!(overlap.contains_text("Persistent harness rule."));
        assert!(overlap.contains_text("Recent conversation fact."));
        assert!(!overlap.contains_text("Old conversation fact."));
        assert!(!overlap.contains_text("Middle turn."));
    }
}
