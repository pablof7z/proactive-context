use super::*;

#[test]
fn parse_query_line_handles_model_formatting() {
    // Plain, the happy path.
    assert_eq!(
        parse_query_line("QUERY: Does the OAuth support include Google?").as_deref(),
        Some("Does the OAuth support include Google?")
    );
    // Case-insensitive, extra whitespace.
    assert_eq!(
        parse_query_line("query:   trimmed  ").as_deref(),
        Some("trimmed")
    );
    // Markdown bullet + bold wrappers (common model embellishments).
    assert_eq!(
        parse_query_line("- **QUERY:** how does billing work?").as_deref(),
        Some("how does billing work?")
    );
    assert_eq!(parse_query_line("• QUERY: foo").as_deref(), Some("foo"));
    // Not a query line / empty payload → None (falls back to raw prompt).
    assert_eq!(parse_query_line("inject-subcommand"), None);
    assert_eq!(parse_query_line("QUERY:"), None);
    assert_eq!(parse_query_line("NOTHING_RELEVANT"), None);
}

#[test]
fn contextualized_query_keeps_recent_context_and_current_prompt_under_cap() {
    let query = contextualized_query(
        "Does it support Google?",
        "User: We are discussing OAuth.",
        200,
    );
    assert!(query.contains("discussing OAuth"));
    assert!(query.ends_with("Does it support Google?"));

    let capped = contextualized_query("CURRENT", &"x".repeat(300), 40);
    assert!(capped.len() <= 40);
    assert!(capped.ends_with("CURRENT"));
}

#[test]
fn prompt_authority_context_detects_live_state_and_explicit_corrections() {
    assert_eq!(
        artifact_context_for_prompt("Check the live logs and current status"),
        crate::artifact_safety::ArtifactContext::LiveState
    );
    assert_eq!(
        artifact_context_for_prompt("What's the status of the relay?"),
        crate::artifact_safety::ArtifactContext::LiveState
    );
    for prompt in [
        "Did the deploy succeed?",
        "Did the tests pass?",
        "Did the build fail?",
        "Did the migration finish?",
        "Did the rollout complete?",
    ] {
        assert_eq!(
            artifact_context_for_prompt(prompt),
            crate::artifact_safety::ArtifactContext::LiveState,
            "{prompt}"
        );
    }
    assert_eq!(
        artifact_context_for_prompt("No, that is wrong; use SQLite instead"),
        crate::artifact_safety::ArtifactContext::ExplicitUserCorrection
    );
    for prompt in [
        "Don't use Redis; use Postgres",
        "do not use Redis; use Postgres",
    ] {
        assert_eq!(
            artifact_context_for_prompt(prompt),
            crate::artifact_safety::ArtifactContext::ExplicitUserCorrection,
            "{prompt}"
        );
    }
    assert_eq!(
        artifact_context_for_prompt("Explain the storage architecture"),
        crate::artifact_safety::ArtifactContext::Standard
    );
}

#[test]
fn live_results_and_negative_replacements_require_authority_labels() {
    let source = [crate::artifact_safety::SourceDocument::new(
        "./docs/runbook.md",
        "The deploy normally succeeds and the application uses Redis.",
    )];
    let artifact = "TITLE: Stored Project Context\nStored fact. (./docs/runbook.md:1)";

    for prompt in ["Did the deploy succeed?", "Did the tests pass?"] {
        assert_eq!(
            crate::artifact_safety::validate_compiled_artifact_for_context(
                artifact,
                &source,
                artifact_context_for_prompt(prompt),
            ),
            Err(
                crate::artifact_safety::ArtifactError::MissingAuthorityLabel {
                    line: 2,
                    required: "STATIC BACKGROUND:",
                }
            )
        );
    }

    for prompt in [
        "Don't use Redis; use Postgres",
        "do not use Redis; use Postgres",
    ] {
        assert_eq!(
            crate::artifact_safety::validate_compiled_artifact_for_context(
                artifact,
                &source,
                artifact_context_for_prompt(prompt),
            ),
            Err(
                crate::artifact_safety::ArtifactError::MissingAuthorityLabel {
                    line: 2,
                    required: "STORED BACKGROUND:",
                }
            )
        );
    }
}

