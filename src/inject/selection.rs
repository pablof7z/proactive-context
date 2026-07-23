use super::*;

pub(crate) fn parse_selected_keys(
    selection: &str,
    valid: &HashSet<&str>,
    max_guides: usize,
) -> Vec<String> {
    selection
        .lines()
        .map(|l| l.trim().trim_start_matches(['-', '*', '•', ' ']).trim())
        .filter(|l| !l.is_empty())
        .filter(|l| !l.eq_ignore_ascii_case("NOTHING_RELEVANT"))
        .filter(|l| valid.contains(*l))
        .take(max_guides)
        .map(|s| s.to_string())
        .collect()
}

pub(crate) fn is_nothing_relevant_line(line: &str) -> bool {
    line.trim()
        .trim_start_matches(['-', '*', '•', ' '])
        .trim_matches('*')
        .trim()
        .eq_ignore_ascii_case("NOTHING_RELEVANT")
}

pub(crate) fn parse_selection_decision(
    selection: &str,
    valid: &HashSet<&str>,
    max_guides: usize,
) -> Result<Vec<String>> {
    let selected = parse_selected_keys(selection, valid, max_guides);
    if !selected.is_empty() {
        return Ok(selected);
    }
    if selection.lines().any(is_nothing_relevant_line) {
        return Ok(Vec::new());
    }
    anyhow::bail!(
        "malformed_selection_response: expected at least one catalog key or NOTHING_RELEVANT; got `{}`",
        truncate(selection, 160)
    )
}
