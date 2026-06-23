//! doctor.rs — `pc wiki doctor`: periodic, off-hot-path wiki consolidation/compaction.
//!
//! ## Why this exists
//! The capture pipeline (EXTRACT/AUTHORITY/ROUTE/RECONCILE) is tuned to admit knowledge
//! safely, but over many sessions the wiki accretes near-duplicate guides for one topic
//! (e.g. ~8 `agent-awareness` guides, ~14 `capture-redesign` guides) and within-guide
//! contradictions. ROUTE's retrieve-then-rerank stops NEW near-dups forming going forward,
//! but it can't retroactively heal what already accreted. `wiki doctor` is the periodic
//! repair pass: it finds same-topic clusters and LLM-merges each into one canonical guide.
//!
//! ## Discipline: embeddings propose, LLM confirms, code guarantees integrity
//!   - DETECT (cheap, deterministic): embed each guide's (title+summary) repr — the SAME
//!     representation ROUTE recall uses — and cluster guides whose pairwise cosine exceeds
//!     `tau`. This PROPOSES candidate clusters; it never decides alone.
//!   - CONFIRM (one cheap LLM call/cluster): "are these the same topic that should be one
//!     guide? YES/NO". Mirrors ROUTE's rerank — embeddings shortlist, the model adjudicates.
//!     Guards against tau accidentally yoking distinct sub-concerns (inject-gate vs
//!     inject-compile) that happen to score high.
//!   - MERGE (one LLM call/confirmed cluster): fold the guides into ONE; pick canonical
//!     slug/title from the most-recently-updated guide; drop duplicate/contradictory
//!     statements keeping the live one.
//!   - PRESERVE (deterministic code, NOT LLM trust): every distinct `[^id]` citation marker
//!     in the source guides MUST survive into the merged guide. We compute the source set,
//!     check the merged output, and APPEND any markers the LLM dropped — citation loss is
//!     impossible by construction, not merely reported.
//!
//! ## Safety
//! Default is dry-run: it reads the LIVE wiki read-only and WRITES the proposed consolidated
//! wiki to `--output-dir` (the complete wiki — every unmerged guide copied through, each
//! cluster replaced by its merge — so counts and citation totals are meaningful). It NEVER
//! touches the real `docs/wiki/` unless `--apply`. Mirrors the archeologist `--output-dir`
//! safety pattern.
//!
//! TODO(cascade): orphan / cascade-gap detection is OUT of v1. It should reuse this same
//! embedding-recall machinery to find statements *adjacent* to a superseded claim (the
//! statements that silently went stale when their neighbor was overturned). True cascade
//! detection wants the event-log as its destination so gaps surface as events, not a batch
//! report.

use crate::config::Config;
use crate::embed::{build_embedder, Embedder};
use crate::provider::ModelSpec;
use crate::route_recall::cosine;
use crate::wiki::{
    self, enforce_bidirectional_links, read_index_live, wiki_dir, Guide,
    IndexRow,
};
use anyhow::{Context, Result};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Default similarity threshold for clustering. Empirically tuned on the live 160-guide wiki:
/// tau ≤ 0.60 chains capture+wiki+archeologist into a 17–36-guide poison blob (single-linkage
/// transitivity); tau = 0.65 breaks it into topically-coherent sub-clusters (capture-accretion,
/// capture-redesign, agent-awareness×5, wiki-tools×4) while keeping the must-not-merge pair
/// inject-gate vs inject-compile in SEPARATE clusters; tau = 0.70 under-merges (agent-awareness
/// fragments to two disjoint pairs). Overridable via `PC_DOCTOR_TAU` or `--tau`.
pub const DEFAULT_TAU: f32 = 0.65;

pub struct DoctorArgs {
    /// Where to write the proposed consolidated wiki (dry-run). If None and !apply, a temp
    /// dir is chosen and printed.
    pub output_dir: Option<PathBuf>,
    /// If true, write the consolidation in-place to the real wiki. v1: do NOT use on the
    /// real wiki from a worktree (it writes the MAIN repo's docs/wiki).
    pub apply: bool,
    /// Skip the LLM confirm + merge steps; only detect + print clusters. For tau tuning.
    pub detect_only: bool,
    /// Override clustering threshold (else PC_DOCTOR_TAU env, else DEFAULT_TAU).
    pub tau: Option<f32>,
    /// Topic-taxonomy mode: instead of merging near-dups, assign every guide a coherent
    /// `topic` (one LLM pass over the whole catalog) and stamp it into frontmatter. This
    /// GROUPS the flat wiki without merging (lossless — bodies/citations untouched).
    /// Dry-run prints the proposed taxonomy; with --apply it rewrites the `topic` field.
    pub retopic: bool,
    /// Override the model for the (one-shot) retopic taxonomy call, e.g.
    /// `ollama:glm-5.1:cloud`. Defaults to `capture_model`. Useful when the configured
    /// capture model is a slow local model unfit for a large single-call taxonomy.
    pub model: Option<String>,
}

// ─── Detection (deterministic, unit-testable) ──────────────────────────────────

