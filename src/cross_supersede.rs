//! cross_supersede.rs — `pc wiki doctor --cross-supersede`: heal cross-GUIDE staleness.
//!
//! ## Why this exists
//! RECONCILE only ever sees claims routed to the SAME guide, so a statement that goes stale
//! because a *later* fact landed in a DIFFERENT guide is structurally invisible to capture.
//! Real case: `nip17-dm-relay-requirement.md` still asserts "NIP-17 DM receive-side cold-start
//! is unverified" (true when written 2026-05-26), while the June closure facts ("#1080 wired the
//! kind:10050 planner trigger; fresh accounts now receive DMs") were routed to
//! `publish-action-ledger.md`. Two guides, no same-guide overlap → the inversion persists.
//!
//! ## Discipline (mirrors `wiki doctor` merge): embeddings propose, LLM confirms, code revises.
//!   - SPLIT (deterministic): each guide body → statements (blank-line paragraphs within
//!     sections), tracking each statement's byte span and owning guide date.
//!   - EMBED (cheap, local fastembed): embed every statement once.
//!   - RETRIEVE (cheap gate): for each statement, top-K most-similar statements FROM OTHER
//!     GUIDES with cosine ≥ tau AND a strictly-NEWER source date (≥1 day). This shortlists;
//!     it never decides.
//!   - CONFIRM (ONE LLM call per guide, batched over its candidate pairs): "does the NEWER
//!     statement make the OLDER one false/stale (not merely related, not additive)?" — strict,
//!     with the additive-capability negative example.
//!   - REVISE (deterministic): replace ONLY the stale statement's text in the older guide with
//!     the terminal truth + a "(Previously: <old>, superseded — see <slug>.)" breadcrumb,
//!     citing the newer guide. Never delete; `stamp_updated` bumps the date.
//!
//! ## Safety
//! `--dry-run` prints the would-revise list (old + new text) and writes nothing. Without it,
//! revisions are applied in place to the guides under `--wiki-dir` (or the discovered wiki).

use crate::embed::build_embedder;
use crate::provider::ModelSpec;
use crate::route_recall::cosine;
use crate::wiki::{self, Guide};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Default similarity floor for proposing a cross-guide supersession candidate. Lower than the
/// merge tau (0.65) because we compare individual STATEMENTS (shorter, noisier), the lexical
/// entity channel runs alongside it, and the strict LLM confirm is the real precision gate.
/// Empirically: the nip17 cold-start ↔ closure pair embeds at only ~0.31 and shares no rare
/// token, so a 0.45 floor misses it; 0.30 admits it while the confirm step holds precision.
/// Overridable via `--tau`.
pub const DEFAULT_CROSS_TAU: f32 = 0.30;

/// Top-K most-similar newer statements retrieved per statement. Wider than the doctor merge's
/// shortlist so a genuine superseder that ranks below same-topic siblings still reaches the LLM
/// (a stale claim's nearest neighbours are its same-topic siblings, not its superseder). The
/// strict LLM confirm is the precision gate, so a generous K trades a longer prompt for recall.
fn top_k() -> usize {
    std::env::var("PC_CROSS_TOPK").ok().and_then(|s| s.parse().ok()).unwrap_or(15)
}

// ─── Statement model ──────────────────────────────────────────────────────────

/// One statement extracted from a guide body, with the byte span it occupies (so a confirmed
/// revision can replace exactly this text) and the owning guide's `updated` date (the staleness
/// clock — per-guide is the best available granularity).
#[derive(Debug, Clone)]
pub struct Statement {
    pub slug: String,
    pub title: String,
    pub date: String, // owning guide `updated` (YYYY-MM-DD)
    pub text: String,
    /// Byte range [start, end) of `text` within the guide body.
    pub start: usize,
    pub end: usize,
}

/// Split a guide body into statements: blank-line-separated paragraphs that are real prose.
/// Skips heading lines (`#`/`##`), the `## See Also` section, citation/HTML-comment lines
/// (`<!-- ... -->`), and pure-marker paragraphs. Returns each statement with its byte span.
pub fn split_statements(body: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let bytes = body.as_bytes();
    let mut in_see_also = false;

    // Walk line by line, tracking byte offsets, grouping consecutive prose lines into a
    // paragraph until a blank line / heading / comment boundary.
    let mut para_start: Option<usize> = None;
    let mut para_end = 0usize;
    let mut offset = 0usize;

    let mut flush = |start: Option<usize>, end: usize, out: &mut Vec<(usize, usize, String)>| {
        if let Some(s) = start {
            // Trim trailing whitespace bytes from the paragraph span.
            let mut e = end;
            while e > s && (bytes[e - 1] == b'\n' || bytes[e - 1] == b' ' || bytes[e - 1] == b'\t') {
                e -= 1;
            }
            if e > s {
                // Split the paragraph into SENTENCES with byte-accurate spans, so a single
                // dense paragraph that packs several facts (e.g. an action-ledger blob that
                // ends with the DM cold-start closure) yields one comparable unit per fact —
                // the embedding of each isn't diluted by its neighbors. The revise machinery
                // targets the precise sentence span. Sentences too short to be a real fact are
                // merged forward so a span always covers prose.
                for (ss, se, text) in split_sentences(body, s, e) {
                    if is_prose_statement(&text) {
                        out.push((ss, se, text));
                    }
                }
            }
        }
    };

    for line in body.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        let trimmed = line.trim();

        // Heading lines break paragraphs and toggle See-Also tracking.
        if trimmed.starts_with('#') {
            flush(para_start.take(), para_end, &mut out);
            in_see_also = trimmed.trim_start_matches('#').trim().eq_ignore_ascii_case("see also");
            continue;
        }
        if in_see_also {
            flush(para_start.take(), para_end, &mut out);
            continue;
        }
        if trimmed.is_empty() {
            flush(para_start.take(), para_end, &mut out);
            continue;
        }
        // A standalone citation/HTML comment line is not prose; it breaks a paragraph.
        if trimmed.starts_with("<!--") {
            flush(para_start.take(), para_end, &mut out);
            continue;
        }
        // Accumulate this line into the current paragraph.
        if para_start.is_none() {
            para_start = Some(line_start);
        }
        para_end = line_start + line.len();
    }
    flush(para_start.take(), para_end, &mut out);
    out
}

