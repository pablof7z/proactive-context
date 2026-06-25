"""recall.mapreduce — query-time agentic map-reduce with completeness guarantees.

NO precompiled distillation. Everything happens at query time over the cheap FTS
index. High-recall candidate selection → shard + concurrent map → iterative
loop-until-dry → synthesize with citations.

Usage:
    python3 -m recall.mapreduce "what was the way we solved event-driven design in my projects?"
"""
from __future__ import annotations

import json
import re
import sys
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from typing import Optional

from .store import Store, DEFAULT_DB
from . import glm

# ---------------------------------------------------------------------------
# ANSI colors for live progress
# ---------------------------------------------------------------------------

C_RESET  = "\033[0m"
C_BOLD   = "\033[1m"
C_CYAN   = "\033[36m"
C_GREEN  = "\033[32m"
C_YELLOW = "\033[33m"
C_BLUE   = "\033[34m"
C_MAGENTA= "\033[35m"
C_RED    = "\033[31m"
C_DIM    = "\033[2m"


def _p(color: str, *args, **kw):
    print(f"{color}", end="", flush=True)
    print(*args, **kw, flush=True)
    print(C_RESET, end="", flush=True)


# ---------------------------------------------------------------------------
# Step 1: Union candidate selection
# ---------------------------------------------------------------------------

def expand_aliases(query: str) -> list[str]:
    """Ask GLM for 15 alternative phrasings/synonyms. Returns comma-separated terms."""
    prompt = (
        f"List 15 alternative terms/phrasings/synonyms a developer might use for: {query}\n"
        "Output comma-separated only. No explanation. No numbering."
    )
    try:
        raw = glm.complete(prompt, num_ctx=4096, temperature=0.3)
        terms = [t.strip() for t in raw.split(",") if t.strip()]
        # Filter out empties and obvious non-terms
        terms = [t for t in terms if 2 < len(t) < 80]
        return terms[:15]
    except Exception as e:
        _p(C_RED, f"  [alias expansion error: {e}]")
        return []


def union_candidates(store: Store, query: str, aliases: list[str]) -> tuple[list[str], dict]:
    """
    Returns (ordered_candidate_ids, stats_dict).
    ordered: original query hits first (by FTS rank), then alias-only hits appended.
    """
    seen = set()
    ordered = []

    # Primary: search for original query
    primary_ids = store.search_ids(query, limit=2000)
    for tid in primary_ids:
        if tid not in seen:
            seen.add(tid)
            ordered.append(tid)

    # Alias expansion: each alias term searched
    alias_hit_counts = {}
    for alias in aliases:
        ids = store.search_ids(alias, limit=500)
        alias_hit_counts[alias] = len(ids)
        for tid in ids:
            if tid not in seen:
                seen.add(tid)
                ordered.append(tid)

    stats = {
        "primary_hits": len(primary_ids),
        "alias_hit_counts": alias_hit_counts,
        "total_candidates": len(ordered),
    }
    return ordered, stats


# ---------------------------------------------------------------------------
# Step 2: Shard + Map
# ---------------------------------------------------------------------------

SHARD_SIZE = 50
MAX_SHARDS = 40  # cap to keep latency reasonable


def build_shards(store: Store, candidate_ids: list[str], max_shards: int = MAX_SHARDS) -> tuple[list[list[tuple]], int]:
    """
    Fetches full text for each candidate and groups into shards of ~SHARD_SIZE.
    Returns (shards, n_dropped) where each shard is a list of (id, ts, project, text) tuples.
    """
    turns = []
    for tid in candidate_ids:
        row = store.get(tid)
        if row:
            # row = (id, source, project, session, line, ts, text, raw_path)
            turns.append((row[0], row[5], row[2], row[6]))

    # Split into shards
    all_shards = [turns[i:i+SHARD_SIZE] for i in range(0, len(turns), SHARD_SIZE)]

    n_dropped = 0
    if len(all_shards) > max_shards:
        n_dropped = len(all_shards) - max_shards
        all_shards = all_shards[:max_shards]

    return all_shards, n_dropped


