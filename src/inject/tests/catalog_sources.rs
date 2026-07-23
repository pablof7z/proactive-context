use super::*;

/// Write a minimal episode card into `<wiki>/episodes/<name>.md`.
fn write_episode_card(wiki: &std::path::Path, name: &str, title: &str, decision: &str) {
    let dir = wiki.join("episodes");
    fs::create_dir_all(&dir).unwrap();
    let card = format!(
        "---\ntype: episode-card\ndate: 2026-05-29\nsession: sess-x\ntranscript: /t.jsonl\n\
salience: reversal\nstatus: active\nsubjects:\n  - embedding-provider\nsupersedes: []\n\
related_claims: []\nsource_lines:\n  - 1-2\ncaptured_at: 2026-06-12T09:00:00Z\n---\n\n\
# Episode: {title}\n\n## Prior State\n\nBefore.\n\n## Trigger\n\nCause.\n\n## Decision\n\n{decision}\n\n\
## Consequences\n\n- c\n\n## Open Tail\n\n*(none)*\n\n## Evidence\n\n- transcript lines 1-2\n",
        title = title,
        decision = decision
    );
    fs::write(dir.join(format!("{}.md", name)), card).unwrap();
}

#[test]
fn catalog_includes_episode_cards_as_typed_rows() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let wiki = root.join("docs/wiki");
    fs::create_dir_all(&wiki).unwrap();
    write_episode_card(
        &wiki,
        "2026-05-29-1-local-embeddings-default",
        "Local embeddings become the default",
        "The default embedder is local MiniLM; OpenRouter is no longer the default.",
    );

    // No wiki guides, no RAG hits — only the episode card should surface.
    // PC_CLAIM_CATALOG is off (default) so project_dir/embedder are not touched.
    let dummy_project_dir = std::env::temp_dir();
    let catalog = build_catalog(
        root,
        &wiki,
        &dummy_project_dir,
        &[],
        &[],
        150,
        "",
        "",
        None::<Box<dyn crate::embed::Embedder>>,
    );
    let episode_rows: Vec<_> = catalog
        .iter()
        .filter(|c| c.key.starts_with(EPISODE_KEY_PREFIX))
        .collect();
    assert_eq!(episode_rows.len(), 1, "expected one episode catalog row");
    let row = episode_rows[0];
    assert_eq!(row.key, "episode:2026-05-29-1-local-embeddings-default");
    // Title is prefixed so the selector can tell history from current guides.
    assert!(
        row.title.contains("[episode 2026-05-29"),
        "title missing episode tag: {}",
        row.title
    );
    assert!(row.title.contains("Local embeddings become the default"));
    // Summary is the Decision line.
    assert!(
        row.summary.contains("local MiniLM"),
        "summary should carry the Decision: {}",
        row.summary
    );
}

#[test]
fn noun_catalog_uses_promoted_realness_not_noun_files() {
    let _g = VARIANT_ENV_LOCK.lock().unwrap();
    std::env::set_var("PC_NOUN_CATALOG", "1");
    std::env::remove_var("PC_RESEARCH_CATALOG");
    std::env::remove_var("PC_CLAIM_CATALOG");

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let wiki = root.join("docs/wiki");
    let project_dir = root.join("pc-project");
    fs::create_dir_all(wiki.join("nouns")).unwrap();
    fs::create_dir_all(&project_dir).unwrap();

    fs::write(
        wiki.join("nouns/junk-file.md"),
        "---\ntype: noun-entry\nslug: junk-file\nname: \"Junk File\"\norigin: extracted\nsource_refs:\n  []\n---\n\n# Junk File\n\nShould not be cataloged.\n",
    )
    .unwrap();
    let guide = crate::wiki::guide_path(&wiki, "real-thing");
    fs::create_dir_all(guide.parent().unwrap()).unwrap();
    fs::write(
        &guide,
        "---\ntitle: Real Thing\nsummary: Definition from guide.\n---\n\n# Real Thing\n\nDefinition from guide.\n",
    )
    .unwrap();
    crate::nouns::write_realness_registry(
        &wiki,
        &[crate::nouns::RealnessNoun::new("Real Thing", 3)],
    )
    .unwrap();
    let index_rows = crate::wiki::rebuild_index(&wiki, "2026-07-23").unwrap();
    let hits = vec![query_hit("pc-memory/guides/real-thing.md", 0, 0.91)];

    let catalog = build_catalog(
        root,
        &wiki,
        &project_dir,
        &index_rows,
        &hits,
        150,
        "what is real thing?",
        "",
        None,
    );
    let noun_rows: Vec<_> = catalog
        .iter()
        .filter(|item| item.key.starts_with("noun:"))
        .collect();
    assert_eq!(noun_rows.len(), 1);
    assert_eq!(noun_rows[0].key, "noun:real-thing");
    assert_eq!(noun_rows[0].source_key, "real-thing");
    assert_eq!(
        noun_rows[0].kind,
        crate::content_kind::ContentKind::NounEntry
    );

    // The noun key itself is never a compile source. Only its resolved real guide is readable.
    assert!(read_catalog_content(root, &wiki, &project_dir, "noun:real-thing").is_none());
    let content = read_catalog_content(root, &wiki, &project_dir, &noun_rows[0].source_key)
        .expect("resolved guide source should be readable");
    assert!(content.contains("Definition from guide."));
    assert!(read_catalog_content(root, &wiki, &project_dir, "noun:junk-file").is_none());

    std::env::remove_var("PC_NOUN_CATALOG");
}

