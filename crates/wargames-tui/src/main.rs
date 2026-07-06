//! Wargames binary entrypoint.
//!
//! Splash → country picker → game. Settings are loaded from the hardcoded
//! path `~/.blumi/settings.json`. If the file is missing the binary exits
//! with code 2.

mod app;
mod config;
mod llm;
mod net;
mod panes;
mod picker;
mod settings;
mod splash;
mod text;
mod theme;
mod tts;
mod widget_action;
mod widget_log;
mod widget_predict;
mod widget_radar;
mod widget_spinner;
mod widget_state;

use app::{App, KeyCode, Screen};
use clap::Parser;
use llm::SOVIET_SYSTEM_PROMPT;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;
use tokio::runtime::Runtime;

#[derive(Parser, Debug)]
#[command(name = "wargames", version, about = "WOPR-style war game TUI")]
struct Cli {
    /// Directory containing scenario JSON files. Defaults to the
    /// installer's data directory (`$XDG_DATA_HOME/wargames/scenarios/`,
    /// or `~/.local/share/wargames/scenarios/` if XDG is unset) so the
    /// installed binary finds its bundled scenarios without the user
    /// having to point at a path. Pass `--scenarios-dir scenarios` to
    /// run against an in-source tree.
    #[arg(long)]
    scenarios_dir: Option<PathBuf>,
    /// Print the resolved settings path and exit.
    #[arg(long)]
    print_config_path: bool,
    /// Disable the splash countdown and start directly at the picker.
    /// Mostly useful for the e2e smoke test.
    #[arg(long)]
    skip_splash: bool,
    /// Enter AI vs AI mode immediately: skip the picker and start a match
    /// driven by two learned agents (separate personas + separate
    /// memory + adaptive learner). The scenario is generated from the
    /// conflict corpus using the current time as a seed. Implies
    /// `--skip-splash`. Combine with `--regen` to force a fresh
    /// scenario every launch.
    #[arg(long)]
    ai_vs_ai: bool,
    /// Force a fresh generated scenario (and, if used with `--ai-vs-ai`,
    /// a fresh seed). Without this flag the AI-vs-AI path uses
    /// time-derived seeds which vary between launches but are stable
    /// across saves.
    #[arg(long)]
    regen: bool,
}

/// Resolve the default scenarios directory at runtime: prefer
/// `$XDG_DATA_HOME/wargames/scenarios`, fall back to
/// `$HOME/.local/share/wargames/scenarios`. The installer
/// (`scripts/install.sh`) populates this directory with the bundled
/// `scenarios/*.json` files so the installed binary can find its
/// data without the user passing `--scenarios-dir` or running from
/// inside a source tree. The path may not exist at startup (the user
/// might have deleted it); `App::new` → `resolve_scenarios_dir` will
/// fall through to other candidates before giving up.
fn default_scenarios_dir() -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".local").join("share")))
        .unwrap_or_else(|| PathBuf::from("/usr/local/share"));
    base.join("wargames").join("scenarios")
}

fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    if cli.print_config_path {
        println!("{}", config::blumi_settings_path().display());
        return std::process::ExitCode::SUCCESS;
    }

    let settings = match config::BlumiSettings::from_default_path() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}", e);
            eprintln!(
                "this app shares its config with every blumi app on this device"
            );
            eprintln!("create the file or symlink it from the canonical location");
            return std::process::ExitCode::from(2);
        }
    };

    // Resolve the scenarios directory: --scenarios-dir wins; otherwise use
    // the XDG data dir the installer populates (`scripts/install.sh` copies
    // the bundled `scenarios/` here). This keeps the installed binary
    // self-sufficient — the user doesn't need to cd into a source tree or
    // pass a path. The old default of "scenarios" (relative) silently
    // produced an empty picker post-install, which was the (a) bug.
    let scenarios_dir = cli
        .scenarios_dir
        .unwrap_or_else(default_scenarios_dir);

    let mut app = App::new(settings, scenarios_dir);
    if cli.skip_splash || cli.ai_vs_ai {
        app.skip_splash();
    }
    if cli.ai_vs_ai {
        // `--regen` forces a fresh nanos seed so two consecutive
        // `--ai-vs-ai --regen` runs produce different scenarios.
        if cli.regen {
            use std::time::{SystemTime, UNIX_EPOCH};
            let seed = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0xBEEF);
            app.enter_ai_vs_ai_with_seed(seed);
        } else {
            app.enter_ai_vs_ai();
        }
    }

    let mut terminal = ratatui::init();
    let res = run_loop(&mut terminal, &mut app);
    ratatui::restore();
    res
}

