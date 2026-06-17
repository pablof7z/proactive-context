//! Central content-type model for the capture/inject taxonomy.
//!
//! proactive-context already has several artifact classes on disk — current guides,
//! claims, episode cards, research records, noun entries, the realness ledger, and raw
//! transcripts. Until now their "kind" was implicit: scattered across `type:` frontmatter
//! string comparisons, key prefixes (`episode:<stem>`), and directory layout. This module
//! makes the kind first-class **Rust-owned data** (Design Rule 5) so capture and injection
//! can reason about *what sort of memory* they are handling.
//!
//! Phase 1 contract: this module classifies and renders keys. It introduces **no behavior
//! change** on its own — nothing here is wired into the inject SELECT/COMPILE path yet.
//! Unknown / malformed input degrades to an explicit variant, never a panic.

#![allow(dead_code)] // Phases 2-7 wire these in; Phase 1 only lands the model + tests.

/// What kind of memory an artifact is. The single source of truth for taxonomy classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentKind {
    /// A flat wiki guide: present-tense current project truth. Has no `type:` frontmatter.
    CurrentGuide,
    /// An atomic, evidence-backed fact (a row in claims.jsonl / a claim cluster).
    Claim,
    /// An immutable investigation record (`type: research-record`, `<wiki>/research/`).
    ResearchRecord,
    /// An immutable historical decision/reversal card (`type: episode-card`, `<wiki>/episodes/`).
    EpisodeCard,
    /// A project-specific entity definition (`type: noun-entry`, `<wiki>/nouns/`).
    NounEntry,
    /// A user-stance realness ledger entry (`<wiki>/nouns/realness.jsonl`).
    RealnessNoun,
    /// A raw session transcript: provenance / recall fallback, not concise memory.
    RawTranscript,
    /// A known-unresolved question surfaced for context.
    OpenQuestion,
    /// Repo-authored markdown (e.g. README, docs) — not generated project memory, and the
    /// safe landing spot for any unrecognized `type:` value (degrade, don't panic).
    CommittedMarkdown,
}

impl ContentKind {
    /// Classify by the frontmatter `type:` string. `None` (no `type:` field) is a guide.
    /// Any unrecognized value degrades to [`ContentKind::CommittedMarkdown`] — never panics.
    pub fn from_frontmatter_type(ty: Option<&str>) -> ContentKind {
        match ty.map(str::trim) {
            None | Some("") => ContentKind::CurrentGuide,
            Some("episode-card") => ContentKind::EpisodeCard,
            Some("research-record") => ContentKind::ResearchRecord,
            Some("noun-entry") => ContentKind::NounEntry,
            Some("realness-noun") => ContentKind::RealnessNoun,
            Some("open-question") => ContentKind::OpenQuestion,
            Some(_) => ContentKind::CommittedMarkdown,
        }
    }

    /// The canonical catalog key prefix, e.g. `episode:` (empty for guides, which use a bare slug).
    pub fn key_prefix(self) -> &'static str {
        match self {
            ContentKind::CurrentGuide => "",
            ContentKind::Claim => "claim:",
            ContentKind::ResearchRecord => "research:",
            ContentKind::EpisodeCard => "episode:",
            ContentKind::NounEntry => "noun:",
            ContentKind::RealnessNoun => "realness:",
            ContentKind::RawTranscript => "transcript:",
            ContentKind::OpenQuestion => "question:",
            ContentKind::CommittedMarkdown => "",
        }
    }

    /// Render a catalog key for an artifact of this kind from its stem/slug.
    /// Guides and committed markdown keep their bare identifier (backward-compatible).
    pub fn render_key(self, stem: &str) -> String {
        format!("{}{}", self.key_prefix(), stem)
    }

    /// Parse a catalog key into `(kind, rest)`. A key with no recognized prefix is treated as
    /// a guide slug (bare keys are the legacy guide convention). Committed markdown — which
    /// also uses bare keys — is indistinguishable here by prefix alone and is left to the
    /// catalog builder to disambiguate via path; this returns [`ContentKind::CurrentGuide`].
    pub fn parse_key(key: &str) -> (ContentKind, &str) {
        for kind in [
            ContentKind::Claim,
            ContentKind::ResearchRecord,
            ContentKind::EpisodeCard,
            ContentKind::NounEntry,
            ContentKind::RealnessNoun,
            ContentKind::RawTranscript,
            ContentKind::OpenQuestion,
        ] {
            let prefix = kind.key_prefix();
            if !prefix.is_empty() {
                if let Some(rest) = key.strip_prefix(prefix) {
                    return (kind, rest);
                }
            }
        }
        (ContentKind::CurrentGuide, key)
    }

    /// A short, stable human label (used in audit/debug output and SELECT type hints).
    pub fn label(self) -> &'static str {
        match self {
            ContentKind::CurrentGuide => "current-guide",
            ContentKind::Claim => "claim",
            ContentKind::ResearchRecord => "research-record",
            ContentKind::EpisodeCard => "episode-card",
            ContentKind::NounEntry => "noun-entry",
            ContentKind::RealnessNoun => "realness-noun",
            ContentKind::RawTranscript => "raw-transcript",
            ContentKind::OpenQuestion => "open-question",
            ContentKind::CommittedMarkdown => "committed-markdown",
        }
    }

    /// Whether this kind represents present-tense current truth (vs. a historical/evidence
    /// artifact). Used later to keep historical artifacts from being read as "what is true now".
    pub fn is_current_truth(self) -> bool {
        matches!(self, ContentKind::CurrentGuide | ContentKind::Claim)
    }

    /// Whether this kind is an immutable historical artifact (decision/investigation record).
    pub fn is_historical(self) -> bool {
        matches!(self, ContentKind::EpisodeCard | ContentKind::ResearchRecord)
    }
}