/// The text we embed to represent a guide — title + summary, identical to ROUTE recall's
/// `guide_repr`, so doctor clusters on the same signal the router routes on.
fn guide_repr(row: &IndexRow) -> String {
    let summary = row.summary.trim();
    if summary.is_empty() {
        row.title.trim().to_string()
    } else {
        format!("{}. {}", row.title.trim(), summary)
    }
}

/// A proposed cluster of guide slugs that DETECT thinks are the same topic.
#[derive(Debug, Clone, PartialEq)]
pub struct Cluster {
    pub slugs: Vec<String>,
    /// Lowest pairwise cosine within the cluster (cluster cohesion floor).
    pub min_pair: f32,
}

fn uf_find(parent: &mut [usize], x: usize) -> usize {
    let mut r = x;
    while parent[r] != r {
        r = parent[r];
    }
    let mut c = x;
    while parent[c] != r {
        let next = parent[c];
        parent[c] = r;
        c = next;
    }
    r
}

/// Single-linkage agglomerative clustering over pairwise cosine: any two guides with
/// similarity >= tau are linked into the same cluster (union-find). Single-linkage is
/// intentional — a topic guide can drift in wording across its near-dups, so we chain via
/// transitive similarity rather than demanding all-pairs cohesion. CONFIRM is the brake that
/// stops a chain from over-merging distinct concerns.
///
/// Returns only multi-guide clusters (singletons are not consolidation candidates), sorted
/// largest-first for stable reporting.
pub fn cluster_by_cosine(embs: &[Vec<f32>], slugs: &[String], tau: f32) -> Vec<Cluster> {
    let n = slugs.len();
    let mut parent: Vec<usize> = (0..n).collect();
    let mut sims: HashMap<(usize, usize), f32> = HashMap::new();
    for i in 0..n {
        for j in (i + 1)..n {
            let s = cosine(&embs[i], &embs[j]);
            if s >= tau {
                let (a, b) = (uf_find(&mut parent, i), uf_find(&mut parent, j));
                if a != b {
                    parent[a] = b;
                }
            }
            sims.insert((i, j), s);
        }
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        let r = uf_find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    let mut clusters: Vec<Cluster> = Vec::new();
    for (_root, members) in groups {
        if members.len() < 2 {
            continue;
        }
        let mut min_pair = 1.0f32;
        for a in 0..members.len() {
            for b in (a + 1)..members.len() {
                let (i, j) = (members[a].min(members[b]), members[a].max(members[b]));
                if let Some(s) = sims.get(&(i, j)) {
                    if *s < min_pair {
                        min_pair = *s;
                    }
                }
            }
        }
        let mut cl_slugs: Vec<String> = members.iter().map(|&i| slugs[i].clone()).collect();
        cl_slugs.sort();
        clusters.push(Cluster { slugs: cl_slugs, min_pair });
    }
    clusters.sort_by(|a, b| b.slugs.len().cmp(&a.slugs.len()).then(a.slugs[0].cmp(&b.slugs[0])));
    clusters
}

/// Detect candidate clusters from the live wiki rows (embeds in one batch).
pub fn detect_clusters(
    embedder: &mut dyn Embedder,
    rows: &[IndexRow],
    tau: f32,
) -> Result<Vec<Cluster>> {
    if rows.len() < 2 {
        return Ok(Vec::new());
    }
    let texts: Vec<String> = rows.iter().map(guide_repr).collect();
    let embs = embedder.embed(&texts)?;
    let slugs: Vec<String> = rows.iter().map(|r| r.slug.clone()).collect();
    Ok(cluster_by_cosine(&embs, &slugs, tau))
}

// ─── Citation extraction (deterministic) ────────────────────────────────────────

/// Collect distinct `[^id]` citation markers from a body. The id charset matches the
/// capture pipeline's `<session-prefix>-<n>` (alphanumeric, dash, underscore).
pub fn citation_markers(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for_each_marker(text, |m| {
        out.insert(m.to_string());
    });
    out
}

fn raw_marker_count(text: &str) -> usize {
    let mut n = 0;
    for_each_marker(text, |_| n += 1);
    n
}

/// Scan `text` for `[^id]` markers, invoking `f` on each (including duplicates).
fn for_each_marker<F: FnMut(&str)>(text: &str, mut f: F) {
    let b = text.as_bytes();
    let mut i = 0;
    while i + 1 < b.len() {
        if b[i] == b'[' && b[i + 1] == b'^' {
            let mut j = i + 2;
            while j < b.len() {
                let c = b[j] as char;
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    j += 1;
                } else {
                    break;
                }
            }
            if j < b.len() && b[j] == b']' && j > i + 2 {
                f(&text[i..=j]);
            }
            i = j;
        } else {
            i += 1;
        }
    }
}

// ─── Merge (LLM) ────────────────────────────────────────────────────────────────

