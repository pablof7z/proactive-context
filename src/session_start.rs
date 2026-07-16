use anyhow::Result;
use std::io::{self, Read};
use std::path::Path;

/// Legacy SessionStart hook surface retained so old installed hook configs fail open.
pub fn run_session_start(_harness: &str) -> Result<()> {
    if crate::project_store::hooks_disabled() {
        return Ok(());
    }
    let mut raw = String::new();
    let _ = io::stdin().read_to_string(&mut raw);
    let cwd = serde_json::from_str::<serde_json::Value>(raw.trim())
        .ok()
        .and_then(|value| value.get("cwd").and_then(|value| value.as_str()).map(str::to_owned));
    let Some(cwd) = cwd else {
        return Ok(());
    };
    if crate::project_store::discover_hook_subject(Path::new(&cwd))?.is_none() {
        return Ok(());
    }
    Ok(())
}