MAPPER_SYSTEM = """You are a precise relevance extractor for a developer recall system.
Given a USER QUERY and a batch of human-typed developer utterances, identify which ones are relevant.

BIAS HARD TOWARD INCLUSION. Include anything that:
- Directly addresses the query topic
- Shows a decision, preference, or opinion related to the query
- Mentions a technology, pattern, or approach related to the query
- Shows evolution or change of opinion on a related topic
- Provides important context or constraints

For each relevant turn, output a JSON array. Each element:
{
  "id": "<the turn id>",
  "verbatim_quote": "<exact text fragment from the turn, max 200 chars>",
  "stance": "expressed|considered|rejected|evolved",
  "facet": "<short topic label, 2-5 words>",
  "why_relevant": "<one sentence why this is relevant>"
}

Output ONLY valid JSON array, no preamble. If nothing is relevant, output [].
Do NOT wrap in markdown fences."""


def _format_shard_for_mapper(shard: list[tuple], query: str) -> str:
    """Format shard turns into the mapper prompt."""
    lines = [f"USER QUERY: {query}", "", "TURNS TO ANALYZE:"]
    for tid, ts, project, text in shard:
        date = ts[:10] if ts else "unknown"
        # Truncate very long texts to ~800 chars
        snippet = text if len(text) <= 800 else text[:800] + "…"
        lines.append(f"\n[{tid}] date={date} project={project}")
        lines.append(snippet)
    return "\n".join(lines)


def _parse_mapper_json(raw: str) -> list[dict]:
    """Robustly parse mapper JSON output."""
    if not raw:
        return []
    # Strip markdown fences
    raw = re.sub(r"^```(?:json)?\s*", "", raw.strip(), flags=re.MULTILINE)
    raw = re.sub(r"\s*```$", "", raw.strip(), flags=re.MULTILINE)
    raw = raw.strip()

    # Try direct parse
    try:
        result = json.loads(raw)
        if isinstance(result, list):
            return result
        if isinstance(result, dict) and "results" in result:
            return result["results"]
        return []
    except json.JSONDecodeError:
        pass

    # Try to find JSON array in the output
    m = re.search(r"\[.*\]", raw, re.DOTALL)
    if m:
        try:
            result = json.loads(m.group(0))
            if isinstance(result, list):
                return result
        except Exception:
            pass

    # Try to extract individual JSON objects
    records = []
    for m in re.finditer(r'\{[^{}]*"id"\s*:\s*"[^"]+[^{}]*\}', raw, re.DOTALL):
        try:
            obj = json.loads(m.group(0))
            if "id" in obj:
                records.append(obj)
        except Exception:
            pass
    return records


def run_mapper(shard_idx: int, shard: list[tuple], query: str,
               print_lock=None) -> tuple[int, list[dict], Optional[str]]:
    """Run a mapper on one shard. Returns (shard_idx, records, error_msg)."""
    try:
        prompt = _format_shard_for_mapper(shard, query)
        raw = glm.complete(
            prompt,
            system=MAPPER_SYSTEM,
            num_ctx=32768,
            temperature=0.1,
        )
        records = _parse_mapper_json(raw)
        # Validate records have required fields
        valid = []
        for r in records:
            if isinstance(r, dict) and "id" in r:
                valid.append(r)
        return shard_idx, valid, None
    except Exception as e:
        return shard_idx, [], str(e)


# ---------------------------------------------------------------------------
# Step 3: Reduce/Synthesize
# ---------------------------------------------------------------------------

REDUCER_SYSTEM = """You are a dense, precise synthesizer for a developer recall system.
You receive extracted relevant turns from multiple mappers, organized by facet/theme.

Write a dense, sourced answer grouped by theme. Requirements:
- Every factual claim must carry an inline citation [turn_id]
- Show contradictions, evolution, or changed positions over time
- Be specific: quote distinctive phrasing, mention project names, API names
- End with a COVERAGE SUMMARY noting: themes found, time range, any gaps
- Use markdown headers for themes
- No preamble like "Based on the records..."
- If you see a contradiction between two stances on the same topic, surface it explicitly"""


def _group_by_facet(records: list[dict]) -> str:
    """Group records by facet for the reducer prompt."""
    facets: dict[str, list[dict]] = {}
    for r in records:
        facet = r.get("facet", "General")
        if facet not in facets:
            facets[facet] = []
        facets[facet].append(r)

    lines = []
    for facet, items in sorted(facets.items()):
        lines.append(f"\n## Facet: {facet}")
        for item in items:
            tid = item.get("id", "?")
            quote = item.get("verbatim_quote", "")
            stance = item.get("stance", "expressed")
            why = item.get("why_relevant", "")
            lines.append(f"[{tid}] stance={stance}")
            if quote:
                lines.append(f"  quote: \"{quote}\"")
            if why:
                lines.append(f"  why: {why}")
    return "\n".join(lines)


