use super::*;

pub(crate) fn build_compile_preamble(
    current_prompt: &str,
    recent: &str,
    already_injected: &str,
    guides: &[(String, String)],
    wiki_dir: &Path,
    root: &Path,
    project_dir: Option<&Path>,
) -> Result<(String, String)> {
    let artifact_context = artifact_context_for_prompt(current_prompt);
    // Label each source by a cwd-relative path the model must cite.
    let sources: Vec<(String, String, ContentKind, Currentness, Authority)> = guides
        .iter()
        .map(|(slug, content)| {
            let label = source_label_for_key(root, wiki_dir, project_dir, slug);
            let (kind, currentness, authority) =
                source_metadata_for_key(wiki_dir, project_dir, slug);
            (label, content.clone(), kind, currentness, authority)
        })
        .collect();
    let source_documents = sources
        .iter()
        .map(|(label, content, kind, currentness, authority)| {
            SourceDocument::new(label, content).with_metadata(*kind, *currentness, *authority)
        })
        .collect::<Vec<_>>();

    let mut context = String::new();
    if !recent.is_empty() {
        context.push_str("RECENT CONVERSATION (background only):\n\n");
        context.push_str(recent);
        context.push_str("\n\n");
    }
    if !already_injected.is_empty() {
        context.push_str(
            "ALREADY DELIVERED THIS SESSION — the facts below were injected on earlier turns and \
are session-absolute delivery exclusions, including after transcript compaction. Your job is to \
surface ONLY genuinely NEW facts that have not already been delivered.\n\
- A fact counts as already-delivered even if the user now asks about it directly — do NOT restate it \
just because the question foregrounds it.\n\
- Example: if \"the manifest lives at .lumen/manifest.json\" was delivered and the user asks \
\"where is the manifest?\", that fact is NOT new — do not emit it.\n\
- If the sources contain NOTHING beyond what was already delivered, output exactly: TITLE: none\n\n\
ALREADY-DELIVERED FACTS:\n",
        );
        context.push_str(already_injected);
        context.push_str("\n\n");
    }
    context.push_str("SOURCE DOCUMENTS (line-numbered; synthesize only what is relevant):\n\n");
    context.push_str(&render_untrusted_sources(&source_documents)?);
    if !already_injected.is_empty() {
        context.push_str(
            "\nBEFORE YOU ANSWER: re-read ALREADY-DELIVERED FACTS above. Drop every claim already \
covered there. Emit only what remains. If nothing remains, output exactly: TITLE: none\n",
        );
    }

    Ok((
        format!(
            "{}\n\n{}\n\n{}\n\n{}\n\n{}",
            compile_preamble(),
            authority_rules(artifact_context),
            COMPILE_RELEVANCE_RULES,
            UNTRUSTED_SOURCE_RULES,
            context
        ),
        current_prompt.to_string(),
    ))
}

pub(crate) fn finalize_compiled_response(response: &str, wiki_dir: &Path) -> String {
    // The synthesized briefing is the output as-is. Its leading `TITLE:` line is stripped by the
    // caller for the status bar; an empty body or `TITLE: none` degrades to a no-inject outcome.
    let resp = response.trim();
    if resp.is_empty() {
        return "NONE".to_string();
    }

    // If the synthesis carried any [^id] markers (copied from source prose), prepend the
    // citation-log preamble — but keep the leading TITLE: line first so the status bar reads it.
    if resp.contains("[^") {
        let citations_log = wiki_dir.join("_citations.log");
        let citations_dir = wiki_dir.join("_citations");
        let pre = format!(
            "Inline [^id] markers cite verbatim source-conversation evidence under {}; \
             {} is a derived convenience cache.\n\n",
            citations_dir.display(),
            citations_log.display()
        );
        if let Some(nl) = resp.find('\n') {
            return format!("{}\n{}{}", &resp[..nl], pre, resp[nl + 1..].trim_start());
        }
    }

    resp.to_string()
}

pub(crate) fn validate_and_finalize_compiled_response(
    response: &str,
    current_prompt: &str,
    guides: &[(String, String)],
    wiki_dir: &Path,
    root: &Path,
    project_dir: Option<&Path>,
) -> Result<String> {
    let sources: Vec<(String, String, ContentKind, Currentness, Authority)> = guides
        .iter()
        .map(|(slug, content)| {
            let label = source_label_for_key(root, wiki_dir, project_dir, slug);
            let (kind, currentness, authority) =
                source_metadata_for_key(wiki_dir, project_dir, slug);
            (label, content.clone(), kind, currentness, authority)
        })
        .collect::<Vec<_>>();
    let source_documents = sources
        .iter()
        .map(|(label, content, kind, currentness, authority)| {
            SourceDocument::new(label, content).with_metadata(*kind, *currentness, *authority)
        })
        .collect::<Vec<_>>();
    let (deduplicated, removed_claims) = deduplicate_compiled_response(response);
    if removed_claims > 0 {
        log_event(
            "inject.compile_dedup",
            None,
            serde_json::json!({"removed_claims": removed_claims}),
        );
    }
    let validated = validate_compile_response_for_context(
        &deduplicated,
        &source_documents,
        artifact_context_for_prompt(current_prompt),
    )?;
    Ok(finalize_compiled_response(validated, wiki_dir))
}

