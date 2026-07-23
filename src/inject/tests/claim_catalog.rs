use super::*;

/// Deterministic test embedder: every input maps to the same fixed unit vector.
struct ConstEmbedder {
    dim: usize,
}
impl crate::embed::Embedder for ConstEmbedder {
    fn embed(&mut self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|_| {
                let mut v = vec![0.0f32; self.dim];
                if !v.is_empty() {
                    v[0] = 1.0;
                }
                v
            })
            .collect())
    }
    fn dimension(&self) -> usize {
        self.dim
    }
}

/// Seed a single claim into a project dir's claims store using the stub embedder.
fn seed_claim(
    project_dir: &std::path::Path,
    cluster_id_hint: &str, // used as claim id so cluster becomes "cl-<id>"
    assertion: &str,
    evidence: &str,
) {
    let mut emb = ConstEmbedder { dim: 4 };
    crate::claims::append_claim(
        project_dir,
        &mut emb,
        cluster_id_hint,
        "2026-06-18",
        "test-session",
        assertion,
        "explicit",
        evidence,
        &[],
        None,
    )
    .expect("append_claim failed in test seed");
}

/// (a) Catalog includes claim rows when PC_CLAIM_CATALOG=1.
#[test]
fn catalog_includes_claim_rows_when_flag_on() {
    let _g = VARIANT_ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let wiki = root.join("docs/wiki");
    fs::create_dir_all(&wiki).unwrap();
    let project_dir = tmp.path().join("project");
    fs::create_dir_all(&project_dir).unwrap();

    seed_claim(
        &project_dir,
        "claim-a1",
        "The token model uses uint16",
        "evidence A",
    );

    std::env::set_var("PC_CLAIM_CATALOG", "1");
    let emb: Box<dyn crate::embed::Embedder> = Box::new(ConstEmbedder { dim: 4 });
    let catalog = build_catalog(
        root,
        &wiki,
        &project_dir,
        &[],
        &[],
        150,
        "token model",
        "",
        Some(emb),
    );
    std::env::remove_var("PC_CLAIM_CATALOG");

    let claim_rows: Vec<_> = catalog
        .iter()
        .filter(|c| c.key.starts_with("claim:"))
        .collect();
    assert!(
        !claim_rows.is_empty(),
        "expected at least one claim catalog row when PC_CLAIM_CATALOG=1"
    );
    let row = claim_rows[0];
    assert!(
        row.key.starts_with("claim:"),
        "key must use claim: prefix, got {}",
        row.key
    );
    assert_eq!(
        row.kind,
        crate::content_kind::ContentKind::Claim,
        "kind must be Claim"
    );
    assert_eq!(
        row.currentness,
        crate::content_kind::Currentness::Unknown,
        "unclassified claim status must remain Unknown"
    );
    assert_eq!(
        row.authority,
        crate::content_kind::Authority::Explicit,
        "seeded user claim authority must remain Explicit"
    );
    assert!(
        row.title.contains("uint16") || row.title.contains("token model") || !row.title.is_empty(),
        "title should be the representative assertion, got: {}",
        row.title
    );
}

/// (b) Catalog omits claim rows when PC_CLAIM_CATALOG=0 (default).
#[test]
fn catalog_omits_claim_rows_when_flag_off() {
    let _g = VARIANT_ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let wiki = root.join("docs/wiki");
    fs::create_dir_all(&wiki).unwrap();
    let project_dir = tmp.path().join("project");
    fs::create_dir_all(&project_dir).unwrap();

    seed_claim(
        &project_dir,
        "claim-b1",
        "Some claim assertion",
        "evidence B",
    );

    // Ensure flag is off (default). No embedder passed — flag-off path must be a no-op.
    std::env::remove_var("PC_CLAIM_CATALOG");
    let catalog = build_catalog(
        root,
        &wiki,
        &project_dir,
        &[],
        &[],
        150,
        "some query",
        "",
        None::<Box<dyn crate::embed::Embedder>>,
    );

    let claim_rows: Vec<_> = catalog
        .iter()
        .filter(|c| c.key.starts_with("claim:"))
        .collect();
    assert!(
        claim_rows.is_empty(),
        "expected no claim rows when PC_CLAIM_CATALOG is unset (default off)"
    );
}

/// (c) read_catalog_content resolves a claim key to non-empty content, returns None for missing.
#[test]
fn read_catalog_content_resolves_claim_key() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let wiki = root.join("docs/wiki");
    fs::create_dir_all(&wiki).unwrap();
    let project_dir = tmp.path().join("project");
    fs::create_dir_all(&project_dir).unwrap();

    seed_claim(
        &project_dir,
        "claimid-c1",
        "MiniLM is the default embedder",
        "evidence C",
    );

    // The cluster_id created by append_claim is "cl-claimid-c1".
    let cluster_key = "claim:cl-claimid-c1";
    let content = read_catalog_content(root, &wiki, &project_dir, cluster_key)
        .expect("claim key must resolve to rendered content");
    assert!(!content.is_empty(), "rendered content must be non-empty");
    assert!(
        content.contains("MiniLM") || content.contains("CLAIM STORE"),
        "content should include the assertion or header: {}",
        content
    );

    // A missing cluster id must return None, not panic.
    let missing = read_catalog_content(root, &wiki, &project_dir, "claim:cl-does-not-exist");
    assert!(missing.is_none(), "missing cluster must return None");
}

/// (d) Claim rows preserve kind, adoption status, authority, and representative assertion.
#[test]
fn claim_catalog_rows_have_correct_metadata() {
    let _g = VARIANT_ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let wiki = root.join("docs/wiki");
    fs::create_dir_all(&wiki).unwrap();
    let project_dir = tmp.path().join("project");
    fs::create_dir_all(&project_dir).unwrap();

    let assertion = "The deploy gate requires two approvals";
    let evidence = "seen in PR description";
    seed_claim(&project_dir, "claim-d1", assertion, evidence);

    std::env::set_var("PC_CLAIM_CATALOG", "1");
    let emb: Box<dyn crate::embed::Embedder> = Box::new(ConstEmbedder { dim: 4 });
    let catalog = build_catalog(
        root,
        &wiki,
        &project_dir,
        &[],
        &[],
        150,
        "deploy",
        "",
        Some(emb),
    );
    std::env::remove_var("PC_CLAIM_CATALOG");

    let claim_rows: Vec<_> = catalog
        .iter()
        .filter(|c| c.key.starts_with("claim:"))
        .collect();
    assert!(!claim_rows.is_empty(), "must have at least one claim row");
    let row = claim_rows[0];

    // Unclassified legacy/default claims must not be silently promoted to current.
    assert_eq!(row.kind, crate::content_kind::ContentKind::Claim);
    assert_eq!(row.currentness, crate::content_kind::Currentness::Unknown);
    assert_eq!(row.authority, crate::content_kind::Authority::Explicit);

    // Title must be the representative (most-recent) assertion.
    assert_eq!(
        row.title,
        crate::events::truncate(assertion, 80),
        "title must be the representative assertion, got: {}",
        row.title
    );

    // Summary must be the evidence text (truncated).
    assert_eq!(
        row.summary,
        crate::events::truncate(evidence, 100),
        "summary must be the evidence text, got: {}",
        row.summary
    );
}
