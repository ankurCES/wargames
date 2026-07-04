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
mod widget_state;

use app::{App, KeyCode, Screen};
use clap::Parser;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(name = "wargames", version, about = "WOPR-style war game TUI")]
struct Cli {
    /// Directory containing scenario JSON files.
    #[arg(long, default_value = "scenarios")]
    scenarios_dir: PathBuf,
    /// Print the resolved settings path and exit.
    #[arg(long)]
    print_config_path: bool,
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

    let mut terminal = ratatui::init();

    let res = run_loop(&mut terminal, &mut app);

    ratatui::restore();
    res
}

fn run_loop(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
) -> std::process::ExitCode {
    let tick = Duration::from_millis(100);
    loop {
        terminal
            .draw(|f| app.render(f))
            .expect("terminal draw");

        // Splash countdown.
        if app.screen == Screen::Splash {
            app.tick_splash();
            std::thread::sleep(tick);
            continue;
        }

        if event::poll(tick).unwrap_or(false) {
            if let Some(code) = event::read_key() {
                if app.screen == Screen::GameOver {
                    return std::process::ExitCode::SUCCESS;
                }
                let quit = match app.screen {
                    Screen::Picker => app.handle_picker_key(code),
                    Screen::Game => app.handle_game_key(code),
                    Screen::Splash | Screen::GameOver => false,
                };
                if quit {
                    return std::process::ExitCode::SUCCESS;
                }
                // After committing a player action, run opponent turn.
                if app.screen == Screen::Game && matches!(code, KeyCode::Enter) {
                    app.opponent_turn();
                }
                // Splash skip on any key.
                if app.screen == Screen::Splash {
                    app.skip_splash();
                }
            }
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