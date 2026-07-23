use super::*;

#[test]
fn noun_selector_abstention_and_invalid_compile_never_produce_a_briefing() {
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
        "---\ntitle: PurplePages\nslug: purplepages\ntopic: product\nsummary: PurplePages is the public directory.\n---\n\n# PurplePages\n\nPurplePages is the public directory.\n",
    )
    .unwrap();
    let index_rows = crate::wiki::rebuild_index(&wiki, "2026-07-23").unwrap();
    crate::nouns::write_realness_registry(
        &wiki,
        &[crate::nouns::RealnessNoun::new("PurplePages", 3)],
    )
    .unwrap();
    let hits = vec![query_hit("pc-memory/guides/purplepages.md", 0, 0.94)];
    let overlap =
        crate::context_overlap::ContextOverlap::from_hook("what is PurplePages?", None, "eval", 0);
    let runtime = tokio::runtime::Runtime::new().unwrap();

    let mut abstaining = ScriptedPipelineBackend {
        replies: VecDeque::from(["NOTHING_RELEVANT".to_string()]),
        calls: vec![],
    };
    let (outcome, _) = runtime
        .block_on(super::wiki_navigate_and_compile_with_backend(
            &mut abstaining,
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
    assert!(matches!(
        outcome,
        super::NavigateResult::ShortCircuit { .. }
    ));
    assert_eq!(
        abstaining.calls.len(),
        1,
        "COMPILE must not run after abstention"
    );

    let mut malformed_selection = ScriptedPipelineBackend {
        replies: VecDeque::from([
            "I might choose PurplePages.".to_string(),
            "Still no protocol decision.".to_string(),
        ]),
        calls: vec![],
    };
    let malformed = runtime.block_on(super::wiki_navigate_and_compile_with_backend(
        &mut malformed_selection,
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
    ));
    let malformed = match malformed {
        Ok(_) => panic!("malformed SELECT output must fail closed"),
        Err(error) => error,
    };
    assert_eq!(
        malformed.trace.as_ref().unwrap().outcome,
        "selection_parse_error"
    );
    assert_eq!(malformed_selection.calls.len(), 2);
    assert_eq!(malformed.trace.as_ref().unwrap().provider_call_count, 2);

    let mut invalid_compile = ScriptedPipelineBackend {
        replies: VecDeque::from([
            "noun:purplepages".to_string(),
            "TITLE: PurplePages grounding\nUncited model prose.".to_string(),
            "TITLE: PurplePages grounding\nStill uncited model prose.".to_string(),
        ]),
        calls: vec![],
    };
    let result = runtime.block_on(super::wiki_navigate_and_compile_with_backend(
        &mut invalid_compile,
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
    ));
    let compile_failure = match result {
        Ok(_) => panic!("invalid uncited compile must fail closed"),
        Err(error) => error,
    };
    assert!(compile_failure
        .to_string()
        .contains("malformed_compile_response"));
    let compile_trace = compile_failure
        .trace
        .as_ref()
        .expect("replay capture must survive compile validation failure");
    assert_eq!(compile_trace.outcome, "compile_error");
    assert_eq!(compile_trace.provider_call_count, 3);
    assert_eq!(compile_trace.selected_keys, vec!["noun:purplepages"]);
    assert_eq!(compile_trace.selected_sources.len(), 1);
    assert_eq!(compile_trace.selected_sources[0].source_key, "purplepages");
    let compile_replay = super::replay_outcome_from_navigation(Err(compile_failure)).unwrap();
    match compile_replay {
        super::PipelineReplayOutcome::Failed { error, trace } => {
            assert!(error.contains("malformed_compile_response"));
            assert_eq!(trace.outcome, "compile_error");
            assert_eq!(trace.candidates.len(), 2);
        }
        super::PipelineReplayOutcome::Completed { .. } => {
            panic!("compile failure must remain a replay failure")
        }
    }
    assert_eq!(invalid_compile.calls.len(), 3);

    let mut failing = FailingPipelineBackend { calls: 0 };
    let failure = runtime.block_on(super::wiki_navigate_and_compile_with_backend(
        &mut failing,
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
    ));
    let failure = match failure {
        Ok(_) => panic!("provider failure must not produce a navigation result"),
        Err(error) => error,
    };
    assert!(failure.to_string().contains("scripted provider failure"));
    let provider_trace = failure
        .trace
        .as_ref()
        .expect("replay capture must survive SELECT provider failure");
    assert_eq!(provider_trace.outcome, "select_provider_error");
    assert_eq!(provider_trace.provider_call_count, 1);
    let noun = provider_trace
        .candidates
        .iter()
        .find(|candidate| candidate.key == "noun:purplepages")
        .expect("noun candidate mapping must survive provider failure");
    assert_eq!(noun.source_key, "purplepages");
    let provider_replay = super::replay_outcome_from_navigation(Err(failure)).unwrap();
    match provider_replay {
        super::PipelineReplayOutcome::Failed { error, trace } => {
            assert!(error.contains("scripted provider failure"));
            assert_eq!(trace.outcome, "select_provider_error");
            assert!(trace
                .candidates
                .iter()
                .any(|candidate| candidate.key == "noun:purplepages"
                    && candidate.source_key == "purplepages"));
        }
        super::PipelineReplayOutcome::Completed { .. } => {
            panic!("provider failure must remain a replay failure")
        }
    }
    assert_eq!(
        failing.calls, 1,
        "failure must not invoke a fallback model path"
    );
}
