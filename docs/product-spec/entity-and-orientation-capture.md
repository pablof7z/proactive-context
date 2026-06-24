# proactive-context — Entity & Orientation Capture (the Nouns Problem)

**Status:** Proposed (v0.5) — extends [Citation-Anchored Capture (v0.4)](./citation-anchored-capture.md)

**One-line description:**
Today's wiki captures how the product *behaves* but never says what the product *is* or what its *nouns* mean. It is a pile of true statements about a machine with no diagram of the machine. This spec adds a first-class **entity layer** — the project's nouns and how they relate — that the behavioral spec statements hang off of, plus a **top-level orientation** that answers "what is this." The nouns are not invented from implementation; they are admitted from the conversation and defined from the project's own in-session investigation of them.

> **Meta note:** Like its sibling specs, this document is structured as the artifact the system should eventually produce automatically: it names the entities (Entity, Definition, Open Question, Orientation), defines them, and attaches the requirements to them. It is its own worked example of "define the nouns first."

---

## Problem

The capture pipeline extracts **atomic, cited spec facts** — desired-state assertions, one fact each ("the balance command fetches `kind:7375`", "mint discovery shows only Cashu mints", "transaction history is sorted newest-first"). It does this well. But every fact it produces is a *predicate about a system that is assumed to already exist*. The pipeline has **no first-class notion of the subject those predicates are about** — the entity, the noun, the thing being specified.

The result, observed on a real captured project (a Rust CLI Cashu/nostr wallet):

1. **The nouns are undefined.** Nothing in the wiki says what a *mint*, a *nutzap*, a *proof*, a *relay list*, a *token event*, or *npub.cash* is. A reader — usually an LLM agent injected cold onto a task — is handed dozens of behaviors over a vocabulary it was never given.

2. **There is no orientation.** No page answers "what is this project, what is it for, what are its parts, how do they fit." The closest auto-generated overview is a **run-on concatenation of feature-facts**, because the facts have no entity to group under, so no hierarchy is possible.

3. **The taxonomy has no bucket for definitions — so they are dropped even when present.** The extract taxonomy (decisions, requirements, behaviors, constraints, gotchas) cannot represent "X *is* Y." This is not merely a presupposition gap. In the captured project, a session contained an explicit, fully citable concept glossary ("Facts confirmed: kind 38172 = Cashu mint, `u` tag = URL, content = optional name JSON"; a "Key Concepts" block defining NIP-60/61/65/44, Cashu, purplepag.es). EXTRACT was fed those lines verbatim and emitted **zero** definitional claims from them. The definitions were sitting in the transcript, citable, and discarded for lack of a slot.

4. **The emergent topic clusters are pseudo-nouns.** The wiki's topic grouping (`cashu-transactions`, `relay-connections`, `wallet-state`) is emergent-from-fact-similarity, not an entity model. They read like nouns but are actually fact-buckets; the real entity set (Mint, Nutzap, Proof, Relay List, the `kind:NNNN` events) is nowhere.

### Why this matters against the north-star

The project's [north-star test](./citation-anchored-capture.md) is: *drop the wiki onto a fresh project, ask a model to one-shot the app, and it should reproduce the product at full nuance.* A model cannot regenerate an app from a list of behaviors when it does not know what the entities those behaviors operate on *are*, or how they connect. The missing ontology is not cosmetic — it is a direct, measurable gap in the regenerable spec. The nouns are the scaffolding the behaviors hang on; without them the behaviors do not cohere into a buildable whole.

---

## The reframe: behaviors hang off an entity spine

The organizing unit of the wiki today is the **fact**; entities exist only accidentally, as clusters of similar facts. Invert it. **The nouns are the stable spine; the facts are the volatile flesh.** What a *mint* or a *proof* is barely changes across the life of a project; the behaviors around it change every session. The pipeline currently captures the volatile layer with high fidelity and lets the stable layer evaporate.

Two distinct things must be kept separate:

- **Noun detection / promotion** — "this project cares about a concept called *NIP60*." This is cheap, mechanical, and authority-gated (below).
- **Noun definition** — "what *NIP60* is, in this project." This needs a *source*, and the source must not be implementation (see *Rejected approaches*).

A spec fact gains a second axis: not only a *type* (behavior, constraint, decision) but a **subject** — the entity it is about. Once facts are addressed by subject, the run-on overview dissolves, because there is finally a key to group on. "Balance fetches `kind:7375`" stops being free-floating and becomes a behavior *of Balance, which reads Token Events*.

---

## Requirements

### R1 — Entities are first-class

The wiki MUST represent project nouns as first-class entities, each an anchor that behavioral facts attach to. Entities relate to each other in a typed graph (e.g. *issues*, *contains*, *references*, *sent-over*); that graph is the "diagram of the machine" a cold reader is missing. The orientation node is then derivable — it is the principal entities and the edges between them, rendered.

### R2 — Promotion is authority-gated, not implementation-derived

A noun is **registered** when it appears in the conversation, regardless of who said it (keep-all). A noun is **promoted to a defined entity** when the **user** engages it — this rides the *existing* explicit/implicit authority tag (the line-to-role map already computed in EXTRACT), not a new model judgment.

