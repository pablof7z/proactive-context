use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::{Args, ValueEnum};

use super::extract::{self, DumpOptions, DumpRecord, DumpSource};

#[derive(Debug, Clone, Args)]
pub struct DumpArgs {
    /// Project directory to match by recorded transcript cwd. Defaults to the current directory.
    #[arg(long)]
    pub cwd: Option<PathBuf>,

    /// Dump every local project instead of filtering to --cwd/current directory.
    #[arg(long)]
    pub all_projects: bool,

    /// Also include path-prefix matches below --cwd for non-git directories.
    #[arg(long)]
    pub include_subdirs: bool,

    /// Provider to read.
    #[arg(long, value_enum, default_value = "both")]
    pub provider: ProviderFilter,

    /// Output format.
    #[arg(long, value_enum, default_value = "jsonl")]
    pub format: DumpFormat,

    /// Write to a file instead of stdout.
    #[arg(long, short)]
    pub output: Option<PathBuf>,

    /// Keep original message text instead of eliding pasted code/diffs and inline wrappers.
    #[arg(long)]
    pub raw_text: bool,

    /// Skip ~/.codex/archived_sessions.
    #[arg(long)]
    pub no_archived_codex: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ProviderFilter {
    Both,
    Claude,
    Codex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DumpFormat {
    Jsonl,
    Markdown,
    Text,
}

pub fn run(args: DumpArgs) -> Result<()> {
    if args.all_projects && args.cwd.is_some() {
        bail!("--cwd cannot be combined with --all-projects");
    }

    let cwd = if args.all_projects {
        None
    } else {
        Some(match args.cwd {
            Some(cwd) => cwd,
            None => std::env::current_dir().context("read current directory")?,
        })
    };

    let options = DumpOptions {
        source: match args.provider {
            ProviderFilter::Both => DumpSource::Both,
            ProviderFilter::Claude => DumpSource::Claude,
            ProviderFilter::Codex => DumpSource::Codex,
        },
        cwd,
        include_subdirs: args.include_subdirs,
        include_archived_codex: !args.no_archived_codex,
        clean: !args.raw_text,
    };

    let records = extract::dump_records(&options)?;
    let rendered = render(args.format, &records)?;

    match args.output {
        Some(path) if path.as_os_str() == "-" => {
            print!("{rendered}");
        }
        Some(path) => {
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("create {}", parent.display()))?;
            }
            std::fs::write(&path, rendered).with_context(|| format!("write {}", path.display()))?;
            eprintln!(
                "pc recall dump: wrote {} record(s) to {}",
                records.len(),
                path.display()
            );
        }
        None => {
            print!("{rendered}");
        }
    }

    Ok(())
}

fn render(format: DumpFormat, records: &[DumpRecord]) -> Result<String> {
    match format {
        DumpFormat::Jsonl => render_jsonl(records),
        DumpFormat::Markdown => Ok(render_markdown(records)),
        DumpFormat::Text => Ok(render_text(records)),
    }
}

fn render_jsonl(records: &[DumpRecord]) -> Result<String> {
    let mut out = String::new();
    for record in records {
        out.push_str(&serde_json::to_string(record)?);
        out.push('\n');
    }
    Ok(out)
}

fn render_text(records: &[DumpRecord]) -> String {
    let mut out = String::new();
    for record in records {
        let ts = if record.timestamp.is_empty() {
            "unknown-time"
        } else {
            &record.timestamp
        };
        out.push_str(&format!(
            "[{}] {} {} {}:{} cwd={}\n{}\n\n",
            ts,
            record.provider,
            record.session_id,
            record.transcript_path,
            record.line,
            record.cwd,
            record.message.trim(),
        ));
    }
    out
}

fn render_markdown(records: &[DumpRecord]) -> String {
    let mut out = String::new();
    out.push_str("# Transcript Dump\n\n");
    out.push_str(&format!("{} human-authored message(s)\n\n", records.len()));

    for record in records {
        let ts = if record.timestamp.is_empty() {
            "unknown-time"
        } else {
            &record.timestamp
        };
        out.push_str(&format!(
            "## {} - {} - {}\n\n",
            ts, record.provider, record.session_id
        ));
        out.push_str(&format!(
            "- Transcript: `{}` line `{}`\n- CWD: `{}`\n\n",
            record.transcript_path, record.line, record.cwd
        ));
        let fence = markdown_fence(&record.message);
        out.push_str(&fence);
        out.push('\n');
        out.push_str(record.message.trim());
        out.push('\n');
        out.push_str(&fence);
        out.push_str("\n\n");
    }

    out
}

fn markdown_fence(text: &str) -> String {
    let mut max_run = 0usize;
    let mut current = 0usize;
    for c in text.chars() {
        if c == '`' {
            current += 1;
            max_run = max_run.max(current);
        } else {
            current = 0;
        }
    }
    "`".repeat(max_run.max(3) + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonl_renders_one_record_per_line() {
        let records = vec![DumpRecord {
            provider: "codex".into(),
            project: "proj".into(),
            cwd: "/tmp/proj".into(),
            session_id: "s1".into(),
            timestamp: "2026-01-01T00:00:00Z".into(),
            line: 2,
            transcript_path: "/tmp/rollout.jsonl".into(),
            message: "hello\nworld".into(),
        }];

        let rendered = render(DumpFormat::Jsonl, &records).unwrap();
        let lines: Vec<_> = rendered.lines().collect();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains(r#""provider":"codex""#));
        assert!(lines[0].contains(r#"hello\nworld"#));
    }

    #[test]
    fn markdown_fence_is_longer_than_message_backticks() {
        assert_eq!(markdown_fence("```code```"), "````");
        assert_eq!(markdown_fence("````code````"), "`````");
    }
}
