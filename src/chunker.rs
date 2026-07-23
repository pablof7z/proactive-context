use crate::config::Config;

/// A chunk of markdown content with its position info.
#[derive(Debug, Clone)]
pub struct Chunk {
    pub content: String,
    pub index: usize,
}

#[derive(Debug)]
enum MarkdownBlock {
    Heading { level: usize, rendered: String },
    Body(String),
}

#[derive(Debug, Default)]
struct Section {
    headings: Vec<String>,
    blocks: Vec<String>,
}

/// Split markdown into overlapping chunks.
///
/// Headings are treated as section metadata: every chunk from a section starts
/// with its heading path, so a retrieved continuation never loses the subject
/// that made it meaningful. Body text is packed at paragraph boundaries first,
/// then line/sentence/word boundaries when a single block is too large. Overlap
/// only carries complete semantic units; it never copies a raw character tail.
pub fn chunk_markdown(text: &str, cfg: &Config) -> Vec<Chunk> {
    if text.trim().is_empty() {
        return vec![];
    }

    let target = cfg.chunk_size.max(1);
    let sections = markdown_sections(text);
    let mut contents = Vec::new();

    for section in sections {
        contents.extend(chunk_section(&section, target, cfg.chunk_overlap));
    }

    contents
        .into_iter()
        .enumerate()
        .map(|(index, content)| Chunk { content, index })
        .collect()
}

/// Turn a Markdown document into sections while respecting fenced code blocks.
///
/// Heading-only sections are deliberately retained in the active heading path,
/// but they do not produce standalone retrieval chunks.
fn markdown_sections(text: &str) -> Vec<Section> {
    let blocks = markdown_blocks(text);
    let mut sections = Vec::new();
    let mut heading_stack: Vec<(usize, String)> = Vec::new();
    let mut current = Section::default();

    for block in blocks {
        match block {
            MarkdownBlock::Heading { level, rendered } => {
                if !current.blocks.is_empty() {
                    sections.push(current);
                }

                while heading_stack
                    .last()
                    .is_some_and(|(ancestor_level, _)| *ancestor_level >= level)
                {
                    heading_stack.pop();
                }
                heading_stack.push((level, rendered));
                current = Section {
                    headings: heading_stack
                        .iter()
                        .map(|(_, heading)| heading.clone())
                        .collect(),
                    blocks: Vec::new(),
                };
            }
            MarkdownBlock::Body(body) => current.blocks.push(body),
        }
    }

    if !current.blocks.is_empty() {
        sections.push(current);
    }

    sections
}

fn markdown_blocks(text: &str) -> Vec<MarkdownBlock> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut blocks = Vec::new();
    let mut pending = Vec::new();
    let mut fence: Option<(char, usize)> = None;

    for line in normalized.lines() {
        if let Some((marker, width)) = fence {
            pending.push(line);
            if is_closing_fence(line, marker, width) {
                push_body_or_setext_heading(&mut blocks, &mut pending);
                fence = None;
            }
            continue;
        }

        if let Some((marker, width)) = opening_fence(line) {
            push_body_or_setext_heading(&mut blocks, &mut pending);
            pending.push(line);
            fence = Some((marker, width));
            continue;
        }

        if let Some((level, rendered)) = atx_heading(line) {
            push_body_or_setext_heading(&mut blocks, &mut pending);
            blocks.push(MarkdownBlock::Heading { level, rendered });
        } else if line.trim().is_empty() {
            push_body_or_setext_heading(&mut blocks, &mut pending);
        } else {
            pending.push(line);
        }
    }

    push_body_or_setext_heading(&mut blocks, &mut pending);
    blocks
}

fn push_body_or_setext_heading(blocks: &mut Vec<MarkdownBlock>, pending: &mut Vec<&str>) {
    if pending.is_empty() {
        return;
    }

    if pending.len() == 2 {
        if let Some(level) = setext_heading_level(pending[1]) {
            blocks.push(MarkdownBlock::Heading {
                level,
                rendered: format!("{}\n{}", pending[0].trim(), pending[1].trim()),
            });
            pending.clear();
            return;
        }
    }

    let body = pending.join("\n").trim().to_string();
    if !body.is_empty() {
        blocks.push(MarkdownBlock::Body(body));
    }
    pending.clear();
}

