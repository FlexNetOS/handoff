//! rusty-idd-tui — the interactive ratatui OpenSpec TUI, exposed as a library so
//! the unified `rusty-idd` binary launches it via [`run`] (`rusty-idd tui`). The
//! standalone `openspec-tui` binary was retired in slice 8; this is the sole
//! entry point now.

pub mod app;
pub mod ui;

// The execution layer lives in the `rusty-idd-runner` crate. Re-export its
// modules at the crate root so `crate::config` / `crate::data` / `crate::runner`
// paths in `app.rs` and `ui.rs` keep resolving unchanged.
pub use rusty_idd_runner::{config, data, runner};

use std::io;

use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::prelude::CrosstermBackend;

use app::{App, Screen};

/// Launch the interactive OpenSpec TUI. Sets up the terminal (raw mode +
/// alternate screen), installs a panic hook to restore it, runs the event loop,
/// and tears the terminal back down on exit.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Install panic hook to restore terminal on panic
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original_hook(panic_info);
    }));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let result = run_app(&mut terminal);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    if let Err(err) = result {
        eprintln!("Error: {err}");
        std::process::exit(1);
    }

    Ok(())
}

/// Suspend the TUI, open `$EDITOR` (fallback: `vi`) with a temp file containing `content`,
/// and return the edited text.
fn edit_in_external_editor(
    terminal: &mut ratatui::Terminal<CrosstermBackend<io::Stdout>>,
    content: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    use std::io::Write as _;

    let mut tmp = tempfile::NamedTempFile::new()?;
    tmp.write_all(content.as_bytes())?;
    tmp.flush()?;
    let path = tmp.path().to_path_buf();

    // Restore terminal for the editor
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let status = std::process::Command::new(&editor).arg(&path).status();

    // Re-enter TUI mode
    enable_raw_mode()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    terminal.clear()?;

    status?;
    let edited = std::fs::read_to_string(&path)?;
    Ok(edited)
}

/// Suspend the TUI, run the configured interactive command, and restore the TUI.
fn launch_interactive_tool(
    terminal: &mut ratatui::Terminal<CrosstermBackend<io::Stdout>>,
    config: &config::TuiConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some((binary, args)) = config.build_interactive_command() else {
        return Ok(());
    };

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    let _ = std::process::Command::new(&binary).args(&args).status();

    enable_raw_mode()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    terminal.clear()?;

    Ok(())
}

fn run_app(
    terminal: &mut ratatui::Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut app = App::new()?;

    loop {
        terminal.draw(|frame| ui::draw(frame, &app))?;

        app.poll_implementation();

        if event::poll(Duration::from_millis(500))?
            && let Event::Key(key) = event::read()?
        {
            if key.kind != KeyEventKind::Press {
                continue;
            }

            // In Config screen, all keys go to the config handler
            if matches!(app.screen, Screen::Config { .. }) {
                let open_editor = app.handle_config_input(key.code);
                if open_editor
                    && let Screen::Config {
                        prompt,
                        post_implementation_prompt,
                        focused_field,
                        ..
                    } = &app.screen
                {
                    let (content, is_post_prompt) = match focused_field {
                        app::ConfigField::PostImplementationPrompt => {
                            (post_implementation_prompt.clone(), true)
                        }
                        _ => (prompt.clone(), false),
                    };
                    if let Ok(new_text) = edit_in_external_editor(terminal, &content) {
                        if is_post_prompt {
                            app.set_config_post_prompt(new_text);
                        } else {
                            app.set_config_prompt(new_text);
                        }
                    }
                }
                if app.should_quit {
                    return Ok(());
                }
                continue;
            }

            if key.code == KeyCode::Char('q') {
                return Ok(());
            }

            if key.code == KeyCode::Char('S') {
                app.stop_running_implementation();
                continue;
            }

            if key.code == KeyCode::Char('r') {
                app.refresh_screen();
                continue;
            }

            match &app.screen {
                Screen::ChangeList { .. } => {
                    app.handle_change_list_input(key.code);
                    if app.launch_interactive {
                        app.launch_interactive = false;
                        launch_interactive_tool(terminal, &app.config)?;
                        app.reload_changes();
                    }
                }
                Screen::ArtifactMenu { .. } => {
                    app.handle_artifact_menu_input(key.code);
                }
                Screen::ArtifactView { .. } => {
                    app.handle_artifact_view_input(key.code);
                }
                Screen::DependencyView { .. } => {
                    app.handle_dependency_view_input(key.code);
                }
                Screen::DependencyAdd { .. } => {
                    app.handle_dependency_add_input(key.code);
                }
                Screen::DependencyGraph { .. } => {
                    app.handle_dependency_graph_input(key.code);
                }
                Screen::RunAllSelection { .. } => {
                    app.handle_run_all_selection_input(key.code);
                }
                Screen::Config { .. } => {
                    // Handled above
                }
            }

            if app.should_quit {
                return Ok(());
            }
        }
    }
}