pub(crate) struct CompileBriefingFailure {
    pub(crate) error: anyhow::Error,
    pub(crate) provider_call_count: usize,
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn compile_briefing_with_backend<B: PipelineModelBackend>(
    backend: &mut B,
    current_prompt: &str,
    recent: &str,
    already_injected: &str,
    guides: &[(String, String)],
    wiki_dir: &Path,
    root: &Path,
    project_dir: Option<&Path>,
    max_tokens: usize,
) -> std::result::Result<(String, usize), CompileBriefingFailure> {
    let (system, user) = build_compile_preamble(
        current_prompt,
        recent,
        already_injected,
        guides,
        wiki_dir,
        root,
        project_dir,
    )
    .map_err(|error| CompileBriefingFailure {
        error,
        provider_call_count: 0,
    })?;
    let response = backend
        .complete(PipelineModelRequest {
            stage: PipelineModelStage::Compile,
            system: system.clone(),
            user: user.clone(),
            max_tokens,
        })
        .await
        .map_err(|error| CompileBriefingFailure {
            error,
            provider_call_count: 1,
        })?;
    match validate_and_finalize_compiled_response(
        &response,
        current_prompt,
        guides,
        wiki_dir,
        root,
        project_dir,
    ) {
        Ok(artifact) => Ok((artifact, 1)),
        Err(first_error) => {
            log_event(
                "inject.model_format_retry",
                None,
                serde_json::json!({"stage": "compile", "attempt": 2}),
            );
            let repair_system = format!(
                "{}\n\nFORMAT REPAIR RETRY: the previous output failed structural validation. \
Return a fresh artifact from the same sources. Write exactly one factual claim per non-empty body \
line, at most four body lines, exactly one terminal citation containing one source path per line, and \
no uncited heading or body text. Do not combine source paths inside a citation. TITLE: none must be \
the entire response with no body. If you cannot comply, output exactly TITLE: none.\n\
VALIDATOR ERROR TO CORRECT: {}",
                system,
                truncate(&first_error.to_string(), 500)
            );
            let repaired = backend
                .complete(PipelineModelRequest {
                    stage: PipelineModelStage::Compile,
                    system: repair_system,
                    user,
                    max_tokens,
                })
                .await
                .map_err(|error| CompileBriefingFailure {
                    error,
                    provider_call_count: 2,
                })?;
            validate_and_finalize_compiled_response(
                &repaired,
                current_prompt,
                guides,
                wiki_dir,
                root,
                project_dir,
            )
            .map(|artifact| (artifact, 2))
            .map_err(|second_error| CompileBriefingFailure {
                error: anyhow::anyhow!(
                    "{}; one format-repair retry also failed: {}",
                    first_error,
                    second_error
                ),
                provider_call_count: 2,
            })
        }
    }
}

/// Ask the compile model to synthesize a dense, relevant briefing from the selected sources,
/// requiring an inline `(path:line)` citation after every claim (enforced by prompt, then
/// surfaced verbatim to Claude Code). The model's prose IS the output — sources are presented
/// line-numbered under their absolute path so its citations point back at openable locations.
pub(crate) async fn compile_briefing(
    api_key: &str,
    ollama_api_key: Option<&str>,
    ollama_base_url: &str,
    spec: &ModelSpec,
    current_prompt: &str,
    recent: &str,
    already_injected: &str,
    guides: &[(String, String)],
    wiki_dir: &Path,
    root: &Path,
    project_dir: Option<&Path>,
    max_tokens: usize,
) -> Result<String> {
    let (system, user) = build_compile_preamble(
        current_prompt,
        recent,
        already_injected,
        guides,
        wiki_dir,
        root,
        project_dir,
    )?;
    let response = call_pipeline_model(
        api_key,
        ollama_api_key,
        ollama_base_url,
        spec,
        2,
        &system,
        &user,
        max_tokens,
    )
    .await?;
    validate_and_finalize_compiled_response(
        &response,
        current_prompt,
        guides,
        wiki_dir,
        root,
        project_dir,
    )
}

pub(crate) fn validate_compile_response<'a>(
    response: &'a str,
    source_documents: &[SourceDocument<'_>],
) -> Result<&'a str> {
    validate_compile_response_for_context(response, source_documents, ArtifactContext::Standard)
}

