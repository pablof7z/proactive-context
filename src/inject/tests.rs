use super::*;
use std::collections::{HashSet, VecDeque};
use std::fs;

// Environment variables are process-global, so variant tests share one lock.
static VARIANT_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn query_hit(path: &str, chunk_index: i64, score: f64) -> crate::query::QueryResult {
    crate::query::QueryResult {
        path: path.to_string(),
        chunk_index,
        content: format!("{path} chunk {chunk_index}"),
        content_hash: format!("{path}-{chunk_index}"),
        score,
    }
}

mod catalog_sources;
mod claim_catalog;
mod hook_contract;
mod pipeline;
mod pipeline_failures;
mod pipeline_retries;
mod prompt_variants;
mod protocol_delivery;
mod relevance_policy;
mod support;

use support::{FailingPipelineBackend, PipelineFixture, ScriptedPipelineBackend};
