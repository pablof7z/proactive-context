use super::*;

#[test]
fn compiled_pipeline_trace_uses_scripted_provider_and_records_exact_stages() {
    let _g = VARIANT_ENV_LOCK.lock().unwrap();
    std::env::remove_var("PC_CLAIM_CATALOG");
    std::env::remove_var("PC_RESEARCH_CATALOG");
    std::env::remove_var("PC_NOUN_CATALOG");

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("subject");
    let wiki = tmp.path().join("wiki");
    let project_dir = tmp.path().join("project-state");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&wiki).unwrap();
    fs::create_dir_all(&project_dir).unwrap();
    let guide = crate::wiki::guide_path(&wiki, "auth-flow");
    fs::create_dir_all(guide.parent().unwrap()).unwrap();
    fs::write(
        &guide,
        "---\n\
title: Auth flow\n\
slug: auth-flow\n\
topic: auth\n\
summary: Current authentication route\n\
tags: [auth]\n\
volatility: warm\n\
confidence: high\n\
created: 2026-07-23\n\
updated: 2026-07-23\n\
verified: 2026-07-23\n\
compiled_from: test\n\
sources: [session:test]\n\
---\n\n\
The live route is POST /v2/session.\n",
    )
    .unwrap();
    let index_rows = crate::wiki::rebuild_index(&wiki, "2026-07-23").unwrap();
    let source_label = super::source_label_for_key(&root, &wiki, Some(&project_dir), "auth-flow");
    let hits = vec![crate::query::QueryResult {
        path: "auth-flow.md".to_string(),
        chunk_index: 0,
        content: "The live route is POST /v2/session.".to_string(),
        content_hash: "hash-auth".to_string(),
        score: 0.93,
    }];
    let mut backend = ScriptedPipelineBackend {
        replies: VecDeque::from([
            "auth-flow".to_string(),
            format!("TITLE: current auth route\nUse POST /v2/session. ({source_label}:14)"),
        ]),
        calls: vec![],
    };
    let overlap = crate::context_overlap::ContextOverlap::from_hook(
        "Which endpoint creates a session?",
        None,
        "eval",
        0,
    );
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (outcome, trace) = runtime
        .block_on(super::wiki_navigate_and_compile_with_backend(
            &mut backend,
            "Which endpoint creates a session?",
            "",
            &hits,
            &wiki,
            &index_rows,
            &root,
            &project_dir,
            4,
            300,
            false,
            "",
            false,
            &overlap,
            true,
        ))
        .unwrap();
    let trace = trace.expect("test requested a pipeline trace");

    match outcome {
        super::NavigateResult::Briefing { text, guides_read } => {
            assert!(text.contains("POST /v2/session"));
            assert_eq!(guides_read, vec!["auth-flow"]);
        }
        super::NavigateResult::ShortCircuit { .. } => panic!("expected compiled artifact"),
    }
    assert_eq!(backend.calls.len(), 2);
    assert!(backend.calls[0].0.contains("CATALOG:"));
    assert!(backend.calls[1].0.contains("SOURCE DOCUMENTS"));
    assert_eq!(trace.provider_call_count, 2);
    assert_eq!(trace.selected_keys, vec!["auth-flow"]);
    assert_eq!(trace.selected_sources.len(), 1);
    assert!(trace.selected_sources[0]
        .content
        .contains("POST /v2/session"));
    assert!(trace
        .compiled_artifact
        .as_deref()
        .unwrap()
        .contains("POST /v2/session"));
    assert!(trace.select_latency_ms.is_some());
    assert!(trace.compile_latency_ms.is_some());
}

#[test]
fn noun_alias_runs_select_compile_and_cites_only_its_backing_guide() {
    let _g = VARIANT_ENV_LOCK.lock().unwrap();
    std::env::remove_var("PC_NOUN_CATALOG");
    std::env::remove_var("PC_CLAIM_CATALOG");
    std::env::remove_var("PC_RESEARCH_CATALOG");

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("subject");
    let wiki = tmp.path().join("wiki");
    let project_dir = tmp.path().join("project-state");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&project_dir).unwrap();
    let guide = crate::wiki::guide_path(&wiki, "purplepages");
    fs::create_dir_all(guide.parent().unwrap()).unwrap();
    fs::write(
        &guide,
        "---\ntitle: PurplePages\nslug: purplepages\ntopic: product\nsummary: PurplePages is the project's public directory.\n---\n\n# PurplePages\n\nPurplePages is the project's public directory.\n",
    )
    .unwrap();
    let index_rows = crate::wiki::rebuild_index(&wiki, "2026-07-23").unwrap();
    crate::nouns::write_realness_registry(
        &wiki,
        &[crate::nouns::RealnessNoun::new("PurplePages", 3)],
    )
    .unwrap();
    let source_label = super::source_label_for_key(&root, &wiki, Some(&project_dir), "purplepages");
    let hits = vec![query_hit("pc-memory/guides/purplepages.md", 0, 0.94)];
    let mut backend = ScriptedPipelineBackend {
        replies: VecDeque::from([
            "noun:purplepages".to_string(),
            format!(
                "TITLE: PurplePages grounding\nPurplePages is the project's public directory. ({source_label}:10)"
            ),
        ]),
        calls: vec![],
    };
    let overlap =
        crate::context_overlap::ContextOverlap::from_hook("what is PurplePages?", None, "eval", 0);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (outcome, trace) = runtime
        .block_on(super::wiki_navigate_and_compile_with_backend(
            &mut backend,
            "what is PurplePages?",
            "",
            &hits,
            &wiki,
            &index_rows,
            &root,
            &project_dir,
            4,
            300,
            false,
            "",
            true,
            &overlap,
            true,
        ))
        .unwrap();
    let trace = trace.expect("trace requested");

    match outcome {
        super::NavigateResult::Briefing { text, guides_read } => {
            assert!(text.contains("PurplePages is the project's public directory."));
            assert_eq!(guides_read, vec!["purplepages"]);
        }
        super::NavigateResult::ShortCircuit { .. } => panic!("expected noun-backed briefing"),
    }
    assert_eq!(backend.calls.len(), 2);
    assert!(backend.calls[0].0.contains("noun:purplepages"));
    assert!(backend.calls[1].0.contains("kind=\"current-guide\""));
    assert!(!backend.calls[1].0.contains("realness.jsonl"));
    assert!(!backend.calls[1].0.contains("unresolved-noun-alias"));

    let noun_candidate = trace
        .candidates
        .iter()
        .find(|candidate| candidate.key == "noun:purplepages")
        .expect("noun candidate");
    assert_eq!(noun_candidate.source_key, "purplepages");
    assert_eq!(trace.selected_keys, vec!["noun:purplepages"]);
    assert_eq!(trace.selected_sources.len(), 1);
    assert_eq!(trace.selected_sources[0].catalog_key, "noun:purplepages");
    assert_eq!(trace.selected_sources[0].source_key, "purplepages");
    assert!(
        !project_dir.join("noun-ledger").exists(),
        "safe noun aliases must not create the retired primed-noun ledger"
    );
}
