use super::*;
use anyhow::{bail, Context, Result};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

pub(super) fn load_fixtures(path: Option<&Path>) -> Result<(Vec<CaseFixture>, String)> {
    let (raw, source) = match path {
        Some(path) => (
            fs::read_to_string(path)
                .with_context(|| format!("read recipient-value fixtures {}", path.display()))?,
            path.display().to_string(),
        ),
        None => (
            DEFAULT_FIXTURES.to_string(),
            "embedded:src/fixtures/recipient_value_canaries.jsonl".to_string(),
        ),
    };

    let mut fixtures = Vec::new();
    for (idx, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let fixture: CaseFixture = serde_json::from_str(line)
            .with_context(|| format!("parse recipient-value fixture line {}", idx + 1))?;
        fixtures.push(fixture);
    }
    Ok((fixtures, source))
}

pub(super) fn validate_fixtures(fixtures: &[CaseFixture]) -> Result<()> {
    if fixtures.is_empty() {
        bail!("recipient-value fixture corpus is empty");
    }
    let mut ids = BTreeSet::new();
    for fixture in fixtures {
        if fixture.id.trim().is_empty() {
            bail!("recipient-value fixture has an empty id");
        }
        if !ids.insert(fixture.id.as_str()) {
            bail!("duplicate recipient-value fixture id `{}`", fixture.id);
        }
        if fixture.category.trim().is_empty() {
            bail!(
                "recipient-value fixture `{}` has an empty category",
                fixture.id
            );
        }
        if fixture.canary_role.trim().is_empty() {
            bail!(
                "recipient-value fixture `{}` has an empty canary_role",
                fixture.id
            );
        }
        if fixture.prompt.trim().is_empty() {
            bail!(
                "recipient-value fixture `{}` has an empty prompt",
                fixture.id
            );
        }
        for probe in fixture
            .required_facts
            .iter()
            .chain(&fixture.harmful_facts)
            .chain(&fixture.persona_leaks)
        {
            if probe.label.trim().is_empty() || probe.any_of.iter().all(|s| s.trim().is_empty()) {
                bail!(
                    "recipient-value fixture `{}` has an empty probe label or alternatives",
                    fixture.id
                );
            }
        }
    }
    Ok(())
}

pub(super) fn validate_default_category_coverage(fixtures: &[CaseFixture]) -> Result<()> {
    let categories: BTreeSet<&str> = fixtures.iter().map(|f| f.category.as_str()).collect();
    let missing: Vec<&str> = REQUIRED_CATEGORIES
        .iter()
        .copied()
        .filter(|category| !categories.contains(category))
        .collect();
    if !missing.is_empty() {
        bail!(
            "embedded recipient-value canaries are missing categories: {}",
            missing.join(", ")
        );
    }
    let roles: BTreeSet<&str> = fixtures
        .iter()
        .map(|fixture| fixture.canary_role.as_str())
        .collect();
    let missing_roles: Vec<&str> = REQUIRED_CANARY_ROLES
        .iter()
        .copied()
        .filter(|role| !roles.contains(role))
        .collect();
    if !missing_roles.is_empty() {
        bail!(
            "embedded recipient-value canaries are missing roles: {}",
            missing_roles.join(", ")
        );
    }
    Ok(())
}