/// Pick the canonical guide for a cluster: the most-recently updated/verified, then by
/// longest body (richest), then by slug. Returns an index into `guides`.
fn pick_canonical(guides: &[Guide]) -> usize {
    let mut best = 0usize;
    for i in 1..guides.len() {
        let a = &guides[best].frontmatter;
        let b = &guides[i].frontmatter;
        let a_key = (a.updated.as_str(), a.verified.as_str());
        let b_key = (b.updated.as_str(), b.verified.as_str());
        let better = b_key > a_key
            || (b_key == a_key && guides[i].body.len() > guides[best].body.len())
            || (b_key == a_key
                && guides[i].body.len() == guides[best].body.len()
                && b.slug < a.slug);
        if better {
            best = i;
        }
    }
    best
}

fn merge_prompt(guides: &[Guide], canonical_slug: &str, canonical_title: &str) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "You are consolidating {} near-duplicate wiki guides about the SAME topic into ONE.\n\n",
        guides.len()
    ));
    s.push_str(&format!(
        "Canonical slug: {}\nCanonical title: {}\n\n",
        canonical_slug, canonical_title
    ));
    s.push_str(
        "RULES:\n\
         - Output ONLY the merged guide BODY in Markdown (no YAML frontmatter, no code fences wrapping the whole output).\n\
         - Start with a `# Title` heading then a one-line `> summary` blockquote.\n\
         - Merge all sections; collapse duplicate sections; when two statements contradict, KEEP the one from the most recently updated guide and drop the stale one.\n\
         - PRESERVE EVERY citation marker of the form [^id] exactly as written. Do NOT invent new markers. Do NOT drop markers. Attach each marker to the statement it supports, as in the sources.\n\
         - Keep a `## See Also` section if any source had one; merge its links and de-duplicate.\n\
         - Write dense, coherent prose. No meta-commentary about the merge.\n\n",
    );
    for (i, g) in guides.iter().enumerate() {
        s.push_str(&format!(
            "===== SOURCE GUIDE {} (slug: {}, updated: {}) =====\n",
            i + 1,
            g.frontmatter.slug,
            g.frontmatter.updated
        ));
        s.push_str(g.body.trim());
        s.push_str("\n\n");
    }
    s
}

fn confirm_prompt(guides: &[Guide]) -> String {
    let mut s = String::from(
        "Below are several wiki guides. Are they all about the SAME topic such that they \
         SHOULD be consolidated into ONE guide? Answer with a single word: YES or NO.\n\n",
    );
    for (i, g) in guides.iter().enumerate() {
        s.push_str(&format!(
            "--- GUIDE {} ({}): {} ---\n{}\n\n",
            i + 1,
            g.frontmatter.slug,
            g.frontmatter.title,
            g.frontmatter.summary
        ));
    }
    s
}

/// Model client bundle so detection logic stays testable without a network.
pub struct LlmClient {
    pub spec: ModelSpec,
    pub openrouter_api_key: String,
    pub ollama_base_url: String,
    pub ollama_api_key: Option<String>,
}

