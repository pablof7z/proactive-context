use crate::artifact_safety::{
    escape_markup_text, render_untrusted_sources, validate_compiled_artifact_for_context,
    ArtifactContext, SourceDocument,
};
use crate::config::{
    load_config, project_context_dir, project_db_path, resolve_project_root, validate_config,
    ConfigScope,
};
use crate::content_kind::{Authority, ContentKind, Currentness};
use crate::events::{init_store_context_with_request, log_event, truncate};
use crate::openrouter::{chat_once, make_client, system_msg, user_msg};
use crate::provider::{build_ollama_client, ModelSpec, Provider};
use crate::query::{run_query, QueryResult, MINIMUM_RELEVANCE_SCORE};
use crate::transcript::parse_transcript;
use crate::wiki::{self, guide_path, IndexRow};
use anyhow::{Context, Result};
use ignore::WalkBuilder;
use rig_core::client::CompletionClient;
use rig_core::completion::Prompt;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::future::Future;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::Instant;
use tokio::runtime::Runtime;

mod activation;
mod catalog_build;
mod catalog_nouns;
mod catalog_rank;
mod catalog_sources;
mod compile;
mod delivery;
mod hook;
mod model;
mod navigation;
mod prompts;
mod relevance;
mod replay;
mod selection;

pub(crate) use activation::*;
pub(crate) use catalog_build::*;
pub(crate) use catalog_nouns::*;
pub(crate) use catalog_rank::*;
pub(crate) use catalog_sources::*;
pub(crate) use compile::*;
pub(crate) use delivery::*;
pub(crate) use hook::*;
pub(crate) use model::*;
pub(crate) use navigation::*;
pub(crate) use prompts::*;
pub(crate) use relevance::*;
pub(crate) use replay::*;
pub(crate) use selection::*;

#[cfg(test)]
mod tests;
