# Plan: Topic-Organized Capture + Staleness Retirement

**Status:** proposed — awaiting go/no-go per phase.
**Branch discipline:** peers are active on `master`. Do all work on a feature branch in a git worktree; **register nothing as "merged/superseded"** (a peer's branch-GC deleted a prior worktree — recover via `git fsck` dangling commits if it happens again; commits are content-addressed and survive). Land via PR/merge to master only when a phase is validated.

**Date:** 2026-06-04. Supersedes the inline sketch in `capture-redesign.md` §11.5 with an executable sequence.

---

## 0. Problem recap (what we are fixing)

The capture path over-produces and under-organizes:

1. **Over-granularity (FIXED, shipped `21617b2`).** ROUTE altitude said "one mechanism" + "~25-30 guides"; produced 172 flat guides for nostr. Now: "one coherent topic, split only at real topic seams, no count target."
2. **No global view in routing.** RERANK sees only ~8 per-claim recall candidates → mints parallel guides for topics it cannot see. (Phase 2)
3. **Flat layout.** Everything is `docs/wiki/<slug>.md`; no `topics/` organization, contrary to the llm-wiki thesis. (Phase 2)
4. **Render noise (FIXED, shipped `c957432`/`4b89fea`).** Dangling `[^id]`, empty `## See Also`. `normalize_for_publish` at save + `pc wiki tidy`.
5. **Staleness by absence-of-signal (NOT built).** A guide goes stale because nobody discusses the topic anymore (nmp 0.1.0 → 0.2.0); no claim ever routes there, so supersession never fires. (Phase 3)

Already landed on master: render fix (#4), altitude fix (#1), `pc wiki doctor` (consolidation), `pc wiki tidy`, spec §11.5.

**Invariants that constrain every phase** (from capture-redesign.md): keep-everything / never-delete (demote+breadcrumb only); Rust-verified citations (`[^id]` carry-forward must keep working); projection never at inject-time (guides materialized to disk before inject reads); recall-bias ("when in doubt, capture").

---

## Phase 1 — Validate the altitude fix already shipped  *(do FIRST, cheap, gates Phase 2)*

The altitude change is live but **unvalidated** — we must not trust a prompt change on assertion (this lever has a track record of failing; that is why the embedding router exists).

**Steps**
1. Fresh isolated rebuild of nostr into a temp dir:
   `pc archeologist --project=/Users/pablofernandez/Work/nostr-multi-platform --output-dir=/tmp/nostr-altitude`
2. Measure vs. the current 172-guide baseline:
   - guide count (expect meaningfully < 172 if altitude bit)
   - guides-per-contributing-session ratio (was ~1.0; want lower)
   - dup-scan (the Jaccard scan used before) — should stay ~0
   - real-panic grep (`panicked at|slice index starts at|RUST_BACKTRACE`) = 0
3. **Decision gate:**
   - If count dropped substantially (say < ~120) and content still coherent → altitude works; proceed to Phase 2 building on it.
   - If count barely moved → altitude prompt alone is insufficient (expected per history); Phase 2's full-catalog view is doing the real work. Proceed but do not rely on the prompt.

**Cost:** ~1.5h unattended (glm). **Output:** a number that tells us how much of #1 the prompt actually fixed.

---

## Phase 2 — Topic-organized routing (the structural "uses topics" fix)

Two coupled changes: (A) give RERANK the **full catalog organized by topic**; (B) emit a constrained `topic` per guide and write `topics/<topic>/<slug>.md`.

### 2.1 Data model
- Add `topic: String` to `GuideFrontmatter` (wiki.rs). Serialized in frontmatter; back-compat: missing `topic` ⇒ `"general"` (or derive from existing tags) on load.
- Add `topic` to `IndexRow`.

### 2.2 Physical layout + addressing (the blast radius — all assume flat `wiki_dir/<slug>.md`)
Decision required up-front: **physical nesting** (`topics/<topic>/<slug>.md`) vs **flat files + logical topic in frontmatter + index**. Recommendation: **flat files on disk, topic as metadata**, rendered into a topic-grouped `_index.md` and (optionally) a generated `topics/` view. Rationale: avoids touching every path function, avoids the podcast collision, keeps slugs globally unique (no two topics owning the same slug), and still delivers the navigable topic hierarchy via the index. If the user specifically wants real folders, do nesting and pay the refactor.

  - **If metadata-only (recommended):** changes are small — frontmatter field, index grouping, ROUTE prompt. `guide_path`, `load_guide`, `rebuild_index`, `read_index_live`, inject catalog, doctor: **unchanged**.
  - **If physical nesting:** must update `guide_path` (needs topic, wiki.rs:246), `rebuild_index` (single-level `read_dir`, wiki.rs:497 → walk 1 level), `read_index_live` (wiki.rs:649 → walk), inject `read_catalog_content`/`build_catalog` (inject.rs:663/940), doctor addressing (doctor.rs:599/612), archeologist, **and migrate 4 existing wikis**, **and** resolve the **podcast `docs/wiki/topics/` collision** (its hand-built KB; gate pc files on `compiled-from: conversation` and/or namespace pc's subtree). 10 `guide_path`/`load_guide` call sites enumerated.

### 2.3 ROUTE: full-catalog view + topic decision
- `read_index_live()` already returns the full wiki and is **already in scope** at the ROUTE call site (capture.rs:~1735) — today only fed to RECALL. Additionally render it to RERANK as a **topic-grouped catalog** (topic → [slug | title | summary]), with the per-claim cosine-recalled candidates marked as strongest suggestions.
- Keep RECALL (cosine) for **ranking**; the full catalog is for the **decision**. Hierarchy (pick topic among ~N, then guide within topic) makes a global decision tractable vs. "pick among 172 flat slugs" — this is why full-view can work now where unconstrained freedom over-split (Spin 3).
- RERANK output schema gains `topic` (+ `is_new_topic`). Constrain topic vocabulary to existing topics + bias reuse; new topic only when none fits (else "relay" vs "networking" sprawl one level up).
- Within-batch sibling convergence (existing) extends to topic: sibling claims about a new topic share one topic name.

### 2.4 Latency guard
Full catalog at ~170 guides adds prompt tokens to a per-capture call. Measure RERANK latency on the largest project; if it bloats, send **topic list + only the recalled candidates' topics expanded** rather than the entire catalog. (Capture is not as latency-bound as inject, but archeologist replays hundreds of sessions, so per-call cost compounds.)

### 2.5 Validation gate (do NOT trust on assertion)
Fresh `--output-dir` rebuild on nostr; measure: guide count, **topic distribution** (how many topics, sizes — sane?), reuse rate, over-split reproduced?, RERANK latency delta. Compare to Phase 1 numbers. Ship only if topics are coherent and count is sane.

### 2.6 Migration of the 4 existing wikis
After validation, backfill topic assignments onto existing guides. Two options: (a) re-run archeologist fresh into the new layout (clean, ~hours each); (b) a one-pass `pc wiki retopic` that assigns topics to existing guides via the same embedding clusters wiki-doctor already computes (cheap). Recommend (b) for already-clean wikis, (a) only if also re-capturing.

---

## Phase 3 — Staleness retirement (the absence-of-signal gap)

**New problem, design-first.** Supersession is push (new fact contradicts old); staleness is the *absence* of any signal — the contradicting claim never arrives. Must honor keep-everything: **demote + breadcrumb, never delete.**

### 3.1 Where it lives
The periodic `pc wiki doctor` (NOT capture). Same rationale as consolidation: needs the global, off-hot-path view; capture is per-session and structurally blind to absence. Add a `--staleness` pass (or fold into the default doctor run).

### 3.2 Detection signals, by reliability (strongest first)
1. **Code-grounding (strongest — grounded in truth, not absence).** Each guide's `_citations.log` evidence and inline refs name symbols/files/versions. If a cited symbol/file no longer exists in the repo (grep/LSP/tree-sitter), that's a deterministic staleness signal. Build a checker that resolves a guide's cited anchors against the current repo; unresolved ⇒ candidate.
2. **Topic-version conflict (rides on Phase 2's topics).** Within one `topics/<topic>/`, two guides whose content names different versions (or one names a version older than the code) ⇒ flag the older as superseded-in-place.
3. **Age since `verified` (weakest — flag-only, never auto-act).** A `hot`/`warm` guide untouched past a window ⇒ surface for human review only.

### 3.3 Action (demote, not delete)
- Confirmed-stale guide: prepend a rendered banner / frontmatter `status: superseded` and a breadcrumb ("Described nmp 0.1.0; the project migrated to 0.2.0 — see <guide>."). Keep the body (archaeology).
- Volatility/`verified` downgraded; inject's ranking should de-prioritize `status: superseded` guides (small inject change).
- LLM-confirm before demoting (like doctor's CONFIRM) — code-grounding proposes, LLM confirms it's genuinely stale vs. intentionally historical.

### 3.4 Cascade (known open problem, capture-redesign §10.2)
Demoting a guide should surface dependent guides (claims that only made sense under the now-stale rule). Reuse embedding recall to find adjacent guides; flag, don't auto-act. Out of scope for v1 of Phase 3 — note and defer.

### 3.5 Validation
Construct a known-stale case (the podcast nmp-0.1.0 guide is a real one). Run the staleness pass; confirm it flags that guide, demotes-not-deletes, and leaves current guides untouched. No false positives on the clean nostr wiki.

---

## Sequencing & gates

1. **Phase 1** (validate altitude) — cheap, gates everything. ~1.5h.
2. **Phase 2** — only after Phase 1 shows the altitude baseline. Decision on physical-nesting-vs-metadata is the first sub-gate (recommend metadata). Validate before migrating wikis.
3. **Phase 3** — independent of Phase 2 except signal #2 (topic-version) needs topics. Can start signal #1 (code-grounding) in parallel.

**Do not batch phases into one big PR.** Each phase: branch → build → unit tests → fresh-rebuild validation → measure → merge. Keep master green for the peers.

## Risks
- **Peer churn on master / branch-GC.** Work in a worktree; expect possible deletion; recover via `git fsck`. Prefer landing small validated increments fast over long-lived branches.
- **Full-catalog routing regresses to over-split** (the lever's history). Mitigated by topic-hierarchy framing + the validation gate; fall back to topic-list-only context if it over-splits or bloats latency.
- **Podcast topics/ collision** (Phase 2 nesting only). Avoided entirely by the metadata-only layout.
- **Staleness false-positives demoting live guides.** Mitigated by code-grounding (truth-based) + LLM-confirm + flag-only for the weak age signal.

## Open decisions for the user
- **D1 (Phase 2 layout):** physical `topics/<topic>/<slug>.md` folders, or flat files + topic metadata rendered into a grouped index? (Recommend metadata; folders are a large refactor + podcast collision.)
- **D2 (Phase 2 migration):** re-capture the 4 wikis fresh, or cheap `retopic` pass on existing guides? (Recommend retopic for clean ones.)
- **D3 (Phase 3 scope):** ship signal #1 (code-grounding) alone first, or all three signals together?
