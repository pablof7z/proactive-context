//! recall REPL — interactive load-everything with live model selection + usage.
//! Builds the corpus once; answers each question against it. Tracks token/cost/
//! cache usage per model and renders a statusbar after every answer.
//!
//! Commands: /model  /brief  /usage  /status  /help  /quit
//! (gemini-cloud re-prefills per question; OpenRouter reports cost + cached tokens.)

use anyhow::Result;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{self, ClearType},
};
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;

use super::{
    ask, corpus, picker,
    store::{self, Store},
    usage::Ledger,
};
use crate::provider::ModelSpec;

const GATE_DEFAULT: &str = "openrouter:deepseek/deepseek-v4-flash";
const PROMPT: &str = "recall> ";

fn label(s: &ModelSpec) -> String {
    format!(
        "{}:{}",
        s.provider_name().to_lowercase().replace(' ', "-"),
        s.model
    )
}

fn help() {
    println!("Ask a question in plain English, or use a command:");
    println!("  /model [spec]   pick or set the processing model");
    println!("  /gate [spec]    pick or set the gate model used by `pc recall gate`");
    println!("  /brief          toggle terse agent-facing answers");
    println!("  /full           switch back to full answers");
    println!("  /last           ask the previous question again");
    println!("  /status         show corpus, models, and answer mode");
    println!("  /usage          token / cost / cache breakdown for this session");
    println!("  /examples       show useful recall questions");
    println!("  /clear          clear the screen");
    println!("  /help           this help");
    println!("  /quit           exit (Ctrl-D also exits)");
    println!();
    println!("Shortcuts: ↑/↓ history, Ctrl-C cancels the current line.");
}

fn examples() {
    println!("Examples:");
    println!("  what are my current preferences for iOS UI?");
    println!("  what did I decide about raw transcripts vs claims?");
    println!("  summarize reversals around OpenRouter and local models");
    println!("  for podcast-player, what architecture boundaries did I insist on?");
    println!("  /brief");
    println!("  what should an agent remember before touching proactive-context?");
}

fn select(title: &str, current: &ModelSpec) -> Option<ModelSpec> {
    eprintln!("fetching models…");
    let entries = picker::fetch_models();
    match picker::pick(title, &label(current), &entries) {
        Ok(Some(spec)) => Some(ModelSpec::parse(&spec)),
        _ => None,
    }
}

#[derive(Clone, Copy)]
enum AnswerMode {
    Full,
    Brief,
}

impl AnswerMode {
    fn is_brief(self) -> bool {
        matches!(self, AnswerMode::Brief)
    }

    fn label(self) -> &'static str {
        match self {
            AnswerMode::Full => "full cited answer",
            AnswerMode::Brief => "brief agent-facing bullets",
        }
    }
}

struct ReplState {
    proc_spec: ModelSpec,
    gate_spec: ModelSpec,
    mode: AnswerMode,
    last_question: Option<String>,
    history: Vec<String>,
}

impl ReplState {
    fn new(spec: &ModelSpec) -> Self {
        Self {
            proc_spec: spec.clone(),
            gate_spec: ModelSpec::parse(GATE_DEFAULT),
            mode: AnswerMode::Full,
            last_question: None,
            history: Vec::new(),
        }
    }
}

struct CorpusView {
    messages: usize,
    dupes: usize,
    token_est: usize,
    db_path: PathBuf,
}

fn print_banner(state: &ReplState, corpus: &CorpusView) {
    println!("recall");
    println!(
        "  corpus: {} messages · {} dupes collapsed · ~{}k tokens",
        corpus.messages, corpus.dupes, corpus.token_est
    );
    println!("  processing: {}", label(&state.proc_spec));
    println!("  answer mode: {}", state.mode.label());
    println!("  database: {}", corpus.db_path.display());
    println!();
    println!("Ask a question, or type /help. Use ↑/↓ for history, /brief for terse answers, /quit to exit.");
    println!();
}

