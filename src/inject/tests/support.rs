use super::*;

pub(super) struct ScriptedPipelineBackend {
    pub(super) replies: VecDeque<String>,
    pub(super) calls: Vec<(String, String, usize)>,
}

impl PipelineModelBackend for ScriptedPipelineBackend {
    fn complete<'a>(
        &'a mut self,
        request: PipelineModelRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + 'a>> {
        self.calls
            .push((request.system, request.user, request.max_tokens));
        let reply = self.replies.pop_front().expect("scripted pipeline reply");
        Box::pin(async move { Ok(reply) })
    }
}

pub(super) struct FailingPipelineBackend {
    pub(super) calls: usize,
}

impl PipelineModelBackend for FailingPipelineBackend {
    fn complete<'a>(
        &'a mut self,
        _request: PipelineModelRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + 'a>> {
        self.calls += 1;
        Box::pin(async { anyhow::bail!("scripted provider failure") })
    }
}

pub(super) struct PipelineFixture {
    pub(super) _temp: tempfile::TempDir,
    pub(super) root: PathBuf,
    pub(super) wiki: PathBuf,
    pub(super) project_dir: PathBuf,
    pub(super) index_rows: Vec<IndexRow>,
    pub(super) hits: Vec<QueryResult>,
    pub(super) overlap: crate::context_overlap::ContextOverlap,
    pub(super) source_label: String,
}

impl PipelineFixture {
    pub(super) fn noun() -> Self {
        std::env::remove_var("PC_NOUN_CATALOG");
        std::env::remove_var("PC_CLAIM_CATALOG");
        std::env::remove_var("PC_RESEARCH_CATALOG");

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("subject");
        let wiki = temp.path().join("wiki");
        let project_dir = temp.path().join("project-state");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&project_dir).unwrap();
        let guide = crate::wiki::guide_path(&wiki, "purplepages");
        fs::create_dir_all(guide.parent().unwrap()).unwrap();
        fs::write(
            &guide,
            "---\ntitle: PurplePages\nslug: purplepages\ntopic: product\nsummary: PurplePages is the public directory.\n---\n\n# PurplePages\n\nPurplePages is the public directory.\n",
        )
        .unwrap();
        let index_rows = crate::wiki::rebuild_index(&wiki, "2026-07-23").unwrap();
        crate::nouns::write_realness_registry(
            &wiki,
            &[crate::nouns::RealnessNoun::new("PurplePages", 3)],
        )
        .unwrap();
        let hits = vec![query_hit("pc-memory/guides/purplepages.md", 0, 0.94)];
        let overlap = crate::context_overlap::ContextOverlap::from_hook(
            "what is PurplePages?",
            None,
            "eval",
            0,
        );
        let source_label = source_label_for_key(&root, &wiki, Some(&project_dir), "purplepages");
        Self {
            _temp: temp,
            root,
            wiki,
            project_dir,
            index_rows,
            hits,
            overlap,
            source_label,
        }
    }
}