#[test]
fn authority_contract_uses_deterministic_live_and_correction_labels() {
    let live = authority_rules(crate::artifact_safety::ArtifactContext::LiveState);
    assert!(live.contains("begin EVERY body line exactly with `STATIC BACKGROUND:`"));

    let correction =
        authority_rules(crate::artifact_safety::ArtifactContext::ExplicitUserCorrection);
    assert!(correction.contains("begin EVERY body line exactly with"));
    assert!(correction.contains("`STORED BACKGROUND:`"));
}

#[test]
fn retrieval_diversity_prefers_distinct_paths_without_losing_rank_order() {
    let hits = vec![
        query_hit("a.md", 0, 0.9),
        query_hit("a.md", 1, 0.8),
        query_hit("b.md", 0, 0.7),
        query_hit("c.md", 0, 0.6),
    ];
    let kept = diversify_hits(&hits, 3);
    let identities = kept
        .iter()
        .map(|hit| (hit.path.as_str(), hit.chunk_index))
        .collect::<Vec<_>>();
    assert_eq!(identities, vec![("a.md", 0), ("b.md", 0), ("c.md", 0)]);
}

#[test]
fn catalog_requires_existing_semantic_relevance_evidence() {
    use crate::content_kind::{Authority, ContentKind, Currentness};
    use crate::query::MINIMUM_RELEVANCE_SCORE;

    let item = |key: &str, score| CatalogItem {
        key: key.to_string(),
        source_key: key.to_string(),
        title: key.to_string(),
        summary: String::new(),
        score,
        matched_passages: Vec::new(),
        kind: ContentKind::CurrentGuide,
        currentness: Currentness::Current,
        authority: Authority::Unknown,
    };
    let kept = relevance_evidenced_catalog(vec![
        item("at-floor", Some(MINIMUM_RELEVANCE_SCORE)),
        item("below", Some(MINIMUM_RELEVANCE_SCORE - 0.001)),
        item("unscored", None),
    ]);
    assert_eq!(
        kept.iter()
            .map(|item| item.key.as_str())
            .collect::<Vec<_>>(),
        vec!["at-floor"]
    );
}

#[test]
fn catalog_relevance_evidence_does_not_cross_same_stem_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let wiki = root.join("wiki");
    let project_dir = root.join("project");
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::create_dir_all(root.join("other")).unwrap();
    fs::create_dir_all(&wiki).unwrap();
    fs::create_dir_all(&project_dir).unwrap();
    fs::write(root.join("docs/readme.md"), "# Relevant\n\nOAuth details.").unwrap();
    fs::write(
        root.join("other/readme.md"),
        "# Unrelated\n\nGardening details.",
    )
    .unwrap();

    let hits = vec![query_hit("docs/readme.md", 0, 0.8)];
    let catalog = build_catalog(
        root,
        &wiki,
        &project_dir,
        &[],
        &hits,
        150,
        "OAuth",
        "",
        None,
    );
    let kept = relevance_evidenced_catalog(catalog);

    assert_eq!(
        kept.iter()
            .map(|item| item.key.as_str())
            .collect::<Vec<_>>(),
        vec!["docs/readme.md"]
    );
}