/// Result returned by the LLM task over the channel. Three variants:
///   - `Delta(String)`: a single text delta from the SSE stream. Appended
///     to `App::streaming_message` so the UI renders live.
///   - `Final(Option<CommanderAction>)`: the tool-use stop signal. The
///     action is the assembled payload from input_json_deltas.
///   - `Ok { action, message }`: terminal success with the action + the
///     full message collected during streaming.
///   - `Err(String)`: terminal failure; caller falls back to heuristic.
enum LlmResult {
    Delta(String),
    Final(Option<crate::llm::CommanderAction>),
    Ok { action: String, message: String },
    Err(String),
}
fn short_err(s: &str) -> String {
    if s.len() > 80 {
        format!("{}…", &s[..80])
    } else {
        s.to_string()
    }
}

fn run_loop(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
) -> std::process::ExitCode {
    // Tokio runtime so the LLM task runs on a worker thread; the UI stays
    // responsive on the main thread. Event-driven render — we redraw once
    // per loop iteration (top), then block on input up to `tick` so the
    // spinner updates while a task is in flight.
let rt = Runtime::new().expect("tokio runtime");
    // `LlmResult` is the *terminal* signal from the SSE task. Live text
    // deltas flow over the per-turn `stream_tx` (`tokio::sync::mpsc`),
    // which the run loop drains into `App.streaming_message` while the
    // task is in flight.
    let (tx, rx) = mpsc::channel::<LlmResult>();
    let tick = Duration::from_millis(50);

    loop {
        terminal.draw(|f| app.render(f)).expect("terminal draw");

// 1) Drain any completed LLM result (non-blocking).
        //    We loop because multiple `Delta`s can arrive between redraws.
        loop {
            match rx.try_recv() {
                Ok(LlmResult::Delta(text)) => {
                    // First delta this turn — push a placeholder comm
                    // entry into the log so the user sees the
                    // transcript grow live. Subsequent deltas edit
                    // that same entry in place by index.
                    if app.streaming_comm_idx.is_none() {
                        if let Some(w) = app.world.as_mut() {
                            let turn = w.turn;
                            w.log.push(wargames_core::log::LogEntry::comm(
                                turn,
                                "opp",
                                "",
                            ));
                            app.streaming_comm_idx = Some(w.log.len() - 1);
                        }
                    }
                    if let Some(idx) = app.streaming_comm_idx {
                        if let Some(w) = app.world.as_mut() {
                            if let Some(entry) = w.log.get_mut(idx) {
                                entry.message.push_str(&text);
                                // Trim so a runaway stream doesn't grow
                                // the log row without bound.
                                if entry.message.len() > 256 {
                                    let drop = entry.message.len() - 256;
                                    entry.message.drain(..drop);
                                }
                            }
                        }
                    }
                    app.streaming_message.push_str(&text);
                    // Trim so the live buffer doesn't grow without bound.
                    if app.streaming_message.len() > 256 {
                        let drop = app.streaming_message.len() - 256;
                        app.streaming_message.drain(..drop);
                    }
                }
                Ok(LlmResult::Final(action)) => {
                    if let Some(a) = action {
                        app.streaming_action = Some(a.action);
                    }
                }
                Ok(LlmResult::Ok { action, message }) => {
                    app.set_idle();
                    app.apply_opponent_action(&action, &message);
                    app.opponent_pending = false;
                    // apply_opponent_action consumes the streaming
                    // placeholder internally; ensure the field is
                    // cleared here too as a safety net.
                    app.streaming_comm_idx = None;
                }
                Ok(LlmResult::Err(e)) => {
                    app.set_idle();
                    app.status = format!(
                        "LLM error: {} — falling back to heuristic",
                        short_err(&e)
                    );
                    let _ = app.apply_heuristic_opponent();
                    app.opponent_pending = false;
                    app.streaming_comm_idx = None;
                }
                Err(_) => break,
            }
        }
        // 2) Spawn LLM task if pending and not already in flight.
        if app.opponent_pending && !app.bg.is_busy() {
            if let (Some(llm), Some(msg)) = (app.llm.clone(), app.build_llm_user_msg()) {
                app.set_llm_busy();
                // Reset streaming buffers for this turn.
                app.streaming_message.clear();
                app.streaming_comm_idx = None;
                app.streaming_action = None;
                // Hide the prior turn's comm strip during the new
                // call. We replace it once the LLM returns a
                // canonical response — see `apply_opponent_action`.
                app.last_comm = None;
                app.comm_scroll = 0;
                let tx_done = tx.clone();
                rt.spawn(async move {
                    let (stream_tx, mut stream_rx) =
                        tokio::sync::mpsc::unbounded_channel::<crate::llm::StreamToken>();
                    // Forward live tokens to a thread-safe handle so the
                    // UI thread can append them while the task runs.
                    // We post them back over the terminal `tx` channel
                    // as `LlmResult::Delta`s.
                    let tx_done2 = tx_done.clone();
                    let forwarder = tokio::spawn(async move {
                        while let Some(tok) = stream_rx.recv().await {
                            match tok {
                                crate::llm::StreamToken::Text(s) => {
                                    let _ = tx_done2.send(LlmResult::Delta(s));
                                }
                                crate::llm::StreamToken::Done(action) => {
                                    let _ = tx_done2.send(LlmResult::Final(action));
                                    return;
                                }
                            }
                        }
                    });
                    let action = llm.decide_stream(SOVIET_SYSTEM_PROMPT, &msg, stream_tx).await;
                    let _ = forwarder.await;
                    let res = match action {
                        Some(parsed) => LlmResult::Ok {
                            action: parsed.action,
                            message: parsed.message,
                        },
                        None => LlmResult::Err("LLM returned no commander action".into()),
                    };
                    let _ = tx_done.send(res);
                });
            } else {
                // No LLM configured — use the heuristic immediately.
                let _ = app.apply_heuristic_opponent();
            }
        }
        // 3) Block on input (with a small ceiling so the spinner updates).
        if event::poll(tick).unwrap_or(false) {
            if let Some(code) = event::read_key() {
                if app.screen == Screen::GameOver {
                    return std::process::ExitCode::SUCCESS;
                }
                let quit = match app.screen {
                    Screen::Picker => app.handle_picker_key(code),
                    Screen::Game => app.handle_game_key(code),
                    Screen::Settings => app.handle_settings_key(code),
                    Screen::Splash => {
                        app.skip_splash();
                        false
                    }
                    Screen::GameOver => false,
                };
                if quit {
                    return std::process::ExitCode::SUCCESS;
                }
            }
        }

        // 4) Splash countdown.
        if app.screen == Screen::Splash {
            app.tick_splash();
        }
    }
}

mod event {
    use super::KeyCode;
    use std::time::Duration;

    pub fn poll(timeout: Duration) -> std::io::Result<bool> {
        crossterm::event::poll(timeout)
    }

    pub fn read_key() -> Option<KeyCode> {
        match crossterm::event::read().ok()? {
            crossterm::event::Event::Key(k) => Some(match k.code {
                crossterm::event::KeyCode::Up => KeyCode::Up,
                crossterm::event::KeyCode::Down => KeyCode::Down,
                crossterm::event::KeyCode::Enter => KeyCode::Enter,
                crossterm::event::KeyCode::Esc => KeyCode::Esc,
                crossterm::event::KeyCode::Tab => KeyCode::Tab,
                crossterm::event::KeyCode::BackTab => KeyCode::BackTab,
                crossterm::event::KeyCode::PageUp => KeyCode::PageUp,
                crossterm::event::KeyCode::PageDown => KeyCode::PageDown,
                crossterm::event::KeyCode::Home => KeyCode::Home,
                crossterm::event::KeyCode::End => KeyCode::End,
                crossterm::event::KeyCode::Char(c) => KeyCode::Char(c),
                _ => KeyCode::Any,
            }),
            _ => Some(KeyCode::Any),
        }
    }
}