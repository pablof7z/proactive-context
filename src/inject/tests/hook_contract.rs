use super::*;

#[test]
fn hook_cannot_deliver_noun_facts_on_retrieval_failure_or_short_circuit() {
    // Noun capture/indexing remains available elsewhere, but proposed,
    // implicit, superseded, or zero-overlap noun facts must not have a
    // direct hook path around SELECT+COMPILE and artifact validation.
    let hook = include_str!("../hook.rs");
    assert!(!hook.contains("resolve_noun_primer"));
    assert!(!hook.contains("noun_resolution"));
    assert!(!hook.contains("NounCommit"));
    assert_eq!(
        hook.matches("commit_context(").count(),
        1,
        "only the validated compiled-artifact arm may commit hook context"
    );

    let retrieval_failure = hook
        .split_once("let retrieved_hits = match")
        .expect("retrieval arm")
        .1
        .split_once("let retrieved_hit_count")
        .expect("end retrieval arm")
        .0;
    assert!(!retrieval_failure.contains("commit_context("));
    assert!(!retrieval_failure.contains("Some(&"));

    let short_circuit = hook
        .split_once("Ok(Ok(NavigateResult::ShortCircuit")
        .expect("short-circuit arm")
        .1
        .split_once("Ok(Err(e))")
        .expect("end short-circuit arm")
        .0;
    assert!(!short_circuit.contains("commit_context("));
    assert!(!short_circuit.contains("Some(&"));
    assert!(short_circuit.contains("&out_mode"));
    assert!(short_circuit.contains("None,"));
}

#[test]
fn compiled_context_commit_keeps_exhaustion_distinct_from_ledger_failure() {
    let _home_lock = crate::config::PC_HOME_TEST_LOCK.lock().unwrap();
    let home = tempfile::tempdir().unwrap();
    let _pc_home = crate::config::ScopedPcHome::set(home.path());

    let healthy_root = tempfile::tempdir().unwrap();
    let init = std::process::Command::new("git")
        .arg("init")
        .arg("--quiet")
        .arg("--initial-branch=master")
        .arg(healthy_root.path())
        .status()
        .unwrap();
    assert!(init.success());

    assert!(matches!(
        commit_context(
            healthy_root.path(),
            "healthy-session",
            Some("Compiled"),
            "A compiled fact."
        ),
        ContextCommit::Delivered(_)
    ));
    assert!(matches!(
        commit_context(
            healthy_root.path(),
            "healthy-session",
            Some("Compiled"),
            "A compiled fact."
        ),
        ContextCommit::Exhausted
    ));

    let broken_root = tempfile::tempdir().unwrap();
    let init = std::process::Command::new("git")
        .arg("init")
        .arg("--quiet")
        .arg("--initial-branch=master")
        .arg(broken_root.path())
        .status()
        .unwrap();
    assert!(init.success());
    let ledger_path = crate::config::project_context_dir(broken_root.path()).join("ledger");
    std::fs::write(&ledger_path, "not a directory").unwrap();

    assert!(matches!(
        commit_context(
            broken_root.path(),
            "broken-compiled-session",
            Some("Compiled"),
            "A compiled fact."
        ),
        ContextCommit::LedgerUnavailable
    ));

    let long_prompt = "x".repeat(200);
    let done = ledger_unavailable_done_payload(2, &long_prompt);
    assert_eq!(done["outcome"], "empty");
    assert_eq!(done["failure_stage"], "ledger");
    assert_eq!(done["reason"], "ledger_unavailable");
    assert_ne!(done["reason"], "already_delivered");
    assert_eq!(
        done["prompt_preview"],
        crate::events::truncate(&long_prompt, 150)
    );
}