pub(crate) fn validate_compile_response_for_context<'a>(
    response: &'a str,
    source_documents: &[SourceDocument<'_>],
    context: ArtifactContext,
) -> Result<&'a str> {
    let response = response.trim();
    if response.is_empty() {
        anyhow::bail!("malformed_compile_response: response was empty");
    }
    validate_compiled_artifact_for_context(response, source_documents, context)
        .map_err(|error| anyhow::anyhow!("malformed_compile_response: {error}"))?;
    let claim_lines = response
        .lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    if claim_lines.len() > 4 {
        anyhow::bail!(
            "malformed_compile_response: compiled artifact has {} claim lines; maximum is 4",
            claim_lines.len()
        );
    }
    reject_repeated_claims(&claim_lines)?;
    Ok(response)
}

fn reject_repeated_claims(claim_lines: &[&str]) -> Result<()> {
    for right in 1..claim_lines.len() {
        for left in 0..right {
            if claims_semantically_repeat(claim_lines[left], claim_lines[right]) {
                anyhow::bail!(
                    "malformed_compile_response: compiled artifact line {} semantically repeats line {}",
                    right + 2,
                    left + 2
                );
            }
        }
    }
    Ok(())
}

pub(crate) fn deduplicate_compiled_response(response: &str) -> (String, usize) {
    let mut lines = response.lines();
    let Some(title) = lines.next() else {
        return (response.to_string(), 0);
    };
    let mut kept_claims: Vec<&str> = Vec::new();
    let mut removed = 0usize;
    for line in lines.filter(|line| !line.trim().is_empty()) {
        if kept_claims
            .iter()
            .any(|kept| claims_semantically_repeat(kept, line))
        {
            removed += 1;
        } else {
            kept_claims.push(line);
        }
    }
    let mut output = title.to_string();
    for claim in kept_claims {
        output.push('\n');
        output.push_str(claim);
    }
    (output, removed)
}

fn claims_semantically_repeat(left: &str, right: &str) -> bool {
    let left = claim_terms(left);
    let right = claim_terms(right);
    let smaller = left.len().min(right.len());
    if smaller == 0 {
        return false;
    }
    let shared = left.intersection(&right).count();
    shared >= 6 && shared * 10 >= smaller * 4
}

fn claim_terms(line: &str) -> HashSet<String> {
    let claim = line.rsplit_once(" (").map_or(line, |(claim, _)| claim);
    claim
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter_map(|raw| {
            let token = stem_claim_term(&raw.to_ascii_lowercase());
            (token.len() >= 3 && !is_claim_stopword(&token)).then_some(token)
        })
        .collect()
}

fn stem_claim_term(raw: &str) -> String {
    let mut token = raw.to_string();
    for suffix in ["ing", "ed", "es", "s"] {
        if token.len() > suffix.len() + 3 && token.ends_with(suffix) {
            token.truncate(token.len() - suffix.len());
            if token.as_bytes().last() == token.as_bytes().iter().rev().nth(1) {
                token.pop();
            }
            break;
        }
    }
    token
}

fn is_claim_stopword(token: &str) -> bool {
    matches!(
        token,
        "the"
            | "and"
            | "for"
            | "from"
            | "with"
            | "that"
            | "this"
            | "only"
            | "when"
            | "after"
            | "before"
            | "into"
            | "while"
            | "same"
            | "each"
            | "every"
            | "their"
            | "then"
    )
}

// ─── Eval harness public wrappers ────────────────────────────────────────────

/// Public async wrapper for `compile_briefing`, callable from the eval runner.
/// Signature mirrors the private function exactly so the eval can call it via
/// `tokio::runtime::Runtime::block_on`.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn compile_briefing_pub(
    api_key: &str,
    ollama_api_key: Option<&str>,
    ollama_base_url: &str,
    spec: &crate::provider::ModelSpec,
    current_prompt: &str,
    recent: &str,
    already_injected: &str,
    guides: &[(String, String)],
    wiki_dir: &std::path::Path,
    root: &std::path::Path,
    max_tokens: usize,
) -> anyhow::Result<String> {
    compile_briefing(
        api_key,
        ollama_api_key,
        ollama_base_url,
        spec,
        current_prompt,
        recent,
        already_injected,
        guides,
        wiki_dir,
        root,
        None,
        max_tokens,
    )
    .await
}

// ─── Helpers ─────────────────────────────────────────────────────────────────