def _dedup_records(records: list[dict]) -> list[dict]:
    """Dedup near-identical quotes while keeping all distinct citations."""
    seen_quotes: set[str] = set()
    seen_ids: set[str] = set()
    out = []
    for r in records:
        tid = r.get("id", "")
        quote = r.get("verbatim_quote", "").strip().lower()
        # Always keep if new ID
        if tid in seen_ids:
            continue
        # Skip near-identical quotes (first 80 chars) but keep distinct IDs
        quote_key = quote[:80]
        if quote_key and quote_key in seen_quotes:
            # Keep the record but without the duplicate quote marker
            r = dict(r)
            r["verbatim_quote"] = r.get("verbatim_quote", "")[:40] + "…[dup]"
        if quote_key:
            seen_quotes.add(quote_key)
        seen_ids.add(tid)
        out.append(r)
    return out


def run_reducer(query: str, records: list[dict]) -> str:
    """Run the reducer synthesis call, streaming output to terminal."""
    grouped = _group_by_facet(records)
    prompt = (
        f"ORIGINAL QUERY: {query}\n\n"
        f"RELEVANT TURNS (grouped by facet):\n{grouped}\n\n"
        "Write the dense sourced answer:"
    )

    # Stream the synthesis
    _p(C_BOLD + C_GREEN, "\n=== SYNTHESIS ===")
    msgs = [
        {"role": "system", "content": REDUCER_SYSTEM},
        {"role": "user", "content": prompt},
    ]
    result_parts = []
    try:
        for chunk in glm.chat(msgs, stream=True, num_ctx=65536, temperature=0.2):
            delta = (chunk.get("message") or {}).get("content", "")
            if delta:
                print(delta, end="", flush=True)
                result_parts.append(delta)
        print()  # newline after streaming
    except Exception as e:
        _p(C_RED, f"\n[reducer streaming error: {e}]")
    return "".join(result_parts)


# ---------------------------------------------------------------------------
# Step 5: Iterative loop-until-dry
# ---------------------------------------------------------------------------

def harvest_new_vocabulary(records: list[dict]) -> list[str]:
    """Extract project names, API names, unique identifiers from mapper hits."""
    text_blob = " ".join(
        r.get("verbatim_quote", "") + " " + r.get("why_relevant", "") + " " + r.get("facet", "")
        for r in records
    )
    # Find CamelCase identifiers, dotted names, hyphenated names that look like project/API names
    candidates = set()

    # CamelCase words (likely API/class names)
    for m in re.finditer(r"\b[A-Z][a-z]+(?:[A-Z][a-z]+)+\b", text_blob):
        candidates.add(m.group(0))

    # Words in brackets or quotes that look like identifiers
    for m in re.finditer(r'"([a-zA-Z][a-zA-Z0-9_-]{2,30})"', text_blob):
        candidates.add(m.group(1))

    # Hyphenated compound words (project names)
    for m in re.finditer(r"\b[a-z][a-z0-9]*(?:-[a-z][a-z0-9]+)+\b", text_blob):
        w = m.group(0)
        if len(w) > 4:
            candidates.add(w)

    # Filter stopwords / common words
    stopwords = {"the", "and", "for", "with", "that", "this", "from", "have",
                 "been", "were", "they", "what", "when", "where", "why", "how",
                 "will", "would", "should", "could", "about", "into", "over",
                 "some", "more", "also", "just", "like", "than", "then", "than",
                 "there", "their", "which", "while", "your", "very", "each"}
    filtered = [c for c in candidates
                if c.lower() not in stopwords and len(c) > 3]
    return filtered[:20]


# ---------------------------------------------------------------------------
# Coverage ledger
# ---------------------------------------------------------------------------

