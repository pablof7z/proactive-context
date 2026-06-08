//! `pc install` — detect harnesses, let the user pick, and wire each one.
//!
//! Each [`InstallStrategy`] knows how to render and apply our integration into
//! one harness's config, idempotently and reversibly. Adding a harness needs no
//! changes here as long as it reuses an existing strategy.

use super::selector::{self, Item};
use super::{registry, HarnessSpec, InstallStrategy, Scope, Wiring};
use anyhow::Result;
use colored::Colorize;
use std::path::{Path, PathBuf};

const SENTINEL_OPEN: &str = "# >>> proactive-context (managed) — edit via `pc install` >>>";
const SENTINEL_CLOSE: &str = "# <<< proactive-context <<<";
const PLUGIN_BAKE_MARKER: &str = "const PC_BIN_BAKED = \"\"";
/// The opencode plugin, embedded so `pc install` is self-contained.
const PLUGIN_TS: &str = include_str!("../../integrations/opencode/proactive-context.ts");

pub struct InstallOpts {
    pub harnesses: Option<Vec<String>>,
    pub all: bool,
    pub project: Option<PathBuf>,
    pub dry_run: bool,
    pub status: bool,
    pub uninstall: bool,
}

pub fn run_install(opts: InstallOpts) -> Result<()> {
    let bin = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("pc"));
    let specs = registry();

    if opts.status {
        print_status(&specs, &bin, &opts);
        return Ok(());
    }

    // Resolve the selection.
    let selected: Vec<&HarnessSpec> = if let Some(ids) = &opts.harnesses {
        specs.iter().filter(|s| ids.iter().any(|id| id == s.id)).collect()
    } else if opts.all {
        // --all wires every detected GLOBAL harness. Project-scoped harnesses
        // (TENEX) target a specific project, so they require explicit
        // `--harness <id>` (optionally with `--project`).
        let skipped: Vec<&str> = specs
            .iter()
            .filter(|s| matches!(s.scope, Scope::Project) && (s.detect)())
            .map(|s| s.id)
            .collect();
        if !skipped.is_empty() {
            println!(
                "{} project-scoped harness(es) skipped by --all: {} (install with `pc install --harness {}` [--project DIR])",
                "note:".yellow(),
                skipped.join(", "),
                skipped.join(",")
            );
        }
        specs
            .iter()
            .filter(|s| (s.detect)() && matches!(s.scope, Scope::Global))
            .collect()
    } else {
        match interactive_select(&specs, &opts) {
            Some(sel) => sel,
            None => {
                println!("Cancelled.");
                return Ok(());
            }
        }
    };

    if selected.is_empty() {
        println!("No harnesses selected. Detected: {}", detected_list(&specs));
        return Ok(());
    }

    let verb = if opts.uninstall { "Uninstalling from" } else { "Installing into" };
    for spec in selected {
        let path = config_path(spec, &opts);
        println!("\n{} {} ({})", verb.bold(), spec.name.cyan().bold(), path.display());
        let res = if opts.uninstall {
            uninstall_one(spec, &bin, &path)
        } else {
            install_one(spec, &bin, &path, opts.dry_run)
        };
        match res {
            Ok(summary) => {
                println!("  {} {}", "✓".green(), summary);
                if !opts.uninstall && !opts.dry_run {
                    if let Some(note) = spec.note {
                        println!("  {} {}", "note:".yellow(), note);
                    }
                }
            }
            Err(e) => println!("  {} {}", "✗".red(), e),
        }
    }
    if opts.dry_run {
        println!("\n{}", "(dry run — nothing was written)".dimmed());
    }
    Ok(())
}

// ─── Selection ────────────────────────────────────────────────────────────────

fn interactive_select<'a>(specs: &'a [HarnessSpec], opts: &InstallOpts) -> Option<Vec<&'a HarnessSpec>> {
    let items: Vec<Item> = specs
        .iter()
        .map(|s| {
            let detected = (s.detect)();
            let scope = if matches!(s.scope, Scope::Project) { " · project" } else { "" };
            Item {
                label: format!("{:<12}", s.name),
                hint: if detected {
                    format!("{}{}", "detected".green(), scope.dimmed())
                } else {
                    format!("{}{}", "not detected".dimmed(), scope.dimmed())
                },
                checked: detected,
            }
        })
        .collect();
    let _ = opts;
    let chosen = selector::multiselect("Select harnesses to wire up:", items)?;
    Some(chosen.into_iter().map(|i| &specs[i]).collect())
}