/// Split a paragraph `body[start..end]` into sentence-level spans (byte-accurate against the
/// FULL body). Boundary heuristic: a period (or `?`/`!`) followed by whitespace and then an
/// uppercase letter or a backtick begins a new sentence — robust for the wiki's full-sentence
/// prose while avoiding splits inside decimals/abbreviations (`v0.5`, `e.g.`) which are
/// lowercase-followed. Sentences shorter than a few words are merged into the next so a span
/// always covers a real statement. Returns at least the whole paragraph if no boundary is found.
fn split_sentences(body: &str, start: usize, end: usize) -> Vec<(usize, usize, String)> {
    let para = &body[start..end];
    let pbytes = para.as_bytes();
    let mut spans: Vec<(usize, usize)> = Vec::new(); // relative to `start`
    let mut seg_start = 0usize;
    let mut i = 0usize;
    while i < pbytes.len() {
        let c = pbytes[i];
        if c == b'.' || c == b'?' || c == b'!' {
            // Look past the terminator + following whitespace run.
            let mut j = i + 1;
            // allow a closing ) or " right after the period
            while j < pbytes.len() && (pbytes[j] == b')' || pbytes[j] == b'"' || pbytes[j] == b'\'') {
                j += 1;
            }
            let mut k = j;
            while k < pbytes.len() && (pbytes[k] == b' ' || pbytes[k] == b'\n' || pbytes[k] == b'\t') {
                k += 1;
            }
            if k > j && k < pbytes.len() {
                let next = pbytes[k];
                let starts_sentence = next.is_ascii_uppercase() || next == b'`';
                if starts_sentence {
                    spans.push((seg_start, j)); // sentence text ends after the terminator/quote
                    seg_start = k;
                    i = k;
                    continue;
                }
            }
        }
        i += 1;
    }
    if seg_start < pbytes.len() {
        spans.push((seg_start, pbytes.len()));
    }

    // Merge segments that are too short (< 4 words) into the following one so every emitted
    // span covers a real statement; keep byte spans contiguous.
    let mut merged: Vec<(usize, usize)> = Vec::new();
    let mut pending_start: Option<usize> = None;
    for (s, e) in spans {
        let seg = para[s..e].trim();
        let words = strip_inline_markers(seg).split_whitespace().count();
        let real_start = pending_start.unwrap_or(s);
        if words < 4 {
            // too short alone — keep accumulating
            pending_start = Some(real_start);
            continue;
        }
        merged.push((real_start, e));
        pending_start = None;
    }
    if let Some(s) = pending_start {
        // trailing short fragment — attach to the previous span if any, else emit alone
        if let Some(last) = merged.last_mut() {
            last.1 = end - start; // extend to paragraph end
            let _ = s;
        } else {
            merged.push((s, para.len()));
        }
    }
    if merged.is_empty() {
        merged.push((0, para.len()));
    }

    merged
        .into_iter()
        .map(|(s, e)| {
            // Trim trailing whitespace within the relative span.
            let mut re = e;
            let pb = para.as_bytes();
            while re > s && (pb[re - 1] == b' ' || pb[re - 1] == b'\n' || pb[re - 1] == b'\t') {
                re -= 1;
            }
            (start + s, start + re, para[s..re].to_string())
        })
        .collect()
}

/// Is a paragraph substantive prose worth comparing? Rejects empties and pure-marker text.
fn is_prose_statement(text: &str) -> bool {
    let stripped = strip_inline_markers(text);
    let s = stripped.trim();
    // Need at least a few words of real content.
    s.split_whitespace().count() >= 3
}

/// Remove inline `[^id]` citation markers from a string (for embedding / prose checks).
/// UTF-8-safe: iterates by char so multi-byte characters (e.g. `→`, `≤`) survive intact.
fn strip_inline_markers(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.char_indices().peekable();
    while let Some((idx, c)) = chars.next() {
        if c == '[' && chars.peek().map(|(_, n)| *n == '^').unwrap_or(false) {
            if let Some(close_rel) = s[idx..].find(']') {
                // Skip past the closing ']' (advance the char iterator to that byte).
                let target = idx + close_rel + 1;
                while chars.peek().map(|(i, _)| *i < target).unwrap_or(false) {
                    chars.next();
                }
                continue;
            }
        }
        out.push(c);
    }
    out
}

// ─── Date helpers ─────────────────────────────────────────────────────────────

/// Is `newer` at least one day after `older`? Both are YYYY-MM-DD; lexical compare suffices
/// for ordering, and we require strict inequality (a different, later day).
pub fn is_strictly_newer(newer: &str, older: &str) -> bool {
    !newer.is_empty() && !older.is_empty() && newer > older
}

// ─── Candidate retrieval ──────────────────────────────────────────────────────

/// A proposed (stale?, fresh) statement pair for one older statement.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// Index of the newer statement in the global statement vector.
    pub newer_idx: usize,
    pub similarity: f32,
}

