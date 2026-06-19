use anyhow::Result;
use std::io::{self, Read};

/// Legacy SessionStart hook surface retained so old installed hook configs fail open.
pub fn run_session_start(_harness: &str) -> Result<()> {
    let mut raw = String::new();
    let _ = io::stdin().read_to_string(&mut raw);
    Ok(())
}