fn detected_list(specs: &[HarnessSpec]) -> String {
    let d: Vec<&str> = specs.iter().filter(|s| (s.detect)()).map(|s| s.id).collect();
    if d.is_empty() { "(none)".into() } else { d.join(", ") }
}

// ─── Status ───────────────────────────────────────────────────────────────────

fn print_status(specs: &[HarnessSpec], bin: &Path, opts: &InstallOpts) {
    println!("{}  ({})", "proactive-context harness status".bold(), bin.display());
    for s in specs {
        let path = config_path(s, opts);
        let detected = if (s.detect)() { "detected".green() } else { "not detected".dimmed() };
        let installed = if is_installed(s, &path) { "installed".green() } else { "—".dimmed() };
        let pending = if s.id == "tenex" { " (pending TENEX hook loader)".yellow().to_string() } else { String::new() };
        println!(
            "  {:<12} {:<14} {:<10}{}  {}",
            s.name.cyan(), detected, installed, pending, path.display().to_string().dimmed()
        );
    }
}

// ─── Paths ────────────────────────────────────────────────────────────────────

fn config_path(spec: &HarnessSpec, opts: &InstallOpts) -> PathBuf {
    match spec.scope {
        Scope::Global => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(spec.config_rel),
        Scope::Project => opts
            .project
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
            .join(spec.config_rel),
    }
}

fn command_str(bin: &Path, wiring: &Wiring, id: &str) -> String {
    format!("{} {} --harness {}", bin.display(), wiring.args, id)
}

// ─── Install / uninstall dispatch ─────────────────────────────────────────────

fn install_one(spec: &HarnessSpec, bin: &Path, path: &Path, dry: bool) -> Result<String> {
    match spec.strategy {
        InstallStrategy::JsonMerge => json_merge(spec, bin, path, dry),
        InstallStrategy::TomlSentinel => sentinel_install(spec, bin, path, dry, render_toml(spec, bin)),
        InstallStrategy::YamlSentinel => yaml_install(spec, bin, path, dry),
        InstallStrategy::FileDrop => file_drop(bin, path, dry),
    }
}

fn uninstall_one(spec: &HarnessSpec, bin: &Path, path: &Path) -> Result<String> {
    match spec.strategy {
        InstallStrategy::JsonMerge => json_unmerge(bin, path),
        InstallStrategy::TomlSentinel | InstallStrategy::YamlSentinel => sentinel_uninstall(path),
        InstallStrategy::FileDrop => {
            if path.exists() {
                std::fs::remove_file(path)?;
                Ok("removed plugin".into())
            } else {
                Ok("nothing to remove".into())
            }
        }
    }
}

fn is_installed(spec: &HarnessSpec, path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else { return false };
    match spec.strategy {
        InstallStrategy::FileDrop => true, // file exists ⇒ read ok
        InstallStrategy::JsonMerge => text.contains("--harness"),
        _ => text.contains(SENTINEL_OPEN),
    }
}

// ─── Strategy: JSON merge (Claude settings.json, TENEX .tenex-hooks.json) ──────

fn json_merge(spec: &HarnessSpec, bin: &Path, path: &Path, dry: bool) -> Result<String> {
    let mut root: serde_json::Value = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if !root.is_object() {
        root = serde_json::json!({});
    }

    let bin_prefix = bin.display().to_string();
    let hooks = root
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));
    if !hooks.is_object() {
        *hooks = serde_json::json!({});
    }
    let hooks = hooks.as_object_mut().unwrap();

    for w in spec.wirings {
        let arr = hooks.entry(w.event).or_insert_with(|| serde_json::json!([]));
        let groups = match arr.as_array_mut() {
            Some(a) => a,
            None => {
                *arr = serde_json::json!([]);
                arr.as_array_mut().unwrap()
            }
        };
        // Drop any prior group of ours (idempotent re-install).
        groups.retain(|g| !group_is_ours(g, &bin_prefix));
        let mut hook = serde_json::json!({
            "type": "command",
            "command": command_str(bin, w, spec.id),
            "timeout": w.timeout,
        });
        let mut group = serde_json::Map::new();
        if let Some(m) = w.matcher {
            group.insert("matcher".into(), serde_json::Value::String(m.into()));
        }
        group.insert("hooks".into(), serde_json::json!([hook.take()]));
        groups.push(serde_json::Value::Object(group));
    }

    if spec.statusline {
        root.as_object_mut().unwrap().insert(
            "statusLine".into(),
            serde_json::json!({ "type": "command", "command": format!("{} statusline", bin_prefix) }),
        );
    }

    let pretty = serde_json::to_string_pretty(&root)?;
    if dry {
        return Ok(format!("would write {} hook event(s) to JSON\n{}", spec.wirings.len(), indent(&pretty)));
    }
    write_with_parents(path, &pretty)?;
    Ok(format!("wired {} hook event(s){}", spec.wirings.len(), if spec.statusline { " + statusline" } else { "" }))
}

