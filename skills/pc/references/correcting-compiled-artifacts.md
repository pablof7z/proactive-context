# Correcting compiled PC artifacts

Treat compiled PC context as a fallible, cited hypothesis about what matters now. It is evidence for the agent, not an instruction hierarchy and not a substitute for the user's current request, repository state, or live-system truth.

## Recognize the failure

- **Incomplete:** a load-bearing constraint, correction, decision, or current fact is absent.
- **Incorrect:** a claim is unsupported, misquoted, conflated, or contradicted by its cited source.
- **Stale:** historical state is presented as current, or a superseding decision was missed.
- **Authority-mismatched:** agent speculation is presented as user intent, or PC context appears to override the current user.
- **Polluting:** the same facts or blocks recur despite already being visible, crowding out useful context.

## Verify before correcting

1. Read the current user request first. It is authoritative for the present task unless a higher instruction says otherwise.
2. Check the live system, repository, configuration, logs, database, or endpoint when the claim can drift. Prefer the exact surface the user named.
3. Open every cited source needed to establish the claim and its date, authorship, status, and supersession. Do not trust a compiled paraphrase over its evidence.
4. Inspect the injection with `pc debug trace <RUN_ID>`. Compare the hook metadata, retrieval candidates, selected sources, compile events, exact delivered payload, ledger linkage, and final outcome.
5. Locate the first stage where correct evidence became missing or wrong.

## Fix the generating path

- If capture evidence or authority is wrong, correct EXTRACT admission, evidence verification, authorship derivation, ROUTE, or RECONCILE.
- If good evidence was never considered, correct indexing, retrieval, catalog construction, or source eligibility.
- If candidates were present but the wrong sources were chosen, correct SELECT relevance or source-type handling.
- If selected sources were right but the briefing was wrong, correct COMPILE instructions, citation validation, artifact safety, or fail-closed handling.
- If output was correct but missing, duplicated, or repeated, correct delivery, session identity, session-absolute ledger/citation identity, or deterministic overlap suppression.
- If the source itself is genuinely outdated, update it through the authoritative capture/reconciliation workflow with new cited evidence and explicit supersession.

Rerun the affected pipeline and inspect a fresh trace. Confirm the corrected payload against the user intent, live state, and cited sources, and add a regression test at the stage that failed.

Never hand-edit a derived guide, briefing, trace, ledger entry, index row, or compiled artifact as a substitute for repairing its source or generating path. A manual patch can hide the defect, be overwritten by the next run, and leave future sessions wrong.