fn print_status(state: &ReplState, corpus: &CorpusView) {
    println!("status");
    println!(
        "  corpus: {} messages · {} dupes collapsed · ~{}k tokens",
        corpus.messages, corpus.dupes, corpus.token_est
    );
    println!("  processing model: {}", label(&state.proc_spec));
    println!("  gate model:       {}", label(&state.gate_spec));
    println!("  answer mode:      {}", state.mode.label());
    println!("  database:         {}", corpus.db_path.display());
    if let Some(q) = &state.last_question {
        println!("  last question:    {}", q);
    }
}

enum Command {
    Ask(String),
    Continue,
    Quit,
}

fn handle_command(
    input: &str,
    state: &mut ReplState,
    ledger: &Ledger,
    corpus: &CorpusView,
) -> Command {
    let trimmed = input.trim();
    let (cmd, arg) = trimmed
        .split_once(char::is_whitespace)
        .map(|(c, rest)| (c, rest.trim()))
        .unwrap_or((trimmed, ""));

    match cmd {
        "/quit" | "/q" | "/exit" | ":q" => Command::Quit,
        "/help" | "/h" | "?" => {
            help();
            Command::Continue
        }
        "/examples" | "/example" => {
            examples();
            Command::Continue
        }
        "/status" | "/s" => {
            print_status(state, corpus);
            Command::Continue
        }
        "/usage" | "/u" => {
            print!("{}", ledger.detailed());
            println!(
                "models — processing: {} · gate: {}",
                label(&state.proc_spec),
                label(&state.gate_spec)
            );
            Command::Continue
        }
        "/brief" | "/b" => {
            state.mode = AnswerMode::Brief;
            println!("answer mode → {}", state.mode.label());
            Command::Continue
        }
        "/full" | "/f" => {
            state.mode = AnswerMode::Full;
            println!("answer mode → {}", state.mode.label());
            Command::Continue
        }
        "/last" | "/again" => {
            if let Some(q) = &state.last_question {
                Command::Ask(q.clone())
            } else {
                println!("No previous question yet.");
                Command::Continue
            }
        }
        "/ask" => {
            if arg.is_empty() {
                println!("usage: /ask <question>");
                Command::Continue
            } else {
                Command::Ask(arg.to_string())
            }
        }
        "/model" => {
            if arg.is_empty() {
                if let Some(s) = select("select PROCESSING model", &state.proc_spec) {
                    state.proc_spec = s;
                    println!("processing model → {}", label(&state.proc_spec));
                }
            } else {
                state.proc_spec = ModelSpec::parse(arg);
                println!("processing model → {}", label(&state.proc_spec));
            }
            Command::Continue
        }
        "/gate" => {
            if arg.is_empty() {
                if let Some(s) = select("select GATE model", &state.gate_spec) {
                    state.gate_spec = s;
                    println!(
                        "gate model → {} (used by `pc recall gate`)",
                        label(&state.gate_spec)
                    );
                }
            } else {
                state.gate_spec = ModelSpec::parse(arg);
                println!(
                    "gate model → {} (used by `pc recall gate`)",
                    label(&state.gate_spec)
                );
            }
            Command::Continue
        }
        "/clear" | "/cls" => {
            let mut out = io::stdout();
            let _ = execute!(out, terminal::Clear(ClearType::All), cursor::MoveTo(0, 0));
            print_banner(state, corpus);
            Command::Continue
        }
        unknown if unknown.starts_with('/') => {
            println!("Unknown command: {unknown}. Type /help for commands, or ask without a leading slash.");
            Command::Continue
        }
        _ => Command::Ask(trimmed.to_string()),
    }
}

enum Input {
    Line(String),
    Eof,
    Interrupted,
}

fn read_prompt(history: &mut Vec<String>) -> Result<Input> {
    if !io::stdin().is_terminal() {
        print!("{PROMPT}");
        io::stdout().flush().ok();
        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 {
            return Ok(Input::Eof);
        }
        return Ok(Input::Line(line.trim_end_matches(['\r', '\n']).to_string()));
    }

    terminal::enable_raw_mode()?;
    let result = read_prompt_raw(history);
    terminal::disable_raw_mode()?;
    result
}

