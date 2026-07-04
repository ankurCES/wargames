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
mod splash;
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
    /// Directory containing scenario JSON files.
    #[arg(long, default_value = "scenarios")]
    scenarios_dir: PathBuf,
    /// Print the resolved settings path and exit.
    #[arg(long)]
    print_config_path: bool,
    /// Disable the splash countdown and start directly at the picker.
    /// Mostly useful for the e2e smoke test.
    #[arg(long)]
    skip_splash: bool,
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

    let mut app = App::new(settings, cli.scenarios_dir);
    if cli.skip_splash {
        app.skip_splash();
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
                }
                Ok(LlmResult::Err(e)) => {
                    app.set_idle();
                    app.status = format!(
                        "LLM error: {} — falling back to heuristic",
                        short_err(&e)
                    );
                    let _ = app.apply_heuristic_opponent();
                    app.opponent_pending = false;
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
                app.streaming_action = None;
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
                crossterm::event::KeyCode::Char(c) => KeyCode::Char(c),
                _ => KeyCode::Any,
            }),
            _ => Some(KeyCode::Any),
        }
    }
}