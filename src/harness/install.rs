//! `pc install` — detect harnesses, let the user pick, and wire each one.
//!
//! Each [`InstallStrategy`] knows how to render and apply our integration into
//! one harness's config, idempotently and reversibly. Adding a harness needs no
//! changes here as long as it reuses an existing strategy.

use super::selector::{self, Item};
use super::{registry, HarnessSpec, InstallStrategy, Scope, Wiring};
use anyhow::{anyhow, Result};
use colored::Colorize;
use std::path::{Path, PathBuf};

pub(crate) const SENTINEL_OPEN: &str = "# >>> proactive-context (managed) — edit via `pc install` >>>";
pub(crate) const SENTINEL_CLOSE: &str = "# <<< proactive-context <<<";
const PLUGIN_BAKE_MARKER: &str = "const PC_BIN_BAKED = \"\"";
const CODEX_HOOK_EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "Stop",
    "SubagentStart",
    "SubagentStop",
    "PreToolUse",
    "PostToolUse",
    "PreCompact",
    "PostCompact",
    "PermissionRequest",
];
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
    let mut summary = match spec.strategy {
        InstallStrategy::JsonMerge => json_merge(spec, bin, path, dry),
        InstallStrategy::TomlSentinel => sentinel_install(spec, bin, path, dry, render_toml(spec, bin)),
        InstallStrategy::YamlSentinel => yaml_install(spec, bin, path, dry),
        InstallStrategy::FileDrop => file_drop(bin, path, dry),
    }?;
    if spec.id == "codex" && cleanup_legacy_codex_toml(path, dry)? {
        summary.push_str(if dry {
            "; would remove legacy config.toml hook block"
        } else {
            "; removed legacy config.toml hook block"
        });
    }
    Ok(summary)
}

fn uninstall_one(spec: &HarnessSpec, bin: &Path, path: &Path) -> Result<String> {
    let mut summary = match spec.strategy {
        InstallStrategy::JsonMerge => json_unmerge(spec, bin, path),
        InstallStrategy::TomlSentinel | InstallStrategy::YamlSentinel => sentinel_uninstall(path),
        InstallStrategy::FileDrop => {
            if path.exists() {
                std::fs::remove_file(path)?;
                Ok("removed plugin".into())
            } else {
                Ok("nothing to remove".into())
            }
        }
    }?;
    if spec.id == "codex" && cleanup_legacy_codex_toml(path, false)? {
        summary.push_str("; removed legacy config.toml hook block");
    }
    Ok(summary)
}

fn is_installed(spec: &HarnessSpec, path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else { return false };
    match spec.strategy {
        InstallStrategy::FileDrop => true, // file exists ⇒ read ok
        InstallStrategy::JsonMerge => text.contains("--harness"),
        _ => text.contains(SENTINEL_OPEN),
    }
}

// ─── Strategy: JSON merge (Claude, Codex, TENEX hook JSON) ───────────────

fn json_merge(spec: &HarnessSpec, bin: &Path, path: &Path, dry: bool) -> Result<String> {
    let mut root = read_json_config_for_merge(path)?;
    if spec.id == "codex" {
        migrate_codex_root_events(&mut root);
    }

    let bin_prefix = bin.display().to_string();
    let hooks = root
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));
    if !hooks.is_object() {
        return Err(anyhow!(
            "refusing to overwrite {}; existing `hooks` value is {}, expected object",
            path.display(),
            json_type_name(hooks)
        ));
    }
    let hooks = hooks.as_object_mut().unwrap();

    for w in spec.wirings {
        let arr = hooks.entry(w.event).or_insert_with(|| serde_json::json!([]));
        let event_type = json_type_name(arr);
        let groups = arr.as_array_mut().ok_or_else(|| {
            anyhow!(
                "refusing to overwrite {}; existing hooks.{} value is {}, expected array",
                path.display(),
                w.event,
                event_type
            )
        })?;
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

    let statusline_summary = if spec.statusline {
        install_statusline(&mut root, &bin_prefix)
    } else {
        String::new()
    };

    let pretty = serde_json::to_string_pretty(&root)?;
    if dry {
        return Ok(format!(
            "would write {} hook event(s){} to JSON\n{}",
            spec.wirings.len(),
            statusline_summary,
            indent(&pretty)
        ));
    }
    write_with_parents(path, &pretty)?;
    Ok(format!("wired {} hook event(s){}", spec.wirings.len(), statusline_summary))
}

fn read_json_config_for_merge(path: &Path) -> Result<serde_json::Value> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(serde_json::json!({})),
        Err(e) => {
            return Err(anyhow!(
                "could not read existing JSON config {}: {}",
                path.display(),
                e
            ));
        }
    };
    let root: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
        anyhow!(
            "refusing to overwrite {}; existing config is not valid JSON: {}",
            path.display(),
            e
        )
    })?;
    if !root.is_object() {
        return Err(anyhow!(
            "refusing to overwrite {}; existing config is {}, expected object",
            path.display(),
            json_type_name(&root)
        ));
    }
    Ok(root)
}

fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

fn install_statusline(root: &mut serde_json::Value, bin_prefix: &str) -> String {
    let obj = root.as_object_mut().unwrap();
    if obj
        .get("statusLine")
        .map(|existing| !statusline_is_ours(existing, bin_prefix))
        .unwrap_or(false)
    {
        return "; preserved existing statusLine".into();
    }

    obj.insert(
        "statusLine".into(),
        serde_json::json!({ "type": "command", "command": format!("{} hook statusline", bin_prefix) }),
    );
    " + statusline".into()
}

fn statusline_is_ours(statusline: &serde_json::Value, bin_prefix: &str) -> bool {
    statusline
        .get("command")
        .and_then(|c| c.as_str())
        .map(|c| c == format!("{} hook statusline", bin_prefix))
        .unwrap_or(false)
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

fn migrate_codex_root_events(root: &mut serde_json::Value) {
    let Some(obj) = root.as_object_mut() else {
        return;
    };

    let mut moved = Vec::new();
    for event in CODEX_HOOK_EVENTS {
        if let Some(value) = obj.remove(*event) {
            moved.push(((*event).to_string(), value));
        }
    }
    if moved.is_empty() {
        return;
    }

    let hooks = obj.entry("hooks").or_insert_with(|| serde_json::json!({}));
    if !hooks.is_object() {
        *hooks = serde_json::json!({});
    }
    let hooks = hooks.as_object_mut().unwrap();

    for (event, incoming) in moved {
        if let Some(existing) = hooks.get_mut(&event) {
            merge_hook_event(existing, incoming);
        } else {
            hooks.insert(event, incoming);
        }
    }
}

fn merge_hook_event(existing: &mut serde_json::Value, incoming: serde_json::Value) {
    if let Some(existing_arr) = existing.as_array_mut() {
        if let serde_json::Value::Array(mut incoming_arr) = incoming {
            existing_arr.append(&mut incoming_arr);
        }
    } else if existing.is_null() {
        *existing = incoming;
    }
}

fn json_unmerge(spec: &HarnessSpec, bin: &Path, path: &Path) -> Result<String> {
    let Ok(text) = std::fs::read_to_string(path) else { return Ok("nothing to remove".into()) };
    let Ok(mut root) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Ok("config not JSON — skipped".into());
    };
    if spec.id == "codex" {
        migrate_codex_root_events(&mut root);
    }
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

// ─── Strategy: TOML sentinel block ──────────────────────────────────────────

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
pub(crate) fn strip_sentinel(text: &str) -> String {
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

fn cleanup_legacy_codex_toml(hooks_json_path: &Path, dry: bool) -> Result<bool> {
    let Some(dir) = hooks_json_path.parent() else {
        return Ok(false);
    };
    let path = dir.join("config.toml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(false);
    };
    if !text.contains(SENTINEL_OPEN) {
        return Ok(false);
    }
    if !dry {
        let cleaned = strip_legacy_codex_toml_hooks(&text);
        std::fs::write(path, format!("{}\n", cleaned.trim_end()))?;
    }
    Ok(true)
}

fn strip_legacy_codex_toml_hooks(text: &str) -> String {
    let mut out = Vec::new();
    let mut in_managed = false;
    let mut skip_hook_table = false;

    for line in text.lines() {
        if line == SENTINEL_OPEN {
            in_managed = true;
            skip_hook_table = false;
            continue;
        }
        if in_managed && line == SENTINEL_CLOSE {
            in_managed = false;
            skip_hook_table = false;
            continue;
        }

        if in_managed && line.starts_with("[[hooks.") {
            skip_hook_table = true;
            continue;
        }
        if in_managed && line.starts_with('[') {
            skip_hook_table = false;
        }
        if in_managed && skip_hook_table {
            continue;
        }

        out.push(line);
    }

    out.join("\n")
}

pub(crate) fn write_with_parents(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, contents)?;
    Ok(())
}

fn indent(s: &str) -> String {
    s.lines().map(|l| format!("    {}", l)).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn harness(id: &str) -> HarnessSpec {
        crate::harness::registry()
            .into_iter()
            .find(|s| s.id == id)
            .unwrap_or_else(|| panic!("{id} spec exists"))
    }

    #[test]
    fn codex_install_writes_hooks_json_and_migrates_root_events() {
        let temp = tempfile::tempdir().unwrap();
        let codex_dir = temp.path().join(".codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        let hooks_path = codex_dir.join("hooks.json");
        std::fs::write(
            &hooks_path,
            r#"{
  "Stop": [
    {
      "hooks": [
        {
          "type": "command",
          "command": "./bin/brew lgtm",
          "timeout": 360
        }
      ]
    }
  ]
}"#,
        )
        .unwrap();
        std::fs::write(
            codex_dir.join("config.toml"),
            format!(
                "model = \"gpt-5\"\n\n{}\n[[hooks.Stop]]\n[[hooks.Stop.hooks]]\ntype = \"command\"\ncommand = \"/tmp/pc hook capture --harness codex\"\ntimeout = 10\n\n[hooks.state]\n[hooks.state.\"/tmp/hooks.json:stop:0:0\"]\ntrusted_hash = \"sha256:abc\"\n{}\n",
                SENTINEL_OPEN, SENTINEL_CLOSE
            ),
        )
        .unwrap();

        let specs = crate::harness::registry();
        let codex = specs.iter().find(|s| s.id == "codex").unwrap();
        assert_eq!(codex.config_rel, ".codex/hooks.json");

        install_one(codex, Path::new("/tmp/pc"), &hooks_path, false).unwrap();

        let root: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&hooks_path).unwrap()).unwrap();
        assert!(root.get("Stop").is_none());
        assert!(root.get("SessionStart").is_none());
        assert!(root.get("hooks").is_some());
        assert!(root.pointer("/hooks/UserPromptSubmit").is_some());
        assert_eq!(root.pointer("/hooks/Stop").and_then(|v| v.as_array()).unwrap().len(), 2);

        let config_toml = std::fs::read_to_string(codex_dir.join("config.toml")).unwrap();
        assert!(config_toml.contains("model = \"gpt-5\""));
        assert!(config_toml.contains("[hooks.state]"));
        assert!(config_toml.contains("trusted_hash = \"sha256:abc\""));
        assert!(!config_toml.contains(SENTINEL_OPEN));
        assert!(!config_toml.contains("[[hooks.Stop]]"));
    }

    #[test]
    fn json_install_refuses_malformed_existing_config_without_writing() {
        let temp = tempfile::tempdir().unwrap();
        let settings_path = temp.path().join(".claude").join("settings.json");
        std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        let original = r#"{ "permissions": { "allow": ["Bash(git status)"] }, "#;
        std::fs::write(&settings_path, original).unwrap();

        let claude = harness("claude");
        let err = install_one(&claude, Path::new("/tmp/pc"), &settings_path, false)
            .unwrap_err()
            .to_string();

        assert!(err.contains("not valid JSON"));
        assert_eq!(std::fs::read_to_string(&settings_path).unwrap(), original);
    }

    #[test]
    fn json_install_refuses_existing_non_object_config_without_writing() {
        let temp = tempfile::tempdir().unwrap();
        let hooks_path = temp.path().join(".codex").join("hooks.json");
        std::fs::create_dir_all(hooks_path.parent().unwrap()).unwrap();
        let original = r#"["not", "an", "object"]"#;
        std::fs::write(&hooks_path, original).unwrap();

        let codex = harness("codex");
        let err = install_one(&codex, Path::new("/tmp/pc"), &hooks_path, false)
            .unwrap_err()
            .to_string();

        assert!(err.contains("expected object"));
        assert_eq!(std::fs::read_to_string(&hooks_path).unwrap(), original);
    }

    #[test]
    fn json_install_creates_missing_config() {
        let temp = tempfile::tempdir().unwrap();
        let hooks_path = temp.path().join(".codex").join("hooks.json");
        let codex = harness("codex");

        install_one(&codex, Path::new("/tmp/pc"), &hooks_path, false).unwrap();

        let root: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&hooks_path).unwrap()).unwrap();
        assert!(root.is_object());
        assert!(root.pointer("/hooks/UserPromptSubmit").is_some());
        assert!(root.pointer("/hooks/Stop").is_some());
    }

    #[test]
    fn claude_install_preserves_foreign_statusline() {
        let temp = tempfile::tempdir().unwrap();
        let settings_path = temp.path().join(".claude").join("settings.json");
        std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        std::fs::write(
            &settings_path,
            r#"{
  "model": "sonnet",
  "statusLine": {
    "type": "command",
    "command": "/usr/local/bin/other-statusline"
  }
}"#,
        )
        .unwrap();

        let claude = harness("claude");
        let summary = install_one(&claude, Path::new("/tmp/pc"), &settings_path, false).unwrap();

        assert!(summary.contains("preserved existing statusLine"));
        let root: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(
            root.pointer("/statusLine/command").and_then(|v| v.as_str()),
            Some("/usr/local/bin/other-statusline")
        );
        assert_eq!(root.pointer("/model").and_then(|v| v.as_str()), Some("sonnet"));
        assert!(root.pointer("/hooks/UserPromptSubmit").is_some());
        assert!(root.pointer("/hooks/SessionEnd").is_some());
        assert!(root.pointer("/hooks/Stop").is_some());
    }
}