fn read_prompt_raw(history: &[String]) -> Result<Input> {
    let mut out = io::stdout();
    let mut buf: Vec<char> = Vec::new();
    let mut cursor_pos = 0usize;
    let mut history_pos: Option<usize> = None;

    render_line(&mut out, &buf, cursor_pos)?;
    loop {
        if let Event::Key(k) = event::read()? {
            match k.code {
                KeyCode::Enter => {
                    return submit_line(&mut out, &buf);
                }
                KeyCode::Char('j') | KeyCode::Char('m')
                    if k.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    return submit_line(&mut out, &buf);
                }
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                    writeln!(out, "^C")?;
                    out.flush()?;
                    return Ok(Input::Interrupted);
                }
                KeyCode::Char('d')
                    if k.modifiers.contains(KeyModifiers::CONTROL) && buf.is_empty() =>
                {
                    writeln!(out)?;
                    out.flush()?;
                    return Ok(Input::Eof);
                }
                KeyCode::Char(c) => {
                    buf.insert(cursor_pos, c);
                    cursor_pos += 1;
                }
                KeyCode::Backspace => {
                    if cursor_pos > 0 {
                        cursor_pos -= 1;
                        buf.remove(cursor_pos);
                    }
                }
                KeyCode::Delete => {
                    if cursor_pos < buf.len() {
                        buf.remove(cursor_pos);
                    }
                }
                KeyCode::Left => {
                    cursor_pos = cursor_pos.saturating_sub(1);
                }
                KeyCode::Right => {
                    if cursor_pos < buf.len() {
                        cursor_pos += 1;
                    }
                }
                KeyCode::Home => {
                    cursor_pos = 0;
                }
                KeyCode::End => {
                    cursor_pos = buf.len();
                }
                KeyCode::Up => {
                    if !history.is_empty() {
                        let next = match history_pos {
                            Some(i) if i > 0 => i - 1,
                            Some(i) => i,
                            None => history.len() - 1,
                        };
                        history_pos = Some(next);
                        buf = history[next].chars().collect();
                        cursor_pos = buf.len();
                    }
                }
                KeyCode::Down => {
                    if let Some(i) = history_pos {
                        if i + 1 < history.len() {
                            let next = i + 1;
                            history_pos = Some(next);
                            buf = history[next].chars().collect();
                        } else {
                            history_pos = None;
                            buf.clear();
                        }
                        cursor_pos = buf.len();
                    }
                }
                _ => {}
            }
            render_line(&mut out, &buf, cursor_pos)?;
        }
    }
}

fn submit_line(out: &mut io::Stdout, buf: &[char]) -> Result<Input> {
    execute!(
        out,
        cursor::MoveToColumn(0),
        terminal::Clear(ClearType::CurrentLine)
    )?;
    let line: String = buf.iter().collect();
    writeln!(out, "{PROMPT}{line}")?;
    out.flush()?;
    Ok(Input::Line(line))
}

fn render_line(out: &mut io::Stdout, buf: &[char], cursor_pos: usize) -> Result<()> {
    let line: String = buf.iter().collect();
    execute!(
        out,
        cursor::MoveToColumn(0),
        terminal::Clear(ClearType::CurrentLine)
    )?;
    write!(out, "{PROMPT}{line}")?;
    execute!(
        out,
        cursor::MoveToColumn((PROMPT.chars().count() + cursor_pos) as u16)
    )?;
    out.flush()?;
    Ok(())
}

fn remember(history: &mut Vec<String>, line: &str) {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return;
    }
    if history.last().map(|s| s.as_str()) != Some(trimmed) {
        history.push(trimmed.to_string());
    }
}

fn friendly_error(e: &anyhow::Error) {
    let msg = e.to_string();
    eprintln!("error: {msg}");
    if msg.contains("no OpenRouter key") {
        eprintln!("hint: run `pc configure`, set OPENROUTER_API_KEY, or switch with `/model ollama:<model>`.");
    } else if msg.contains("429") || msg.contains("rate") {
        eprintln!("hint: the provider is throttling; try again, switch models with /model, or use a local Ollama model.");
    } else if msg.contains("context")
        || msg.contains("maximum context")
        || msg.contains("too many tokens")
    {
        eprintln!("hint: this REPL loads the whole corpus each question; use a 1M-context model here, or `pc recall ask --chunk` for small-context models.");
    } else if msg.contains("Ollama") {
        eprintln!(
            "hint: check `ollama list`, start Ollama, or set RECALL_OLLAMA=http://host:11434."
        );
    }
}

