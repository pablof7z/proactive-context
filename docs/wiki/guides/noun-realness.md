---
title: Noun Realness
slug: noun-realness
topic: inject-pipeline
summary: "Noun realness is determined by accumulated user stance scored per reference: operate-on/endorse is positive, question-existence/reject is negative, neutral ment"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-17
updated: 2026-06-17
verified: 2026-06-17
compiled-from: conversation
sources:
  - session:0323ebcf-373e-4e5d-b1c6-8dac16f3055d
---

# Noun Realness

## Noun Realness

Noun realness is determined by accumulated user stance scored per reference: operate-on/endorse is positive, question-existence/reject is negative, neutral mention is near-zero. A noun crossing +3 becomes real and eligible for injection priming, while ≤−2 stays suppressed. Rejected nouns can recover: if the user later starts wielding a previously-rejected noun as real, its score accumulates positive deltas and can cross the threshold again. A noun only becomes real/primeable once its accumulated score crosses +3 across multiple sessions; a single session usually will not push much past threshold. Frequency of noun mentions is chance-level at distinguishing real from rejected nouns (AUC 0.500); it is the direction (stance) of references, not the count, that makes a noun real. <!-- [^0323e-11] -->

## Noun Sourcing

Nouns are sourced from user turns across sessions (what the user actually references), not from wiki guide titles or slugs; the prior C3 derivation from guide titles was a design error that produced confabulations the user never named. Transcript-cited definitions are required (not code-inferred) for noun extraction; inferring 'what the tui client is' from the code was explicitly rejected as a corruption chain because code is a consequence of intent, not its source. <!-- [^0323e-12] -->

## Stance Classification

The stance classifier uses an LLM to read user stance because mechanical token-matching cannot distinguish 'fix the X bug' (operate-on, positive) from 'what the fuck is X, I never asked for it' (reject, negative). A bare 'what is X?' is classified as REJECT only when it is incredulous or dismissive; it is NEUTRAL when it is genuine curiosity. <!-- [^0323e-13] -->

## Realness Scorer Bake-Off

The signed-delta accumulator (Approach A) won the realness scorer bake-off over holistic per-noun re-judgment (B) and the lifecycle state-machine (C), passing every gate including promotion-precision 1.000 and zero flip rate. Promotion-precision is the fraction of everything an approach would prime that is genuinely real; it generalizes reject-precision to neutral noise and is what disqualified Approach B (which over-primed code-symbol noise at 0.588 promotion-precision). <!-- [^0323e-14] -->

## Alias Normalization

Alias normalization collapses phrasing variants (e.g. 'the fabric provider', 'fabric-provider', 'FabricProvider') into one canonical ID before realness accumulates, lifting Approach-A real-recall from 0.333 to 0.500 on the evaluation corpus. <!-- [^0323e-15] -->

## Capture-Time Realness Writer

The capture-time realness writer reads only user turns, drops non-entity noise (code symbols, file:line, snippet fragments), runs the stance classifier batched with thinking-ON, and folds signed deltas per canonical alias key into realness.jsonl across sessions. The PC_REALNESS_MAX_REFS env var (default 150) caps per-session references to bound capture-time LLM cost for the stance pass. <!-- [^0323e-16] -->

## Realness Gate

When the realness gate is enabled and the realness registry is empty, the noun primer is inert (fires on nothing) and cannot prime a confabulation; this is the safety invariant tested by `build_inject_primer_gate_on_empty_realness_is_inert`. The realness gate default is ON; it replaces the guide-title population (not augments it), so only user-real nouns are primed, with guide definitions used only as enrichment for already-promoted nouns. The off-switches are PC_NOUNS=0 (disables the entire primer) and PC_NOUNS_REALNESS=0 (reverts to the old C3 guide-title path). <!-- [^0323e-17] -->

## Noun Primer

The first-mention noun primer injects a short block defining what the noun is plus prompt-relevant facts (the A2/facts level) when a real noun is first mentioned in a session; it does not inject for suppressed or neutral nouns. The noun primer's content level is definition plus prompt-filtered facts (A2), because the experiment showed definition-only drifted 29% and adding inferred intent (A3) reintroduced 14% drift, while A2 had 0% drift. The inject primer is composed by deterministic Rust string assembly with zero LLM calls on the hot path; facts are retrieved by walking the noun's source_refs to guide/claim bodies and lexically filtering to the top 3 by prompt-word overlap. <!-- [^0323e-18] -->

## Noun Registry and Matcher

The noun registry is derived in memory from existing guide summaries (zero LLM calls) rather than persisted to disk; the C1 definitional extraction path (which uses an LLM prompt) is gated off and was experimentally shown to be unnecessary. The production matcher uses whole-token phrase match as the high-confidence path plus a precision-leaning alias layer where multi-token nouns match on distinctive component tokens (≥5 chars, alphabetic, not in a stoplist); it never fires on bare generic or numeric tokens alone. <!-- [^0323e-19] -->

## Idiosyncrasy Filter

The idiosyncrasy filter excludes nouns the bare model already knows (classifying them as 'contained'), so the noun-grounding experiment only measures nouns where injection is genuinely counterfactual; this operationalizes the finding that implicit/idiosyncratic facts are 80–86% load-bearing while explicit direction is only 0–31%. <!-- [^0323e-20] -->

## Null Experiment Variants

The prompt-variant experiments (I1 verdict, I2 divergence, S1 select-verdict, C1 typed, C2 terminal) were an honest null: no inject or capture variant earned its place; C2 was killed for a real fact-coverage regression (restatement recall dropped 85.7%→59.5%). <!-- [^0323e-21] -->

## Predict-the-Correction Baseline

Predict-the-correction (whether the store could anticipate a correction the user would make) misses ~75% across all sources; this is the honest current distance to the 'one-shot a project from accumulated direction' north star, and 100% is not the theoretical ceiling since many corrections are genuinely unpredictable. <!-- [^0323e-22] -->