impl LlmClient {
    fn call(&self, system: &str, user: &str) -> Result<String> {
        // Doctor calls are off-hot-path batch jobs (whole-catalog taxonomy / cluster merge)
        // with large prompts + large structured outputs; a slow local model can take minutes.
        // Use a generous timeout rather than the 120s hot-path default.
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

pub struct MergeResult {
    pub canonical_slug: String,
    pub merged: Guide,
    pub absorbed_slugs: Vec<String>,
    pub src_markers: BTreeSet<String>,
    pub out_markers: BTreeSet<String>,
    pub carried_forward: Vec<String>,
}

/// Merge one confirmed cluster's guides into a single Guide, enforcing citation preservation.
fn merge_cluster(llm: &LlmClient, guides: &[Guide], today: &str) -> Result<MergeResult> {
    let canon_idx = pick_canonical(guides);
    let canon_fm = guides[canon_idx].frontmatter.clone();
    let canonical_slug = canon_fm.slug.clone();
    let canonical_title = canon_fm.title.clone();

    let mut src_markers: BTreeSet<String> = BTreeSet::new();
    for g in guides {
        src_markers.extend(citation_markers(&g.body));
    }

    let system = "You are a meticulous technical editor consolidating engineering wiki guides. \
                  You never drop or invent citation markers.";
    let user = merge_prompt(guides, &canonical_slug, &canonical_title);
    let mut body = strip_code_fence(&llm.call(system, &user)?);

    // CITATION PRESERVATION — deterministic guarantee.
    let mut out_markers = citation_markers(&body);
    let missing: Vec<String> = src_markers.difference(&out_markers).cloned().collect();
    let mut carried_forward = Vec::new();
    if !missing.is_empty() {
        body.push_str("\n\n## Citations Carried Forward\n\n");
        body.push_str(
            "> Markers preserved from consolidated source guides whose statements were \
             merged or deduplicated.\n\n",
        );
        for m in &missing {
            body.push_str(m);
            body.push(' ');
        }
        body.push('\n');
        carried_forward = missing.clone();
        out_markers = citation_markers(&body);
    }

    let mut fm = canon_fm;
    let mut tag_set: BTreeSet<String> = fm.tags.iter().cloned().collect();
    let mut src_set: BTreeSet<String> = fm.sources.iter().cloned().collect();
    let mut min_created = fm.created.clone();
    for g in guides {
        for t in &g.frontmatter.tags {
            tag_set.insert(t.clone());
        }
        for sc in &g.frontmatter.sources {
            src_set.insert(sc.clone());
        }
        if !g.frontmatter.created.is_empty()
            && (min_created.is_empty() || g.frontmatter.created < min_created)
        {
            min_created = g.frontmatter.created.clone();
        }
    }
    fm.tags = tag_set.into_iter().collect();
    fm.sources = src_set.into_iter().collect();
    fm.created = min_created;
    fm.updated = today.to_string();
    fm.verified = today.to_string();

    let absorbed_slugs: Vec<String> = guides
        .iter()
        .map(|g| g.frontmatter.slug.clone())
        .filter(|s| s != &canonical_slug)
        .collect();

    Ok(MergeResult {
        canonical_slug,
        merged: Guide { frontmatter: fm, body },
        absorbed_slugs,
        src_markers,
        out_markers,
        carried_forward,
    })
}

fn strip_code_fence(s: &str) -> String {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix("```markdown") {
        return rest.trim_end_matches("```").trim().to_string();
    }
    if let Some(rest) = t.strip_prefix("```md") {
        return rest.trim_end_matches("```").trim().to_string();
    }
    if t.starts_with("```") {
        let inner = t.trim_start_matches("```");
        let inner = inner.splitn(2, '\n').nth(1).unwrap_or(inner);
        return inner.trim_end_matches("```").trim().to_string();
    }
    t.to_string()
}

// ─── See-Also remap (deterministic) ─────────────────────────────────────────────

/// Rewrite See-Also references to absorbed slugs across a guide body, pointing them at the
/// canonical slug. Operates on the common link forms.
fn remap_see_also(body: &str, map: &HashMap<String, String>) -> String {
    let mut out = body.to_string();
    for (from, to) in map {
        if from == to {
            continue;
        }
        out = out.replace(&format!("[[{}]]", from), &format!("[[{}]]", to));
        out = out.replace(&format!("[[{}|", from), &format!("[[{}|", to));
        out = out.replace(&format!("]({}.md)", from), &format!("]({}.md)", to));
        out = out.replace(&format!("](../{}.md)", from), &format!("](../{}.md)", to));
    }
    out
}

// ─── Report ──────────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct DoctorReport {
    pub input_guides: usize,
    pub output_guides: usize,
    pub clusters_detected: usize,
    pub clusters_confirmed: usize,
    pub merges: Vec<MergeSummary>,
    pub failed_merges: Vec<String>,
    pub input_distinct_markers: usize,
    pub output_distinct_markers: usize,
    pub input_raw_markers: usize,
    pub output_raw_markers: usize,
}

pub struct MergeSummary {
    pub canonical_slug: String,
    pub absorbed: Vec<String>,
    pub src_distinct: usize,
    pub out_distinct: usize,
    pub carried_forward: usize,
}

// ─── Orchestration ────────────────────────────────────────────────────────────────

pub fn run_doctor(root: &Path, args: DoctorArgs) -> Result<()> {
    let cfg = crate::config::load_config()?;
    let tau = args
        .tau
        .or_else(|| std::env::var("PC_DOCTOR_TAU").ok().and_then(|s| s.parse().ok()))
        .unwrap_or(DEFAULT_TAU);

    let live_wiki = wiki_dir(root);
    println!("wiki doctor: reading live wiki at {}", live_wiki.display());
    if !live_wiki.exists() {
        anyhow::bail!("no wiki found at {}", live_wiki.display());
    }

    let rows = read_index_live(&live_wiki);
    println!("  {} guides found; tau = {:.3}", rows.len(), tau);

    // Topic-taxonomy mode is independent of cosine clustering — handle it before DETECT.
    if args.retopic {
        return run_retopic(root, &live_wiki, &rows, &cfg, args.apply, args.model.as_deref());
    }

    // DETECT
    let mut embedder = build_embedder(&cfg).context("build embedder")?;
    let clusters = detect_clusters(embedder.as_mut(), &rows, tau)?;
    println!("\n=== DETECTED {} candidate cluster(s) ===", clusters.len());
    for (i, c) in clusters.iter().enumerate() {
        println!(
            "  [{}] min_pair={:.3}  ({} guides)\n      {}",
            i + 1,
            c.min_pair,
            c.slugs.len(),
            c.slugs.join(", ")
        );
    }

    let mut report = DoctorReport {
        input_guides: rows.len(),
        clusters_detected: clusters.len(),
        ..Default::default()
    };

    if args.detect_only {
        println!("\n--detect-only: skipping confirm + merge.");
        return Ok(());
    }

    let out_dir = resolve_output_dir(&live_wiki, &args)?;
    println!("\nwriting consolidated wiki to {}", out_dir.display());

    // Load every guide; tally input citations.
    let mut input_distinct: BTreeSet<String> = BTreeSet::new();
    let mut input_raw = 0usize;
    let mut loaded: HashMap<String, Guide> = HashMap::new();
    for p in list_guide_paths(&live_wiki) {
        if let Some(g) = wiki::load_guide(&p) {
            input_distinct.extend(citation_markers(&g.body));
            input_raw += raw_marker_count(&g.body);
            loaded.insert(g.frontmatter.slug.clone(), g);
        }
    }
    report.input_distinct_markers = input_distinct.len();
    report.input_raw_markers = input_raw;

    let llm = LlmClient {
        spec: ModelSpec::parse(&cfg.capture_model),
        openrouter_api_key: cfg.openrouter_api_key.clone().unwrap_or_default(),
        ollama_base_url: cfg.ollama_base_url.clone(),
        ollama_api_key: cfg.ollama_api_key.clone(),
    };
    let today = today_str();

    let mut slug_remap: HashMap<String, String> = HashMap::new();
    let mut merged_guides: HashMap<String, Guide> = HashMap::new();
    let mut absorbed: HashSet<String> = HashSet::new();

    for c in &clusters {
        let cluster_guides: Vec<Guide> =
            c.slugs.iter().filter_map(|s| loaded.get(s).cloned()).collect();
        if cluster_guides.len() < 2 {
            continue;
        }

        // CONFIRM
        let confirm = llm
            .call(
                "You judge whether wiki guides are the same topic. Answer YES or NO only.",
                &confirm_prompt(&cluster_guides),
            )
            .unwrap_or_else(|e| {
                eprintln!("  confirm call failed ({e}); treating as NO");
                "NO".into()
            });
        let yes = confirm.trim().to_uppercase().starts_with("YES");
        println!(
            "  cluster [{}]: confirm => {}",
            c.slugs.join(","),
            if yes { "YES (merge)" } else { "NO (keep separate)" }
        );
        if !yes {
            continue;
        }
        report.clusters_confirmed += 1;

        // MERGE — a failed merge (e.g. model timeout on a large cluster) must NOT abort the
        // whole run: skip this cluster (its guides stay separate) and record it as a residual.
        let m = match merge_cluster(&llm, &cluster_guides, &today) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("    merge failed ({e}); keeping cluster guides separate");
                report.failed_merges.push(c.slugs.join(", "));
                continue;
            }
        };
        for ab in &m.absorbed_slugs {
            slug_remap.insert(ab.clone(), m.canonical_slug.clone());
            absorbed.insert(ab.clone());
        }
        if !m.carried_forward.is_empty() {
            println!(
                "    citation guard: carried forward {} dropped marker(s): {}",
                m.carried_forward.len(),
                m.carried_forward.join(" ")
            );
        }
        report.merges.push(MergeSummary {
            canonical_slug: m.canonical_slug.clone(),
            absorbed: m.absorbed_slugs.clone(),
            src_distinct: m.src_markers.len(),
            out_distinct: m.out_markers.len(),
            carried_forward: m.carried_forward.len(),
        });
        merged_guides.insert(m.canonical_slug.clone(), m.merged);
    }

    // WRITE the COMPLETE consolidated wiki.
    std::fs::create_dir_all(&out_dir)?;
    let mut written = 0usize;
    let mut out_distinct: BTreeSet<String> = BTreeSet::new();
    let mut out_raw = 0usize;

    for (slug, guide) in &loaded {
        if absorbed.contains(slug) {
            continue;
        }
        let mut g = merged_guides.get(slug).cloned().unwrap_or_else(|| guide.clone());
        g.body = remap_see_also(&g.body, &slug_remap);
        out_distinct.extend(citation_markers(&g.body));
        out_raw += raw_marker_count(&g.body);
        wiki::save_guide(&wiki::guide_path(&out_dir, slug), &g)?;
        written += 1;
    }

    // In --apply mode out_dir IS the live wiki, which still holds the absorbed (now-stale)
    // guide files on disk. Dry-run writes a fresh temp dir, so absorbed slugs simply never
    // get created there — but in-place we must DELETE them, else rebuild_index re-lists the
    // zombies and enforce_bidirectional_links can re-link to them, defeating consolidation.
    // NOTE: this in-place delete path is NOT yet validated against a real wiki (v1 forbids
    // --apply on the live wiki; dry-run is the tested path). Exercise it on a throwaway copy
    // before trusting it.
    if args.apply {
        for slug in &absorbed {
            let p = wiki::guide_path(&out_dir, slug);
            if p.exists() {
                let _ = std::fs::remove_file(&p);
            }
        }
    }

    // Copy citation receipts through unchanged.
    let cit_dir_src = live_wiki.join("_citations");
    if cit_dir_src.exists() {
        let _ = copy_dir_all(&cit_dir_src, &out_dir.join("_citations"));
    }
    let cit_src = live_wiki.join("_citations.log");
    if cit_src.exists() {
        let _ = std::fs::copy(&cit_src, out_dir.join("_citations.log"));
    }

    wiki::rebuild_index(&out_dir, &today)?;
    let added = enforce_bidirectional_links(&out_dir, &today).unwrap_or(0);
    wiki::rebuild_index(&out_dir, &today)?;

    report.output_guides = written;
    report.output_distinct_markers = out_distinct.len();
    report.output_raw_markers = out_raw;

    print_report(&report, added, &out_dir);
    if args.apply {
        println!(
            "\nNOTE: --apply requested; the consolidated wiki was written directly to {}.",
            live_wiki.display()
        );
    }
    Ok(())
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Topic-taxonomy pass: one LLM call over the whole catalog proposes a flat set of
/// coherent topics and assigns every guide to exactly one. We then stamp each guide's
/// `topic` frontmatter field in place. This GROUPS the wiki (navigability) WITHOUT merging
/// — bodies, citations, and slugs are untouched (lossless; honors keep-everything).
///
/// Rationale (empirical, 2026-06-04): embedding cosine clustering finds near-DUPLICATES,
/// not topics. A real topic like `nostr-protocol` spans nip01..nip66, which are
/// cosine-distant, so flat pairwise clustering either over-chains (one 25-guide nmp blob
/// at tau 0.55) or under-groups (singletons at 0.62+). Topic assignment needs the global
/// semantic view only an LLM reading all titles+summaries at once provides.
/// Run a blocking closure while a side thread prints a spinner + elapsed seconds to stderr,
/// so a long, non-streaming model call doesn't read as a hang. Clears its line when done so
/// it doesn't collide with the structured stdout report that follows.
fn with_heartbeat<T>(label: &str, f: impl FnOnce() -> Result<T>) -> Result<T> {
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let done = Arc::new(AtomicBool::new(false));
    let label = label.to_string();
    let ticker = {
        let done = Arc::clone(&done);
        std::thread::spawn(move || {
            let frames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
            let start = std::time::Instant::now();
            let mut i = 0usize;
            while !done.load(Ordering::Relaxed) {
                eprint!(
                    "\r  {} {} … {}s elapsed   ",
                    frames[i % frames.len()],
                    label,
                    start.elapsed().as_secs()
                );
                let _ = std::io::stderr().flush();
                i += 1;
                std::thread::sleep(std::time::Duration::from_millis(120));
            }
            eprint!("\r{:width$}\r", "", width = label.len() + 28);
            let _ = std::io::stderr().flush();
        })
    };

    let out = f();
    done.store(true, Ordering::Relaxed);
    let _ = ticker.join();
    out
}