- "Add NIP60 support" → *NIP60* is user-uttered → promoted.
- "ensure the `PubkeyDecoderService` is idempotent" → user engaged it → promoted.
- `PubkeyDecoderService` appearing only in agent turns or code → registered, tagged implicit, **not** surfaced as a defined entity unless/until the user engages it.

This is the same pattern already settled for direction tagging ([explicit/implicit as metadata, keep-all, nothing deleted](./citation-anchored-capture.md)): source controls *promotion and surfacing*, never *capture vs discard*. It resolves the over-definition risk (code has thousands of incidental nouns; the user's vocabulary is the project's vocabulary) without reintroducing a drop rule.

### R3 — Definitions are sourced from in-session investigation, transcript-cited

When the agent investigates a noun in-session (reads a spec, explores the domain) it **states its findings in the transcript**. Those findings ARE the definition, and they are citable to the transcript line ranges where they appear. This keeps definitions inside the existing **integrity-by-construction** invariant — the model supplies indices, Rust slices verbatim, fabrication is unreachable — with **no new provenance tier**. A definition is just another cited spec statement whose subject is an entity.

### R4 — Undefined user-nouns become Open Questions, resolved across sessions

A noun the user engaged but that has no available definition (never investigated, never explained) MUST become an **Open Question** ("what is the TUI client?") rather than a fabricated or low-confidence definition. The un-citable case collapses cleanly into the open-question state the system already has. Resolution is a **cross-session loop, not a synchronous interrupt**:

1. **Capture (Stop)** detects user-engaged undefined nouns and records them as open questions. Persistence lives here because the whole session — including everything the agent investigated — is only available at Stop, and there is no latency budget to respect.
2. **Inject** surfaces the open question into the agent's context as a cheap *nudge* ("the project mentions X but does not define it; if you learn what it is, say so"). It MUST NOT block the turn to ask-and-wait — that violates the hook latency ceiling, and pre-investigation the agent cannot answer anyway.
3. A later session investigates or explains the noun.
4. The next **Stop** harvests the answer and promotes the open question to a defined entity (R3).

Accepted cost: a noun can remain an open question across several sessions before it is answered and harvested. This is acceptable — the loop is self-healing — but it is a real latency in knowledge completeness and is stated, not hidden.

### R5 — Deep-research guides capture investigation over known nouns

When the agent does substantive investigative work over a promoted noun ("how does NIP60 work in this CLI? how do we choose the mint?"), capture MUST persist that investigation as a **deep-research guide attached to the entity**. This is the highest-value, least-reconstructable content in the system — it is the *reasoning that maps the external spec onto this app*, which exists nowhere in the code. It is gated on **promoted-noun + lasting spec implication**, not every exploratory read (the existing "skip transient debugging" rule is the discriminator), so transient greps are not persisted.

### R6 — Entity bodies are scoped to project-specific delta, not textbook definitions

An entity's body MUST capture only what is *project-specific or divergent from the consumer's prior*, never a generic re-definition of a well-known domain noun. The consumer is an LLM that already knows what a Cashu mint generically is; re-defining it wastes inject budget, invites staleness, and is exactly the generic slop the project's "depth over summaries" principle exists to reject.

- ❌ "A mint is a Cashu server that issues ecash."
- ✅ "A mint here is referenced by URL, discovered via `kind:38172`, and **must be shared with the recipient** for a nutzap to land."

Generic, user-uttered nouns with no project-specific content remain thin anchors (a node facts can attach to) rather than prose pages.

### R7 — A top-level orientation seed

The single highest-altitude statement — *what is this project, who is it for, what is the spine* — is the one thing atomic, line-cited extraction **structurally cannot produce** (it has no vantage point above a single line). The system MUST support a small, human-authored (or human-confirmed) orientation seed for the root node, rather than synthesizing altitude the pipeline cannot ground. This also covers the one residual the in-session loop cannot reach: a **project-specific meaning both user and agent already knew and never spelled out** (no confusion → no investigation → no open question raised). That, and only that, is where a human seed earns its keep.

---

## Sequencing — the cheap fix first

The misses divide into two cases, and they are not equally common:

- **Present-and-dropped** — the definition was stated in-session and EXTRACT discarded it for lack of a bucket. *Empirically dominant* in the captured project (see Problem §3).
- **Genuinely-absent** — the noun was never investigated or explained.

Therefore, in priority order:

1. **First (handles the dominant case, no new machinery):** add the entity/definition bucket to EXTRACT and the **entity↔fact subject link**, so the definitions already present in transcripts stop being discarded and facts gain a noun to hang on. This alone removes the run-on-overview symptom.
2. **Promotion via the existing explicit/implicit map** (R2) — nearly free if it reuses the line-role step EXTRACT already runs.
3. **Then (the tail):** the Open-Question cross-session loop (R4) and deep-research guides (R5).
4. **Throughout:** delta-scoping (R6) as the quality guardrail; the orientation seed (R7) for the top.

