export type Tone =
  | "amber"
  | "blue"
  | "cyan"
  | "magenta"
  | "green"
  | "warning"
  | "red"
  | "stone";

export interface ArtifactItem {
  title: string;
  label: string;
  path: string;
  body: string;
  glyph: string;
  badges: string[];
  tone: Tone;
}

export interface CommandItem {
  command: string;
  description: string;
  state: string;
  glyph: string;
  trace: string;
  tone: Tone;
}

export interface MetricItem {
  metric: string;
  question: string;
  label: string;
  glyph: string;
  tone: Tone;
}

export interface RefusalItem {
  title: string;
  body: string;
  tone: Tone;
}

export interface ComparisonRow {
  pattern: string;
  failure: string;
  answer: string;
}

export const trustItems = [
  "Rust CLI",
  "Markdown wiki",
  "Cited capture",
  "Local embeddings",
  "Hot-path injection",
  "Inspectable audit trail"
];

export const artifacts: ArtifactItem[] = [
  {
    title: "Guides",
    label: "Current truth",
    path: "docs/wiki/guides/",
    body: "Current truth, reconciled in place. Cited prose future agents can reuse.",
    glyph: "✓",
    badges: ["current", "cited", "mutable"],
    tone: "green"
  },
  {
    title: "Episode cards",
    label: "Direction changes",
    path: "docs/wiki/episodes/",
    body: "Immutable records for reversals, root causes, and product movement.",
    glyph: "◆",
    badges: ["historical", "supersession", "immutable"],
    tone: "amber"
  },
  {
    title: "Research records",
    label: "Investigations",
    path: "docs/wiki/research/",
    body: "Experiment records with method, verdict, and evidence.",
    glyph: "⟜",
    badges: ["eval", "verdict", "preserved"],
    tone: "blue"
  },
  {
    title: "Claim log",
    label: "Lossless substrate",
    path: "claims.jsonl",
    body: "Append-only atomic claims with authority tags and citations.",
    glyph: "✚",
    badges: ["lossless", "explicit/implicit", "append-only"],
    tone: "magenta"
  }
];

export const commands: CommandItem[] = [
  {
    command: "pc capture --in",
    description: "Runs after a session ends. Extracts cited claims and updates the wiki off the hot path.",
    state: "◆ capture",
    glyph: "◆",
    trace: "capture.done · 17 claims · 1 episode",
    tone: "cyan"
  },
  {
    command: "pc inject",
    description: "Runs before the agent responds. Selects sources, compiles a cited briefing, and emits a system reminder.",
    state: "✎ 312c",
    glyph: "✎",
    trace: "inject.done · 1.2s · compiled",
    tone: "magenta"
  },
  {
    command: "pc archeologist",
    description: "Replays historical transcripts through the same capture pipeline to cold-start a project wiki.",
    state: "◇ replay",
    glyph: "◇",
    trace: "history.replay · chronological",
    tone: "amber"
  },
  {
    command: "pc tail",
    description: "Streams capture, retrieval, source selection, compile, injection, fallback, and errors across projects.",
    state: "▶ live",
    glyph: "▶",
    trace: "events.jsonl · local",
    tone: "blue"
  },
  {
    command: "pc statusline",
    description: "Renders a compact state indicator inside the agent environment.",
    state: "⬡ ✎ 312c · 14g",
    glyph: "⬡",
    trace: "sub-10ms · no network",
    tone: "stone"
  }
];

export const metrics: MetricItem[] = [
  {
    metric: "Predict-the-correction",
    question: "Could the system surface the correction before the user made it again?",
    label: "standing instrument",
    glyph: "✓",
    tone: "amber"
  },
  {
    metric: "Restatement recall",
    question: "Did the user have to repeat something the system should already know?",
    label: "temporal holdout",
    glyph: "⟜",
    tone: "blue"
  },
  {
    metric: "Direction-change fidelity",
    question: "Does the briefing assert the current truth without leaking stale direction?",
    label: "trajectory check",
    glyph: "◆",
    tone: "green"
  },
  {
    metric: "Stale-context suppression",
    question: "Did obsolete knowledge stay out of the hot path?",
    label: "stale gate",
    glyph: "⊘",
    tone: "warning"
  },
  {
    metric: "Attention efficiency",
    question: "Was the injected context counterfactually useful?",
    label: "load-bearing",
    glyph: "✎",
    tone: "cyan"
  },
  {
    metric: "Injection auditability",
    question: "Can the system say why it injected something and which cited artifact it came from?",
    label: "audit trace",
    glyph: "⬡",
    tone: "magenta"
  }
];

export const refusals: RefusalItem[] = [
  {
    title: "No transcript landfill",
    body: "Raw conversations are not the main retrieval substrate. They are too noisy and too easy to overfit.",
    tone: "stone"
  },
  {
    title: "No whole-wiki dumps",
    body: "Context window is attention. Telling a model to attend to everything means it attends to nothing.",
    tone: "amber"
  },
  {
    title: "No uncited summaries",
    body: "Every captured claim needs evidence. Citations are structural, not decorative.",
    tone: "blue"
  },
  {
    title: "No deletion as cleanup",
    body: "Superseded direction is demoted, linked, and preserved. Product archaeology matters.",
    tone: "green"
  },
  {
    title: "No agent-discretionary pull",
    body: "The agent should not have to remember to ask for context. Relevant direction is pushed at the point of action.",
    tone: "magenta"
  },
  {
    title: "No fake certainty",
    body: "Research-grade tools should say what is proven, what failed, and what is still under active development.",
    tone: "red"
  }
];

export const comparisonRows: ComparisonRow[] = [
  {
    pattern: "Static context file",
    failure: "Always loaded, often stale, encourages bloat",
    answer: "Selective prompt-specific injection"
  },
  {
    pattern: "Session compaction",
    failure: "Preserves continuity while compressing away rationale",
    answer: "Evidence-preserving capture outside the window"
  },
  {
    pattern: "Vector memory",
    failure: "Retrieves semantic similarity without authority",
    answer: "Cited claims, guides, episodes, and current-truth reconciliation"
  },
  {
    pattern: "MCP memory",
    failure: "Agent must decide to retrieve",
    answer: "Hot-path pre-action injection"
  },
  {
    pattern: "Raw transcript search",
    failure: "Full fidelity but too noisy",
    answer: "Distilled artifacts with transcript provenance"
  },
  {
    pattern: "Autonomous coding loop",
    failure: "More action without better judgment",
    answer: "Human decisions become durable constraints"
  }
];

export const messBefore = [
  ["AGENTS.md", "maybe stale"],
  ["CLAUDE.md", "host-specific"],
  [".cursor/rules", "another copy"],
  ["MCP memory", "model must remember to call it"],
  ["session summary", "loses nuance"],
  ["chat history", "noisy"],
  ["human attention", "overloaded"]
];

export const messAfter = [
  ["docs/wiki/guides", "current truth"],
  ["docs/wiki/episodes", "reversals"],
  ["docs/wiki/research", "investigations"],
  ["claims.jsonl", "cited substrate"],
  ["pc inject", "hot-path briefing"],
  ["pc tail", "audit stream"]
];
