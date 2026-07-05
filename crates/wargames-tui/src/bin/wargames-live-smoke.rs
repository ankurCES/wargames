//! Live smoke harness. Bypasses the TUI; loads the real `~/.blumi/settings.json`,
//! calls `LlmClient::decide_stream` against `providers.minimax` with three
//! varied user prompts, and asserts that the actions come back varied.
//!
//! Why: parsing tests prove the SSE parser works in isolation. Only a live
//! run proves the user-visible opponent varies. This harness exists so the
//! loop on "opponent is always feint" can be closed: the binary hits the
//! real configured provider, the parser eats the real Anthropic-format
//! response, and three distinct prompts return three distinct actions.
//!
//! Run with: `cargo run -p wargames-tui --bin wargames-live-smoke`
//!
//! Exits 0 iff at least two distinct actions are returned across the three
//! prompts. Exits 1 otherwise, with the actions printed to stdout so the
//! user can see what their live game would render.
//!
//! Network ceiling is set short (8s) so a stuck provider fails fast.

#[tokio::main(flavor = "current_thread")]
async fn main() {
    use wargames_tui::config::BlumiSettings;
    use wargames_tui::llm::{StreamToken, SOVIET_SYSTEM_PROMPT};

    // SAFETY: this smoke binary owns the env for its lifetime; no
    // concurrent readers.
    unsafe {
        std::env::set_var("WOPR_NET_TIMEOUT_MS", "8000");
    }

    let settings = match BlumiSettings::from_default_path() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[smoke] settings load failed: {}", e);
            std::process::exit(2);
        }
    };

    let Some(client) = wargames_tui::llm::LlmClient::from_settings(&settings) else {
        eprintln!("[smoke] LlmClient::from_settings returned None — provider/router missing");
        std::process::exit(2);
    };

    eprintln!(
        "[smoke] targeting provider at {} model={}",
        client.base_url, client.model
    );

    let prompts: &[&str] = &[
        "DEFCON 2. Carrier group detected in the Bearing Sea. Player just issued: LAUNCH_COUNTER_STRIKE on grid B-12.",
        "DEFCON 3. Player just opened a hotline and issued: REQUEST_NEGOTIATIONS, offering verification of submarine patrols.",
        "DEFCON 4. Player just issued: STAND_DOWN — they pulled the patrol back from the line. Public opinion shifting toward disarmament.",
    ];

    let mut actions: Vec<(String, String)> = Vec::new();

    for prompt in prompts {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let result = client.decide_stream(SOVIET_SYSTEM_PROMPT, prompt, tx).await;

        while let Some(tok) = rx.recv().await {
            if matches!(tok, StreamToken::Done(_)) {
                break;
            }
        }

        match result {
            Some(a) => {
                eprintln!("[smoke] prompt -> action={:?} message={:?}", a.action, a.message);
                actions.push((a.action, a.message));
            }
            None => {
                eprintln!("[smoke] prompt -> None (parse failure, timeout, or no tool_use)");
            }
        }
    }

    let distinct: std::collections::HashSet<_> = actions.iter().map(|(a, _)| a).collect();
    eprintln!(
        "[smoke] summary: {} responses, {} distinct actions",
        actions.len(),
        distinct.len()
    );
    for (a, m) in &actions {
        println!("{}\t{}", a, m);
    }

    // Whole point of this harness: under the broken parser, every one
    // of these came back as None and the heuristic took over. With the
    // parser fix, three distinct prompts return three distinct actions
    // from the LLM, proving the user-visible opponent now varies.
    if distinct.len() >= 2 {
        std::process::exit(0);
    }
    eprintln!(
        "[smoke] FAIL: expected at least 2 distinct actions across {} prompts; got {:?}",
        actions.len(),
        distinct
    );
    std::process::exit(1);
}