/// Extract RARE, distinctive tokens from a statement — issue refs (`#1080`), kinded/colon ids
/// (`kind:10050`), snake_case / dotted identifiers, and hyphenated domain terms (`cold-start`).
/// These are the entity anchors that link a stale claim to its closure even when the prose
/// wording diverges (so embedding similarity alone misses it). Lowercased.
pub fn rare_tokens(text: &str) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    let lower = strip_inline_markers(text).to_lowercase();
    let is_tokchar =
        |c: char| c.is_alphanumeric() || c == '#' || c == ':' || c == '_' || c == '-' || c == '.';
    // Split on any non-token character; UTF-8-safe because we split by `char`, not byte.
    for raw in lower.split(|c: char| !is_tokchar(c)) {
        let tok = raw.trim_matches(|c: char| c == '.' || c == '-' || c == ':');
        if is_rare_token(tok) {
            set.insert(tok.to_string());
        }
    }
    set
}

/// A token is "rare/distinctive" if it carries entity identity: an issue/PR ref (`#1080`), a
/// colon/underscore/dot compound id (`kind:10050`, `swap_dm_inbox_observer`, `actor.rs`), or a
/// multi-segment hyphenated term (`cold-start`, `receive-side`). Plain English words are not
/// rare — they would over-link.
fn is_rare_token(tok: &str) -> bool {
    if tok.len() < 4 {
        return false;
    }
    if tok.starts_with('#') && tok[1..].chars().any(|c| c.is_ascii_digit()) {
        return true; // issue/PR ref
    }
    if tok.contains(':') || tok.contains('_') {
        return true; // kind:NNNN, snake_case identifier
    }
    // hyphenated multi-word domain term (e.g. cold-start, receive-side) — needs 2+ segments
    if tok.matches('-').count() >= 1 && tok.split('-').filter(|s| s.len() >= 3).count() >= 2 {
        return true;
    }
    // dotted file/path identifiers (actor.rs, dm.rs) — has a non-numeric dotted segment
    if tok.contains('.') && tok.split('.').any(|s| s.len() >= 2 && s.chars().any(|c| c.is_alphabetic())) {
        return true;
    }
    false
}

/// Does a statement assert a NEGATIVE / open status (unverified, untested, missing, broken, a
/// gap, inert, stub, deferred, "not yet", "zero callers", "no ... yet")? These are the highest-
/// value supersession TARGETS — a later closure most often invalidates exactly such a claim —
/// so they earn a wider retrieval net (a content-word channel) to reach their superseder even
/// when it is lexically divergent and low-cosine.
pub fn is_negative_status(text: &str) -> bool {
    let t = strip_inline_markers(text).to_lowercase();
    const MARKERS: &[&str] = &[
        "unverified", "untested", "not verified", "not tested", "missing", "is broken",
        "a gap", "the gap", "inert", "stub", "scaffold-only", "deferred", "not yet",
        "no longer", "never receive", "zero callers", "zero swift callers", "not implemented",
        "unimplemented", "dead code", "does not exist", "doesn't exist", "no seam", "post-v1",
        "must move", "must be renamed", "needs a pr", "needs to", "not wired",
    ];
    MARKERS.iter().any(|m| t.contains(m))
}

/// Significant lowercased content words (len ≥ 4, not a stopword) for the negative-status
/// content channel. Used only to widen recall for negative-status statements.
fn content_words(text: &str) -> std::collections::HashSet<String> {
    const STOP: &[&str] = &[
        "this", "that", "with", "from", "have", "must", "when", "where", "which", "their",
        "there", "these", "those", "into", "than", "then", "they", "them", "been", "being",
        "would", "could", "should", "will", "shall", "does", "done", "also", "only", "such",
        "each", "both", "some", "more", "most", "other", "over", "under", "after", "before",
    ];
    strip_inline_markers(text)
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 4 && !STOP.contains(w))
        .map(|w| w.trim_end_matches('s').to_string()) // fold simple plural
        .filter(|w| w.len() >= 4)
        .collect()
}