#[test]
fn catalog_surfaces_the_exact_retrieved_passage_to_select() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let wiki = root.join("wiki");
    let project_dir = root.join("project");
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::create_dir_all(&wiki).unwrap();
    fs::create_dir_all(&project_dir).unwrap();
    fs::write(
        root.join("docs/session-state.md"),
        "# Session State\n\nGeneric session lifecycle documentation.\n",
    )
    .unwrap();
    let hits = vec![crate::query::QueryResult {
        path: "docs/session-state.md".to_string(),
        chunk_index: 4,
        content: "A headless, idle runtime with no pending delivery is evicted after ten minutes."
            .to_string(),
        content_hash: "matched-chunk".to_string(),
        score: 0.82,
    }];

    let catalog = build_catalog(
        root,
        &wiki,
        &project_dir,
        &[],
        &hits,
        150,
        "When is an idle runtime evicted?",
        "",
        None,
    );
    let item = catalog
        .iter()
        .find(|item| item.key == "docs/session-state.md")
        .unwrap();

    assert!(item.summary.contains("matched passage:"));
    assert!(item.summary.contains("no pending delivery"));
    assert_eq!(item.matched_passages, vec![hits[0].content.clone()]);
    assert!(render_catalog(std::slice::from_ref(item)).contains("evicted after ten minutes"));
}

#[test]
fn selected_catalog_rerank_spends_budget_across_source_kinds() {
    use crate::content_kind::{Authority, ContentKind, Currentness};

    let item = |key: &str, score: f64, kind| CatalogItem {
        key: key.to_string(),
        source_key: key.to_string(),
        title: key.to_string(),
        summary: String::new(),
        score: Some(score),
        matched_passages: Vec::new(),
        kind,
        currentness: Currentness::Current,
        authority: Authority::Unknown,
    };
    let catalog = vec![
        item("guide-a", 0.9, ContentKind::CurrentGuide),
        item("guide-b", 0.8, ContentKind::CurrentGuide),
        item("claim:c", 0.7, ContentKind::Claim),
    ];
    let selected = vec![
        "guide-b".to_string(),
        "claim:c".to_string(),
        "guide-a".to_string(),
    ];
    let kept = rerank_selected_catalog(&catalog, &selected, 2);
    assert_eq!(
        kept.iter()
            .map(|item| item.key.as_str())
            .collect::<Vec<_>>(),
        vec!["guide-a", "claim:c"]
    );
}

#[test]
fn noun_and_guide_aliases_consume_one_source_budget_slot() {
    use crate::content_kind::{Authority, ContentKind, Currentness};

    let catalog = vec![
        CatalogItem {
            key: "real-thing".to_string(),
            source_key: "real-thing".to_string(),
            title: "Real Thing".to_string(),
            summary: String::new(),
            score: Some(0.92),
            matched_passages: Vec::new(),
            kind: ContentKind::CurrentGuide,
            currentness: Currentness::Current,
            authority: Authority::Unknown,
        },
        CatalogItem {
            key: "noun:real-thing".to_string(),
            source_key: "real-thing".to_string(),
            title: "Real Thing".to_string(),
            summary: String::new(),
            score: Some(0.92),
            matched_passages: Vec::new(),
            kind: ContentKind::NounEntry,
            currentness: Currentness::Current,
            authority: Authority::Unknown,
        },
    ];
    let selected = vec!["real-thing".to_string(), "noun:real-thing".to_string()];
    let kept = rerank_selected_catalog(&catalog, &selected, 8);

    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].source_key, "real-thing");
}

#[test]
fn selected_catalog_never_lets_proposed_material_outrank_current_truth() {
    use crate::content_kind::{Authority, ContentKind, Currentness};

    let catalog = vec![
        CatalogItem {
            key: "claim:proposal".to_string(),
            source_key: "claim:proposal".to_string(),
            title: "Proposal".to_string(),
            summary: String::new(),
            score: Some(0.99),
            matched_passages: Vec::new(),
            kind: ContentKind::Claim,
            currentness: Currentness::Proposed,
            authority: Authority::Explicit,
        },
        CatalogItem {
            key: "docs/current.md".to_string(),
            source_key: "docs/current.md".to_string(),
            title: "Current".to_string(),
            summary: String::new(),
            score: Some(0.30),
            matched_passages: Vec::new(),
            kind: ContentKind::Claim,
            currentness: Currentness::Current,
            authority: Authority::Unknown,
        },
    ];
    let selected = vec!["claim:proposal".to_string(), "docs/current.md".to_string()];

    let kept = rerank_selected_catalog(&catalog, &selected, 2);
    assert_eq!(
        kept.iter()
            .map(|item| item.key.as_str())
            .collect::<Vec<_>>(),
        vec!["docs/current.md", "claim:proposal"]
    );
}