fn group_is_ours(group: &serde_json::Value, bin_prefix: &str) -> bool {
    group
        .get("hooks")
        .and_then(|h| h.as_array())
        .map(|a| {
            a.iter().any(|h| {
                h.get("command")
                    .and_then(|c| c.as_str())
                    .map(|c| c.starts_with(bin_prefix))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn json_unmerge(bin: &Path, path: &Path) -> Result<String> {
    let Ok(text) = std::fs::read_to_string(path) else { return Ok("nothing to remove".into()) };
    let Ok(mut root) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Ok("config not JSON — skipped".into());
    };
    let bin_prefix = bin.display().to_string();
    let mut removed = 0;
    if let Some(hooks) = root.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        for (_event, arr) in hooks.iter_mut() {
            if let Some(groups) = arr.as_array_mut() {
                let before = groups.len();
                groups.retain(|g| !group_is_ours(g, &bin_prefix));
                removed += before - groups.len();
            }
        }
        hooks.retain(|_, v| v.as_array().map(|a| !a.is_empty()).unwrap_or(true));
    }
    if let Some(obj) = root.as_object_mut() {
        if obj.get("statusLine").and_then(|s| s.get("command")).and_then(|c| c.as_str())
            .map(|c| c.starts_with(&bin_prefix)).unwrap_or(false)
        {
            obj.remove("statusLine");
        }
    }
    write_with_parents(path, &serde_json::to_string_pretty(&root)?)?;
    Ok(format!("removed {} hook group(s)", removed))
}

// ─── Strategy: TOML sentinel block (Codex config.toml) ────────────────────────

fn render_toml(spec: &HarnessSpec, bin: &Path) -> String {
    let mut out = String::new();
    for w in spec.wirings {
        out.push_str(&format!("[[hooks.{}]]\n", w.event));
        if let Some(m) = w.matcher {
            out.push_str(&format!("matcher = {:?}\n", m));
        }
        out.push_str(&format!("[[hooks.{}.hooks]]\n", w.event));
        out.push_str("type = \"command\"\n");
        out.push_str(&format!("command = {:?}\n", command_str(bin, w, spec.id)));
        out.push_str(&format!("timeout = {}\n", w.timeout));
        out.push('\n');
    }
    out
}

fn sentinel_install(spec: &HarnessSpec, _bin: &Path, path: &Path, dry: bool, body: String) -> Result<String> {
    let block = format!("{}\n{}{}", SENTINEL_OPEN, body, SENTINEL_CLOSE);
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let stripped = strip_sentinel(&existing);
    let sep = if stripped.trim().is_empty() { "" } else { "\n" };
    let next = format!("{}{}{}\n", stripped.trim_end(), sep, block);
    if dry {
        return Ok(format!("would append block to {}\n{}", path.display(), indent(&block)));
    }
    write_with_parents(path, &next)?;
    Ok(format!("wired {} hook(s)", spec.wirings.len()))
}

// ─── Strategy: YAML sentinel block (Hermes config.yaml) ───────────────────────

fn yaml_install(spec: &HarnessSpec, bin: &Path, path: &Path, dry: bool) -> Result<String> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let stripped = strip_sentinel(&existing);
    // A pre-existing top-level `hooks:` key needs care: YAML can't have two.
    // An *empty* one (`hooks: {}` / `hooks: []` / bare) is safe to replace; one
    // with real nested content we won't touch (print a paste-in block instead).
    let foreign = foreign_hooks_state(&stripped);

    let mut body = String::from("hooks:\n");
    // Group wirings by event (Hermes events are distinct here).
    for w in spec.wirings {
        body.push_str(&format!("  {}:\n", w.event));
        if let Some(m) = w.matcher {
            body.push_str(&format!("    - matcher: {:?}\n", m));
            body.push_str(&format!("      command: {:?}\n", command_str(bin, w, spec.id)));
            body.push_str(&format!("      timeout: {}\n", w.timeout));
        } else {
            body.push_str(&format!("    - command: {:?}\n", command_str(bin, w, spec.id)));
            body.push_str(&format!("      timeout: {}\n", w.timeout));
        }
    }
    let block = format!("{}\n{}{}", SENTINEL_OPEN, body, SENTINEL_CLOSE);

    if matches!(foreign, ForeignHooks::NonEmpty) {
        return Ok(format!(
            "{} already has a populated top-level `hooks:` — add these entries under it manually:\n{}",
            path.display(), indent(&body)
        ));
    }
    if dry {
        return Ok(format!("would append block to {}\n{}", path.display(), indent(&block)));
    }
    // Drop an empty `hooks: {}` / `hooks: []` / bare `hooks:` line so we don't
    // create a duplicate top-level key, then append our managed block.
    let base = remove_empty_hooks_line(&stripped);
    let sep = if base.trim().is_empty() { "" } else { "\n" };
    write_with_parents(path, &format!("{}{}{}\n", base.trim_end(), sep, block))?;
    Ok(format!("wired {} hook(s)", spec.wirings.len()))
}

enum ForeignHooks { None, Empty, NonEmpty }

/// Classify a pre-existing, non-managed top-level `hooks:` key.
fn foreign_hooks_state(text: &str) -> ForeignHooks {
    let lines: Vec<&str> = text.lines().collect();
    for (i, l) in lines.iter().enumerate() {
        // top-level key: starts at column 0, not `hooks_auto_accept` etc.
        if l.starts_with("hooks:") && !l.starts_with("hooks_") {
            let val = l["hooks:".len()..].trim();
            if val == "{}" || val == "[]" {
                return ForeignHooks::Empty;
            }
            if !val.is_empty() {
                return ForeignHooks::NonEmpty; // inline value with content
            }
            // bare `hooks:` — empty iff no indented child follows
            let has_child = lines[i + 1..]
                .iter()
                .find(|n| !n.trim().is_empty())
                .map(|n| n.starts_with(' ') || n.starts_with('\t'))
                .unwrap_or(false);
            return if has_child { ForeignHooks::NonEmpty } else { ForeignHooks::Empty };
        }
    }
    ForeignHooks::None
}

/// Remove an empty top-level `hooks:` line (`{}`, `[]`, or bare with no children).
fn remove_empty_hooks_line(text: &str) -> String {
    text.lines()
        .filter(|l| {
            if l.starts_with("hooks:") && !l.starts_with("hooks_") {
                let val = l["hooks:".len()..].trim();
                return !(val.is_empty() || val == "{}" || val == "[]");
            }
            true
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ─── Strategy: file drop (opencode plugin) ────────────────────────────────────

fn file_drop(bin: &Path, path: &Path, dry: bool) -> Result<String> {
    let baked = PLUGIN_TS.replace(
        PLUGIN_BAKE_MARKER,
        &format!("const PC_BIN_BAKED = {:?}", bin.display().to_string()),
    );
    if dry {
        return Ok(format!("would write plugin to {} ({} bytes, binary baked in)", path.display(), baked.len()));
    }
    write_with_parents(path, &baked)?;
    Ok(format!("dropped plugin ({} bytes)", baked.len()))
}

// ─── Sentinel + file helpers ──────────────────────────────────────────────────

/// Remove a previously-written sentinel block (and the blank line before it).
fn strip_sentinel(text: &str) -> String {
    let (Some(start), Some(end)) = (text.find(SENTINEL_OPEN), text.find(SENTINEL_CLOSE)) else {
        return text.to_string();
    };
    if end < start {
        return text.to_string();
    }
    let end = end + SENTINEL_CLOSE.len();
    let mut before = text[..start].to_string();
    let after = &text[end..];
    while before.ends_with('\n') || before.ends_with(' ') {
        before.pop();
    }
    format!("{}{}", before, after)
}

fn sentinel_uninstall(path: &Path) -> Result<String> {
    let Ok(text) = std::fs::read_to_string(path) else { return Ok("nothing to remove".into()) };
    if !text.contains(SENTINEL_OPEN) {
        return Ok("nothing to remove".into());
    }
    let cleaned = strip_sentinel(&text);
    std::fs::write(path, format!("{}\n", cleaned.trim_end()))?;
    Ok("removed managed block".into())
}

fn write_with_parents(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, contents)?;
    Ok(())
}

fn indent(s: &str) -> String {
    s.lines().map(|l| format!("    {}", l)).collect::<Vec<_>>().join("\n")
}
