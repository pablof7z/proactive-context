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
/// merge tau (0.65) because we compare individual STATEMENTS (shorter, noisier) and the LLM
/// confirm is the real precision gate. Overridable via `--tau`.
pub const DEFAULT_CROSS_TAU: f32 = 0.45;

/// Top-K most-similar newer statements retrieved per statement.
const TOP_K: usize = 5;

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
            // Trim trailing whitespace bytes from the span.
            let mut e = end;
            while e > s && (bytes[e - 1] == b'\n' || bytes[e - 1] == b' ' || bytes[e - 1] == b'\t') {
                e -= 1;
            }
            if e > s {
                let text = body[s..e].to_string();
                if is_prose_statement(&text) {
                    out.push((s, e, text));
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

/// Is a paragraph substantive prose worth comparing? Rejects empties and pure-marker text.
fn is_prose_statement(text: &str) -> bool {
    let stripped = strip_inline_markers(text);
    let s = stripped.trim();
    // Need at least a few words of real content.
    s.split_whitespace().count() >= 3
}

/// Remove inline `[^id]` citation markers from a string (for embedding / prose checks).
fn strip_inline_markers(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' && i + 1 < bytes.len() && bytes[i + 1] == b'^' {
            if let Some(close) = s[i..].find(']') {
                i += close + 1;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
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

/// For statement `i`, retrieve up to TOP_K most-similar statements from OTHER guides whose
/// owning guide is strictly newer, with cosine ≥ `tau`. Returns most-similar-first.
pub fn retrieve_candidates(
    i: usize,
    statements: &[Statement],
    embeddings: &[Vec<f32>],
    tau: f32,
) -> Vec<Candidate> {
    let me = &statements[i];
    let my_emb = &embeddings[i];
    let mut scored: Vec<Candidate> = Vec::new();
    for (j, other) in statements.iter().enumerate() {
        if j == i || other.slug == me.slug {
            continue; // never compare within the same guide
        }
        if !is_strictly_newer(&other.date, &me.date) {
            continue; // the candidate must be NEWER than the (possibly stale) statement
        }
        let sim = cosine(my_emb, &embeddings[j]);
        if sim >= tau {
            scored.push(Candidate { newer_idx: j, similarity: sim });
        }
    }
    scored.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(TOP_K);
    scored
}

// ─── LLM confirm (batched per guide) ──────────────────────────────────────────

const CONFIRM_SYSTEM: &str = "You are a strict technical-knowledge auditor deciding whether a \
NEWER statement makes an OLDER statement STALE — i.e. the older statement is now FALSE, \
obsolete, or contradicted by the newer one. Be conservative: only flag genuine staleness, \
NOT statements that are merely related or on the same topic.\n\
NEGATIVE EXAMPLE (do NOT flag): older 'The API supports JSON output'; newer 'The API also \
supports YAML output' — this is ADDITIVE (a new capability), the older statement is still \
true. Only flag when the newer statement REVERSES, CLOSES, or INVALIDATES the older one \
(e.g. older 'X is unverified' + newer 'X was verified/closed by PR #N').";

/// Build the per-guide batch confirm prompt. Each pair is numbered so the model can answer
/// with a compact JSON array referencing pair indices.
fn confirm_prompt(pairs: &[(usize, usize)], statements: &[Statement]) -> String {
    let mut s = String::from(
        "For each numbered PAIR below, decide whether the NEWER statement makes the OLDER one \
stale (false/obsolete/contradicted). If stale, also write the corrected TERMINAL TRUTH — a \
single sentence stating what is now true, drawn from the newer statement.\n\n",
    );
    for (n, (older_idx, newer_idx)) in pairs.iter().enumerate() {
        let o = &statements[*older_idx];
        let nw = &statements[*newer_idx];
        s.push_str(&format!(
            "PAIR {n}:\n  OLDER (guide {oslug}, {odate}): {otext}\n  NEWER (guide {nslug}, {ndate}): {ntext}\n\n",
            n = n,
            oslug = o.slug,
            odate = o.date,
            otext = strip_inline_markers(&o.text).trim(),
            nslug = nw.slug,
            ndate = nw.date,
            ntext = strip_inline_markers(&nw.text).trim(),
        ));
    }
    s.push_str(
        "Output ONLY a JSON array, one object per STALE pair (omit non-stale pairs entirely):\n\
[{\"pair\": <n>, \"terminal_truth\": \"<one corrected sentence>\"}]\n\
If none are stale, output []. No prose outside the JSON.",
    );
    s
}

/// Parse the confirm response into (pair_index, terminal_truth) entries, keeping only valid
/// pair indices.
fn parse_confirm(raw: &str, n_pairs: usize) -> Vec<(usize, String)> {
    let json = extract_json_array(raw);
    let mut out = Vec::new();
    if let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(&json) {
        for it in items {
            let pair = it.get("pair").and_then(|v| v.as_u64()).map(|n| n as usize);
            let truth = it.get("terminal_truth").and_then(|v| v.as_str()).map(str::to_string);
            if let (Some(p), Some(t)) = (pair, truth) {
                if p < n_pairs && !t.trim().is_empty() {
                    out.push((p, t.trim().to_string()));
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

    // ── EMBED all statements once (local fastembed). ──
    let mut embedder = build_embedder(&cfg).context("build embedder")?;
    let texts: Vec<String> = statements.iter().map(|s| strip_inline_markers(&s.text)).collect();
    let embeddings = embedder.embed(&texts).context("embed statements")?;

    // ── RETRIEVE: per statement, gather newer similar candidates. Group by owning guide
    //    so we can issue ONE confirm call per guide (frugal). ──
    // guide index -> Vec<(older_global_idx, newer_global_idx)>
    let mut per_guide_pairs: std::collections::BTreeMap<usize, Vec<(usize, usize)>> =
        std::collections::BTreeMap::new();
    for (i, _s) in statements.iter().enumerate() {
        let cands = retrieve_candidates(i, &statements, &embeddings, tau);
        if cands.is_empty() {
            continue;
        }
        let gi = stmt_loc[i].0;
        for c in cands {
            per_guide_pairs.entry(gi).or_default().push((i, c.newer_idx));
        }
    }
    let total_pairs: usize = per_guide_pairs.values().map(|v| v.len()).sum();
    println!(
        "  {} candidate pair(s) across {} guide(s) passed the cosine+date gate",
        total_pairs,
        per_guide_pairs.len()
    );

    // ── CONFIRM (one LLM call per guide) + plan revisions. ──
    let llm = LlmClient {
        spec: ModelSpec::parse(&cfg.capture_model),
        openrouter_api_key: cfg.openrouter_api_key.clone().unwrap_or_default(),
        ollama_base_url: cfg.ollama_base_url.clone(),
        ollama_api_key: cfg.ollama_api_key.clone(),
    };

    let mut planned: Vec<PlannedRevision> = Vec::new();
    for (gi, pairs) in &per_guide_pairs {
        // Deduplicate: keep at most one (best) newer candidate per older statement for the
        // confirm prompt — the highest-similarity pair is first because retrieve sorts, but
        // per_guide_pairs interleaves; dedup by older_idx keeping first occurrence.
        let mut seen_older = std::collections::HashSet::new();
        let deduped: Vec<(usize, usize)> = pairs
            .iter()
            .filter(|(o, _)| seen_older.insert(*o))
            .copied()
            .collect();

        let slug = &guides[*gi].1.frontmatter.slug;
        let prompt = confirm_prompt(&deduped, &statements);
        let raw = match llm.call(CONFIRM_SYSTEM, &prompt) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("  confirm call failed for {slug} ({e}); skipping guide");
                continue;
            }
        };
        let confirmed = parse_confirm(&raw, deduped.len());
        for (pair_n, terminal) in confirmed {
            let (older_idx, newer_idx) = deduped[pair_n];
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
    let mut v = Vec::new();
    if let Ok(rd) = std::fs::read_dir(wiki) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("md") {
                continue;
            }
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if stem.starts_with('_') {
                continue;
            }
            v.push(p);
        }
    }
    v.sort();
    v
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
    fn is_strictly_newer_requires_later_day() {
        assert!(is_strictly_newer("2026-06-12", "2026-05-26"));
        assert!(!is_strictly_newer("2026-05-26", "2026-05-26")); // same day
        assert!(!is_strictly_newer("2026-05-26", "2026-06-12")); // older
        assert!(!is_strictly_newer("", "2026-06-12")); // missing
    }

    fn stmt(slug: &str, date: &str, text: &str) -> Statement {
        Statement { slug: slug.into(), title: slug.into(), date: date.into(), text: text.into(), start: 0, end: text.len() }
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
        let cands = retrieve_candidates(0, &statements, &embeddings, 0.45);
        // Only idx 2 qualifies (newer + other guide + above tau). idx1 same guide, idx3 older.
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].newer_idx, 2);
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
    fn parse_confirm_keeps_valid_pairs_only() {
        let raw = "Here: [{\"pair\": 0, \"terminal_truth\": \"X is closed\"}, {\"pair\": 9, \"terminal_truth\": \"out of range\"}, {\"pair\": 1, \"terminal_truth\": \"\"}]";
        let got = parse_confirm(raw, 2);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, 0);
        assert_eq!(got[0].1, "X is closed");
    }

    #[test]
    fn confirm_prompt_contains_pairs_and_strips_markers() {
        let statements = vec![
            stmt("a", "2026-05-26", "old fact is unverified [^a-1]"),
            stmt("b", "2026-06-12", "new fact verified it [^b-2]"),
        ];
        let p = confirm_prompt(&[(0, 1)], &statements);
        assert!(p.contains("PAIR 0"));
        assert!(p.contains("old fact is unverified"));
        assert!(!p.contains("[^a-1]"), "markers must be stripped from the prompt");
        assert!(p.contains("terminal_truth"));
    }
}
