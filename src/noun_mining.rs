//! Production noun-mining helpers shared by capture and eval harnesses.

use std::collections::HashSet;

const STOP: &[&str] = &[
    "user", "assistant", "human", "system", "we", "i", "the", "this", "that", "these", "those",
    "add", "make", "let", "lets", "let's", "can", "could", "should", "would", "when", "what",
    "why", "how", "if", "so", "but", "and", "also", "now", "then", "here", "there", "it",
    "is", "are", "do", "does", "please", "ok", "okay", "yes", "no", "maybe", "you", "your",
    "my", "our", "a", "an", "to", "for", "of", "in", "on", "use", "using", "want", "need",
    "first", "next", "great", "good", "thanks", "hmm", "wait", "actually", "just", "like",
    "see", "look", "got", "get", "im", "ive", "dont", "didnt", "doesnt", "eg", "ie", "etc",
    "none", "current", "known", "future", "upon", "non-goals", "hello",
];

const KNOWN_EXTS: &[&str] = &[
    ".rs", ".ts", ".tsx", ".js", ".md", ".json", ".toml", ".py", ".txt", ".sh", ".yaml",
    ".yml", ".html", ".css", ".sql",
];

/// Strip proactive-context's own injected content from a user turn before mining user intent.
pub(crate) fn strip_injected_context(text: &str) -> String {
    let mut s = text.to_string();
    // Remove <system-reminder>...</system-reminder> blocks (both raw and HTML-escaped forms).
    for (open, close) in [
        ("<system-reminder>", "</system-reminder>"),
        ("&lt;system-reminder&gt;", "&lt;/system-reminder&gt;"),
    ] {
        loop {
            let Some(start) = s.find(open) else { break };
            let after = start + open.len();
            if let Some(rel_end) = s[after..].find(close) {
                let end = after + rel_end + close.len();
                s.replace_range(start..end, " ");
            } else {
                // Unterminated reminder -> drop to end of string.
                s.truncate(start);
                break;
            }
        }
    }
    s
}

/// True if a turn is dominated by pc's own injected/derived artifacts rather than human text.
pub(crate) fn is_pc_self_referential(t: &str) -> bool {
    let lower = t.to_lowercase();
    lower.contains("relevant project context (")
        || lower.contains("derived cache — do not hand-edit")
        || lower.contains("rebuilt by proactive-context after each capture")
        || (lower.contains("# wiki index") && lower.contains("| slug |"))
}

/// Extract noun candidates from a single human turn: backticked ids, `kind:NNNN`, `NIP-NN` tokens,
/// and capitalized multi-word phrases. Lowercased, de-duplicated, trimmed.
pub(crate) fn extract_noun_candidates(turn: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let push = |c: String, out: &mut Vec<String>, seen: &mut HashSet<String>| {
        let c = c
            .trim()
            .trim_matches(|ch: char| {
                ch == '.' || ch == ',' || ch == '?' || ch == '!' || ch == ':' || ch == ';'
            })
            .to_string();
        let cl = c.to_lowercase();
        if c.len() >= 3 && c.len() <= 60 && seen.insert(cl) {
            out.push(c);
        }
    };

    // 1. Backticked identifiers: `foo_bar`, `kind:7375`.
    let bytes = turn.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            if let Some(rel) = turn[i + 1..].find('`') {
                let inner = &turn[i + 1..i + 1 + rel];
                if !inner.trim().is_empty() {
                    push(inner.to_string(), &mut out, &mut seen);
                }
                i = i + 1 + rel + 1;
                continue;
            }
        }
        i += 1;
    }

    // 2/3. Token scan for kind:NNNN and NIP-NN; and accumulate Capitalized runs.
    let words: Vec<&str> = turn.split_whitespace().collect();
    let mut cap_run: Vec<String> = Vec::new();
    let flush_run = |run: &mut Vec<String>,
                     out: &mut Vec<String>,
                     seen: &mut HashSet<String>,
                     push: &dyn Fn(String, &mut Vec<String>, &mut HashSet<String>)| {
        if run.len() >= 2 {
            push(run.join(" "), out, seen);
        }
        run.clear();
    };
    for w in &words {
        let clean =
            w.trim_matches(|c: char| !c.is_alphanumeric() && c != ':' && c != '-' && c != '_');
        let cl = clean.to_lowercase();
        if cl.starts_with("kind:") && cl[5..].chars().all(|c| c.is_ascii_digit()) && cl.len() > 5
        {
            push(clean.to_string(), &mut out, &mut seen);
        }
        if (cl.starts_with("nip-") || cl.starts_with("nip "))
            && cl.len() >= 5
            && cl[4..]
                .chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false)
        {
            push(clean.to_string(), &mut out, &mut seen);
        }
        let first = clean.chars().next();
        let is_cap = first.map(|c| c.is_uppercase()).unwrap_or(false)
            && clean.chars().count() >= 2
            && !clean.chars().all(|c| c.is_uppercase());
        if is_cap || (clean.chars().all(|c| c.is_uppercase()) && clean.len() >= 3 && clean.len() <= 6)
        {
            cap_run.push(clean.to_string());
        } else {
            flush_run(&mut cap_run, &mut out, &mut seen, &push);
        }
    }
    flush_run(&mut cap_run, &mut out, &mut seen, &push);

    out
}

