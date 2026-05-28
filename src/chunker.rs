use crate::config::Config;

/// A chunk of markdown content with its position info.
#[derive(Debug, Clone)]
pub struct Chunk {
    pub content: String,
    pub index: usize,
}

/// Split markdown into overlapping chunks.
/// Strategy: prefer paragraph and heading boundaries, fall back to character windows.
pub fn chunk_markdown(text: &str, cfg: &Config) -> Vec<Chunk> {
    if text.trim().is_empty() {
        return vec![];
    }

    let target = cfg.chunk_size;
    let overlap = cfg.chunk_overlap;

    // Split into "blocks" separated by blank lines (paragraphs / sections)
    let blocks: Vec<&str> = text
        .split("\n\n")
        .map(|b| b.trim())
        .filter(|b| !b.is_empty())
        .collect();

    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut idx = 0;

    for block in blocks {
        // If adding this block would exceed target and we already have content, flush
        if !current.is_empty() && current.len() + block.len() + 2 > target {
            let trimmed = current.trim().to_string();
            if !trimmed.is_empty() {
                chunks.push(Chunk {
                    content: trimmed,
                    index: idx,
                });
                idx += 1;
            }

            // Start new chunk with overlap from the end of previous
            current = take_overlap(&current, overlap);
            current.push_str("\n\n");
        }

        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(block);
    }

    // Flush final chunk
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        chunks.push(Chunk {
            content: trimmed,
            index: idx,
        });
    }

    // If we ended up with nothing (very small file), just return the whole thing
    if chunks.is_empty() {
        chunks.push(Chunk {
            content: text.trim().to_string(),
            index: 0,
        });
    }

    chunks
}

/// Take the last `overlap` characters, trying to start at a word or line boundary.
fn take_overlap(text: &str, overlap: usize) -> String {
    if text.len() <= overlap {
        return text.to_string();
    }

    let mut start = text.len() - overlap;
    // Ensure we're at a valid UTF-8 character boundary
    while start > 0 && !text.is_char_boundary(start) {
        start -= 1;
    }
    // Try to find a nice break point (newline or space) near the start
    let slice = &text[start..];

    if let Some(pos) = slice.find('\n') {
        return slice[pos + 1..].to_string();
    }
    if let Some(pos) = slice.find(' ') {
        return slice[pos + 1..].to_string();
    }
    slice.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_respect_size() {
        let cfg = Config {
            chunk_size: 100,
            chunk_overlap: 20,
            ..Default::default()
        };
        let text = "Paragraph one here.\n\nParagraph two is a bit longer and talks about things.\n\nParagraph three.";
        let chunks = chunk_markdown(text, &cfg);
        assert!(!chunks.is_empty());
        for c in &chunks {
            assert!(c.content.len() <= 150, "chunk too large: {}", c.content.len());
        }
    }
}