/// For statement `i`, retrieve up to TOP_K candidate newer statements from OTHER guides whose
/// owning guide is strictly newer, via three merged channels:
///   (A) EMBEDDING: cosine ≥ `tau` (semantic similarity).
///   (B) LEXICAL ENTITY: shares ≥1 rare/distinctive token (issue ref, id, hyphenated term) —
///       catches an entity-linked but lexically divergent closure.
///   (C) NEGATIVE-STATUS CONTENT (only when the OLDER statement asserts a negative/open status):
///       shares ≥3 significant content words with a newer statement. This is the recall budget
///       spent precisely on the cardinal supersession case ("X is unverified" → "X now works"),
///       where the superseder embeds far (~0.31) and shares no rare token, yet shares the domain
///       nouns (DM, receive, cold-start, accounts, …). The strict LLM confirm filters precision.
/// Ranked by a blended score so a strong entity/content match isn't crowded out by higher-cosine
/// same-topic siblings.
pub fn retrieve_candidates(
    i: usize,
    statements: &[Statement],
    embeddings: &[Vec<f32>],
    token_sets: &[std::collections::HashSet<String>],
    tau: f32,
) -> Vec<Candidate> {
    let me = &statements[i];
    let my_emb = &embeddings[i];
    let my_toks = &token_sets[i];
    let neg = is_negative_status(&me.text);
    // For the negative-status content channel, the older statement's word set is its own content
    // words UNION its guide-title words — the title supplies the topic nouns (e.g. "NIP-17 DM
    // Relay Requirement" → nip/dm/relay) that the bare statement omits but its superseder shares.
    let my_words: std::collections::HashSet<String> = if neg {
        let mut w = content_words(&me.text);
        w.extend(content_words(&me.title));
        w
    } else {
        Default::default()
    };
    let mut scored: Vec<(f32, Candidate)> = Vec::new();
    for (j, other) in statements.iter().enumerate() {
        if j == i || other.slug == me.slug {
            continue; // never compare within the same guide
        }
        if !is_strictly_newer(&other.date, &me.date) {
            continue; // the candidate must be NEWER than the (possibly stale) statement
        }
        let sim = cosine(my_emb, &embeddings[j]);
        let shared_tok = my_toks.intersection(&token_sets[j]).count();
        let shared_words = if neg {
            // Compare against the candidate's content words ∪ its title words too.
            let mut ow = content_words(&other.text);
            ow.extend(content_words(&other.title));
            ow.intersection(&my_words).count()
        } else {
            0
        };
        // Admit if ANY channel fires. The neg-status content channel needs ≥2 shared domain
        // words (statement+title nouns) — enough to bridge "receive-side cold-start unverified"
        // ↔ a closure that shares "receive"+"dm" via titles, while the strict LLM confirm keeps
        // precision. Only negative-status statements pay this wider net.
        let admit = sim >= tau || shared_tok >= 1 || (neg && shared_words >= 2);
        if !admit {
            continue;
        }
        // Blended rank: cosine + 0.25/shared rare token + (for neg-status) 0.12/shared content
        // word capped at +0.6. A negative-status claim's true superseder embeds low (~0.31) and
        // is out-ranked by same-topic siblings; a multi-word domain match must lift it into K.
        // The cap bounds over-boost; the strict LLM confirm is the precision gate.
        let word_boost = (0.12 * shared_words as f32).min(0.60);
        let rank = sim + 0.25 * shared_tok as f32 + word_boost;
        scored.push((rank, Candidate { newer_idx: j, similarity: sim }));
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().take(top_k()).map(|(_, c)| c).collect()
}

// ─── LLM confirm (batched per guide) ──────────────────────────────────────────

const CONFIRM_SYSTEM: &str = "You are a strict technical-knowledge auditor deciding whether a \
NEWER statement makes an OLDER statement STALE — i.e. the older statement is now FALSE, \
obsolete, or contradicted by the newer one. Be conservative about ADDITIVE changes, but DO \
flag status changes.\n\
FLAG (stale) when the newer statement REVERSES, CLOSES, FIXES, VERIFIES, or INVALIDATES the \
older one. In particular, an older statement asserting something is UNVERIFIED / UNTESTED / \
MISSING / BROKEN / A GAP / NOT YET DONE is made STALE by a newer statement showing that same \
thing is now VERIFIED, TESTED, IMPLEMENTED, FIXED, WIRED, or CLOSED (by a PR/commit/test) — \
even if the wording differs, as long as they concern the same feature/behaviour/surface. \
Example: older 'NIP-17 DM receive-side cold-start is unverified'; newer 'the kind:10050 \
planner trigger was missing before #1080, causing fresh accounts to never receive DMs (now \
fixed)' or 'the cache-serve test proves DMs render from store' → STALE (the receive path it \
flagged as unverified is now implemented/tested).\n\
DO NOT FLAG purely ADDITIVE changes: older 'The API supports JSON output'; newer 'The API \
also supports YAML output' — the older statement is still true. Sharing only a topic is NOT \
enough.";

/// Build the per-guide batch confirm prompt. Each OLDER statement is numbered and shown with
/// its retrieved CANDIDATE newer statements (also numbered). The model decides, for each older
/// statement, whether ANY candidate makes it stale — picking the candidate that does.
fn confirm_prompt(olders: &[(usize, Vec<usize>)], statements: &[Statement]) -> String {
    let mut s = String::from(
        "Each OLDER statement below is followed by CANDIDATE newer statements from other guides. \
For each OLDER statement, decide whether ANY candidate makes it STALE (false/obsolete/\
contradicted/closed). If so, pick the single candidate that supersedes it and write the \
corrected TERMINAL TRUTH — one sentence stating what is now true, drawn from that candidate.\n\n",
    );
    for (n, (older_idx, cand_idxs)) in olders.iter().enumerate() {
        let o = &statements[*older_idx];
        s.push_str(&format!(
            "OLDER {n} (guide {oslug}, {odate}): {otext}\n",
            n = n,
            oslug = o.slug,
            odate = o.date,
            otext = strip_inline_markers(&o.text).trim(),
        ));
        for (ci, cand_idx) in cand_idxs.iter().enumerate() {
            let c = &statements[*cand_idx];
            s.push_str(&format!(
                "    CANDIDATE {ci} (guide {cslug}, {cdate}): {ctext}\n",
                ci = ci,
                cslug = c.slug,
                cdate = c.date,
                ctext = strip_inline_markers(&c.text).trim(),
            ));
        }
        s.push('\n');
    }
    s.push_str(
        "Output ONLY a JSON array, one object per STALE older statement (omit non-stale ones):\n\
[{\"older\": <n>, \"candidate\": <ci>, \"terminal_truth\": \"<one corrected sentence>\"}]\n\
If none are stale, output []. No prose outside the JSON.",
    );
    s
}

/// Parse the confirm response into (older_local_idx, chosen_newer_global_idx, terminal_truth),
/// validating both indices against the prompt's `olders` structure.
fn parse_confirm(raw: &str, olders: &[(usize, Vec<usize>)]) -> Vec<(usize, usize, String)> {
    let json = extract_json_array(raw);
    let mut out = Vec::new();
    if let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(&json) {
        for it in items {
            let older = it.get("older").and_then(|v| v.as_u64()).map(|n| n as usize);
            let cand = it.get("candidate").and_then(|v| v.as_u64()).map(|n| n as usize);
            let truth = it.get("terminal_truth").and_then(|v| v.as_str()).map(str::to_string);
            if let (Some(o), Some(c), Some(t)) = (older, cand, truth) {
                if o < olders.len() && c < olders[o].1.len() && !t.trim().is_empty() {
                    let newer_global = olders[o].1[c];
                    out.push((o, newer_global, t.trim().to_string()));
                }
            }
        }
    }
    out
}

fn extract_json_array(text: &str) -> String {
    if let Some(start) = text.find('[') {
        if let Some(end) = text.rfind(']') {
            if end > start {
                return text[start..=end].to_string();
            }
        }
    }
    text.to_string()
}

// ─── Deterministic revise ─────────────────────────────────────────────────────

/// Build the replacement text for a stale statement: terminal truth + supersession breadcrumb
/// citing the newer guide. Never deletes the old wording — it is preserved inside the breadcrumb.
pub fn build_revision(old_text: &str, terminal_truth: &str, newer_slug: &str) -> String {
    let old_clean = strip_inline_markers(old_text).trim().trim_end_matches('.').to_string();
    let truth = terminal_truth.trim().trim_end_matches('.').to_string();
    format!(
        "{truth}. (Previously: {old}, superseded — see {slug}.)",
        truth = truth,
        old = old_clean,
        slug = newer_slug,
    )
}

/// Apply a set of statement-span replacements to a guide body. Replacements are applied
/// right-to-left (descending start offset) so earlier spans keep their offsets. Each entry is
/// (start, end, replacement_text). Returns the new body.
pub fn apply_revisions(body: &str, mut edits: Vec<(usize, usize, String)>) -> String {
    edits.sort_by(|a, b| b.0.cmp(&a.0)); // right-to-left
    let mut out = body.to_string();
    for (start, end, text) in edits {
        if start <= end && end <= out.len() {
            out.replace_range(start..end, &text);
        }
    }
    out
}

// ─── Orchestration ────────────────────────────────────────────────────────────

pub struct CrossSupersedeArgs {
    pub wiki_dir: Option<PathBuf>,
    pub dry_run: bool,
    pub tau: Option<f32>,
}

/// A planned revision (for reporting / dry-run / apply).
#[derive(Debug, Clone)]
pub struct PlannedRevision {
    pub slug: String,
    pub path: PathBuf,
    pub start: usize,
    pub end: usize,
    pub old_text: String,
    pub new_text: String,
    pub newer_slug: String,
}

/// Entry point for `pc wiki doctor --cross-supersede`.
pub fn run_cross_supersede(root: &Path, args: CrossSupersedeArgs) -> Result<()> {
    let cfg = crate::config::load_config()?;
    let tau = args.tau.unwrap_or(DEFAULT_CROSS_TAU);
    let wiki = args
        .wiki_dir
        .clone()
        .unwrap_or_else(|| wiki::wiki_dir(root));
    if !wiki.exists() {
        anyhow::bail!("no wiki found at {}", wiki.display());
    }
    println!(
        "wiki doctor --cross-supersede: {} (tau={:.2}, {})",
        wiki.display(),
        tau,
        if args.dry_run { "dry-run" } else { "APPLY" }
    );

    // ── SPLIT: load guides, split into statements (keep per-guide span info). ──
    let paths = list_guide_paths(&wiki);
    let mut guides: Vec<(PathBuf, Guide)> = Vec::new();
    for p in &paths {
        if let Some(g) = wiki::load_guide(p) {
            guides.push((p.clone(), g));
        }
    }
    // Global statement vector + a parallel (guide_idx, span) map so we can edit back.
    let mut statements: Vec<Statement> = Vec::new();
    // statement global index -> (guide index in `guides`, start, end)
    let mut stmt_loc: Vec<(usize, usize, usize)> = Vec::new();
    for (gi, (_p, g)) in guides.iter().enumerate() {
        let date = if g.frontmatter.updated.is_empty() {
            g.frontmatter.created.clone()
        } else {
            g.frontmatter.updated.clone()
        };
        for (start, end, text) in split_statements(&g.body) {
            statements.push(Statement {
                slug: g.frontmatter.slug.clone(),
                title: g.frontmatter.title.clone(),
                date: date.clone(),
                text,
                start,
                end,
            });
            stmt_loc.push((gi, start, end));
        }
    }
    println!("  {} guides, {} statements", guides.len(), statements.len());
    if statements.is_empty() {
        println!("  nothing to do.");
        return Ok(());
    }

    // ── EMBED all statements once (local fastembed) + precompute rare-token sets. ──
    let mut embedder = build_embedder(&cfg).context("build embedder")?;
    let texts: Vec<String> = statements.iter().map(|s| strip_inline_markers(&s.text)).collect();
    let embeddings = embedder.embed(&texts).context("embed statements")?;
    let token_sets: Vec<std::collections::HashSet<String>> =
        statements.iter().map(|s| rare_tokens(&s.text)).collect();

    // ── RETRIEVE: per statement, gather up to TOP_K newer similar candidates. Group by owning
    //    guide so we can issue ONE confirm call per guide (frugal). Each older statement keeps
    //    its FULL candidate set — the genuine superseder is often not the single nearest
    //    neighbour (a stale claim's nearest neighbours are usually its same-topic siblings),
    //    so the LLM must see all K to find it. ──
    // guide index -> Vec<(older_global_idx, Vec<newer_global_idx>)>
    let mut per_guide: std::collections::BTreeMap<usize, Vec<(usize, Vec<usize>)>> =
        std::collections::BTreeMap::new();
    let mut total_pairs = 0usize;
    // Per-older best similarity, used to cap a guide's confirm prompt to its strongest cases.
    let mut best_sim: std::collections::HashMap<usize, f32> = std::collections::HashMap::new();
    let debug_match = std::env::var("PC_CROSS_DEBUG").ok();
    for (i, _s) in statements.iter().enumerate() {
        let cands = retrieve_candidates(i, &statements, &embeddings, &token_sets, tau);
        if let Some(ref needle) = debug_match {
            if statements[i].text.to_lowercase().contains(&needle.to_lowercase()) {
                eprintln!("[DEBUG] older: {}", statements[i].text);
                eprintln!("[DEBUG]   {} candidates:", cands.len());
                for c in &cands {
                    eprintln!("[DEBUG]     sim={:.3} [{}] {}", c.similarity, statements[c.newer_idx].slug, &statements[c.newer_idx].text[..statements[c.newer_idx].text.len().min(90)]);
                }
            }
        }
        if cands.is_empty() {
            continue;
        }
        let top = cands.iter().map(|c| c.similarity).fold(0.0f32, f32::max);
        best_sim.insert(i, top);
        let newer: Vec<usize> = cands.iter().map(|c| c.newer_idx).collect();
        total_pairs += newer.len();
        let gi = stmt_loc[i].0;
        per_guide.entry(gi).or_default().push((i, newer));
    }
    // Bound each guide's confirm prompt: keep at most MAX_OLDERS_PER_GUIDE older statements,
    // prioritizing the strongest-similarity candidates (a lower tau widens recall but must not
    // blow the prompt for a large guide). The lexical-only matches (sim possibly < tau) are
    // kept too — their best_sim reflects their actual cosine, so a strong entity match with
    // modest cosine still competes fairly.
    const MAX_OLDERS_PER_GUIDE: usize = 40;
    for olders in per_guide.values_mut() {
        if olders.len() > MAX_OLDERS_PER_GUIDE {
            olders.sort_by(|a, b| {
                let sa = best_sim.get(&a.0).copied().unwrap_or(0.0);
                let sb = best_sim.get(&b.0).copied().unwrap_or(0.0);
                sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
            });
            olders.truncate(MAX_OLDERS_PER_GUIDE);
        }
    }
    println!(
        "  {} candidate pair(s) across {} guide(s) passed the cosine+date gate",
        total_pairs,
        per_guide.len()
    );

    // ── CONFIRM (one LLM call per guide) + plan revisions. ──
    let llm = LlmClient {
        spec: ModelSpec::parse(&cfg.capture_model),
        openrouter_api_key: cfg.openrouter_api_key.clone().unwrap_or_default(),
        ollama_base_url: cfg.ollama_base_url.clone(),
        ollama_api_key: cfg.ollama_api_key.clone(),
    };

    let mut planned: Vec<PlannedRevision> = Vec::new();
    for (gi, olders) in &per_guide {
        let slug = &guides[*gi].1.frontmatter.slug;
        let prompt = confirm_prompt(olders, &statements);
        let raw = match llm.call(CONFIRM_SYSTEM, &prompt) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("  confirm call failed for {slug} ({e}); skipping guide");
                continue;
            }
        };
        // parse_confirm returns (older_local_idx, chosen_newer_global_idx, terminal_truth).
        for (older_local, newer_idx, terminal) in parse_confirm(&raw, olders) {
            let older_idx = olders[older_local].0;
            let older = &statements[older_idx];
            let newer = &statements[newer_idx];
            let new_text = build_revision(&older.text, &terminal, &newer.slug);
            planned.push(PlannedRevision {
                slug: older.slug.clone(),
                path: guides[*gi].0.clone(),
                start: older.start,
                end: older.end,
                old_text: older.text.clone(),
                new_text,
                newer_slug: newer.slug.clone(),
            });
        }
    }

    println!("\n=== {} confirmed cross-guide supersession(s) ===", planned.len());
    for (k, r) in planned.iter().enumerate() {
        println!("\n[{}] {} ({}..{}) — superseded by {}", k + 1, r.slug, r.start, r.end, r.newer_slug);
        println!("    OLD: {}", strip_inline_markers(&r.old_text).trim());
        println!("    NEW: {}", r.new_text);
    }

    if args.dry_run {
        println!("\n--dry-run: no files written.");
        return Ok(());
    }

    // ── APPLY: group planned revisions by guide path, edit spans, stamp_updated, save. ──
    let today = today_str();
    let mut by_path: std::collections::BTreeMap<PathBuf, Vec<(usize, usize, String)>> =
        std::collections::BTreeMap::new();
    for r in &planned {
        by_path
            .entry(r.path.clone())
            .or_default()
            .push((r.start, r.end, r.new_text.clone()));
    }
    let mut written = 0usize;
    for (gi, (path, guide)) in guides.iter().enumerate() {
        let _ = gi;
        if let Some(edits) = by_path.get(path) {
            let mut g = guide.clone();
            g.body = apply_revisions(&g.body, edits.clone());
            crate::capture::stamp_updated_pub(&mut g.frontmatter, &today);
            wiki::save_guide(path, &g).with_context(|| format!("save {}", path.display()))?;
            written += 1;
        }
    }
    // Rebuild the index so summaries/dates surface.
    let _ = wiki::rebuild_index(&wiki, &today);
    println!("\napplied {} revision(s) across {} guide(s); index rebuilt", planned.len(), written);
    Ok(())
}