fn atx_heading(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim_start();
    let level = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    if !(1..=6).contains(&level) {
        return None;
    }

    let after = &trimmed[level..];
    if !after.is_empty() && !after.starts_with(char::is_whitespace) {
        return None;
    }

    Some((level, trimmed.trim_end().to_string()))
}

fn setext_heading_level(line: &str) -> Option<usize> {
    let trimmed = line.trim();
    if trimmed.len() < 3 {
        return None;
    }

    if trimmed.bytes().all(|byte| byte == b'=') {
        Some(1)
    } else if trimmed.bytes().all(|byte| byte == b'-') {
        Some(2)
    } else {
        None
    }
}

fn opening_fence(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start();
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }

    let width = trimmed.chars().take_while(|ch| *ch == marker).count();
    (width >= 3).then_some((marker, width))
}

fn is_closing_fence(line: &str, marker: char, width: usize) -> bool {
    let trimmed = line.trim();
    let actual = trimmed.chars().take_while(|ch| *ch == marker).count();
    actual >= width && trimmed[actual..].trim().is_empty()
}

fn chunk_section(section: &Section, target: usize, overlap: usize) -> Vec<String> {
    let prefix = section.headings.join("\n");
    let prefix_overhead = if prefix.is_empty() {
        0
    } else {
        prefix.len() + 2
    };
    // Tiny or pathological configured targets must still make forward progress.
    // A section heading may take the rendered chunk over the approximate target,
    // but it is never truncated into misleading metadata.
    let body_budget = target.saturating_sub(prefix_overhead).max(32);
    let overlap_budget = overlap.min(body_budget / 2);

    let mut units = Vec::new();
    for block in &section.blocks {
        units.extend(split_block(block, body_budget));
    }

    let mut chunks = Vec::new();
    let mut current: Vec<String> = Vec::new();

    for unit in units {
        if unit.trim().is_empty() {
            continue;
        }

        if !current.is_empty() && joined_len(&current) + 2 + unit.len() > body_budget {
            push_rendered_chunk(&mut chunks, &prefix, &current);
            current = overlap_units(&current, overlap_budget);
        }

        // If overlap would force the new semantic unit over budget, prefer the
        // new unit. Repetition is optional; coherent content is not.
        if !current.is_empty() && joined_len(&current) + 2 + unit.len() > body_budget {
            current.clear();
        }
        current.push(unit);
    }

    push_rendered_chunk(&mut chunks, &prefix, &current);
    chunks
}

fn push_rendered_chunk(chunks: &mut Vec<String>, prefix: &str, body_units: &[String]) {
    if body_units.is_empty() {
        return;
    }

    let body = body_units.join("\n\n");
    if !is_substantive(&body) {
        return;
    }

    let rendered = if prefix.is_empty() {
        body
    } else {
        format!("{prefix}\n\n{body}")
    };

    if chunks.last().is_none_or(|previous| previous != &rendered) {
        chunks.push(rendered);
    }
}

fn overlap_units(units: &[String], budget: usize) -> Vec<String> {
    if budget == 0 {
        return Vec::new();
    }

    let mut selected = Vec::new();
    let mut used = 0;
    for unit in units.iter().rev() {
        let added = unit.len() + usize::from(!selected.is_empty()) * 2;
        if used + added > budget {
            break;
        }
        selected.push(unit.clone());
        used += added;
    }
    selected.reverse();
    selected
}

fn joined_len(units: &[String]) -> usize {
    units.iter().map(String::len).sum::<usize>() + units.len().saturating_sub(1) * 2
}

fn split_block(block: &str, budget: usize) -> Vec<String> {
    if block.len() <= budget {
        return vec![block.to_string()];
    }

    if let Some(parts) = split_fenced_block(block, budget) {
        return parts;
    }
    if let Some(parts) = split_table(block, budget) {
        return parts;
    }

    split_at_semantic_boundaries(block, budget)
}

