use super::*;

fn run(
    fixture: &PipelineFixture,
    backend: &mut ScriptedPipelineBackend,
) -> std::result::Result<(NavigateResult, Option<PipelineNavigationTrace>), PipelineNavigationFailure>
{
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(wiki_navigate_and_compile_with_backend(
        backend,
        "what is PurplePages?",
        "",
        &fixture.hits,
        &fixture.wiki,
        &fixture.index_rows,
        &fixture.root,
        &fixture.project_dir,
        4,
        300,
        false,
        "",
        true,
        &fixture.overlap,
        true,
    ))
}

#[test]
fn malformed_select_gets_one_bounded_repair_attempt() {
    let _guard = VARIANT_ENV_LOCK.lock().unwrap();
    let fixture = PipelineFixture::noun();
    let mut backend = ScriptedPipelineBackend {
        replies: VecDeque::from([
            "QUERY: what is PurplePages?".to_string(),
            "noun:purplepages".to_string(),
            format!(
                "TITLE: PurplePages grounding\nPurplePages is the public directory. ({}:10)",
                fixture.source_label
            ),
        ]),
        calls: vec![],
    };

    let (result, trace) = run(&fixture, &mut backend).unwrap();
    assert!(matches!(result, NavigateResult::Briefing { .. }));
    let trace = trace.unwrap();
    assert_eq!(backend.calls.len(), 3);
    assert_eq!(trace.provider_call_count, 3);
    assert!(trace
        .selection_response
        .as_deref()
        .unwrap()
        .contains("ATTEMPT 2"));
    assert!(backend.calls[1].0.contains("FORMAT REPAIR RETRY"));
}

#[test]
fn malformed_compile_gets_one_bounded_repair_attempt() {
    let _guard = VARIANT_ENV_LOCK.lock().unwrap();
    let fixture = PipelineFixture::noun();
    let mut backend = ScriptedPipelineBackend {
        replies: VecDeque::from([
            "noun:purplepages".to_string(),
            "TITLE: PurplePages grounding\nUncited prose.".to_string(),
            format!(
                "TITLE: PurplePages grounding\nPurplePages is the public directory. ({}:10)",
                fixture.source_label
            ),
        ]),
        calls: vec![],
    };

    let (result, trace) = run(&fixture, &mut backend).unwrap();
    assert!(matches!(result, NavigateResult::Briefing { .. }));
    let trace = trace.unwrap();
    assert_eq!(backend.calls.len(), 3);
    assert_eq!(trace.provider_call_count, 3);
    assert_eq!(trace.outcome, "compiled");
    assert!(backend.calls[2].0.contains("FORMAT REPAIR RETRY"));
    assert!(backend.calls[2].0.contains("VALIDATOR ERROR TO CORRECT"));
    assert!(backend.calls[2].0.contains("malformed_compile_response"));
}