pub fn run(spec: &ModelSpec) -> Result<()> {
    let store = Store::open()?;
    if store.count()? == 0 {
        anyhow::bail!("recall index is empty — run `pc recall index` first");
    }
    eprintln!("recall: building corpus…");
    let (corpus_txt, stats) = corpus::build(&store)?;

    let mut state = ReplState::new(spec);
    let mut ledger = Ledger::default();
    let corpus_view = CorpusView {
        messages: stats.messages,
        dupes: stats.dupes,
        token_est: stats.chars / 4 / 1000,
        db_path: store::db_path(),
    };

    print_banner(&state, &corpus_view);

    loop {
        let line = match read_prompt(&mut state.history)? {
            Input::Line(line) => line,
            Input::Eof => break,
            Input::Interrupted => continue,
        };
        remember(&mut state.history, &line);
        let q = line.trim();
        if q.is_empty() {
            continue;
        }

        let q = match handle_command(q, &mut state, &ledger, &corpus_view) {
            Command::Ask(q) => q,
            Command::Continue => continue,
            Command::Quit => break,
        };
        if q.trim().is_empty() {
            continue;
        }

        let t = std::time::Instant::now();
        state.last_question = Some(q.clone());
        eprintln!(
            "asking {} ({})…",
            label(&state.proc_spec),
            state.mode.label()
        );
        match ask::ask(
            &state.proc_spec,
            &store,
            &corpus_txt,
            &q,
            state.mode.is_brief(),
        ) {
            Ok(a) => {
                let secs = t.elapsed().as_secs_f64();
                println!("\n{}", a.text);
                let cost = if a.usage.cost_known {
                    format!(" · ${:.4}", a.usage.cost)
                } else {
                    String::new()
                };
                println!(
                    "\n[{}/{} citations valid · {}↑ {}↓ tok · {} cached{} · {:.0}s]",
                    a.cites_valid,
                    a.cites_total,
                    super::usage::fmt_tok(a.usage.prompt_tokens),
                    super::usage::fmt_tok(a.usage.completion_tokens),
                    super::usage::fmt_tok(a.usage.cached_tokens),
                    cost,
                    secs
                );
                ledger.record(&label(&state.proc_spec), &a.usage, secs);
                println!("{}\n", ledger.statusbar());
            }
            Err(e) => friendly_error(&e),
        }
    }
    println!("bye.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_state() -> (ReplState, Ledger, CorpusView) {
        (
            ReplState::new(&ModelSpec::parse("openrouter:test/model")),
            Ledger::default(),
            CorpusView {
                messages: 3,
                dupes: 1,
                token_est: 2,
                db_path: PathBuf::from("/tmp/recall.db"),
            },
        )
    }

    #[test]
    fn slash_ask_strips_command_before_query() {
        let (mut state, ledger, corpus) = fixture_state();
        match handle_command("/ask what did I say?", &mut state, &ledger, &corpus) {
            Command::Ask(q) => assert_eq!(q, "what did I say?"),
            _ => panic!("expected ask"),
        }
    }

    #[test]
    fn direct_model_command_sets_processing_model() {
        let (mut state, ledger, corpus) = fixture_state();
        let _ = handle_command("/model ollama:gemma3:27b", &mut state, &ledger, &corpus);
        assert_eq!(label(&state.proc_spec), "ollama:gemma3:27b");
    }

    #[test]
    fn last_replays_previous_question() {
        let (mut state, ledger, corpus) = fixture_state();
        state.last_question = Some("current stance?".into());
        match handle_command("/last", &mut state, &ledger, &corpus) {
            Command::Ask(q) => assert_eq!(q, "current stance?"),
            _ => panic!("expected ask"),
        }
    }
}