#[test]
fn selected_source_input_obeys_derived_hard_context_budget() {
    use crate::content_kind::{Authority, ContentKind, Currentness};

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let wiki = root.join("wiki");
    let project_dir = root.join("project");
    fs::create_dir_all(wiki.join("guides")).unwrap();
    fs::create_dir_all(&project_dir).unwrap();
    fs::write(wiki.join("guides/first.md"), "a".repeat(100)).unwrap();
    fs::write(wiki.join("guides/second.md"), "b".repeat(100)).unwrap();

    let items = vec![
        CatalogItem {
            key: "first".to_string(),
            source_key: "first".to_string(),
            title: "First".to_string(),
            summary: String::new(),
            score: Some(0.9),
            matched_passages: Vec::new(),
            kind: ContentKind::CurrentGuide,
            currentness: Currentness::Current,
            authority: Authority::Unknown,
        },
        CatalogItem {
            key: "second".to_string(),
            source_key: "second".to_string(),
            title: "Second".to_string(),
            summary: String::new(),
            score: Some(0.8),
            matched_passages: Vec::new(),
            kind: ContentKind::Claim,
            currentness: Currentness::Current,
            authority: Authority::Unknown,
        },
    ];
    let refs = items.iter().collect::<Vec<_>>();
    let budget = 20;
    let (guides, read) = read_guides_with_budget(root, &wiki, &project_dir, &refs, budget);
    let used = guides
        .iter()
        .map(|(_, content)| content.chars().count())
        .sum::<usize>();

    assert_eq!(read, vec!["first", "second"]);
    assert!(used <= budget, "used {used} chars with budget {budget}");
    assert_eq!(source_char_budget(700, 8), 22_400);
}

#[test]
fn selected_source_is_scoped_to_retrieved_passages_without_changing_line_numbers() {
    use crate::content_kind::{Authority, ContentKind, Currentness};

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let wiki = root.join("wiki");
    let project_dir = root.join("project");
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::create_dir_all(&wiki).unwrap();
    fs::create_dir_all(&project_dir).unwrap();
    fs::write(
        root.join("docs/uninstall.md"),
        "# Uninstall\n\nUnrelated setup detail.\nRemove owned hooks.\nPreserve foreign settings.\nUnrelated state detail.\n",
    )
    .unwrap();

    let items = [CatalogItem {
        key: "docs/uninstall.md".to_string(),
        source_key: "docs/uninstall.md".to_string(),
        title: "Uninstall".to_string(),
        summary: String::new(),
        score: Some(0.9),
        matched_passages: vec![
            "# Uninstall\nRemove owned hooks.\nPreserve foreign settings.".to_string(),
        ],
        kind: ContentKind::CommittedMarkdown,
        currentness: Currentness::Current,
        authority: Authority::Unknown,
    }];
    let refs = items.iter().collect::<Vec<_>>();
    let (guides, read) = read_guides_with_budget(root, &wiki, &project_dir, &refs, 10_000);

    assert_eq!(read, vec!["docs/uninstall.md"]);
    let scoped = &guides[0].1;
    assert_eq!(scoped.lines().count(), 6);
    assert_eq!(scoped.lines().nth(3), Some("Remove owned hooks."));
    assert_eq!(scoped.lines().nth(4), Some("Preserve foreign settings."));
    assert!(!scoped.contains("Unrelated setup detail."));
    assert!(!scoped.contains("Unrelated state detail."));
}