/// How current an artifact's content is. Orthogonal to [`ContentKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Currentness {
    /// In force now.
    Current,
    /// A record of something that happened / was investigated; not a current-truth claim.
    Historical,
    /// Replaced by a later artifact.
    Superseded,
    /// Raised but not adopted.
    Proposed,
    /// Indeterminate (legacy data, missing metadata).
    Unknown,
}

/// Who drove an assertion: the user explicitly, or inferred from agent/implementation context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Authority {
    Explicit,
    Implicit,
    Unknown,
}

impl Authority {
    /// Parse the stored authority string (`"explicit"` | `"implicit"`); anything else is unknown.
    pub fn from_str_lossy(s: &str) -> Authority {
        match s.trim().to_ascii_lowercase().as_str() {
            "explicit" => Authority::Explicit,
            "implicit" => Authority::Implicit,
            _ => Authority::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Authority::Explicit => "explicit",
            Authority::Implicit => "implicit",
            Authority::Unknown => "unknown",
        }
    }
}

/// The reconciliation operation a new claim performs against the existing store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimOp {
    New,
    Confirms,
    Supersedes,
    Refines,
}

/// Optional, guide-only sub-classification. **Must not** replace or reinterpret the existing
/// guide `topic` field (Design Rule 3 / Proposed Type Model note).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuideKind {
    Concept,
    Topic,
    Reference,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_frontmatter_types() {
        assert_eq!(
            ContentKind::from_frontmatter_type(Some("episode-card")),
            ContentKind::EpisodeCard
        );
        assert_eq!(
            ContentKind::from_frontmatter_type(Some("research-record")),
            ContentKind::ResearchRecord
        );
        assert_eq!(
            ContentKind::from_frontmatter_type(Some("noun-entry")),
            ContentKind::NounEntry
        );
        // Tolerant of surrounding whitespace from a hand-rolled YAML parser.
        assert_eq!(
            ContentKind::from_frontmatter_type(Some("  episode-card  ")),
            ContentKind::EpisodeCard
        );
    }

    #[test]
    fn missing_type_is_current_guide() {
        assert_eq!(ContentKind::from_frontmatter_type(None), ContentKind::CurrentGuide);
        assert_eq!(ContentKind::from_frontmatter_type(Some("")), ContentKind::CurrentGuide);
    }

    #[test]
    fn unknown_type_degrades_to_committed_markdown_not_panic() {
        assert_eq!(
            ContentKind::from_frontmatter_type(Some("some-future-kind")),
            ContentKind::CommittedMarkdown
        );
    }

    #[test]
    fn key_prefix_roundtrips() {
        let cases = [
            (ContentKind::EpisodeCard, "2026-05-29-1-foo"),
            (ContentKind::ResearchRecord, "2026-06-12-1-bar"),
            (ContentKind::NounEntry, "mint"),
            (ContentKind::Claim, "cluster-7"),
        ];
        for (kind, stem) in cases {
            let key = kind.render_key(stem);
            let (parsed_kind, parsed_stem) = ContentKind::parse_key(&key);
            assert_eq!(parsed_kind, kind, "kind roundtrip for {key}");
            assert_eq!(parsed_stem, stem, "stem roundtrip for {key}");
        }
    }

    #[test]
    fn bare_key_is_guide_slug() {
        let (kind, rest) = ContentKind::parse_key("token-model");
        assert_eq!(kind, ContentKind::CurrentGuide);
        assert_eq!(rest, "token-model");
    }

    #[test]
    fn authority_parses_and_renders() {
        assert_eq!(Authority::from_str_lossy("explicit"), Authority::Explicit);
        assert_eq!(Authority::from_str_lossy("IMPLICIT"), Authority::Implicit);
        assert_eq!(Authority::from_str_lossy("garbage"), Authority::Unknown);
        assert_eq!(Authority::Explicit.as_str(), "explicit");
    }

    #[test]
    fn currentness_semantics_hold() {
        assert!(ContentKind::CurrentGuide.is_current_truth());
        assert!(ContentKind::Claim.is_current_truth());
        assert!(ContentKind::EpisodeCard.is_historical());
        assert!(ContentKind::ResearchRecord.is_historical());
        assert!(!ContentKind::EpisodeCard.is_current_truth());
    }
}
