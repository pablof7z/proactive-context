use super::*;

pub(crate) fn list_committed_markdown(root: &Path) -> Vec<String> {
    use std::process::Command;
    if let Ok(out) = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z", "--", "*.md"])
        .output()
    {
        if out.status.success() {
            return out
                .stdout
                .split(|b| *b == 0)
                .filter(|s| !s.is_empty())
                .filter_map(|s| std::str::from_utf8(s).ok())
                .map(|s| s.to_string())
                .collect();
        }
    }
    // Fallback: gitignore-aware walk (no git repo / git unavailable).
    let mut files = Vec::new();
    for entry in WalkBuilder::new(root).hidden(false).build().flatten() {
        if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("md") {
                if let Ok(rel) = p.strip_prefix(root) {
                    files.push(rel.to_string_lossy().to_string());
                }
            }
        }
    }
    files
}

/// Derive (title, summary): prefer YAML frontmatter title/summary, else first `# heading`
/// (or filename) for the title and the first non-empty body line for the summary.
pub(crate) fn derive_title_summary(content: &str, fallback_name: &str) -> (String, String) {
    let mut title = String::new();
    let mut summary = String::new();

    let mut it = content.lines().peekable();
    if it.peek().map(|l| l.trim() == "---").unwrap_or(false) {
        it.next();
        for line in it.by_ref() {
            let t = line.trim();
            if t == "---" {
                break;
            }
            if let Some(v) = t.strip_prefix("title:") {
                title = v.trim().trim_matches('"').to_string();
            } else if let Some(v) = t.strip_prefix("summary:") {
                summary = v.trim().trim_matches('"').to_string();
            }
        }
    }

    if title.is_empty() || summary.is_empty() {
        for line in content.lines() {
            let t = line.trim();
            if t.is_empty() || t == "---" {
                continue;
            }
            if title.is_empty() {
                if let Some(h) = t.strip_prefix('#') {
                    title = h.trim_start_matches('#').trim().to_string();
                    continue;
                }
            }
            if summary.is_empty() && !t.starts_with('#') {
                summary = t.to_string();
            }
            if !title.is_empty() && !summary.is_empty() {
                break;
            }
        }
    }

    if title.is_empty() {
        title = fallback_name.to_string();
    }
    (truncate(&title, 80), truncate(&summary, 100))
}

/// Read up to `cap` bytes of a file's head (cheap, for title/summary derivation).
pub(crate) fn read_head(path: &Path, cap: usize) -> String {
    use std::io::Read as _;
    let mut buf = Vec::new();
    if let Ok(f) = std::fs::File::open(path) {
        let _ = f.take(cap as u64).read_to_end(&mut buf);
    }
    String::from_utf8_lossy(&buf).to_string()
}

/// Turn the exact vector-retrieved passage into a compact, single-line selector hint.
///
/// Document titles and head-derived summaries are often generic while the relevant passage is
/// deep in the file. SELECT cannot read source files, so hiding the passage that earned the
/// relevance score makes it abstain on genuinely useful documents. Keep this evidence bounded and
/// visibly quoted as source data; COMPILE still reads and cites the authoritative source itself.
pub(crate) fn compact_retrieval_evidence(content: &str) -> String {
    let flattened = content.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate(&flattened, 600)
}

/// Full content of a catalog source by key. Resolution by key shape:
///   - `episode:<stem>`  → `<wiki_dir>/episodes/<stem>.md` (historical episode card)
///   - `noun:<slug>` → never directly readable; noun catalog rows must resolve to a backing guide
///   - `claim:<cluster_id>` → rendered from claims.jsonl records for that cluster (no .md file)
///   - `<path>` containing '/' or ending '.md' → `<root>/<path>` (committed project doc)
///   - bare slug → `<wiki_dir>/<slug>.md` (wiki guide)
pub(crate) fn read_catalog_content(
    root: &Path,
    wiki_dir: &Path,
    project_dir: &Path,
    key: &str,
) -> Option<String> {
    if let Some(stem) = key.strip_prefix(EPISODE_KEY_PREFIX) {
        let path = wiki_dir.join("episodes").join(format!("{}.md", stem));
        return std::fs::read_to_string(path).ok();
    }
    // New taxonomy prefixes (Phase 2+5). parse_key dispatches by prefix; episode/guide
    // resolution above is unchanged.
    match ContentKind::parse_key(key) {
        (ContentKind::ResearchRecord, stem) => {
            let path = wiki_dir.join("research").join(format!("{}.md", stem));
            return std::fs::read_to_string(path).ok();
        }
        // A noun key is a SELECT-only alias. Treating a synthetic rendered noun as if it were the
        // contents of realness.jsonl would create citations to lines that do not exist.
        (ContentKind::NounEntry, _) => return None,
        // Phase 5: claim rows have no backing .md file — content is rendered from ClaimRecords.
        // load_cluster resolves by cluster_id directly from claims.jsonl (no embedder needed,
        // no re-retrieval inconsistency risk). Returns None gracefully on a missing cluster.
        (ContentKind::Claim, cluster_id) => {
            let cluster = crate::claims::load_cluster(project_dir, cluster_id)?;
            let rendered = crate::claims::render_clusters_for_compile(&[cluster]);
            return if rendered.is_empty() {
                None
            } else {
                Some(rendered)
            };
        }
        _ => {}
    }
    let path = if key.ends_with(".md") || key.contains('/') {
        root.join(key)
    } else {
        guide_path(wiki_dir, key)
    };
    std::fs::read_to_string(path).ok()
}