#[test]
fn noun_catalog_requires_realness_prompt_match_and_scored_exact_guide() {
    let _g = VARIANT_ENV_LOCK.lock().unwrap();
    std::env::remove_var("PC_NOUN_CATALOG");
    std::env::remove_var("PC_RESEARCH_CATALOG");
    std::env::remove_var("PC_CLAIM_CATALOG");

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let wiki = root.join("wiki");
    let project_dir = root.join("project");
    fs::create_dir_all(&project_dir).unwrap();
    for (slug, title) in [
        ("realthing", "RealThing"),
        ("maybething", "MaybeThing"),
        ("rejectedthing", "RejectedThing"),
    ] {
        let path = crate::wiki::guide_path(&wiki, slug);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            format!(
                "---\ntitle: {title}\nslug: {slug}\ntopic: entities\nsummary: {title} has a project-specific definition.\n---\n\n# {title}\n\n{title} has a project-specific definition.\n"
            ),
        )
        .unwrap();
    }
    crate::nouns::write_realness_registry(
        &wiki,
        &[
            crate::nouns::RealnessNoun::new("RealThing", 3),
            crate::nouns::RealnessNoun::new("MaybeThing", 1),
            crate::nouns::RealnessNoun::new("RejectedThing", -2),
        ],
    )
    .unwrap();
    let index_rows = crate::wiki::rebuild_index(&wiki, "2026-07-23").unwrap();
    let prompt = "what is RealThing MaybeThing RejectedThing?";

    let unscored = build_catalog(
        root,
        &wiki,
        &project_dir,
        &index_rows,
        &[],
        150,
        prompt,
        "",
        None,
    );
    assert!(!unscored.iter().any(|item| item.key.starts_with("noun:")));

    let below_floor = vec![query_hit(
        "pc-memory/guides/realthing.md",
        0,
        crate::query::MINIMUM_RELEVANCE_SCORE - 0.001,
    )];
    let below = build_catalog(
        root,
        &wiki,
        &project_dir,
        &index_rows,
        &below_floor,
        150,
        prompt,
        "",
        None,
    );
    assert!(!below.iter().any(|item| item.key.starts_with("noun:")));

    let at_floor = vec![
        query_hit(
            "pc-memory/guides/realthing.md",
            0,
            crate::query::MINIMUM_RELEVANCE_SCORE,
        ),
        query_hit("pc-memory/guides/maybething.md", 0, 0.99),
        query_hit("pc-memory/guides/rejectedthing.md", 0, 0.99),
    ];
    let admitted = build_catalog(
        root,
        &wiki,
        &project_dir,
        &index_rows,
        &at_floor,
        150,
        prompt,
        "",
        None,
    );
    let noun_rows = admitted
        .iter()
        .filter(|item| item.key.starts_with("noun:"))
        .collect::<Vec<_>>();
    assert_eq!(noun_rows.len(), 1);
    assert_eq!(noun_rows[0].key, "noun:realthing");
    assert_eq!(noun_rows[0].source_key, "realthing");

    let ordinary_recent = build_catalog(
        root,
        &wiki,
        &project_dir,
        &index_rows,
        &at_floor,
        150,
        "please update RealThing now",
        "User: We already discussed RealThing.",
        None,
    );
    assert!(!ordinary_recent
        .iter()
        .any(|item| item.key == "noun:realthing"));

    let direct_recent = build_catalog(
        root,
        &wiki,
        &project_dir,
        &index_rows,
        &at_floor,
        150,
        "what is RealThing?",
        "User: We mentioned RealThing.",
        None,
    );
    assert!(direct_recent
        .iter()
        .any(|item| item.key == "noun:realthing"));

    assert!(should_skip_prompt("what is RealThing?", 4));
    assert!(noun_direct_activation(
        &wiki,
        &project_dir,
        &index_rows,
        "what is RealThing?",
        "",
    ));
    assert!(!noun_direct_activation(
        &wiki,
        &project_dir,
        &index_rows,
        "what is UnknownThing?",
        "",
    ));

    std::env::set_var("PC_NOUN_CATALOG", "0");
    assert!(!noun_direct_activation(
        &wiki,
        &project_dir,
        &index_rows,
        "what is RealThing?",
        "",
    ));
    std::env::remove_var("PC_NOUN_CATALOG");
}

