use super::*;

pub(crate) enum OutMode {
    Verbose,
    Plain(crate::harness::OutputDialect),
}

/// Verbose: JSON with `systemMessage` (visible to user) and, if there is a
/// context block, `hookSpecificOutput.additionalContext`.
/// Plain: renders `context_block` in the harness's output dialect — raw text
/// (Claude), `hookSpecificOutput.additionalContext` JSON (Codex/TENEX), or
/// `{"context":…}` JSON (Hermes).
pub(crate) fn emit(out: &OutMode, context_block: Option<&str>, verbose_msg: &str) {
    use crate::harness::OutputDialect;

    let (dialect, rendered) = match out {
        OutMode::Verbose => {
            let obj = if let Some(block) = context_block {
                serde_json::json!({
                    "systemMessage": verbose_msg,
                    "hookSpecificOutput": {
                        "hookEventName": "UserPromptSubmit",
                        "additionalContext": block
                    }
                })
            } else {
                serde_json::json!({ "systemMessage": verbose_msg })
            };
            (
                "verbose-json",
                Some(serde_json::to_string(&obj).unwrap_or_default()),
            )
        }
        OutMode::Plain(dialect) => {
            let Some(block) = context_block else { return };
            match dialect {
                OutputDialect::RawText => ("raw-text", Some(block.to_string())),
                OutputDialect::AdditionalContextJson => (
                    "additional-context-json",
                    Some(
                        serde_json::json!({
                            "hookSpecificOutput": {
                                "hookEventName": "UserPromptSubmit",
                                "additionalContext": block
                            }
                        })
                        .to_string(),
                    ),
                ),
                OutputDialect::ContextJson => (
                    "context-json",
                    Some(serde_json::json!({ "context": block }).to_string()),
                ),
            }
        }
    };
    if let Some(rendered) = rendered {
        crate::inject_trace::record_delivery(dialect, &rendered, context_block);
        print!("{rendered}");
    }
}

pub(crate) fn emit_warning(message: &str) {
    let rendered = serde_json::json!({
        "systemMessage": message
    })
    .to_string();
    crate::inject_trace::record_delivery("system-message-json", &rendered, None);
    print!("{rendered}");
}

pub(crate) struct CommittedContext {
    pub(crate) output: String,
    pub(crate) body: String,
}

pub(crate) enum ContextCommit {
    Delivered(CommittedContext),
    Exhausted,
    LedgerUnavailable,
}

/// Deduplicate and durably commit context before exposing it to the harness.
/// Failure to establish the session-absolute ledger fails closed.
pub(crate) fn commit_context(
    root: &Path,
    session_id: &str,
    title: Option<&str>,
    body: &str,
) -> ContextCommit {
    match crate::ledger::commit_unique(root, session_id, title, body) {
        Ok(body) if body.is_empty() => {
            log_event(
                "inject.suppressed",
                None,
                serde_json::json!({
                    "reason": "already_delivered",
                    "out_chars": 0
                }),
            );
            ContextCommit::Exhausted
        }
        Ok(body) => ContextCommit::Delivered(CommittedContext {
            output: wrap_context_reminder(&body),
            body,
        }),
        Err(error) => {
            log_event(
                "inject.failure",
                None,
                serde_json::json!({
                    "failure_stage": "ledger",
                    "reason": "ledger_unavailable",
                    "error": truncate(&error.to_string(), 200),
                    "out_chars": 0
                }),
            );
            ContextCommit::LedgerUnavailable
        }
    }
}

pub(crate) fn ledger_unavailable_done_payload(
    hits: usize,
    prompt_preview: &str,
) -> serde_json::Value {
    serde_json::json!({
        "outcome": "empty",
        "failure_stage": "ledger",
        "reason": "ledger_unavailable",
        "hits": hits,
        "out_chars": 0,
        "prompt_preview": crate::events::truncate(prompt_preview, 150)
    })
}