pub(crate) fn label_for_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|rel| format!("./{}", rel.display()))
        .unwrap_or_else(|_| path.display().to_string())
}

pub(crate) fn source_label_for_key(
    root: &Path,
    wiki_dir: &Path,
    project_dir: Option<&Path>,
    key: &str,
) -> String {
    if let Some(stem) = key.strip_prefix(EPISODE_KEY_PREFIX) {
        return label_for_path(
            root,
            &wiki_dir.join("episodes").join(format!("{}.md", stem)),
        );
    }

    match ContentKind::parse_key(key) {
        (ContentKind::ResearchRecord, stem) => label_for_path(
            root,
            &wiki_dir.join("research").join(format!("{}.md", stem)),
        ),
        // Noun keys are SELECT-only aliases and must never reach source labeling. Keep the
        // defensive label visibly unresolved rather than fabricating a realness.jsonl citation.
        (ContentKind::NounEntry, slug) => format!("unresolved-noun-alias:{}", slug),
        (ContentKind::Claim, cluster_id) => {
            if let Some(project_dir) = project_dir {
                let claim_store = crate::claims::claims_jsonl_path(project_dir);
                format!(
                    "{}#claim-{}",
                    label_for_path(root, &claim_store),
                    cluster_id
                )
            } else {
                format!("claim-store#claim-{}", cluster_id)
            }
        }
        _ => {
            let path = if key.ends_with(".md") || key.contains('/') {
                root.join(key)
            } else {
                guide_path(wiki_dir, key)
            };
            label_for_path(root, &path)
        }
    }
}

pub(crate) fn source_metadata_for_key(
    wiki_dir: &Path,
    project_dir: Option<&Path>,
    key: &str,
) -> (ContentKind, Currentness, Authority) {
    if let Some(stem) = key.strip_prefix(EPISODE_KEY_PREFIX) {
        let status = crate::episode_capture::scan_episode_cards(wiki_dir)
            .into_iter()
            .find(|row| row.filename.strip_suffix(".md") == Some(stem))
            .map(|row| row.status);
        let currentness = if status.as_deref() == Some("superseded") {
            Currentness::Superseded
        } else {
            Currentness::Historical
        };
        return (ContentKind::EpisodeCard, currentness, Authority::Unknown);
    }

    match ContentKind::parse_key(key) {
        (ContentKind::ResearchRecord, _) => (
            ContentKind::ResearchRecord,
            Currentness::Historical,
            Authority::Unknown,
        ),
        (ContentKind::NounEntry, _) => (
            ContentKind::NounEntry,
            Currentness::Current,
            Authority::Unknown,
        ),
        (ContentKind::Claim, cluster_id) => {
            let Some(cluster) =
                project_dir.and_then(|dir| crate::claims::load_cluster(dir, cluster_id))
            else {
                return (ContentKind::Claim, Currentness::Unknown, Authority::Unknown);
            };
            let Some(current) = cluster.claims.first() else {
                return (ContentKind::Claim, Currentness::Unknown, Authority::Unknown);
            };
            let currentness = match current.status {
                crate::claims::ClaimStatus::Settled => Currentness::Current,
                crate::claims::ClaimStatus::Proposed => Currentness::Proposed,
                crate::claims::ClaimStatus::Unknown => Currentness::Unknown,
            };
            (
                ContentKind::Claim,
                currentness,
                Authority::from_str_lossy(&current.authority),
            )
        }
        _ if key.ends_with(".md") || key.contains('/') => (
            ContentKind::CommittedMarkdown,
            Currentness::Current,
            Authority::Unknown,
        ),
        _ => (
            ContentKind::CurrentGuide,
            Currentness::Current,
            Authority::Unknown,
        ),
    }
}