#[test]
fn noun_catalog_excludes_topic_only_and_claim_only_provenance() {
    let _g = VARIANT_ENV_LOCK.lock().unwrap();
    std::env::remove_var("PC_NOUN_CATALOG");
    std::env::remove_var("PC_RESEARCH_CATALOG");
    std::env::remove_var("PC_CLAIM_CATALOG");

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let wiki = root.join("wiki");
    let project_dir = root.join("project");
    fs::create_dir_all(&project_dir).unwrap();

    let guide = crate::wiki::guide_path(&wiki, "routing-details");
    fs::create_dir_all(guide.parent().unwrap()).unwrap();
    fs::write(
        &guide,
        "---\ntitle: Routing Details\nslug: routing-details\ntopic: Routing\nsummary: Routes requests through the relay.\n---\n\n# Routing Details\n\nRoutes requests through the relay.\n",
    )
    .unwrap();
    let index_rows = crate::wiki::rebuild_index(&wiki, "2026-07-23").unwrap();

    let claim = crate::claims::ClaimRecord {
        id: "claim-only".to_string(),
        ts: "2026-07-23".to_string(),
        session: "test".to_string(),
        assertion: "ClaimWidget uses a local queue.".to_string(),
        authority: "explicit".to_string(),
        subject: "ClaimWidget".to_string(),
        evidence_text: "test evidence".to_string(),
        evidence: vec![],
        cluster_id: "cl-claim-only".to_string(),
        supersedes: vec![],
        confirmed_ts: String::new(),
        status: crate::claims::ClaimStatus::Settled,
    };
    fs::write(
        crate::claims::claims_jsonl_path(&project_dir),
        format!("{}\n", serde_json::to_string(&claim).unwrap()),
    )
    .unwrap();
    crate::nouns::write_realness_registry(
        &wiki,
        &[
            crate::nouns::RealnessNoun::new("Routing", 3),
            crate::nouns::RealnessNoun::new("ClaimWidget", 3),
        ],
    )
    .unwrap();

    let hits = vec![query_hit("pc-memory/guides/routing-details.md", 0, 0.95)];
    let catalog = build_catalog(
        root,
        &wiki,
        &project_dir,
        &index_rows,
        &hits,
        150,
        "what is Routing and ClaimWidget?",
        "",
        None,
    );
    assert!(!catalog.iter().any(|item| item.key == "noun:routing"));
    assert!(!catalog.iter().any(|item| item.key == "noun:claimwidget"));
}

#[test]
fn noun_catalog_excludes_two_live_guides_with_the_same_canonical_identity() {
    let _g = VARIANT_ENV_LOCK.lock().unwrap();
    std::env::remove_var("PC_NOUN_CATALOG");
    std::env::remove_var("PC_RESEARCH_CATALOG");
    std::env::remove_var("PC_CLAIM_CATALOG");

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let wiki = root.join("wiki");
    let project_dir = root.join("project");
    fs::create_dir_all(&project_dir).unwrap();
    for (slug, summary) in [
        ("purple-pages", "The public directory."),
        ("purplepages", "The internal directory."),
    ] {
        let guide = crate::wiki::guide_path(&wiki, slug);
        fs::create_dir_all(guide.parent().unwrap()).unwrap();
        fs::write(
            guide,
            format!(
                "---\ntitle: PurplePages\nslug: {slug}\ntopic: product\nsummary: {summary}\n---\n\n# PurplePages\n\n{summary}\n"
            ),
        )
        .unwrap();
    }
    crate::nouns::write_realness_registry(
        &wiki,
        &[crate::nouns::RealnessNoun::new("PurplePages", 3)],
    )
    .unwrap();
    let index_rows = crate::wiki::rebuild_index(&wiki, "2026-07-23").unwrap();
    let hits = vec![
        query_hit("pc-memory/guides/purple-pages.md", 0, 0.96),
        query_hit("pc-memory/guides/purplepages.md", 0, 0.95),
    ];

    for rows in [
        index_rows.clone(),
        index_rows.iter().cloned().rev().collect::<Vec<_>>(),
    ] {
        let catalog = build_catalog(
            root,
            &wiki,
            &project_dir,
            &rows,
            &hits,
            150,
            "what is PurplePages?",
            "",
            None,
        );
        assert!(
            !catalog.iter().any(|item| item.key == "noun:purplepages"),
            "ambiguous live guides must exclude the noun alias regardless of row order"
        );
        assert!(
            !noun_direct_activation(&wiki, &project_dir, &rows, "what is PurplePages?", "",),
            "ambiguity must not bypass the trivial-prompt gate"
        );
    }
}

