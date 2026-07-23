use super::*;

#[test]
fn compile_preamble_default_is_byte_identical() {
    use super::{
        compile_preamble, COMPILE_PREAMBLE, COMPILE_PREAMBLE_DIVERGENCE, COMPILE_PREAMBLE_VERDICT,
    };
    let _g = VARIANT_ENV_LOCK.lock().unwrap();
    std::env::remove_var("PC_COMPILE_VARIANT");
    assert_eq!(compile_preamble(), COMPILE_PREAMBLE);
    std::env::set_var("PC_COMPILE_VARIANT", "librarian");
    assert_eq!(compile_preamble(), COMPILE_PREAMBLE);
    std::env::set_var("PC_COMPILE_VARIANT", "totally-unknown");
    assert_eq!(
        compile_preamble(),
        COMPILE_PREAMBLE,
        "unknown value must fall back to baseline"
    );
    std::env::set_var("PC_COMPILE_VARIANT", "verdict");
    let v = compile_preamble();
    assert_eq!(v, COMPILE_PREAMBLE_VERDICT);
    assert!(
        v.contains("IMPLICATION:"),
        "verdict arm must carry the implication line"
    );
    std::env::set_var("PC_COMPILE_VARIANT", "divergence");
    let d = compile_preamble();
    assert_eq!(d, COMPILE_PREAMBLE_DIVERGENCE);
    assert!(
        d.contains("ORDER BY SURPRISE"),
        "divergence arm must order by surprise"
    );
    assert!(
        d.contains("ALWAYS KEEP user direction"),
        "divergence arm must keep user direction"
    );
    std::env::remove_var("PC_COMPILE_VARIANT");
    assert!(super::COMPILE_RELEVANCE_RULES.contains("Never restate the same fact"));
    assert!(super::COMPILE_RELEVANCE_RULES.contains("Most briefings should use one to three"));
    assert!(super::COMPILE_RELEVANCE_RULES.contains("Emit at most four factual body lines"));
}

#[test]
fn select_preamble_default_is_byte_identical_and_verdict_swaps_only_the_decision() {
    use super::{select_preamble, SELECT_DECISION_BASE, SELECT_DECISION_VERDICT, SELECT_PREAMBLE};
    let _g = VARIANT_ENV_LOCK.lock().unwrap();
    // Isolate the PC_SELECT_VARIANT behavior: disable the now-default-on source-type block.
    std::env::set_var("PC_SELECT_SOURCE_TYPES", "0");
    std::env::remove_var("PC_SELECT_VARIANT");
    assert_eq!(select_preamble().as_ref(), SELECT_PREAMBLE);
    std::env::set_var("PC_SELECT_VARIANT", "base");
    assert_eq!(select_preamble().as_ref(), SELECT_PREAMBLE);
    std::env::set_var("PC_SELECT_VARIANT", "unknown");
    assert_eq!(
        select_preamble().as_ref(),
        SELECT_PREAMBLE,
        "unknown value must fall back to baseline"
    );
    // Sanity: the swap anchor must actually exist in the baseline.
    assert!(SELECT_PREAMBLE.contains(SELECT_DECISION_BASE));
    assert!(SELECT_PREAMBLE.contains("Require an exact entity and capability match"));
    assert!(SELECT_PREAMBLE.contains("does not establish behavior for a spawned subagent"));
    std::env::set_var("PC_SELECT_VARIANT", "verdict");
    let v = select_preamble().into_owned();
    assert!(
        v.contains(SELECT_DECISION_VERDICT),
        "verdict arm must carry counterfactual gate text"
    );
    assert!(
        !v.contains(SELECT_DECISION_BASE),
        "verdict arm must remove the baseline decision sentence"
    );
    // Episode-card paragraph and NOTHING_RELEVANT are preserved unchanged.
    assert!(v.contains("NOTHING_RELEVANT"));
    assert!(v.contains("SESSION EPISODE CARDS"));
    std::env::remove_var("PC_SELECT_VARIANT");
    std::env::remove_var("PC_SELECT_SOURCE_TYPES");
}

#[test]
fn select_source_types_block_defaults_on_and_off_with_flag_0() {
    use super::{select_preamble, SELECT_PREAMBLE};
    let _g = VARIANT_ENV_LOCK.lock().unwrap();
    std::env::remove_var("PC_SELECT_VARIANT");
    // Explicitly disabled: byte-identical to baseline.
    std::env::set_var("PC_SELECT_SOURCE_TYPES", "0");
    assert_eq!(select_preamble().as_ref(), SELECT_PREAMBLE);
    // DEFAULT ON (unset) and explicit on: baseline preserved as prefix + source-type guidance.
    for v in [None, Some("1")] {
        match v {
            None => std::env::remove_var("PC_SELECT_SOURCE_TYPES"),
            Some(s) => std::env::set_var("PC_SELECT_SOURCE_TYPES", s),
        }
        let d = select_preamble().into_owned();
        assert!(
            d.contains("SOURCE-TYPE GUIDANCE"),
            "default-on must append the block (v={v:?})"
        );
    }
    std::env::set_var("PC_SELECT_SOURCE_TYPES", "1");
    let p = select_preamble().into_owned();
    assert!(
        p.starts_with(SELECT_PREAMBLE),
        "baseline must be preserved as prefix"
    );
    assert!(p.contains("SOURCE-TYPE GUIDANCE"));
    assert!(p.contains("[research-record]") && p.contains("[noun-entry]") && p.contains("[claim]"));
    // A2′ tuning: episode cards must be explicitly retained for history/why probes, and the
    // old suppressive "current truth" caution must be gone from SELECT.
    assert!(p.contains("Select EVERY episode card"));
    assert!(!p.contains("as CURRENT truth unless"));
    // Composes with the verdict SELECT variant without losing either piece.
    std::env::set_var("PC_SELECT_VARIANT", "verdict");
    let pv = select_preamble().into_owned();
    assert!(pv.contains("SOURCE-TYPE GUIDANCE"));
    assert!(pv.contains(super::SELECT_DECISION_VERDICT));
    std::env::remove_var("PC_SELECT_VARIANT");
    std::env::remove_var("PC_SELECT_SOURCE_TYPES");
}