The order matters: the elaborate cross-session loop is the *second* fix. The first is the boring taxonomy bucket, which is where the observed bleeding actually is.

---

## What this builds on (and must not contradict)

- **Integrity by construction** ([citation-anchored-capture](./citation-anchored-capture.md)): definitions are transcript-cited (R3); the model never types citation text. No new provenance tier.
- **Keep-all + metadata tagging**: promotion (R2) is a surfacing tag, never a drop gate — consistent with the finalized explicit/implicit decision.
- **Open questions**: R4 reuses the existing open-question concept rather than introducing a parallel mechanism.
- **Stop capture + inject nudge**: R4's loop reuses the SessionEnd capture path and the inject path; it adds no synchronous user-facing prompt.
- **llm-wiki review** ([llm-wiki-review](./llm-wiki-review.md)): llm-wiki already splits extraction into *concepts (nouns) / facts / relationships*; this spec adopts that **shape**. But llm-wiki's definitions come from rich source documents that already contain them — a luxury a coding transcript lacks — so this spec **re-sources** the entity body from in-session investigation, not from the (research-from-web) assumption that the input is definitional. The existing review's adoption list did not include this concept/fact/relationship split; this spec is where it is adopted.

---

## Rejected approaches (with rationale)

- **Infer the noun-set and definitions from code.** Rejected. It inverts the direction of authority: intent → code (possibly buggy) → code-reading (possibly misread) → definition, so the definition inherits both implementation bugs and interpretation error. It over-captures incidental implementation nouns (`PubkeyDecoderService`) that are not project ontology. And it requires a large model to interpret code. The code is a *consequence* of the spec; defining the spec's nouns from its consequence is backwards. (Code may still serve as *corroborating* citation for a noun the user already promoted — but it is never the trigger or the primary source.)
- **A separate "model-provisional" definition trust tier.** Rejected as unnecessary. Because the agent states findings in the transcript, the common case stays transcript-cited; the un-citable case is better modeled as an **Open Question** (R4) than as a low-trust definition that looks authoritative.
- **Synchronous question-raising at inject time.** Rejected. Blocking the turn to ask "what is X?" and wait blows the hook latency ceiling, and the agent cannot answer before it has investigated. Inject's role is a non-blocking nudge (R4.2).
- **A hard "user-nouns captured, code/agent-nouns dropped" gate.** Rejected. It contradicts the settled keep-all model. Source controls promotion/surfacing, not capture (R2).
- **Re-defining well-known domain nouns.** Rejected by R6 — generic definitions are slop and a staleness liability.
- **Auto-synthesizing the top-level "what is this."** Rejected for the root node — atomic extraction cannot reach that altitude; use a human seed (R7).

---

## Open questions / to verify before implementation

- **Promotion reuse.** Confirm noun-promotion can ride the existing explicit/implicit line-role map (a mechanical Rust step) rather than requiring fresh model inference. If it needs new inference, price that.
- **Entity granularity.** What counts as one entity vs. a sub-aspect of another (is `kind:7375` its own entity or an attribute of *Token Event*)? Risk of near-singleton entity sprawl mirrors the [topic-routing](./topic-routing-and-staleness-plan.md) singleton problem; the same broad-clustering discipline may be needed.
- **Orientation seed authoring.** Where does the human seed live, how is it requested, and how is it kept from going stale as the project evolves (does it become a confirm-on-change prompt rather than free prose)?
- **Open-question harvest reliability.** How aggressively should inject nudge on open-question nouns, and how is a noun marked "answered" vs. "still open" across sessions without re-asking endlessly?
- **Eval.** Does adding the entity spine measurably improve the north-star regeneration test (diff regenerated app against the real one), or only improve human readability? The regeneration diff is the objective signal.

---

## Worked example (the captured nostr/Cashu wallet)

What the wiki has today (behaviors, no nouns):
> "The balance command fetches `kind:7375` events, NIP-44 decrypts them with the user's own key, sums Cashu proof amounts." — true, cited, and **meaningless to a reader who does not know what a proof, a token event, or self-encryption is.**

What this spec produces:

- **Orientation seed (R7, human):** "A CLI Cashu wallet on nostr. You onboard with an nsec; it finds your relays and mints; you send/receive ecash as nutzaps. Rust on nostr-sdk + cdk."
- **Entity: Mint** (promoted because the user said "configure their mints"; body delta-scoped per R6): "Referenced by URL, discovered via `kind:38172`; must be shared with the recipient for a nutzap to succeed." Behaviors attach: *discovery shows only Cashu mints*, *dedup across relays*.
- **Entity: Token Event (`kind:7375`)** (definition from the in-session "Key Concepts" block, transcript-cited per R3). Behaviors attach: *balance reads these; they are self-encrypted*.
- **Open Question (R4):** if the user had said "lock the LMDB database" with no in-session explanation of the TUI client's role, "what is the TUI client?" is recorded, nudged on next inject, harvested when answered.
- **Deep-research guide (R5):** "how do we choose the mint?" — the agent's investigation, attached to *Mint* and *Nutzap*.

The behaviors did not change. They now hang off nouns a cold reader — human or model — can actually understand.