/// Split a closed fenced code block into independently valid fenced blocks.
/// An unclosed fence remains intact rather than manufacturing misleading syntax.
fn split_fenced_block(block: &str, budget: usize) -> Option<Vec<String>> {
    let lines: Vec<&str> = block.lines().collect();
    let (marker, width) = opening_fence(lines.first().copied()?)?;
    let closing = lines.last().copied()?;
    if lines.len() < 2 || !is_closing_fence(closing, marker, width) {
        return Some(vec![block.to_string()]);
    }

    let opening = lines[0];
    let overhead = opening.len() + closing.len() + 2;
    if overhead >= budget {
        return Some(vec![block.to_string()]);
    }

    let inner = lines[1..lines.len() - 1].join("\n");
    let parts = split_at_semantic_boundaries(&inner, budget - overhead);
    if parts.is_empty() {
        return Some(vec![block.to_string()]);
    }

    Some(
        parts
            .into_iter()
            .map(|part| format!("{opening}\n{part}\n{closing}"))
            .collect(),
    )
}

/// Repeat a Markdown table's header when row groups have to be split.
fn split_table(block: &str, budget: usize) -> Option<Vec<String>> {
    let lines: Vec<&str> = block.lines().collect();
    if lines.len() < 3 || !is_table_delimiter(lines[1]) || !lines[0].contains('|') {
        return None;
    }

    let header = format!("{}\n{}", lines[0], lines[1]);
    if header.len() + 1 >= budget {
        return Some(vec![block.to_string()]);
    }

    let row_budget = budget - header.len() - 1;
    let mut chunks = Vec::new();
    let mut rows = Vec::new();
    for row in &lines[2..] {
        let prospective =
            rows.iter().map(|line: &&str| line.len()).sum::<usize>() + rows.len() + row.len();
        if !rows.is_empty() && prospective > row_budget {
            chunks.push(format!("{header}\n{}", rows.join("\n")));
            rows.clear();
        }
        rows.push(*row);
    }
    if !rows.is_empty() {
        chunks.push(format!("{header}\n{}", rows.join("\n")));
    }

    Some(chunks)
}

fn is_table_delimiter(line: &str) -> bool {
    let trimmed = line.trim().trim_matches('|');
    let cells: Vec<&str> = trimmed.split('|').map(str::trim).collect();
    !cells.is_empty()
        && cells.iter().all(|cell| {
            let cell = cell.trim_matches(':');
            cell.len() >= 3 && cell.bytes().all(|byte| byte == b'-')
        })
}

/// Prefer newline, then sentence, then word boundaries. A raw UTF-8-safe
/// character cut is only used when a single token itself exceeds the budget.
fn split_at_semantic_boundaries(text: &str, budget: usize) -> Vec<String> {
    if text.trim().is_empty() {
        return Vec::new();
    }

    let budget = budget.max(1);
    let mut remaining = text.trim();
    let mut parts = Vec::new();

    while remaining.len() > budget {
        let limit = floor_char_boundary(remaining, budget);
        let window = &remaining[..limit];
        let split = last_newline_boundary(window)
            .or_else(|| last_sentence_boundary(window))
            .or_else(|| last_word_boundary(window))
            .unwrap_or(limit);

        let part = remaining[..split].trim();
        if !part.is_empty() {
            parts.push(part.to_string());
        }
        remaining = remaining[split..].trim_start();
    }

    if !remaining.trim().is_empty() {
        parts.push(remaining.trim().to_string());
    }
    parts
}

fn floor_char_boundary(text: &str, at: usize) -> usize {
    let mut boundary = at.min(text.len());
    while boundary > 0 && !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary.max(text.chars().next().map(char::len_utf8).unwrap_or(0))
}

fn last_newline_boundary(window: &str) -> Option<usize> {
    window.rfind('\n').map(|index| index + 1)
}

fn last_sentence_boundary(window: &str) -> Option<usize> {
    let mut candidate = None;
    let mut chars = window.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if matches!(ch, '.' | '?' | '!')
            && chars.peek().is_none_or(|(_, next)| next.is_whitespace())
        {
            candidate = Some(index + ch.len_utf8());
        }
    }
    candidate
}

fn last_word_boundary(window: &str) -> Option<usize> {
    window
        .char_indices()
        .filter_map(|(index, ch)| ch.is_whitespace().then_some(index))
        .last()
}