// ─── Shared infra (LLM client + helpers, mirroring doctor.rs) ─────────────────

struct LlmClient {
    spec: ModelSpec,
    openrouter_api_key: String,
    ollama_base_url: String,
    ollama_api_key: Option<String>,
}

impl LlmClient {
    fn call(&self, system: &str, user: &str) -> Result<String> {
        crate::capture::call_model_blocking_with_timeout(
            &self.spec,
            &self.openrouter_api_key,
            &self.ollama_base_url,
            self.ollama_api_key.as_deref(),
            system,
            user,
            600,
        )
    }
}

fn list_guide_paths(wiki: &Path) -> Vec<PathBuf> {
    crate::wiki::guide_files(wiki)
}

fn today_str() -> String {
    let now = crate::capture::rfc3339_now();
    now[..now.len().min(10)].to_string()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_statements_extracts_paragraphs_skips_headings_and_comments() {
        let body = "# Title\n\n## Section\n\nFirst fact about the system here.\n\nSecond fact mentioning detail. [^a-1]\n\n<!-- citations: [^a-1] -->\n\n## See Also\n\n- [[other|Other]]\n";
        let stmts = split_statements(body);
        let texts: Vec<String> = stmts.iter().map(|(_, _, t)| t.clone()).collect();
        assert_eq!(texts.len(), 2, "got: {:?}", texts);
        assert!(texts[0].starts_with("First fact"));
        assert!(texts[1].starts_with("Second fact"));
        // Spans must point at the real text.
        for (s, e, t) in &stmts {
            assert_eq!(&body[*s..*e], t);
        }
        // See-Also link and comment excluded.
        assert!(!texts.iter().any(|t| t.contains("[[other")));
        assert!(!texts.iter().any(|t| t.contains("<!--")));
    }

    #[test]
    fn split_statements_groups_multiline_paragraph() {
        let body = "## S\n\nLine one of a fact\nstill the same fact.\n\nAnother fact entirely.\n";
        let stmts = split_statements(body);
        assert_eq!(stmts.len(), 2);
        assert!(stmts[0].2.contains("Line one") && stmts[0].2.contains("same fact"));
    }

    #[test]
    fn split_statements_isolates_sentences_in_dense_paragraph() {
        // The real failure mode: a dense paragraph that buries the load-bearing closure fact
        // among unrelated sentences. Sentence-splitting must surface it as its own unit so its
        // embedding isn't diluted (this is what let the nip17 cold-start match its closure).
        let body = "## Action Ledger\n\nThe M6 action ledger has PublishAction shapes but no ULID crate and no restart recovery. V-18 fixed PublishOutcome::FailedAfterRetries having no toast. The kind:10050 planner trigger was missing in production (zero callers before #1080), causing fresh accounts to never receive DMs.\n";
        let stmts = split_statements(body);
        // The closure sentence must be its own statement.
        let closure = stmts.iter().find(|(_, _, t)| t.contains("kind:10050 planner trigger"));
        assert!(closure.is_some(), "closure sentence must be isolated: {:?}", stmts.iter().map(|s| &s.2).collect::<Vec<_>>());
        let (s, e, t) = closure.unwrap();
        // Span fidelity + isolation: it must NOT contain the unrelated ULID sentence.
        assert_eq!(&body[*s..*e], t);
        assert!(!t.contains("ULID"), "closure unit must not absorb neighbors: {t}");
        assert!(t.contains("fresh accounts to never receive DMs"));
    }

    #[test]
    fn split_sentences_keeps_byte_spans_exact() {
        let body = "## X\n\nFirst sentence here ok. Second sentence about cold-start verified now.\n";
        let stmts = split_statements(body);
        for (s, e, t) in &stmts {
            assert_eq!(&body[*s..*e], t, "span must reproduce text exactly");
        }
        assert!(stmts.len() >= 2, "two sentences expected: {:?}", stmts);
    }

    #[test]
    fn is_strictly_newer_requires_later_day() {
        assert!(is_strictly_newer("2026-06-12", "2026-05-26"));
        assert!(!is_strictly_newer("2026-05-26", "2026-05-26")); // same day
        assert!(!is_strictly_newer("2026-05-26", "2026-06-12")); // older
        assert!(!is_strictly_newer("", "2026-06-12")); // missing
    }

    fn stmt(slug: &str, date: &str, text: &str) -> Statement {
        Statement { slug: slug.into(), title: slug.into(), date: date.into(), text: text.into(), start: 0, end: text.len() }
    }

    fn toks(statements: &[Statement]) -> Vec<std::collections::HashSet<String>> {
        statements.iter().map(|s| rare_tokens(&s.text)).collect()
    }

    #[test]
    fn retrieve_only_returns_newer_other_guide_above_tau() {
        let statements = vec![
            stmt("a", "2026-05-26", "DM receive-side cold-start is unverified"), // 0 (the stale one)
            stmt("a", "2026-05-26", "DM receive-side cold-start is unverified again"), // 1 same guide → excluded
            stmt("b", "2026-06-12", "DM cold-start receive path was verified and closed"), // 2 newer, other guide
            stmt("c", "2026-05-20", "DM cold-start receive path older note"), // 3 OLDER → excluded
        ];
        // Embeddings: make 0,2,3 similar; 1 identical to 0 but same guide.
        let embeddings = vec![
            vec![1.0, 0.0, 0.0],
            vec![1.0, 0.0, 0.0],
            vec![0.98, 0.2, 0.0],
            vec![0.97, 0.0, 0.2],
        ];
        let tset = toks(&statements);
        let cands = retrieve_candidates(0, &statements, &embeddings, &tset, 0.45);
        // idx 2 qualifies (newer + other guide + above tau). idx1 same guide, idx3 older.
        assert!(cands.iter().any(|c| c.newer_idx == 2));
        assert!(!cands.iter().any(|c| c.newer_idx == 1 || c.newer_idx == 3));
    }

    #[test]
    fn lexical_channel_catches_entity_linked_low_cosine_pair() {
        // The real cold-start case: the stale statement and its closure embed at only ~0.31
        // (below tau), but share the rare token `cold-start`. The lexical channel must admit
        // the closure even though cosine alone would not.
        let statements = vec![
            stmt("nip17", "2026-05-26", "NIP-17 DM receive-side cold-start is unverified."), // 0
            stmt("ledger", "2026-06-12", "The kind:10050 planner trigger was missing before #1080, so cold-start receive never fired."), // 1 newer, shares cold-start + kind:10050
            stmt("other", "2026-06-12", "Completely unrelated note about button colors."), // 2 newer, no shared rare token, low cosine
        ];
        // Low cosine across the board (orthogonal-ish) so ONLY the lexical channel can fire.
        let embeddings = vec![
            vec![1.0, 0.0, 0.0],
            vec![0.0, 1.0, 0.0], // cosine(0,1) = 0 → below tau
            vec![0.0, 0.0, 1.0], // cosine(0,2) = 0
        ];
        let tset = toks(&statements);
        let cands = retrieve_candidates(0, &statements, &embeddings, &tset, 0.45);
        assert!(cands.iter().any(|c| c.newer_idx == 1), "lexical channel must admit the closure: {:?}", cands.iter().map(|c| c.newer_idx).collect::<Vec<_>>());
        assert!(!cands.iter().any(|c| c.newer_idx == 2), "unrelated low-cosine no-shared-token statement must NOT be admitted");
    }

    #[test]
    fn strip_markers_and_rare_tokens_are_utf8_safe() {
        // Real guides contain → ≤ etc.; the old byte-cast mangled them and could panic.
        let s = "stage 1 → stage 2 ≤ done [^abc-1] with kind:10050 and cold-start.";
        let stripped = strip_inline_markers(s);
        assert!(stripped.contains('→') && stripped.contains('≤'), "multibyte must survive: {stripped}");
        assert!(!stripped.contains("[^abc-1]"));
        let t = rare_tokens(s); // must not panic on multibyte input
        assert!(t.contains("kind:10050"));
        assert!(t.contains("cold-start"));
    }


    #[test]
    fn negative_status_detection_and_content_channel() {
        assert!(is_negative_status("NIP-17 DM receive-side cold-start is unverified."));
        assert!(is_negative_status("The ledger is scaffold-only with no restart recovery."));
        assert!(is_negative_status("kind:10050 publish UI has zero Swift callers."));
        assert!(!is_negative_status("The default embedder is local MiniLM."));

        // Content channel: a negative-status older statement reaches a low-cosine, no-shared-
        // rare-token closure that shares domain words.
        let statements = vec![
            stmt("nip17", "2026-05-26", "NIP-17 DM receive-side cold-start is unverified."), // 0 neg-status
            stmt("ledger", "2026-06-12", "Fresh accounts could never receive DMs at cold-start until the planner trigger landed; receive now works."), // 1: shares receive/accounts/cold/start/dms words, low cosine, no rare token
        ];
        let embeddings = vec![vec![1.0, 0.0], vec![0.0, 1.0]]; // cosine 0 → only content channel can fire
        let tset = toks(&statements);
        let cands = retrieve_candidates(0, &statements, &embeddings, &tset, 0.45);
        assert!(cands.iter().any(|c| c.newer_idx == 1), "neg-status content channel must reach the closure: {:?}", cands.len());
    }

    #[test]
    fn rare_tokens_extracts_entity_anchors_not_plain_words() {
        let t = rare_tokens("NIP-17 DM receive-side cold-start is unverified before #1080 via kind:10050 and swap_dm_inbox_observer.");
        assert!(t.contains("cold-start"), "{:?}", t);
        assert!(t.contains("receive-side"));
        assert!(t.contains("#1080"));
        assert!(t.contains("kind:10050"));
        assert!(t.contains("swap_dm_inbox_observer"));
        // plain words must NOT be rare tokens
        assert!(!t.contains("unverified"));
        assert!(!t.contains("before"));
    }

    #[test]
    fn build_revision_preserves_old_text_with_breadcrumb_and_citation() {
        let r = build_revision(
            "NIP-17 DM receive-side cold-start is unverified.",
            "NIP-17 DM receive-side cold-start is verified and closed via #1080",
            "publish-action-ledger",
        );
        assert!(r.contains("verified and closed via #1080"));
        assert!(r.contains("(Previously: NIP-17 DM receive-side cold-start is unverified, superseded — see publish-action-ledger.)"), "got: {r}");
    }

    #[test]
    fn apply_revisions_replaces_spans_right_to_left() {
        let body = "AAAA BBBB CCCC";
        // Replace "BBBB" (5..9) and "CCCC" (10..14); order must not corrupt offsets.
        let edits = vec![
            (5usize, 9usize, "bb".to_string()),
            (10usize, 14usize, "cc".to_string()),
        ];
        let out = apply_revisions(body, edits);
        assert_eq!(out, "AAAA bb cc");
    }

    #[test]
    fn parse_confirm_validates_older_and_candidate_indices() {
        // olders[0] has candidates [global 5, global 7]; olders[1] has [global 9].
        let olders = vec![(0usize, vec![5usize, 7usize]), (1usize, vec![9usize])];
        let raw = "Here: [\
{\"older\": 0, \"candidate\": 1, \"terminal_truth\": \"X is closed\"}, \
{\"older\": 0, \"candidate\": 9, \"terminal_truth\": \"bad candidate idx\"}, \
{\"older\": 5, \"candidate\": 0, \"terminal_truth\": \"bad older idx\"}, \
{\"older\": 1, \"candidate\": 0, \"terminal_truth\": \"\"}]";
        let got = parse_confirm(raw, &olders);
        // Only the first is valid; it maps to candidate global index 7.
        assert_eq!(got.len(), 1, "got: {:?}", got);
        assert_eq!(got[0].0, 0); // older local
        assert_eq!(got[0].1, 7); // chosen newer global
        assert_eq!(got[0].2, "X is closed");
    }


    #[test]
    fn confirm_prompt_lists_candidates_and_strips_markers() {
        let statements = vec![
            stmt("a", "2026-05-26", "old fact is unverified [^a-1]"),
            stmt("b", "2026-06-12", "new fact verified it [^b-2]"),
            stmt("c", "2026-06-12", "another candidate fact [^c-3]"),
        ];
        let p = confirm_prompt(&[(0, vec![1, 2])], &statements);
        assert!(p.contains("OLDER 0"));
        assert!(p.contains("CANDIDATE 0"));
        assert!(p.contains("CANDIDATE 1"));
        assert!(p.contains("old fact is unverified"));
        assert!(p.contains("new fact verified it"));
        assert!(!p.contains("[^a-1]") && !p.contains("[^b-2]"), "markers must be stripped");
        assert!(p.contains("terminal_truth"));
    }
}