/// A `name.ext:line` or `path/to/file:line` reference: a code location, not a project entity.
pub(crate) fn is_file_line_ref(c: &str) -> bool {
    if let Some(idx) = c.rfind(':') {
        let head = &c[..idx];
        let tail = &c[idx + 1..];
        if !tail.is_empty()
            && tail.chars().all(|d| d.is_ascii_digit())
            && (head.contains('.') || head.contains('/'))
        {
            return true;
        }
    }
    false
}

/// Whether a raw candidate is a genuine project noun rather than a code symbol, snippet fragment,
/// transcript artifact, or conversational filler.
pub(crate) fn is_entity_candidate(c: &str) -> bool {
    let c = c.trim();
    let nchars = c.chars().count();
    if nchars < 3 || nchars > 50 {
        return false;
    }
    if is_file_line_ref(c) {
        return false;
    }
    if c.contains(": ") {
        return false;
    }
    let lc0 = c.to_lowercase();
    if c.len() >= 6
        && lc0.chars().all(|ch| ch.is_ascii_hexdigit())
        && lc0.chars().any(|ch| ch.is_ascii_digit())
    {
        return false;
    }
    const CODE_PUNCT: &[char] = &[
        '(', ')', '{', '}', '[', ']', ';', '=', '<', '>', '"', '\'', '\\', '|', '&', '*', '/', '%',
        '@', '$', '+', '~', '^', '`',
    ];
    if c.chars().any(|ch| CODE_PUNCT.contains(&ch)) {
        return false;
    }
    if c.contains("::") {
        return false;
    }
    if c.matches('.').count() >= 2 {
        return false;
    }
    let first = c.split_whitespace().next().unwrap_or("");
    let first_l = first
        .trim_matches(|ch: char| !ch.is_alphanumeric())
        .to_lowercase();
    if STOP.contains(&first_l.as_str()) {
        return false;
    }
    let words: Vec<&str> = c.split_whitespace().collect();
    let lc = c.to_lowercase();
    let is_filename =
        words.len() == 1 && c.matches('.').count() == 1 && KNOWN_EXTS.iter().any(|e| lc.ends_with(e));
    let has_ident = c
        .chars()
        .any(|ch| ch == '_' || ch == '-' || ch == ':' || ch.is_ascii_digit())
        || c.chars().skip(1).any(|ch| ch.is_uppercase());
    let leading_cap = c.chars().next().map(|x| x.is_uppercase()).unwrap_or(false);
    let multiword = words.len() >= 2;
    has_ident || multiword || is_filename || leading_cap
}