fn is_substantive(body: &str) -> bool {
    body.lines().any(|line| {
        let line = line.trim();
        if line.is_empty()
            || opening_fence(line).is_some()
            || line.starts_with("<!--")
            || matches!(line, "---" | "***" | "___")
        {
            return false;
        }
        line.chars().any(char::is_alphanumeric)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(chunk_size: usize, chunk_overlap: usize) -> Config {
        Config {
            chunk_size,
            chunk_overlap,
            ..Default::default()
        }
    }

    #[test]
    fn chunks_respect_size() {
        let cfg = config(100, 20);
        let text = "Paragraph one here.\n\nParagraph two is a bit longer and talks about things.\n\nParagraph three.";
        let chunks = chunk_markdown(text, &cfg);
        assert!(!chunks.is_empty());
        for chunk in &chunks {
            assert!(
                chunk.content.len() <= cfg.chunk_size,
                "chunk too large: {}",
                chunk.content.len()
            );
        }
    }

    #[test]
    fn every_section_continuation_keeps_its_heading_path() {
        let cfg = config(105, 0);
        let text = "\
# Runtime

Introductory material.

## Failure safety

The compiler must fail closed when configuration is absent. It must not expose raw retrieved text.

The hook may emit one concise warning per session.";

        let chunks = chunk_markdown(text, &cfg);
        let safety_chunks: Vec<&Chunk> = chunks
            .iter()
            .filter(|chunk| chunk.content.contains("compiler") || chunk.content.contains("hook"))
            .collect();

        assert!(safety_chunks.len() >= 2, "got: {chunks:#?}");
        for chunk in safety_chunks {
            assert!(
                chunk
                    .content
                    .starts_with("# Runtime\n## Failure safety\n\n"),
                "missing heading path: {}",
                chunk.content
            );
        }
    }

    #[test]
    fn long_paragraphs_split_at_sentence_or_word_boundaries() {
        let cfg = config(45, 0);
        let text =
            "First sentence stays whole. Second sentence stays whole. extraordinarilylongtoken";
        let chunks = chunk_markdown(text, &cfg);

        assert_eq!(chunks[0].content, "First sentence stays whole.");
        assert_eq!(chunks[1].content, "Second sentence stays whole.");
        assert_eq!(chunks[2].content, "extraordinarilylongtoken");
        assert!(chunks.iter().all(|chunk| chunk.content.len() <= 45));
    }

    #[test]
    fn heading_only_sections_do_not_create_unusable_chunks() {
        let cfg = config(200, 0);
        let text = "\
# Root

## Empty parent

### Concrete rule

Reject output that has no substantive body.

## Empty tail";
        let chunks = chunk_markdown(text, &cfg);

        assert_eq!(chunks.len(), 1, "got: {chunks:#?}");
        assert!(chunks[0]
            .content
            .starts_with("# Root\n## Empty parent\n### Concrete rule\n\n"));
        assert!(!chunks[0].content.contains("Empty tail"));
    }

    #[test]
    fn split_code_fences_remain_independently_well_formed() {
        let cfg = config(72, 0);
        let text = "\
# Example

```rust
let first_value = compute_the_first_value();
let second_value = compute_the_second_value();
let third_value = compute_the_third_value();
```";
        let chunks = chunk_markdown(text, &cfg);

        assert!(chunks.len() >= 2, "got: {chunks:#?}");
        for chunk in chunks {
            assert_eq!(chunk.content.matches("```rust").count(), 1);
            assert_eq!(
                chunk
                    .content
                    .lines()
                    .filter(|line| line.trim() == "```")
                    .count(),
                1
            );
        }
    }

    #[test]
    fn split_tables_repeat_the_header() {
        let cfg = config(70, 0);
        let text = "\
| Name | Meaning |
| --- | --- |
| alpha | first value with detail |
| beta | second value with detail |
| gamma | third value with detail |";
        let chunks = chunk_markdown(text, &cfg);

        assert!(chunks.len() >= 2, "got: {chunks:#?}");
        for chunk in chunks {
            assert!(chunk
                .content
                .starts_with("| Name | Meaning |\n| --- | --- |\n"));
        }
    }

    #[test]
    fn overlap_carries_complete_units_only() {
        let cfg = config(62, 24);
        let text =
            "Alpha fact is intact.\n\nBeta fact is also intact.\n\nGamma fact remains intact.";
        let chunks = chunk_markdown(text, &cfg);

        assert!(chunks.len() >= 2, "got: {chunks:#?}");
        for chunk in &chunks {
            for paragraph in chunk.content.split("\n\n") {
                assert!(
                    paragraph.starts_with("Alpha")
                        || paragraph.starts_with("Beta")
                        || paragraph.starts_with("Gamma"),
                    "overlap began mid-unit: {paragraph:?}"
                );
            }
        }
    }
}