fn run_retopic(
    root: &Path,
    live_wiki: &Path,
    rows: &[IndexRow],
    cfg: &Config,
    apply: bool,
    model_override: Option<&str>,
) -> Result<()> {
    if rows.is_empty() {
        println!("retopic: no guides to organize.");
        return Ok(());
    }

    // Build the catalog the model sees: one line per guide (index | slug | title | summary),
    // plus the current topic so the model can reuse a sane existing one.
    let mut catalog = String::new();
    for (i, r) in rows.iter().enumerate() {
        let cur = if r.topic.is_empty() { "-" } else { r.topic.as_str() };
        catalog.push_str(&format!(
            "{} | {} | {} | {} | (current topic: {})\n",
            i, r.slug, r.title, r.summary, cur
        ));
    }

    let system = "You are a senior technical writer organizing an engineering wiki into a \
                  clean topic taxonomy. You group guides by SUBSYSTEM / DOMAIN a reader would \
                  browse together — not by surface word overlap. You never invent guides and \
                  never drop one.";
    let user = format!(
        "Below is the full list of wiki guides: `index | slug | title | summary | current topic`.\n\n\
         Propose a FLAT set of coherent topics (aim for roughly one topic per ~5-15 guides; a \
         large project may have 8-20 topics, a small one 3-6). Each topic is a 1-3 word \
         kebab-case label (e.g. `nostr-protocol`, `chirp-ui`, `relay-management`, \
         `build-and-release`). Then assign EVERY guide to EXACTLY ONE topic. Reuse a guide's \
         current topic when it is already sane; otherwise pick the best-fitting topic. Group \
         related-but-distinct guides together (e.g. all NIP guides → `nostr-protocol`; all \
         relay selection/admission/settings → `relay-management`) — do NOT give each guide its \
         own topic.\n\n\
         Output STRICT JSON, nothing else:\n\
         {{\"topics\": [\"topic-a\", \"topic-b\", ...], \"assignments\": {{\"<slug>\": \"topic-a\", ...}}}}\n\
         Every slug below MUST appear exactly once in assignments, mapped to a topic present \
         in the topics array.\n\n\
         GUIDES:\n{}",
        catalog
    );

    let model = model_override.unwrap_or(&cfg.capture_model);
    let llm = LlmClient {
        spec: ModelSpec::parse(model),
        openrouter_api_key: cfg.openrouter_api_key.clone().unwrap_or_default(),
        ollama_base_url: cfg.ollama_base_url.clone(),
        ollama_api_key: cfg.ollama_api_key.clone(),
    };

    println!("\nretopic: asking {} to propose a taxonomy for {} guides...", model, rows.len());
    // The taxonomy call is a single blocking, non-streaming request over the whole catalog
    // to an often-slow cloud reasoning model (up to a 600s timeout). Without feedback the CLI
    // looks hung for minutes; print an elapsed-time heartbeat on a side thread so it's
    // visibly alive and the user can tell waiting-on-model from actually-stuck.
    let raw = with_heartbeat(&format!("retopic: waiting on {}", model), || {
        llm.call(system, &user)
    })?;
    let json = strip_code_fence(&raw);

    #[derive(serde::Deserialize)]
    struct Taxonomy {
        assignments: HashMap<String, String>,
    }
    let tax: Taxonomy = serde_json::from_str(&json)
        .with_context(|| format!("retopic: model did not return valid JSON. Raw:\n{}", raw))?;

    // Validate coverage: every guide assigned, no guide dropped.
    let known: HashSet<&str> = rows.iter().map(|r| r.slug.as_str()).collect();
    let assigned: HashSet<&str> = tax.assignments.keys().map(|s| s.as_str()).collect();
    let missing: Vec<&str> = known.difference(&assigned).copied().collect();
    if !missing.is_empty() {
        println!(
            "retopic: WARNING — {} guide(s) unassigned by the model; they keep their current topic: {}",
            missing.len(),
            missing.join(", ")
        );
    }

    // Tally the proposed taxonomy.
    let mut by_topic: std::collections::BTreeMap<String, Vec<&str>> = std::collections::BTreeMap::new();
    for r in rows {
        let topic = tax
            .assignments
            .get(&r.slug)
            .cloned()
            .unwrap_or_else(|| if r.topic.is_empty() { "general".to_string() } else { r.topic.clone() });
        by_topic.entry(topic).or_default().push(r.slug.as_str());
    }

    println!(
        "\n=== PROPOSED TAXONOMY: {} topics for {} guides (ratio {:.2} guides/topic) ===",
        by_topic.len(),
        rows.len(),
        rows.len() as f32 / by_topic.len().max(1) as f32
    );
    for (topic, slugs) in &by_topic {
        println!("  {} ({})", topic, slugs.len());
        for s in slugs {
            println!("      - {}", s);
        }
    }

    if !apply {
        println!("\nretopic (dry-run): no files changed. Re-run with --apply to stamp the `topic` field.");
        return Ok(());
    }

    // APPLY: rewrite only the `topic` frontmatter field. Body/citations untouched.
    let _ = root; // wiki writes target live_wiki (the real docs/wiki)
    let mut changed = 0usize;
    for (topic, slugs) in &by_topic {
        for slug in slugs {
            let path = wiki::guide_path(live_wiki, slug);
            if let Some(mut g) = wiki::load_guide(&path) {
                if &g.frontmatter.topic != topic {
                    g.frontmatter.topic = topic.to_string();
                    wiki::save_guide(&path, &g)?;
                    changed += 1;
                }
            }
        }
    }
    // Rebuild the index so the topic-grouped catalog reflects the new assignments.
    let _ = enforce_bidirectional_links(live_wiki, &today_str());
    let _ = wiki::rebuild_index(live_wiki, &today_str());
    println!(
        "\nretopic: applied — {} guide(s) re-topiced into {} topics in {}",
        changed,
        by_topic.len(),
        live_wiki.display()
    );
    Ok(())
}