class Ledger:
    def __init__(self, query: str):
        self.query = query
        self.aliases: list[str] = []
        self.total_candidates = 0
        self.primary_hits = 0
        self.alias_hit_counts: dict[str, int] = {}
        self.shards_run = 0
        self.shards_dropped = 0
        self.turns_inspected = 0
        self.relevant_turns = 0
        self.waves: list[dict] = []
        self.t0 = time.time()
        self.errors: list[str] = []

    def add_wave(self, wave_n: int, new_candidates: int, new_relevant: int,
                 shards: int, inspected: int):
        self.waves.append({
            "wave": wave_n,
            "new_candidates": new_candidates,
            "new_relevant": new_relevant,
            "shards": shards,
            "inspected": inspected,
        })

    def print(self):
        elapsed = time.time() - self.t0
        _p(C_BOLD + C_CYAN, "\n=== COVERAGE LEDGER ===")
        print(f"  Query:            {self.query}")
        print(f"  Query terms:      {self.query.split()}")
        print(f"  Alias terms ({len(self.aliases)}):  {', '.join(self.aliases[:10])}" +
              ("…" if len(self.aliases) > 10 else ""))
        print(f"  Primary hits:     {self.primary_hits}")
        print(f"  Total candidates: {self.total_candidates}")
        print(f"  Shards run:       {self.shards_run}")
        if self.shards_dropped:
            _p(C_YELLOW, f"  Shards dropped:   {self.shards_dropped} (cap={MAX_SHARDS})")
        print(f"  Turns inspected:  {self.turns_inspected}")
        print(f"  Relevant turns:   {self.relevant_turns}")
        print(f"  Waves run:        {len(self.waves)}")
        for w in self.waves:
            print(f"    Wave {w['wave']}: {w['new_candidates']} candidates, "
                  f"{w['shards']} shards, {w['inspected']} inspected, "
                  f"{w['new_relevant']} new relevant")
        if self.errors:
            _p(C_RED, f"  Errors: {len(self.errors)}")
            for e in self.errors[:5]:
                print(f"    {e}")
        print(f"  Total latency:    {elapsed:.1f}s")


# ---------------------------------------------------------------------------
# Main pipeline
# ---------------------------------------------------------------------------

