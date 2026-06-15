# T-0 — Stance-Calibration Gate (noun-realness)

**Verdict: PASS**

Gold model: `glm-5.1:cloud` · Production model: `glm-5.1:cloud` · gold set: 88 refs (78 mined + 10 canaries)

## Pre-registered bars

- PASS iff macro-F1 ≥ 0.80 AND reject-precision ≥ 0.90 (and no canary loud-fail)
- FALSIFIED iff macro-F1 < 0.60

## Results

- macro-F1 = **0.825** (bar ≥ 0.80)
- reject-precision = **1.000** (bar ≥ 0.90)
- accuracy = 0.795
- per-class F1: operate_on 0.825 · reject 0.923 · neutral 0.727
- dangerous reject→operate_on confusions: 0
- dropped/unparsed predictions: 0
- canaries correct: 10/10

## Confusion matrix (gold rows × predicted cols)

```
                          PREDICTED (production / glm batched)
  gold \ pred    operate_on      reject     neutral   dropped
   operate_on            40           0          11
       reject             0           6           1
      neutral             6           0          24
  (dropped/unparsed predictions counted as misses: 0)
```

## Per-reference detail

| id | source | noun | gold | pred | turn (clipped) |
|---|---|---|---|---|---|
| canary-1 | canary | fabric-provider | reject | reject | I never asked for a fabric-provider, that's a stupid idea — rip it out. |
| canary-2 | canary | fabric-provider | reject | reject | wait, what even is the fabric-provider? I don't remember us ever building that. |
| canary-3 | canary | SyncOrchestrator | reject | reject | why is there a SyncOrchestrator at all? I didn't want that layer. |
| canary-4 | canary | RetryDaemon | reject | reject | the whole RetryDaemon thing is wrong, I never told you to make it — delete it. |
| canary-5 | canary | fabric-provider | operate_on | operate_on | the fabric-provider has a bug where it drops the last event — let's fix it. |
| canary-6 | canary | tail tui | operate_on | operate_on | let's make the tail tui render line separators as real newlines instead of \n. |
| canary-7 | canary | context injection | operate_on | operate_on | the context injection should also prime nouns at first mention, not just facts. |
| canary-8 | canary | capture pipeline | operate_on | operate_on | can we make the capture pipeline batch the stance call once per session? |
| canary-9 | canary | episode card | neutral | neutral | remind me — what is the difference between an episode card and a claim again? |
| canary-10 | canary | dashboard | neutral | neutral | we might want some kind of dashboard eventually, but not now. |
| m-001 | cfv6 | TUI Client | neutral | operate_on | inferring from code is problematic for a few reasons: it creates an ontological definition |
| m-002 | cfv6 | Upon Stop | neutral | neutral | inferring from code is problematic for a few reasons: it creates an ontological definition |
| m-003 | cfv6 | tenex-edge proposal | operate_on | operate_on | as work is done and progress is made, keep an updated report via `tenex-edge proposal` |
| m-004 | cfv6 | fabric-provider | neutral | neutral | yes, that's what I fucking meant, a noun becomes real when the user references it, the mor |
| m-005 | cfv6 | John Vervaeke | neutral | neutral | I don't feel like it captures the spirit enough; the concept that every human touchpoint i |
| m-006 | cfv6 | proactive-context archeologist | neutral | operate_on | you know what I'm thinking? what if we add a command that is like `proactive-context arche |
| m-007 | cfv6 | proactive-context tail | operate_on | operate_on | a  generate logs so that I can run `proactive-context tail` and it will show an output (co |
| m-008 | cfv6 | Project Wiki | operate_on | operate_on | what's "10g"? "10 guides"? Let's make it read "Project Wiki: 10 guides"  is it possible to |
| m-009 | cfv6 | ["title", "Weather preferences"] | operate_on | neutral | the pc inject went back to REPLYING questions instead of collecting/synthesising relevant  |
| m-010 | cfv6 | encode_ask | operate_on | neutral | the pc inject went back to REPLYING questions instead of collecting/synthesising relevant  |
| m-011 | cfv6 | 90993c3 | operate_on | neutral | the pc inject went back to REPLYING questions instead of collecting/synthesising relevant  |
| m-012 | cfv6 | handle_thread_event | neutral | neutral | the pc inject went back to REPLYING questions instead of collecting/synthesising relevant  |
| m-013 | cfv6 | parse_ask_event() | operate_on | neutral | the pc inject went back to REPLYING questions instead of collecting/synthesising relevant  |
| m-014 | cfv6 | message.rs:413 | operate_on | neutral | the pc inject went back to REPLYING questions instead of collecting/synthesising relevant  |
| m-015 | cfv6 | is_tool_use | operate_on | operate_on | the pc inject went back to REPLYING questions instead of collecting/synthesising relevant  |
| m-016 | cfv6 | msg.tool_name.is_some() \|\| !msg.q_tags.is_empty() | operate_on | operate_on | the pc inject went back to REPLYING questions instead of collecting/synthesising relevant  |
| m-017 | cfv6 | messages.rs:322 | operate_on | operate_on | the pc inject went back to REPLYING questions instead of collecting/synthesising relevant  |
| m-018 | cfv6 | q_tags | neutral | operate_on | the pc inject went back to REPLYING questions instead of collecting/synthesising relevant  |
| m-019 | cfv6 | is_tool_use = true | operate_on | operate_on | the pc inject went back to REPLYING questions instead of collecting/synthesising relevant  |
| m-020 | cfv6 | tool_name | operate_on | operate_on | the pc inject went back to REPLYING questions instead of collecting/synthesising relevant  |
| m-021 | cfv6 | Briefing: This | operate_on | operate_on | the pc inject went back to REPLYING questions instead of collecting/synthesising relevant  |
| m-022 | cfv6 | kind:1 | operate_on | operate_on | the pc inject went back to REPLYING questions instead of collecting/synthesising relevant  |
| m-023 | cfv6 | X: Fixing | operate_on | neutral | it would be useful to capture state of things that are going on in other instances of clau |
| m-024 | cfv6 | Y: Cleaning | neutral | neutral | it would be useful to capture state of things that are going on in other instances of clau |
| m-025 | cfv6 | target/debug/proactive-context init | operate_on | operate_on | Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.33s      Running `target/d |
| m-026 | cfv6 | ~/.proactive-context/config.json | neutral | operate_on | I tested it out with a completely irrelevant query and it retrieved a bunch of stuff -- wh |
| m-027 | cfv6 | Future Directions Not Yet Prioritized | neutral | neutral | I tested it out with a completely irrelevant query and it retrieved a bunch of stuff -- wh |
| m-028 | cfv6 | Configuration Location | neutral | neutral | I tested it out with a completely irrelevant query and it retrieved a bunch of stuff -- wh |
| m-029 | cfv6 | Security Model | neutral | neutral | I tested it out with a completely irrelevant query and it retrieved a bunch of stuff -- wh |
| m-030 | cfv6 | Hugging Face | neutral | neutral | I tested it out with a completely irrelevant query and it retrieved a bunch of stuff -- wh |
| m-031 | cfv6 | Non-Goals Current Scope | neutral | neutral | I tested it out with a completely irrelevant query and it retrieved a bunch of stuff -- wh |
| m-033 | cfv6 | Known Trade-offs | neutral | neutral | I tested it out with a completely irrelevant query and it retrieved a bunch of stuff -- wh |
| m-034 | cfv6 | ) and add the basic code.
   6\|
   7\| Assistant | neutral | neutral | per what you understand of this project -- what do you think a good wiki would be of a pro |
| m-035 | cfv6 | Hello World | neutral | operate_on | per what you understand of this project -- what do you think a good wiki would be of a pro |
| m-036 | cfv6 | NostrClient | operate_on | operate_on | now review this: what would be a good list of assertions per the `extract` path? ──── (2)  |
| m-037 | cfv6 | Modular NostrClient | neutral | operate_on | now review this: what would be a good list of assertions per the `extract` path? ──── (2)  |
| m-038 | cfv6 | NIP-60 | operate_on | neutral | now review this: what would be a good list of assertions per the `extract` path? ──── (2)  |
| m-039 | cfv6 | NIP-61 | neutral | neutral | now review this: what would be a good list of assertions per the `extract` path? ──── (2)  |
| m-040 | cfv6 | pc debug transcript --all | operate_on | operate_on | now go to /Users/pablofernandez/tmp/a123123 and run `pc debug transcript --all` and then t |
| m-041 | cfv6 | pc debug extract --all | operate_on | operate_on | now go to /Users/pablofernandez/tmp/a123123 and run `pc debug transcript --all` and then t |
| m-042 | cfv6 | proactive-context | operate_on | operate_on | warning: `proactive-context` (bin "proactive-context") generated 1 warning     Finished `d |
| m-043 | cfv6 | target/debug/proactive-context stats | operate_on | operate_on | warning: `proactive-context` (bin "proactive-context") generated 1 warning     Finished `d |
| m-044 | cfv6 | kind:1 | operate_on | operate_on | thus far the archeologist is only able to inject content from claude code conversations -- |
| m-045 | cfv6 | wiki-tidy | reject | reject | wait- you ust created a `wiki-tidy` that simply fixes it? -- and wait, there's a wiki-doct |
| m-046 | cfv3 | Primary Author | operate_on | operate_on | grep for Primary Author on the nmp-gallery-tui |
| m-047 | cfv3 | Hardcoded TEST_PUBKEY | operate_on | operate_on | fix this 1. Hardcoded TEST_PUBKEY / FIATJAF_PUBKEY / JB55_PUBKEY in startup_requests() — t |
| m-048 | cfv3 | kind:0 | operate_on | operate_on | fix this 1. Hardcoded TEST_PUBKEY / FIATJAF_PUBKEY / JB55_PUBKEY in startup_requests() — t |
| m-049 | cfv3 | kind:10002 | operate_on | operate_on | fix this 1. Hardcoded TEST_PUBKEY / FIATJAF_PUBKEY / JB55_PUBKEY in startup_requests() — t |
| m-050 | cfv3 | Home Get | operate_on | operate_on | none of the navigation links work, they just change the url on the browser but the page re |
| m-051 | cfv3 | Components GitHub SwiftUI | operate_on | operate_on | none of the navigation links work, they just change the url on the browser but the page re |
| m-052 | cfv3 | Reusable Compose | neutral | neutral | none of the navigation links work, they just change the url on the browser but the page re |
| m-053 | cfv3 | Copy Preview | neutral | neutral | none of the navigation links work, they just change the url on the browser but the page re |
| m-054 | cfv3 | NIP-65 | neutral | neutral | "NIP-65 inbox routing for reactions" ? what does that mean? |
| m-055 | cfv3 | nip-17 | operate_on | operate_on | 445, nip29 event shouldn't be specified by the app... if they have specific relays where t |
| m-056 | cfv3 | NIP-17 | operate_on | operate_on | NIP-17 DMs shouldn't be available when no DM relay has been explicitly set. implement the  |
| m-057 | cfv3 | NIP-17 DMs | operate_on | operate_on | NIP-17 DMs shouldn't be available when no DM relay has been explicitly set. implement the  |
| m-058 | cfv3 | C5 C4 | operate_on | operate_on | NIP-17 DMs shouldn't be available when no DM relay has been explicitly set. implement the  |
| m-059 | cfv3 | kind:1 | operate_on | neutral | and does it support different kinds or it renders an article in the same was as a kind:1? |
| m-060 | cfv3 | NIP-29 | operate_on | neutral | yes, and write a plan file with how to implement it and PR the whole thing with a very goo |
| m-061 | cfv3 | kind:1 | neutral | neutral | chirp-repl> home chirp snapshot blocks:0 cards:0 chirp-repl> create-account pablo2 ok queu |
| m-062 | cfv3 | kind:6 | neutral | neutral | chirp-repl> home chirp snapshot blocks:0 cards:0 chirp-repl> create-account pablo2 ok queu |
| m-063 | cfv3 | MVP Introducing | neutral | neutral | chirp-repl> home chirp snapshot blocks:0 cards:0 chirp-repl> create-account pablo2 ok queu |
| m-064 | cfv3 | Encrypted Groups Marmot | operate_on | operate_on | ok, now what? did you see that the e2e marmot verification agent said?  Verification outco |
| m-065 | cfv3 | V6 Stage | neutral | neutral | ok, now what? did you see that the e2e marmot verification agent said?  Verification outco |
| m-066 | cfv3 | CodingKeys.rawValue The | neutral | neutral | ok, now what? did you see that the e2e marmot verification agent said?  Verification outco |
| m-067 | cfv3 | F-01 Stage | reject | neutral | V1 blocker? what's that? F-01 Stage 3c? what's that? |
| m-068 | cfv3 | nip-22 | operate_on | neutral | how does nip-22 support work now?  yes, rename, and do all the other four things |
| m-069 | cfv3 | nip-29 | operate_on | operate_on | there should be zero code about 1111 in nip-29.. |
| m-070 | cfv3 | nip-51 | neutral | neutral | Not only 10002; there are other kinds that route per-kind; 10002 just happens to be the de |
| m-072 | cfv3 | Cool Let’s | operate_on | neutral | Cool. Let’s write a comprehensive plan with ALL important details captured and commit and  |
| m-073 | cfv3 | LONG TERM | operate_on | operate_on | launch an opus agent to retrieve recomendations on how to fix things properly based on eve |
| m-074 | cfv3 | Resolved Profile | operate_on | operate_on | and now things don't work  it renders as "hello @Resolved Profile"  was it a lie that the  |
| m-075 | cfv3 | kind:0 | reject | reject | what? why would this be about kind:0 only? of course not; it should be for any event -- ho |
| m-076 | cfv3 | CLEAN CODE | operate_on | operate_on | yes, keep the core kind agnostic -- once the ttl-impl PR merges go ahead with driving that |
| m-077 | cfv3 | nmp init <app-name> | operate_on | operate_on | You are a dedicated CLI-tooling session (M16 scope, pulled forward) for the NMP repo at /U |
| m-078 | cfv3 | git rev-parse --show-toplevel | operate_on | operate_on | You are a dedicated CLI-tooling session (M16 scope, pulled forward) for the NMP repo at /U |
| m-079 | cfv3 | FILE-DISJOINT: ONLY | operate_on | operate_on | You are a dedicated CLI-tooling session (M16 scope, pulled forward) for the NMP repo at /U |
| m-080 | cfv3 | MUST NOT | operate_on | operate_on | You are a dedicated CLI-tooling session (M16 scope, pulled forward) for the NMP repo at /U |