fn print_report(r: &DoctorReport, links_added: usize, out_dir: &Path) {
    println!("\n========== WIKI DOCTOR REPORT ==========");
    println!("input guides:         {}", r.input_guides);
    println!("output guides:        {}", r.output_guides);
    println!("clusters detected:    {}", r.clusters_detected);
    println!("clusters confirmed:   {}", r.clusters_confirmed);
    println!("clusters merged:      {}", r.merges.len());
    println!("merges failed/skipped:{}", r.failed_merges.len());
    println!("bidir links added:    {}", links_added);
    println!(
        "citations (distinct): in {} → out {}  ({})",
        r.input_distinct_markers,
        r.output_distinct_markers,
        if r.output_distinct_markers >= r.input_distinct_markers {
            "PRESERVED"
        } else {
            "LOSS — INVESTIGATE"
        }
    );
    println!(
        "citations (raw [^):   in {} → out {}  (raw may shift: merged prose can restate an \
         existing marker; no NEW ids appear — distinct set is the integrity check)",
        r.input_raw_markers, r.output_raw_markers
    );
    println!("\ncluster → canonical merge table:");
    for m in &r.merges {
        println!(
            "  {{{}}} → {}  [distinct {}->{}, carried-fwd {}]",
            m.absorbed.join(", "),
            m.canonical_slug,
            m.src_distinct,
            m.out_distinct,
            m.carried_forward
        );
    }
    if !r.failed_merges.is_empty() {
        println!("\nfailed/skipped merges (guides kept separate):");
        for f in &r.failed_merges {
            println!("  - {{{}}}", f);
        }
    }
    println!("\nconsolidated wiki written to: {}", out_dir.display());
    println!("========================================");
}