pub(crate) fn log_ledger_unavailable_done(elapsed_ms: u64, hits: usize, prompt_preview: &str) {
    log_event(
        "inject.done",
        Some(elapsed_ms),
        ledger_unavailable_done_payload(hits, prompt_preview),
    );
}

pub(crate) fn missing_session_id_warning_payload() -> serde_json::Value {
    serde_json::json!({
        "warning": "missing_session_id",
        "disabled": ["session_ledger_dedup"],
        "impact": "the already-injected ledger cannot be keyed without session_id"
    })
}

pub(crate) fn warn_missing_session_id(session_id: &str) {
    if session_id.trim().is_empty() {
        log_event("inject.warning", None, missing_session_id_warning_payload());
    }
}

// ─── Context renderer ─────────────────────────────────────────────────────────

pub(crate) fn wrap_context_reminder(body: &str) -> String {
    format!(
        "<relevant-context from=\"pc skill\">\n{}\n</relevant-context>",
        escape_markup_text(body)
    )
}

// ─── Activation gate ─────────────────────────────────────────────────────────

/// Strip XML-like blocks (<tag>...</tag> and <tag />) from a prompt using a simple
/// state machine, returning only the non-tag text remainder.
pub(crate) fn strip_xml_content(prompt: &str) -> String {
    let bytes = prompt.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len);
    let mut i = 0;

    while i < len {
        if bytes[i] == b'<' {
            // Peek ahead: must start with a letter to be an XML tag
            let peek = i + 1;
            if peek < len && bytes[peek].is_ascii_alphabetic() {
                // Find the end of the opening tag
                if let Some(gt) = bytes[i..].iter().position(|&b| b == b'>') {
                    let tag_end = i + gt; // index of '>'
                    let tag_inner = &prompt[i + 1..tag_end]; // e.g. "task-notification" or "tag attr=x"
                                                             // Extract the tag name (up to first space or '/')
                    let tag_name: &str = tag_inner
                        .split(|c: char| c == ' ' || c == '/')
                        .next()
                        .unwrap_or("")
                        .trim();
                    if !tag_name.is_empty() {
                        // Check if self-closing (ends with /)
                        let self_closing = tag_inner.trim_end().ends_with('/');
                        if self_closing {
                            // Skip the self-closing tag entirely
                            i = tag_end + 1;
                            out.push(' ');
                            continue;
                        }
                        // Look for closing </tag_name>
                        let close_tag = format!("</{}>", tag_name);
                        if let Some(close_pos) = prompt[tag_end + 1..].find(&close_tag) {
                            // Skip everything from '<' through '</tag>'
                            i = tag_end + 1 + close_pos + close_tag.len();
                            out.push(' ');
                            continue;
                        }
                    }
                }
            }
        }
        // Not a recognized XML tag — emit the character
        out.push(prompt[i..].chars().next().unwrap_or(' '));
        i += prompt[i..]
            .chars()
            .next()
            .map(|c| c.len_utf8())
            .unwrap_or(1);
    }

    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Returns true if the prompt should be skipped (trivial / too short).
pub(crate) fn should_skip_prompt(prompt: &str, min_words: usize) -> bool {
    let lower = prompt.trim().to_lowercase();

    // Check exact trivial phrase match
    if TRIVIAL_PHRASES.contains(&lower.as_str()) {
        return true;
    }

    // Strip XML system tags and evaluate what the human actually typed
    let human_text = strip_xml_content(prompt);
    let human_lower = human_text.trim().to_lowercase();

    // If after stripping XML there's nothing left, skip
    if human_text.trim().is_empty() {
        return true;
    }

    // If after stripping XML the remainder is itself a trivial phrase, skip
    if TRIVIAL_PHRASES.contains(&human_lower.as_str()) {
        return true;
    }

    // Word count gate (applied to the XML-stripped text)
    let word_count = human_text.split_whitespace().count();
    if word_count < min_words {
        return true;
    }

    // Very short character check (applied to the XML-stripped text)
    if human_text.trim().len() < 8 {
        return true;
    }

    false
}

// ─── No-index bootstrap logic ────────────────────────────────────────────────