def run_mapreduce(query: str, store: Optional[Store] = None,
                  max_shards: int = MAX_SHARDS, n_workers: int = 7) -> str:
    """
    Full map-reduce pipeline. Returns the final synthesis string.
    """
    if store is None:
        store = Store(DEFAULT_DB)

    ledger = Ledger(query)

    _p(C_BOLD + C_CYAN, f"\n{'='*60}")
    _p(C_BOLD + C_CYAN, f"RECALL MAP-REDUCE: {query}")
    _p(C_BOLD + C_CYAN, f"{'='*60}")

    # ---- Step 1: Alias expansion ----
    _p(C_YELLOW, "\n[1/4] Expanding query aliases...")
    aliases = expand_aliases(query)
    ledger.aliases = aliases
    _p(C_GREEN, f"  Aliases ({len(aliases)}): {', '.join(aliases[:8])}" +
       ("…" if len(aliases) > 8 else ""))

    # ---- Step 1b: Union candidate selection ----
    _p(C_YELLOW, "\n[2/4] Selecting candidates (union FTS)...")
    candidate_ids, cand_stats = union_candidates(store, query, aliases)
    ledger.primary_hits = cand_stats["primary_hits"]
    ledger.alias_hit_counts = cand_stats["alias_hit_counts"]
    ledger.total_candidates = cand_stats["total_candidates"]
    _p(C_GREEN, f"  Primary hits: {cand_stats['primary_hits']}, "
       f"Total after union: {cand_stats['total_candidates']}")

    # ---- Step 2: Shard + Map (iterative loop) ----
    _p(C_YELLOW, "\n[3/4] Map phase (shard + concurrent map)...")

    all_relevant_records: list[dict] = []
    seen_candidate_ids = set(candidate_ids)
    seen_record_ids: set[str] = set()
    wave = 0
    MAX_WAVES = 3

    while wave < MAX_WAVES:
        wave += 1
        _p(C_MAGENTA, f"\n  --- Wave {wave} ---")

        if not candidate_ids:
            _p(C_DIM, "  No candidates for this wave, stopping.")
            break

        shards, n_dropped = build_shards(store, candidate_ids, max_shards=max_shards)
        if n_dropped:
            _p(C_YELLOW, f"  [!] Cap hit: {n_dropped} shards dropped (>{max_shards} shards). "
               f"Processing top {max_shards} by FTS rank.")
            ledger.shards_dropped += n_dropped

        if not shards:
            _p(C_DIM, "  No shards to process.")
            break

        n_inspected = sum(len(s) for s in shards)
        _p(C_CYAN, f"  {len(shards)} shards × ~{SHARD_SIZE} turns = "
           f"{n_inspected} turns to inspect")

        wave_records: list[dict] = []
        wave_errors: list[str] = []

        with ThreadPoolExecutor(max_workers=n_workers) as executor:
            futures = {
                executor.submit(run_mapper, idx, shard, query): idx
                for idx, shard in enumerate(shards)
            }
            for future in as_completed(futures):
                idx = futures[future]
                try:
                    shard_idx, records, err = future.result()
                    if err:
                        msg = f"Shard {shard_idx}: {err}"
                        wave_errors.append(msg)
                        _p(C_RED, f"  [shard {shard_idx:02d}] ERROR: {err[:80]}")
                    else:
                        n_new = 0
                        for r in records:
                            tid = r.get("id", "")
                            if tid and tid not in seen_record_ids:
                                seen_record_ids.add(tid)
                                wave_records.append(r)
                                n_new += 1
                        _p(C_GREEN if n_new > 0 else C_DIM,
                           f"  [shard {shard_idx:02d}] ✓ {n_new} relevant / {len(shards[shard_idx])} turns")
                except Exception as e:
                    wave_errors.append(f"Shard {idx} future error: {e}")
                    _p(C_RED, f"  [shard {idx}] future error: {e}")

        ledger.errors.extend(wave_errors)
        ledger.shards_run += len(shards)
        ledger.turns_inspected += n_inspected

        wave_new_relevant = len(wave_records)
        all_relevant_records.extend(wave_records)
        ledger.relevant_turns = len(all_relevant_records)

        _p(C_CYAN, f"  Wave {wave} done: {wave_new_relevant} new relevant turns found")
        ledger.add_wave(wave, len(candidate_ids), wave_new_relevant,
                        len(shards), n_inspected)

        # ---- Step 5: Harvest new vocabulary & loop ----
        if wave < MAX_WAVES and wave_new_relevant > 0:
            _p(C_YELLOW, f"\n  Harvesting new vocabulary from wave {wave} hits...")
            new_vocab = harvest_new_vocabulary(wave_records)
            if new_vocab:
                _p(C_CYAN, f"  New vocab terms: {', '.join(new_vocab[:10])}")
                # Find new candidate IDs from vocabulary
                next_candidate_ids = []
                for term in new_vocab:
                    new_ids = store.search_ids(term, limit=200)
                    for tid in new_ids:
                        if tid not in seen_candidate_ids:
                            seen_candidate_ids.add(tid)
                            next_candidate_ids.append(tid)
                candidate_ids = next_candidate_ids
                if candidate_ids:
                    _p(C_GREEN, f"  {len(candidate_ids)} new candidates from vocabulary expansion")
                    ledger.total_candidates += len(candidate_ids)
                else:
                    _p(C_DIM, "  No new candidates from vocabulary — stopping.")
                    break
            else:
                _p(C_DIM, "  No new vocabulary harvested — stopping.")
                break
        else:
            break

    if not all_relevant_records:
        _p(C_RED, "\nNo relevant turns found. Try different query terms.")
        ledger.print()
        return ""

    # Dedup
    deduped = _dedup_records(all_relevant_records)
    _p(C_CYAN, f"\n  Total relevant: {len(all_relevant_records)}, "
       f"after dedup: {len(deduped)}")

    # ---- Step 3: Reduce/Synthesize ----
    _p(C_YELLOW, "\n[4/4] Synthesis phase...")
    synthesis = run_reducer(query, deduped)

    # Print coverage ledger
    ledger.print()

    return synthesis


# ---------------------------------------------------------------------------
# CLI entry point
# ---------------------------------------------------------------------------

def main():
    if len(sys.argv) < 2:
        print("Usage: python3 -m recall.mapreduce \"<query>\"")
        sys.exit(1)
    query = " ".join(sys.argv[1:])
    store = Store(DEFAULT_DB)
    stats = store.stats()
    _p(C_DIM, f"[index: {stats['turns']:,} turns, {stats['projects']} projects, "
       f"{stats['sessions']} sessions]")
    run_mapreduce(query, store=store)


if __name__ == "__main__":
    main()