#[test]
fn eval_catalog_enumeration_gives_prompt_matched_noun_its_backing_guide_score() {
    let _g = VARIANT_ENV_LOCK.lock().unwrap();
    std::env::remove_var("PC_NOUN_CATALOG");
    std::env::remove_var("PC_RESEARCH_CATALOG");
    std::env::remove_var("PC_CLAIM_CATALOG");

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let wiki = root.join("wiki");
    let project_dir = root.join("project");
    fs::create_dir_all(&project_dir).unwrap();
    let guide = crate::wiki::guide_path(&wiki, "purplepages");
    fs::create_dir_all(guide.parent().unwrap()).unwrap();
    fs::write(
        guide,
        "---\ntitle: PurplePages\nslug: purplepages\ntopic: product\nsummary: The project's public directory.\n---\n\n# PurplePages\n\nThe project's public directory.\n",
    )
    .unwrap();
    crate::nouns::write_realness_registry(
        &wiki,
        &[crate::nouns::RealnessNoun::new("PurplePages", 3)],
    )
    .unwrap();
    let index_rows = crate::wiki::rebuild_index(&wiki, "2026-07-23").unwrap();
    let hits = super::eval_catalog_hits(&index_rows);
    let catalog = build_catalog(
        root,
        &wiki,
        &project_dir,
        &index_rows,
        &hits,
        150,
        "what is PurplePages?",
        "",
        None,
    );
    let noun = catalog
        .iter()
        .find(|item| item.key == "noun:purplepages")
        .expect("eval enumeration must exercise the noun alias path");
    assert_eq!(noun.source_key, "purplepages");
    assert_eq!(noun.score, Some(crate::query::MINIMUM_RELEVANCE_SCORE));
}

#[test]
fn read_catalog_content_resolves_episode_key() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let wiki = root.join("docs/wiki");
    fs::create_dir_all(&wiki).unwrap();
    write_episode_card(&wiki, "2026-05-29-1-test", "Test arc", "Adopted Z.");

    let dummy_project_dir = std::env::temp_dir();
    let content =
        read_catalog_content(root, &wiki, &dummy_project_dir, "episode:2026-05-29-1-test")
            .expect("episode key must resolve to its file");
    assert!(content.contains("type: episode-card"));
    assert!(content.contains("# Episode: Test arc"));

    // A missing episode key resolves to None, not a panic or wrong file.
    assert!(
        read_catalog_content(root, &wiki, &dummy_project_dir, "episode:does-not-exist").is_none()
    );
}

#[test]
fn source_label_for_key_resolves_typed_catalog_sources() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let wiki = root.join("docs/wiki");
    let project_dir = root.join("pc-project");

    assert_eq!(
        source_label_for_key(root, &wiki, Some(&project_dir), "episode:2026-05-29-test"),
        "./docs/wiki/episodes/2026-05-29-test.md"
    );
    assert_eq!(
        source_label_for_key(root, &wiki, Some(&project_dir), "research:2026-06-12-run"),
        "./docs/wiki/research/2026-06-12-run.md"
    );
    assert_eq!(
        source_label_for_key(root, &wiki, Some(&project_dir), "noun:mint"),
        "unresolved-noun-alias:mint"
    );
    assert!(
        !source_label_for_key(root, &wiki, Some(&project_dir), "noun:mint")
            .contains("realness.jsonl")
    );
    assert_eq!(
        source_label_for_key(root, &wiki, Some(&project_dir), "claim:cl-abc123"),
        "./pc-project/claims.jsonl#claim-cl-abc123"
    );
    assert_eq!(
        source_label_for_key(root, &wiki, None, "claim:cl-abc123"),
        "claim-store#claim-cl-abc123"
    );
    assert_eq!(
        source_label_for_key(root, &wiki, Some(&project_dir), "routing-guide"),
        "./docs/wiki/guides/routing-guide.md"
    );
    assert_eq!(
        source_label_for_key(root, &wiki, Some(&project_dir), "docs/spec.md"),
        "./docs/spec.md"
    );

    let research_label =
        source_label_for_key(root, &wiki, Some(&project_dir), "research:2026-06-12-run");
    assert!(
        !research_label.contains("guides/research:"),
        "research labels must not be fabricated guide paths: {research_label}"
    );
}
