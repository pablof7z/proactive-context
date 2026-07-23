use super::*;

// ─── Trivial-prompt stoplist ──────────────────────────────────────────────────

pub(crate) const TRIVIAL_PHRASES: &[&str] = &[
    "yes",
    "no",
    "ok",
    "okay",
    "sure",
    "thanks",
    "thank you",
    "go",
    "continue",
    "next",
    "done",
    "stop",
    "wait",
    "help",
    "please",
    "hi",
    "hello",
    "hey",
    "great",
    "good",
    "fine",
    "right",
    "correct",
    "wrong",
    "nope",
    "yep",
];

pub(crate) const NO_GENERATION_CONFIG_WARNING: &str = "pc: no generation config.";
pub(crate) const NO_GENERATION_CONFIG_FLAG: &str = "no-generation-config-warning";
/// The project already estimates tokens as four characters in recall/capture
/// accounting. Reuse that convention to turn the configured compile output and
/// source-count budgets into a bounded source-input budget.
pub(crate) const ESTIMATED_CHARS_PER_TOKEN: usize = 4;

// ─── stdin contract ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct InjectInput {
    #[serde(default)]
    pub(crate) prompt: String,
    #[serde(default)]
    pub(crate) cwd: String,
    #[serde(default)]
    pub(crate) session_id: String,
    #[serde(default)]
    pub(crate) transcript_path: Option<String>,
    /// Least-lossy host transcript used only for exact overlap suppression. Codex normalization,
    /// for example, keeps recent conversation in `transcript_path` but points this at the original
    /// rollout so developer instructions and prior PC payloads remain observable.
    #[serde(default)]
    pub(crate) model_context_path: Option<String>,
}

// ─── Compile preamble (briefing step) ────────────────────────────────────────

pub(crate) fn strip_title_line(text: &str) -> (Option<String>, &str) {
    // Try to find a leading "TITLE:" line (case-insensitive)
    let upper = text.to_uppercase();
    if upper.starts_with("TITLE:") {
        // Find end of title line
        let line_end = text.find('\n').unwrap_or(text.len());
        let title_text = text[6..line_end].trim().to_string();
        let title = if title_text.is_empty() {
            None
        } else {
            Some(title_text)
        };
        let rest = if line_end < text.len() {
            &text[line_end + 1..]
        } else {
            ""
        };
        return (title, rest);
    }
    (None, text)
}

/// Parse a single gate-output line into a resolved standalone query, tolerating
/// the formatting models tend to add: a leading list bullet (`- `, `* `, `• `),
/// surrounding `**` bold, and any case. Returns the question text after `QUERY:`,
/// or None if this line isn't a (non-empty) QUERY line.
pub(crate) fn parse_query_line(line: &str) -> Option<String> {
    let t = line
        .trim()
        .trim_start_matches(['-', '*', '•', ' '])
        .trim_start_matches("**")
        .trim();
    // Byte-slice only when byte 6 is a char boundary — a response starting with a
    // multi-byte char would otherwise panic the hot inject path.
    if t.len() >= 6 && t.is_char_boundary(6) && t[..6].eq_ignore_ascii_case("QUERY:") {
        // Payload may carry the closing `**` of a bolded label, e.g. `**QUERY:** q`.
        let q = t[6..].trim_matches(|c: char| c == '*' || c.is_whitespace());
        (!q.is_empty()).then(|| q.to_string())
    } else {
        None
    }
}

// ─── Relevance safety ─────────────────────────────────────────────────────────

pub(crate) fn contextualized_query(current_prompt: &str, recent: &str, char_cap: usize) -> String {
    if recent.is_empty() {
        cap_tail(current_prompt, char_cap)
    } else {
        cap_tail(&format!("{}\n\n{}", recent, current_prompt), char_cap)
    }
}

/// Prefer distinct source paths before taking additional chunks from a source,
/// while preserving the retrieval/reranker order within both passes.
pub(crate) fn diversify_hits(hits: &[QueryResult], top_k: usize) -> Vec<QueryResult> {
    let mut seen_paths = HashSet::new();
    let mut primary = Vec::new();
    let mut overflow = Vec::new();

    for hit in hits {
        if seen_paths.insert(hit.path.as_str()) {
            primary.push(hit.clone());
        } else {
            overflow.push(hit.clone());
        }
    }
    primary.extend(overflow);
    primary.truncate(top_k);
    primary
}

/// Match the production hook's bounded over-fetch before path diversification.
pub(crate) fn retrieval_candidate_limit(top_k: usize) -> usize {
    top_k.saturating_mul(4)
}

pub(crate) fn source_char_budget(max_tokens: usize, max_guides: usize) -> usize {
    max_tokens
        .saturating_mul(ESTIMATED_CHARS_PER_TOKEN)
        .saturating_mul(max_guides)
}

pub(crate) fn truncate_head_to_chars(text: &str, char_budget: usize) -> String {
    if char_budget == 0 {
        return String::new();
    }
    if text.chars().count() <= char_budget {
        return text.to_string();
    }

    let byte_end = text
        .char_indices()
        .nth(char_budget)
        .map(|(index, _)| index)
        .unwrap_or(text.len());
    text[..byte_end].trim_end().to_string()
}

pub(crate) fn recent_context_text(
    transcript_path: Option<&str>,
    context_turns: usize,
    char_cap: usize,
) -> String {
    if context_turns == 0 {
        return String::new();
    }

    let text = transcript_path
        .and_then(|p| {
            if std::path::Path::new(p).exists() {
                Some(p)
            } else {
                None
            }
        })
        .and_then(|p| parse_transcript(p).ok())
        .map(|turns| {
            let last_n: Vec<_> = turns
                .iter()
                .rev()
                .take(context_turns * 2)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            last_n
                .iter()
                .map(|(role, text)| {
                    format!(
                        "{}: {}",
                        if role == "user" { "User" } else { "Assistant" },
                        text
                    )
                })
                .collect::<Vec<_>>()
                .join("\n\n")
        })
        .unwrap_or_default();

    cap_tail(&text, char_cap)
}

pub(crate) fn build_enriched_query(current_prompt: &str, recent: &str, char_cap: usize) -> String {
    contextualized_query(current_prompt, recent, char_cap)
}

pub(crate) fn cap_tail(s: &str, char_cap: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= char_cap {
        return s.to_string();
    }
    let boundary = s
        .char_indices()
        .nth(char_count - char_cap)
        .map(|(index, _)| index)
        .unwrap_or(0);
    s[boundary..].to_string()
}

pub(crate) fn format_guides(guides: &[String]) -> String {
    if guides.is_empty() {
        "(none)".to_string()
    } else {
        guides.join(", ")
    }
}