// ─── Helpers ──────────────────────────────────────────────────────────────────────

fn resolve_output_dir(live_wiki: &Path, args: &DoctorArgs) -> Result<PathBuf> {
    if args.apply {
        return Ok(live_wiki.to_path_buf());
    }
    if let Some(d) = &args.output_dir {
        return Ok(d.clone());
    }
    Ok(std::env::temp_dir().join(format!("wikidoc-{}", std::process::id())))
}

fn list_guide_paths(wiki: &Path) -> Vec<PathBuf> {
    crate::wiki::guide_files(wiki)
}

fn today_str() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
        / 86400;
    civil_date_from_days(days)
}

/// Convert days-since-epoch into a YYYY-MM-DD string (proleptic Gregorian; Howard Hinnant's
/// algorithm). Duplicated small helper to avoid coupling to capture.rs internals.
fn civil_date_from_days(z: i64) -> String {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wiki::GuideFrontmatter;

    struct FakeEmbedder;
    impl Embedder for FakeEmbedder {
        fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            Ok(texts
                .iter()
                .map(|t| {
                    let l = t.to_lowercase();
                    vec![
                        if l.contains("awareness") { 1.0 } else { 0.0 },
                        if l.contains("inject") { 1.0 } else { 0.0 },
                        if l.contains("gate") { 1.0 } else { 0.0 },
                        if l.contains("compile") { 1.0 } else { 0.0 },
                    ]
                })
                .collect())
        }
        fn dimension(&self) -> usize {
            4
        }
    }

    fn row(slug: &str, title: &str, summary: &str) -> IndexRow {
        IndexRow {
            slug: slug.into(),
            topic: String::new(),
            title: title.into(),
            summary: summary.into(),
            tags: vec![],
            volatility: String::new(),
            verified: String::new(),
            updated: String::new(),
        }
    }

    #[test]
    fn citation_markers_extracts_distinct() {
        let body = "a [^5465a-1] b [^5465a-2] c [^5465a-1] d [^abc_9]";
        let m = citation_markers(body);
        assert_eq!(m.len(), 3);
        assert!(m.contains("[^5465a-1]"));
        assert!(m.contains("[^abc_9]"));
    }

    #[test]
    fn raw_count_counts_duplicates() {
        let body = "[^x-1] [^x-1] [^x-2]";
        assert_eq!(raw_marker_count(body), 3);
        assert_eq!(citation_markers(body).len(), 2);
    }

    #[test]
    fn clusters_group_same_topic_keep_distinct_apart() {
        let rows = vec![
            row("agent-awareness", "Agent Awareness", "awareness standup"),
            row("ambient-awareness", "Ambient Awareness", "awareness ambient"),
            row("inject-gate", "Inject Gate", "inject gate reasoning bail"),
            row("inject-compile", "Inject Compile", "inject compile synthesize"),
        ];
        let mut e = FakeEmbedder;
        let clusters = detect_clusters(&mut e, &rows, 0.6).unwrap();
        let aware = clusters
            .iter()
            .find(|c| c.slugs.contains(&"agent-awareness".to_string()))
            .expect("awareness cluster");
        assert!(aware.slugs.contains(&"ambient-awareness".to_string()));
        let same = clusters.iter().any(|c| {
            c.slugs.contains(&"inject-gate".to_string())
                && c.slugs.contains(&"inject-compile".to_string())
        });
        assert!(!same, "inject-gate and inject-compile must stay distinct");
    }

    #[test]
    fn singletons_not_returned() {
        let rows = vec![row("a", "Alpha", "awareness"), row("b", "Beta", "compile")];
        let mut e = FakeEmbedder;
        let clusters = detect_clusters(&mut e, &rows, 0.6).unwrap();
        assert!(clusters.is_empty());
    }

    #[test]
    fn remap_rewrites_links() {
        let mut map = HashMap::new();
        map.insert("old-slug".to_string(), "canon".to_string());
        let body = "See [[old-slug]] and [Name](old-slug.md) and [[old-slug|Label]].";
        let out = remap_see_also(body, &map);
        assert!(out.contains("[[canon]]"));
        assert!(out.contains("](canon.md)"));
        assert!(out.contains("[[canon|Label]]"));
        assert!(!out.contains("old-slug"));
    }

    #[test]
    fn pick_canonical_prefers_recent() {
        let g = |slug: &str, updated: &str, body: &str| Guide {
            frontmatter: GuideFrontmatter {
                slug: slug.into(),
                updated: updated.into(),
                ..Default::default()
            },
            body: body.into(),
        };
        let guides = vec![g("old", "2026-01-01", "short"), g("new", "2026-05-30", "short")];
        assert_eq!(pick_canonical(&guides), 1);
    }

    #[test]
    fn civil_date_known_value() {
        // 2026-05-31 is 20604 days after epoch.
        assert_eq!(civil_date_from_days(20604), "2026-05-31");
    }
}
