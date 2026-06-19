use crossterm::event::KeyCode;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::TuiConfig;
#[cfg(test)]
use crate::data::ArtifactStatus;
use crate::data::{self, ChangeEntry, ChangeStatusOutput, RunMode};
use crate::runner::{self, BatchImplState, ImplState, stop_implementation};

#[derive(Debug, Clone, PartialEq)]
pub enum ChangeTab {
    Active,
    Archived,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfigField {
    Command,
    Prompt,
    PostImplementationPrompt,
    InteractiveCommand,
    RunFinishedCommand,
}

#[derive(Debug, Clone)]
pub enum Screen {
    ChangeList {
        changes: Vec<ChangeEntry>,
        selected: usize,
        error: Option<String>,
        tab: ChangeTab,
        change_deps: HashMap<String, Vec<String>>,
    },
    ArtifactMenu {
        change_name: String,
        change_dir: PathBuf,
        items: Vec<ArtifactMenuItem>,
        selected: usize,
        is_archived: bool,
    },
    ArtifactView {
        title: String,
        content: String,
        scroll: usize,
        is_plain_text: bool,
        file_path: Option<PathBuf>,
    },
    Config {
        command: String,
        prompt: String,
        post_implementation_prompt: String,
        interactive_command: String,
        run_finished_command: String,
        cursor_position: usize,
        focused_field: ConfigField,
        editing: bool,
    },
    DependencyView {
        change_name: String,
        change_dir: PathBuf,
        dependencies: Vec<String>,
        selected: usize,
        run_mode: data::RunMode,
    },
    DependencyAdd {
        change_name: String,
        change_dir: PathBuf,
        available_changes: Vec<String>,
        selected: usize,
    },
    DependencyGraph {
        graph_text: String,
        scroll: usize,
    },
    RunAllSelection {
        entries: Vec<RunAllEntry>,
        selected: usize,
        error: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct RunAllEntry {
    pub change_name: String,
    pub included: bool,
    pub blocked: bool,
    pub blocked_by: Option<String>,
    pub completed_tasks: u32,
    pub total_tasks: u32,
}

#[derive(Debug, Clone)]
pub struct ArtifactMenuItem {
    pub label: String,
    pub available: bool,
    pub file_path: Option<PathBuf>,
    pub is_spec_header: bool,
    pub is_dependency_item: bool,
}

pub struct App {
    pub screen: Screen,
    pub screen_stack: Vec<Screen>,
    pub should_quit: bool,
    pub launch_interactive: bool,
    pub implementation: Option<ImplState>,
    pub batch: Option<BatchImplState>,
    pub config: TuiConfig,
    pub config_path: PathBuf,
}

impl App {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let screen = match data::list_changes() {
            Ok(list) => {
                let change_deps = data::load_change_dependencies(&list.changes).unwrap_or_default();
                Screen::ChangeList {
                    changes: list.changes,
                    selected: 0,
                    error: None,
                    tab: ChangeTab::Active,
                    change_deps,
                }
            }
            Err(e) => Screen::ChangeList {
                changes: Vec::new(),
                selected: 0,
                error: Some(e),
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
        };

        let config_path = PathBuf::from(crate::config::CONFIG_PATH);
        let config = TuiConfig::load_from(&config_path)?;

        Ok(App {
            screen,
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config,
            config_path,
        })
    }

    pub fn poll_implementation(&mut self) {
        let clear_with_success = if let Some(ref mut state) = self.implementation {
            let mut result = None;
            while let Ok(update) = state.receiver.try_recv() {
                match update {
                    runner::ImplUpdate::Progress { completed, total } => {
                        state.completed = completed;
                        state.total = total;
                    }
                    runner::ImplUpdate::Finished { success } => {
                        result = Some(success);
                        break;
                    }
                    runner::ImplUpdate::Stalled => {
                        result = Some(false);
                        break;
                    }
                    runner::ImplUpdate::Error(_) => {
                        result = Some(false);
                        break;
                    }
                }
            }
            result
        } else {
            None
        };
        if let Some(success) = clear_with_success {
            self.implementation = None;
            self.advance_batch(success);
            // Fire notification if everything is done (no new implementation started)
            if self.implementation.is_none() {
                self.spawn_run_finished_command();
            }
        }
    }

    /// Spawn the configured `run_finished_command` as a fire-and-forget child process.
    /// Does nothing if the command is empty.
    pub fn spawn_run_finished_command(&self) {
        if self.config.run_finished_command.is_empty() {
            return;
        }
        #[cfg(not(windows))]
        let _ = std::process::Command::new("sh")
            .arg("-c")
            .arg(&self.config.run_finished_command)
            .spawn();
        #[cfg(windows)]
        let _ = std::process::Command::new("cmd")
            .arg("/C")
            .arg(&self.config.run_finished_command)
            .spawn();
    }

    pub fn stop_running_implementation(&mut self) {
        if let Some(ref state) = self.implementation {
            stop_implementation(state);
            self.implementation = None;
        }
        self.batch = None;
    }

    /// Advance the batch after the current implementation finishes.
    ///
    /// Advances the batch to the next change after the current one finished.
    ///
    /// Uses the provided `success` flag to determine whether the just-finished
    /// change succeeded or failed, then calls `BatchImplState::advance()` to
    /// get the next change. If there is a next change, starts a new
    /// implementation for it. If the batch is complete, clears the batch state.
    pub fn advance_batch(&mut self, success: bool) {
        let Some(ref mut batch) = self.batch else {
            return;
        };

        let next = batch.advance(success);

        if let Some(next_name) = next {
            let change_dir = self.find_change_dir(&next_name, false);
            let run_mode = data::read_run_mode(&change_dir).unwrap_or_default();
            self.implementation = Some(match run_mode {
                RunMode::Normal => runner::start_implementation(&next_name, &self.config),
                RunMode::Apply => runner::start_apply(&next_name, &self.config),
            });
        } else {
            // Batch is finished
            self.batch = None;
        }
    }

    /// Reload the change list data (called after returning from interactive tool).
    pub fn reload_changes(&mut self) {
        if let Screen::ChangeList {
            changes,
            selected,
            tab,
            change_deps,
            ..
        } = &mut self.screen
        {
            if *tab == ChangeTab::Active {
                match data::list_changes() {
                    Ok(list) => {
                        *changes = list.changes;
                        *change_deps = data::load_change_dependencies(changes).unwrap_or_default();
                    }
                    Err(_) => {
                        *changes = Vec::new();
                        *change_deps = HashMap::new();
                    }
                }
            } else {
                match data::list_archived_changes() {
                    Ok(archived) => {
                        *changes = archived;
                    }
                    Err(_) => {
                        *changes = Vec::new();
                    }
                }
                *change_deps = HashMap::new();
            }
            if *selected >= changes.len() && !changes.is_empty() {
                *selected = changes.len() - 1;
            }
        }
    }

    /// Refresh the current screen's data from underlying sources.
    ///
    /// Each screen variant reloads its data by re-calling the same data functions
    /// used during initial construction. Selection indices are clamped to the new
    /// data length. Config screen is a no-op since it is edited directly in the TUI.
    pub fn refresh_screen(&mut self) {
        match &mut self.screen {
            Screen::ChangeList {
                changes,
                selected,
                tab,
                change_deps,
                ..
            } => {
                if *tab == ChangeTab::Active {
                    match data::list_changes() {
                        Ok(list) => {
                            *changes = list.changes;
                            *change_deps =
                                data::load_change_dependencies(changes).unwrap_or_default();
                        }
                        Err(_) => {
                            *changes = Vec::new();
                            *change_deps = HashMap::new();
                        }
                    }
                } else {
                    match data::list_archived_changes() {
                        Ok(archived) => {
                            *changes = archived;
                        }
                        Err(_) => {
                            *changes = Vec::new();
                        }
                    }
                    *change_deps = HashMap::new();
                }
                if !changes.is_empty() {
                    *selected = (*selected).min(changes.len().saturating_sub(1));
                } else {
                    *selected = 0;
                }
            }
            Screen::ArtifactMenu {
                change_name,
                change_dir,
                items,
                selected,
                is_archived,
            } => {
                let status = if *is_archived {
                    data::get_archived_change_status(change_dir)
                } else {
                    match data::get_change_status(change_name) {
                        Ok(s) => s,
                        Err(_) => return,
                    }
                };
                *items = build_artifact_menu_items(&status, change_dir, *is_archived);
                if !items.is_empty() {
                    *selected = (*selected).min(items.len().saturating_sub(1));
                } else {
                    *selected = 0;
                }
            }
            Screen::ArtifactView {
                content, file_path, ..
            } => {
                if let Some(path) = file_path {
                    *content = data::read_artifact_content(path)
                        .unwrap_or_else(|e| format!("Error reading file: {e}"));
                }
            }
            Screen::DependencyView {
                change_dir,
                dependencies,
                selected,
                run_mode,
                ..
            } => {
                let config = data::read_change_config(change_dir).unwrap_or_default();
                *dependencies = config.depends_on;
                *run_mode = config.run_mode;
                if !dependencies.is_empty() {
                    *selected = (*selected).min(dependencies.len().saturating_sub(1));
                } else {
                    *selected = 0;
                }
            }
            Screen::DependencyGraph { graph_text, .. } => {
                // Reload changes and deps from scratch
                if let Ok(list) = data::list_changes() {
                    let change_deps =
                        data::load_change_dependencies(&list.changes).unwrap_or_default();
                    *graph_text = data::generate_dependency_graph(&change_deps);
                }
            }
            Screen::RunAllSelection {
                entries, selected, ..
            } => {
                if let Ok(list) = data::list_changes() {
                    *entries = build_run_all_entries(&list.changes);
                    if !entries.is_empty() {
                        *selected = (*selected).min(entries.len().saturating_sub(1));
                    } else {
                        *selected = 0;
                    }
                }
            }
            Screen::DependencyAdd {
                change_name,
                available_changes,
                selected,
                ..
            } => {
                let change_name = change_name.clone();
                // Reload existing deps to filter them out
                let cwd = std::env::current_dir().unwrap_or_default();
                let change_dir = cwd.join("openspec").join("changes").join(&change_name);
                let current_deps = data::read_dependencies(&change_dir).unwrap_or_default();
                let available: Vec<String> = match data::list_changes() {
                    Ok(list) => list
                        .changes
                        .into_iter()
                        .map(|c| c.name)
                        .filter(|n| *n != change_name && !current_deps.contains(n))
                        .collect(),
                    Err(_) => Vec::new(),
                };
                *available_changes = available;
                if !available_changes.is_empty() {
                    *selected = (*selected).min(available_changes.len().saturating_sub(1));
                } else {
                    *selected = 0;
                }
            }
            Screen::Config { .. } => {
                // No-op: Config is edited directly in the TUI
            }
        }
    }

    pub fn handle_change_list_input(&mut self, key: KeyCode) {
        let Screen::ChangeList {
            changes,
            selected,
            tab,
            change_deps,
            ..
        } = &mut self.screen
        else {
            return;
        };

        match key {
            KeyCode::Down | KeyCode::Char('j') => {
                if !changes.is_empty() && *selected < changes.len() - 1 {
                    *selected += 1;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if *selected > 0 {
                    *selected -= 1;
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if *tab == ChangeTab::Active {
                    *tab = ChangeTab::Archived;
                    *selected = 0;
                    match data::list_archived_changes() {
                        Ok(archived) => {
                            *changes = archived;
                        }
                        Err(_) => {
                            *changes = Vec::new();
                        }
                    }
                    *change_deps = HashMap::new();
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                if *tab == ChangeTab::Archived {
                    *tab = ChangeTab::Active;
                    *selected = 0;
                    match data::list_changes() {
                        Ok(list) => {
                            *changes = list.changes;
                            *change_deps =
                                data::load_change_dependencies(changes).unwrap_or_default();
                        }
                        Err(_) => {
                            *changes = Vec::new();
                            *change_deps = HashMap::new();
                        }
                    }
                }
            }
            KeyCode::Enter => {
                if changes.is_empty() {
                    return;
                }
                let change = &changes[*selected];
                let change_name = change.name.clone();
                let is_archived = *tab == ChangeTab::Archived;
                self.enter_artifact_menu(&change_name, is_archived);
            }
            KeyCode::Char('C') => {
                self.push_config_screen();
            }
            KeyCode::Char('G') => {
                if *tab == ChangeTab::Active {
                    let graph_text = data::generate_dependency_graph(change_deps);
                    let old_screen = std::mem::replace(
                        &mut self.screen,
                        Screen::DependencyGraph {
                            graph_text,
                            scroll: 0,
                        },
                    );
                    self.screen_stack.push(old_screen);
                }
            }
            KeyCode::Char('A') => {
                if *tab == ChangeTab::Active && self.implementation.is_none() {
                    let entries = build_run_all_entries(changes);
                    let old_screen = std::mem::replace(
                        &mut self.screen,
                        Screen::RunAllSelection {
                            entries,
                            selected: 0,
                            error: None,
                        },
                    );
                    self.screen_stack.push(old_screen);
                }
            }
            KeyCode::Char('I') if *tab == ChangeTab::Active && self.implementation.is_none() => {
                self.launch_interactive = true;
            }
            _ => {}
        }
    }

    fn enter_artifact_menu(&mut self, change_name: &str, is_archived: bool) {
        let change_dir = self.find_change_dir(change_name, is_archived);

        let status = if is_archived {
            data::get_archived_change_status(&change_dir)
        } else {
            match data::get_change_status(change_name) {
                Ok(s) => s,
                Err(_) => return,
            }
        };

        let items = build_artifact_menu_items(&status, &change_dir, is_archived);

        let old_screen = std::mem::replace(
            &mut self.screen,
            Screen::ArtifactMenu {
                change_name: change_name.to_string(),
                change_dir,
                items,
                selected: 0,
                is_archived,
            },
        );
        self.screen_stack.push(old_screen);
    }

    fn find_change_dir(&self, change_name: &str, is_archived: bool) -> PathBuf {
        let cwd = std::env::current_dir().unwrap_or_default();
        if is_archived {
            cwd.join("openspec")
                .join("changes")
                .join("archive")
                .join(change_name)
        } else {
            cwd.join("openspec").join("changes").join(change_name)
        }
    }

    pub fn handle_artifact_menu_input(&mut self, key: KeyCode) {
        let Screen::ArtifactMenu {
            change_name,
            items,
            selected,
            change_dir,
            is_archived,
        } = &mut self.screen
        else {
            return;
        };

        match key {
            KeyCode::Down | KeyCode::Char('j') => {
                if !items.is_empty() && *selected < items.len() - 1 {
                    *selected += 1;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if *selected > 0 {
                    *selected -= 1;
                }
            }
            KeyCode::Enter => {
                if items.is_empty() {
                    return;
                }
                let item = &items[*selected];
                if !item.available || item.is_spec_header {
                    return;
                }
                if item.is_dependency_item {
                    let change_name = change_name.clone();
                    let change_dir = change_dir.clone();
                    let config = data::read_change_config(&change_dir).unwrap_or_default();
                    let old_screen = std::mem::replace(
                        &mut self.screen,
                        Screen::DependencyView {
                            change_name,
                            change_dir,
                            dependencies: config.depends_on,
                            selected: 0,
                            run_mode: config.run_mode,
                        },
                    );
                    self.screen_stack.push(old_screen);
                } else if let Some(path) = &item.file_path {
                    let title = item.label.clone();
                    let content = data::read_artifact_content(path)
                        .unwrap_or_else(|e| format!("Error reading file: {e}"));
                    let is_plain_text = path.extension().is_some_and(|ext| ext == "log");
                    let file_path = Some(path.clone());
                    let change_dir = change_dir.clone();
                    let old_screen = std::mem::replace(
                        &mut self.screen,
                        Screen::ArtifactView {
                            title,
                            content,
                            scroll: 0,
                            is_plain_text,
                            file_path,
                        },
                    );
                    let _ = change_dir;
                    self.screen_stack.push(old_screen);
                }
            }
            KeyCode::Char('L') => {
                let log_path = change_dir.join("implementation.log");
                if log_path.exists() {
                    let content = data::read_artifact_content(&log_path)
                        .unwrap_or_else(|e| format!("Error reading file: {e}"));
                    let file_path = Some(log_path);
                    let old_screen = std::mem::replace(
                        &mut self.screen,
                        Screen::ArtifactView {
                            title: "Implementation Log".to_string(),
                            content,
                            scroll: 0,
                            is_plain_text: true,
                            file_path,
                        },
                    );
                    self.screen_stack.push(old_screen);
                }
            }
            KeyCode::Char('R') => {
                if !*is_archived && self.implementation.is_none() {
                    let name = change_name.clone();
                    let log_path = change_dir.clone().join("implementation.log");
                    let run_mode = data::read_run_mode(change_dir).unwrap_or_default();
                    self.implementation = Some(match run_mode {
                        RunMode::Normal => runner::start_implementation(&name, &self.config),
                        RunMode::Apply => runner::start_apply(&name, &self.config),
                    });
                    let content = data::read_artifact_content(&log_path).unwrap_or_default();
                    let file_path = Some(log_path);
                    let old_screen = std::mem::replace(
                        &mut self.screen,
                        Screen::ArtifactView {
                            title: "Implementation Log".to_string(),
                            content,
                            scroll: 0,
                            is_plain_text: true,
                            file_path,
                        },
                    );
                    self.screen_stack.push(old_screen);
                }
            }
            KeyCode::Char('C') => {
                self.push_config_screen();
            }
            KeyCode::Esc => {
                if let Some(prev) = self.screen_stack.pop() {
                    self.screen = prev;
                }
            }
            _ => {}
        }
    }

    pub fn push_config_screen(&mut self) {
        let old_screen = std::mem::replace(
            &mut self.screen,
            Screen::Config {
                command: self.config.command.clone(),
                prompt: self.config.prompt.clone(),
                post_implementation_prompt: self.config.post_implementation_prompt.clone(),
                interactive_command: self.config.interactive_command.clone(),
                run_finished_command: self.config.run_finished_command.clone(),
                cursor_position: self.config.command.len(),
                focused_field: ConfigField::Command,
                editing: false,
            },
        );
        self.screen_stack.push(old_screen);
    }

    /// Handle input on the Config screen. Returns `true` if the caller
    /// should open `$EDITOR` for the prompt field (Enter on Prompt).
    pub fn handle_config_input(&mut self, key: KeyCode) -> bool {
        let Screen::Config {
            command,
            prompt,
            post_implementation_prompt,
            interactive_command,
            run_finished_command,
            cursor_position,
            focused_field,
            editing,
        } = &mut self.screen
        else {
            return false;
        };

        if *editing {
            // Edit mode (Command, InteractiveCommand, or RunFinishedCommand field)
            let edit_target = if *focused_field == ConfigField::InteractiveCommand {
                interactive_command
            } else if *focused_field == ConfigField::RunFinishedCommand {
                run_finished_command
            } else {
                command
            };
            match key {
                KeyCode::Esc | KeyCode::Enter => {
                    *editing = false;
                }
                KeyCode::Char(c) => {
                    edit_target.insert(*cursor_position, c);
                    *cursor_position += 1;
                }
                KeyCode::Backspace => {
                    if *cursor_position > 0 {
                        *cursor_position -= 1;
                        edit_target.remove(*cursor_position);
                    }
                }
                KeyCode::Delete => {
                    if *cursor_position < edit_target.len() {
                        edit_target.remove(*cursor_position);
                    }
                }
                KeyCode::Left => {
                    if *cursor_position > 0 {
                        *cursor_position -= 1;
                    }
                }
                KeyCode::Right => {
                    if *cursor_position < edit_target.len() {
                        *cursor_position += 1;
                    }
                }
                KeyCode::Home => {
                    *cursor_position = 0;
                }
                KeyCode::End => {
                    *cursor_position = edit_target.len();
                }
                _ => {}
            }
        } else {
            // Navigation mode
            match key {
                KeyCode::Tab | KeyCode::BackTab => {
                    *focused_field = match focused_field {
                        ConfigField::Command => ConfigField::Prompt,
                        ConfigField::Prompt => ConfigField::PostImplementationPrompt,
                        ConfigField::PostImplementationPrompt => ConfigField::InteractiveCommand,
                        ConfigField::InteractiveCommand => ConfigField::RunFinishedCommand,
                        ConfigField::RunFinishedCommand => ConfigField::Command,
                    };
                    if *focused_field == ConfigField::Command {
                        *cursor_position = command.len();
                    } else if *focused_field == ConfigField::InteractiveCommand {
                        *cursor_position = interactive_command.len();
                    } else if *focused_field == ConfigField::RunFinishedCommand {
                        *cursor_position = run_finished_command.len();
                    }
                }
                KeyCode::Esc => {
                    // Discard changes and return to previous screen
                    if let Some(prev) = self.screen_stack.pop() {
                        self.screen = prev;
                    }
                }
                KeyCode::Enter => {
                    if *focused_field == ConfigField::Command {
                        *cursor_position = command.len();
                        *editing = true;
                    } else if *focused_field == ConfigField::InteractiveCommand {
                        *cursor_position = interactive_command.len();
                        *editing = true;
                    } else if *focused_field == ConfigField::RunFinishedCommand {
                        *cursor_position = run_finished_command.len();
                        *editing = true;
                    } else {
                        // Prompt or PostImplementationPrompt field: signal caller to open $EDITOR
                        return true;
                    }
                }
                KeyCode::Char('S') => {
                    // Save config and return
                    let new_config = TuiConfig {
                        command: command.clone(),
                        prompt: prompt.clone(),
                        post_implementation_prompt: post_implementation_prompt.clone(),
                        interactive_command: interactive_command.clone(),
                        run_finished_command: run_finished_command.clone(),
                        retry_on_failure: 1,
                    };
                    let _ = new_config.save_to(&self.config_path);
                    self.config = new_config;
                    if let Some(prev) = self.screen_stack.pop() {
                        self.screen = prev;
                    }
                }
                KeyCode::Char('D') => {
                    // Reset to defaults
                    let defaults = TuiConfig::default();
                    *command = defaults.command;
                    *prompt = defaults.prompt;
                    *post_implementation_prompt = defaults.post_implementation_prompt;
                    *interactive_command = defaults.interactive_command;
                    *run_finished_command = defaults.run_finished_command;
                    *cursor_position = command.len();
                    *focused_field = ConfigField::Command;
                }
                _ => {} // Character keys ignored in navigation mode
            }
        }
        false
    }

    /// Update the prompt text on the Config screen (called after $EDITOR exits).
    pub fn set_config_prompt(&mut self, new_prompt: String) {
        if let Screen::Config { prompt, .. } = &mut self.screen {
            *prompt = new_prompt;
        }
    }

    /// Update the post-implementation prompt on the Config screen (called after $EDITOR exits).
    pub fn set_config_post_prompt(&mut self, new_prompt: String) {
        if let Screen::Config {
            post_implementation_prompt,
            ..
        } = &mut self.screen
        {
            *post_implementation_prompt = new_prompt;
        }
    }

    pub fn handle_dependency_view_input(&mut self, key: KeyCode) {
        let Screen::DependencyView {
            change_name,
            change_dir,
            dependencies,
            selected,
            run_mode,
        } = &mut self.screen
        else {
            return;
        };

        match key {
            KeyCode::Down | KeyCode::Char('j') => {
                if !dependencies.is_empty() && *selected < dependencies.len() - 1 {
                    *selected += 1;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if *selected > 0 {
                    *selected -= 1;
                }
            }
            KeyCode::Char('D') => {
                if !dependencies.is_empty() {
                    dependencies.remove(*selected);
                    let _ = data::write_dependencies(change_dir, dependencies);
                    if *selected > 0 && *selected >= dependencies.len() {
                        *selected = dependencies.len().saturating_sub(1);
                    }
                }
            }
            KeyCode::Char('M') => {
                *run_mode = match run_mode {
                    RunMode::Normal => RunMode::Apply,
                    RunMode::Apply => RunMode::Normal,
                };
                let config = data::ChangeConfig {
                    depends_on: dependencies.clone(),
                    run_mode: run_mode.clone(),
                };
                let _ = data::write_change_config(change_dir, &config);
            }
            KeyCode::Char('A') => {
                let change_name = change_name.clone();
                let change_dir = change_dir.clone();
                let deps = dependencies.clone();

                // Get list of active changes, excluding current and already-added
                let available: Vec<String> = match data::list_changes() {
                    Ok(list) => list
                        .changes
                        .into_iter()
                        .map(|c| c.name)
                        .filter(|n| *n != change_name && !deps.contains(n))
                        .collect(),
                    Err(_) => Vec::new(),
                };

                if !available.is_empty() {
                    let old_screen = std::mem::replace(
                        &mut self.screen,
                        Screen::DependencyAdd {
                            change_name,
                            change_dir,
                            available_changes: available,
                            selected: 0,
                        },
                    );
                    self.screen_stack.push(old_screen);
                }
            }
            KeyCode::Esc => {
                if let Some(prev) = self.screen_stack.pop() {
                    self.screen = prev;
                }
            }
            _ => {}
        }
    }

    pub fn handle_dependency_add_input(&mut self, key: KeyCode) {
        let Screen::DependencyAdd {
            change_name: _,
            change_dir,
            available_changes,
            selected,
        } = &mut self.screen
        else {
            return;
        };

        match key {
            KeyCode::Down | KeyCode::Char('j') => {
                if !available_changes.is_empty() && *selected < available_changes.len() - 1 {
                    *selected += 1;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if *selected > 0 {
                    *selected -= 1;
                }
            }
            KeyCode::Enter => {
                if available_changes.is_empty() {
                    return;
                }
                let chosen = available_changes[*selected].clone();
                let change_dir = change_dir.clone();

                // Pop back to DependencyView and add the dependency
                if let Some(Screen::DependencyView {
                    dependencies,
                    selected: dep_selected,
                    ..
                }) = self.screen_stack.last_mut()
                {
                    dependencies.push(chosen);
                    let _ = data::write_dependencies(&change_dir, dependencies);
                    // Reset selection if it was on the empty placeholder
                    if dependencies.len() == 1 {
                        *dep_selected = 0;
                    }
                }

                if let Some(prev) = self.screen_stack.pop() {
                    self.screen = prev;
                }
            }
            KeyCode::Esc => {
                if let Some(prev) = self.screen_stack.pop() {
                    self.screen = prev;
                }
            }
            _ => {}
        }
    }

    pub fn handle_artifact_view_input(&mut self, key: KeyCode) {
        let Screen::ArtifactView {
            scroll, content, ..
        } = &mut self.screen
        else {
            return;
        };

        let line_count = content.lines().count();

        match key {
            KeyCode::Down | KeyCode::Char('j') => {
                if line_count > 0 && *scroll < line_count.saturating_sub(1) {
                    *scroll += 1;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if *scroll > 0 {
                    *scroll -= 1;
                }
            }
            KeyCode::Char('C') => {
                self.push_config_screen();
            }
            KeyCode::Esc => {
                if let Some(prev) = self.screen_stack.pop() {
                    self.screen = prev;
                }
            }
            _ => {}
        }
    }

    pub fn handle_dependency_graph_input(&mut self, key: KeyCode) {
        let Screen::DependencyGraph { scroll, graph_text } = &mut self.screen else {
            return;
        };

        let line_count = graph_text.lines().count();

        match key {
            KeyCode::Down | KeyCode::Char('j') => {
                if line_count > 0 && *scroll < line_count.saturating_sub(1) {
                    *scroll += 1;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if *scroll > 0 {
                    *scroll -= 1;
                }
            }
            KeyCode::Esc => {
                if let Some(prev) = self.screen_stack.pop() {
                    self.screen = prev;
                }
            }
            _ => {}
        }
    }

    pub fn handle_run_all_selection_input(&mut self, key: KeyCode) {
        let Screen::RunAllSelection {
            entries,
            selected,
            error,
        } = &mut self.screen
        else {
            return;
        };

        match key {
            KeyCode::Down | KeyCode::Char('j') => {
                if !entries.is_empty() && *selected < entries.len() - 1 {
                    *selected += 1;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if *selected > 0 {
                    *selected -= 1;
                }
            }
            KeyCode::Char(' ') => {
                if !entries.is_empty() {
                    let entry = &mut entries[*selected];
                    if !entry.blocked {
                        entry.included = !entry.included;
                        // Recalculate blocked state for all entries
                        recalculate_blocked(entries);
                    }
                }
                *error = None;
            }
            KeyCode::Enter => {
                // Collect included changes and check for cycles
                let included: Vec<String> = entries
                    .iter()
                    .filter(|e| e.included)
                    .map(|e| e.change_name.clone())
                    .collect();

                if included.is_empty() {
                    *error = Some("No changes selected.".to_string());
                    return;
                }

                // Build deps map for included changes only
                let cwd = std::env::current_dir().unwrap_or_default();
                let changes_dir = cwd.join("openspec").join("changes");
                let mut deps_map: HashMap<String, Vec<String>> = HashMap::new();
                let included_set: std::collections::HashSet<&str> =
                    included.iter().map(|s| s.as_str()).collect();
                for name in &included {
                    let change_dir = changes_dir.join(name);
                    let deps = data::read_dependencies(&change_dir).unwrap_or_default();
                    // Only include deps that are in the included set
                    let filtered: Vec<String> = deps
                        .into_iter()
                        .filter(|d| included_set.contains(d.as_str()))
                        .collect();
                    deps_map.insert(name.clone(), filtered);
                }

                match data::topological_sort(&deps_map) {
                    Err(cycle_err) => {
                        *error = Some(cycle_err);
                    }
                    Ok(sorted) => {
                        if let Some(first) = sorted.first() {
                            let batch = BatchImplState::new(sorted.clone(), deps_map);
                            self.batch = Some(batch);
                            let change_dir = changes_dir.join(first);
                            let run_mode = data::read_run_mode(&change_dir).unwrap_or_default();
                            self.implementation = Some(match run_mode {
                                RunMode::Normal => {
                                    runner::start_implementation(first, &self.config)
                                }
                                RunMode::Apply => runner::start_apply(first, &self.config),
                            });
                        }
                        if let Some(prev) = self.screen_stack.pop() {
                            self.screen = prev;
                        }
                    }
                }
            }
            KeyCode::Esc => {
                if let Some(prev) = self.screen_stack.pop() {
                    self.screen = prev;
                }
            }
            _ => {}
        }
    }
}

/// Build the list of entries for the RunAllSelection screen.
///
/// Filters changes to those with a `tasks.md`, reads dependencies,
/// and determines which entries are blocked due to excluded dependencies.
pub fn build_run_all_entries(changes: &[data::ChangeEntry]) -> Vec<RunAllEntry> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let changes_dir = cwd.join("openspec").join("changes");
    let archive_dir = cwd.join("openspec").join("changes").join("archive");
    let archived = data::resolve_archived_dependencies(&archive_dir);

    let mut entries: Vec<RunAllEntry> = changes
        .iter()
        .filter(|c| {
            let dir = changes_dir.join(&c.name);
            data::has_tasks_file(&dir)
        })
        .map(|c| RunAllEntry {
            change_name: c.name.clone(),
            included: true,
            blocked: false,
            blocked_by: None,
            completed_tasks: c.completed_tasks,
            total_tasks: c.total_tasks,
        })
        .collect();

    // Determine blocked state based on dependencies
    // A change is blocked if any of its dependencies is not included
    // and not fulfilled (completed or archived)
    let change_names: std::collections::HashSet<String> =
        entries.iter().map(|e| e.change_name.clone()).collect();

    for entry in &mut entries {
        let dir = changes_dir.join(&entry.change_name);
        let deps = data::read_dependencies(&dir).unwrap_or_default();
        for dep in &deps {
            let in_list = change_names.contains(dep);
            let is_archived = archived.contains(dep);
            if !in_list && !is_archived {
                entry.blocked = true;
                entry.blocked_by = Some(dep.clone());
                entry.included = false;
                break;
            }
        }
    }

    entries
}

/// Recalculate blocked state after a toggle.
///
/// A change becomes blocked if any of its dependencies is excluded
/// (not included) in the current selection.
fn recalculate_blocked(entries: &mut [RunAllEntry]) {
    let cwd = std::env::current_dir().unwrap_or_default();
    let changes_dir = cwd.join("openspec").join("changes");
    let archive_dir = cwd.join("openspec").join("changes").join("archive");
    let archived = data::resolve_archived_dependencies(&archive_dir);

    // Build sets from current state before mutating
    let included: std::collections::HashSet<String> = entries
        .iter()
        .filter(|e| e.included)
        .map(|e| e.change_name.clone())
        .collect();

    let all_entry_names: std::collections::HashSet<String> =
        entries.iter().map(|e| e.change_name.clone()).collect();

    // Compute new blocked state per entry
    let updates: Vec<(bool, Option<String>)> = entries
        .iter()
        .map(|entry| {
            let dir = changes_dir.join(&entry.change_name);
            let deps = data::read_dependencies(&dir).unwrap_or_default();
            for dep in &deps {
                let is_included = included.contains(dep);
                let is_archived = archived.contains(dep);
                if !is_included && !is_archived && all_entry_names.contains(dep) {
                    return (true, Some(dep.clone()));
                }
            }
            (false, None)
        })
        .collect();

    // Apply updates
    for (i, (blocked, blocked_by)) in updates.into_iter().enumerate() {
        if blocked && entries[i].included {
            entries[i].included = false;
        }
        entries[i].blocked = blocked;
        entries[i].blocked_by = blocked_by;
    }
}

pub fn build_artifact_menu_items(
    status: &ChangeStatusOutput,
    change_dir: &Path,
    is_archived: bool,
) -> Vec<ArtifactMenuItem> {
    let mut items = Vec::new();

    let artifact_defs = [
        ("proposal", "Proposal", "proposal.md"),
        ("design", "Design", "design.md"),
        ("tasks", "Tasks", "tasks.md"),
    ];

    for (id, label, filename) in &artifact_defs {
        let artifact = status.artifacts.iter().find(|a| a.id == *id);
        let available = artifact.is_some_and(|a| a.status == "done");
        let file_path = if available {
            Some(change_dir.join(filename))
        } else {
            None
        };

        items.push(ArtifactMenuItem {
            label: label.to_string(),
            available,
            file_path,
            is_spec_header: false,
            is_dependency_item: false,
        });
    }

    // Specs header + sub-items
    let specs_artifact = status.artifacts.iter().find(|a| a.id == "specs");
    let specs_available = specs_artifact.is_some_and(|a| a.status == "done");
    let spec_items = if specs_available {
        data::discover_specs(change_dir)
    } else {
        Vec::new()
    };

    items.push(ArtifactMenuItem {
        label: "Specs".to_string(),
        available: specs_available,
        file_path: None,
        is_spec_header: true,
        is_dependency_item: false,
    });

    for spec in &spec_items {
        items.push(ArtifactMenuItem {
            label: format!("  {}", spec.name),
            available: true,
            file_path: Some(spec.path.clone()),
            is_spec_header: false,
            is_dependency_item: false,
        });
    }

    // Add implementation.log entry if the file exists
    let log_path = change_dir.join("implementation.log");
    if log_path.exists() {
        items.push(ArtifactMenuItem {
            label: "Implementation Log".to_string(),
            available: true,
            file_path: Some(log_path),
            is_spec_header: false,
            is_dependency_item: false,
        });
    }

    // Add Dependencies item for active changes
    if !is_archived {
        let dep_count = data::read_dependencies(change_dir)
            .unwrap_or_default()
            .len();
        items.push(ArtifactMenuItem {
            label: format!("Dependencies [{}]", dep_count),
            available: true,
            file_path: None,
            is_spec_header: false,
            is_dependency_item: true,
        });
    }

    items
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_status(artifacts: Vec<(&str, &str)>) -> ChangeStatusOutput {
        ChangeStatusOutput {
            artifacts: artifacts
                .into_iter()
                .map(|(id, status)| ArtifactStatus {
                    id: id.to_string(),
                    status: status.to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn test_screen_transition_change_list_to_artifact_menu() {
        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![ChangeEntry {
                    name: "test".to_string(),
                    completed_tasks: 0,
                    total_tasks: 5,
                }],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        // Pressing Esc on ChangeList shouldn't crash (no parent screen)
        app.handle_change_list_input(KeyCode::Esc);
        assert!(matches!(app.screen, Screen::ChangeList { .. }));
    }

    #[test]
    fn test_screen_transition_esc_from_artifact_menu() {
        let original_screen = Screen::ChangeList {
            changes: vec![],
            selected: 0,
            error: None,
            tab: ChangeTab::Active,
            change_deps: HashMap::new(),
        };

        let mut app = App {
            screen: Screen::ArtifactMenu {
                change_name: "test".to_string(),
                change_dir: PathBuf::from("/tmp"),
                items: vec![],
                selected: 0,
                is_archived: false,
            },
            screen_stack: vec![original_screen],
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_artifact_menu_input(KeyCode::Esc);
        assert!(matches!(app.screen, Screen::ChangeList { .. }));
        assert!(app.screen_stack.is_empty());
    }

    #[test]
    fn test_screen_transition_esc_from_artifact_view() {
        let menu_screen = Screen::ArtifactMenu {
            change_name: "test".to_string(),
            change_dir: PathBuf::from("/tmp"),
            items: vec![],
            selected: 0,
            is_archived: false,
        };

        let mut app = App {
            screen: Screen::ArtifactView {
                title: "Proposal".to_string(),
                content: "hello\nworld".to_string(),
                scroll: 0,
                is_plain_text: false,
                file_path: None,
            },
            screen_stack: vec![menu_screen],
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_artifact_view_input(KeyCode::Esc);
        assert!(matches!(app.screen, Screen::ArtifactMenu { .. }));
    }

    #[test]
    fn test_change_list_navigation() {
        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![
                    ChangeEntry {
                        name: "a".to_string(),
                        completed_tasks: 0,
                        total_tasks: 1,
                    },
                    ChangeEntry {
                        name: "b".to_string(),
                        completed_tasks: 0,
                        total_tasks: 1,
                    },
                    ChangeEntry {
                        name: "c".to_string(),
                        completed_tasks: 0,
                        total_tasks: 1,
                    },
                ],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        // Move down
        app.handle_change_list_input(KeyCode::Char('j'));
        if let Screen::ChangeList { selected, .. } = &app.screen {
            assert_eq!(*selected, 1);
        }

        // Move down again
        app.handle_change_list_input(KeyCode::Down);
        if let Screen::ChangeList { selected, .. } = &app.screen {
            assert_eq!(*selected, 2);
        }

        // At bottom, stays
        app.handle_change_list_input(KeyCode::Down);
        if let Screen::ChangeList { selected, .. } = &app.screen {
            assert_eq!(*selected, 2);
        }

        // Move up
        app.handle_change_list_input(KeyCode::Char('k'));
        if let Screen::ChangeList { selected, .. } = &app.screen {
            assert_eq!(*selected, 1);
        }

        app.handle_change_list_input(KeyCode::Up);
        if let Screen::ChangeList { selected, .. } = &app.screen {
            assert_eq!(*selected, 0);
        }

        // At top, stays
        app.handle_change_list_input(KeyCode::Up);
        if let Screen::ChangeList { selected, .. } = &app.screen {
            assert_eq!(*selected, 0);
        }
    }

    #[test]
    fn test_artifact_view_scrolling() {
        let mut app = App {
            screen: Screen::ArtifactView {
                title: "Test".to_string(),
                content: "line1\nline2\nline3\nline4\nline5".to_string(),
                scroll: 0,
                is_plain_text: false,
                file_path: None,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        // Scroll down
        app.handle_artifact_view_input(KeyCode::Char('j'));
        if let Screen::ArtifactView { scroll, .. } = &app.screen {
            assert_eq!(*scroll, 1);
        }

        app.handle_artifact_view_input(KeyCode::Down);
        if let Screen::ArtifactView { scroll, .. } = &app.screen {
            assert_eq!(*scroll, 2);
        }

        // Scroll up
        app.handle_artifact_view_input(KeyCode::Char('k'));
        if let Screen::ArtifactView { scroll, .. } = &app.screen {
            assert_eq!(*scroll, 1);
        }

        app.handle_artifact_view_input(KeyCode::Up);
        if let Screen::ArtifactView { scroll, .. } = &app.screen {
            assert_eq!(*scroll, 0);
        }

        // At top, stays
        app.handle_artifact_view_input(KeyCode::Up);
        if let Screen::ArtifactView { scroll, .. } = &app.screen {
            assert_eq!(*scroll, 0);
        }
    }

    #[test]
    fn test_artifact_menu_navigation() {
        let items = vec![
            ArtifactMenuItem {
                label: "Proposal".to_string(),
                available: true,
                file_path: Some(PathBuf::from("/tmp/proposal.md")),
                is_spec_header: false,
                is_dependency_item: false,
            },
            ArtifactMenuItem {
                label: "Design".to_string(),
                available: false,
                file_path: None,
                is_spec_header: false,
                is_dependency_item: false,
            },
            ArtifactMenuItem {
                label: "Tasks".to_string(),
                available: true,
                file_path: Some(PathBuf::from("/tmp/tasks.md")),
                is_spec_header: false,
                is_dependency_item: false,
            },
        ];

        let mut app = App {
            screen: Screen::ArtifactMenu {
                change_name: "test".to_string(),
                change_dir: PathBuf::from("/tmp"),
                items,
                selected: 0,
                is_archived: false,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_artifact_menu_input(KeyCode::Down);
        if let Screen::ArtifactMenu { selected, .. } = &app.screen {
            assert_eq!(*selected, 1);
        }

        // Enter on unavailable item -> no transition
        app.handle_artifact_menu_input(KeyCode::Enter);
        assert!(matches!(app.screen, Screen::ArtifactMenu { .. }));
    }

    #[test]
    fn test_build_artifact_menu_items_all_done() {
        let status = make_status(vec![
            ("proposal", "done"),
            ("design", "done"),
            ("tasks", "done"),
            ("specs", "done"),
        ]);
        let change_dir = PathBuf::from("/tmp/nonexistent");
        let items = build_artifact_menu_items(&status, &change_dir, false);

        assert_eq!(items[0].label, "Proposal");
        assert!(items[0].available);
        assert_eq!(items[1].label, "Design");
        assert!(items[1].available);
        assert_eq!(items[2].label, "Tasks");
        assert!(items[2].available);
        assert_eq!(items[3].label, "Specs");
        assert!(items[3].available);
        assert!(items[3].is_spec_header);
    }

    #[test]
    fn test_build_artifact_menu_items_some_pending() {
        let status = make_status(vec![
            ("proposal", "done"),
            ("design", "pending"),
            ("tasks", "done"),
            ("specs", "pending"),
        ]);
        let change_dir = PathBuf::from("/tmp/nonexistent");
        let items = build_artifact_menu_items(&status, &change_dir, false);

        assert!(items[0].available); // proposal done
        assert!(!items[1].available); // design pending
        assert!(items[2].available); // tasks done
        assert!(!items[3].available); // specs pending
    }

    #[test]
    fn test_enter_on_unavailable_artifact_is_noop() {
        let mut app = App {
            screen: Screen::ArtifactMenu {
                change_name: "test".to_string(),
                change_dir: PathBuf::from("/tmp"),
                items: vec![ArtifactMenuItem {
                    label: "Design".to_string(),
                    available: false,
                    file_path: None,
                    is_spec_header: false,
                    is_dependency_item: false,
                }],
                selected: 0,
                is_archived: false,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_artifact_menu_input(KeyCode::Enter);
        // Should still be on ArtifactMenu, not transitioned
        assert!(matches!(app.screen, Screen::ArtifactMenu { .. }));
    }

    #[test]
    fn test_enter_on_spec_header_is_noop() {
        let mut app = App {
            screen: Screen::ArtifactMenu {
                change_name: "test".to_string(),
                change_dir: PathBuf::from("/tmp"),
                items: vec![ArtifactMenuItem {
                    label: "Specs".to_string(),
                    available: true,
                    file_path: None,
                    is_spec_header: true,
                    is_dependency_item: false,
                }],
                selected: 0,
                is_archived: false,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_artifact_menu_input(KeyCode::Enter);
        assert!(matches!(app.screen, Screen::ArtifactMenu { .. }));
    }

    #[test]
    fn test_r_key_starts_implementation() {
        let mut app = App {
            screen: Screen::ArtifactMenu {
                change_name: "test-change".to_string(),
                change_dir: PathBuf::from("/tmp"),
                items: vec![],
                selected: 0,
                is_archived: false,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        assert!(app.implementation.is_none());
        app.handle_artifact_menu_input(KeyCode::Char('R'));
        assert!(app.implementation.is_some());
        assert_eq!(
            app.implementation.as_ref().unwrap().change_name,
            "test-change"
        );
    }

    #[test]
    fn test_r_key_ignored_when_implementation_running() {
        use std::sync::atomic::AtomicBool;
        use std::sync::{Arc, Mutex, mpsc};

        let (_tx, rx) = mpsc::channel();
        let existing_impl = crate::runner::ImplState {
            change_name: "existing-change".to_string(),
            completed: 1,
            total: 5,
            log_path: PathBuf::from("/tmp/existing.log"),
            receiver: rx,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            child_handle: Arc::new(Mutex::new(None)),
        };

        let mut app = App {
            screen: Screen::ArtifactMenu {
                change_name: "new-change".to_string(),
                change_dir: PathBuf::from("/tmp"),
                items: vec![],
                selected: 0,
                is_archived: false,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: Some(existing_impl),
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_artifact_menu_input(KeyCode::Char('R'));
        // Should still be the existing implementation, not replaced
        assert_eq!(
            app.implementation.as_ref().unwrap().change_name,
            "existing-change"
        );
    }

    #[test]
    fn test_s_key_stops_implementation() {
        use std::sync::atomic::AtomicBool;
        use std::sync::{Arc, Mutex, mpsc};

        let (_tx, rx) = mpsc::channel();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_flag_clone = cancel_flag.clone();
        let existing_impl = crate::runner::ImplState {
            change_name: "test-change".to_string(),
            completed: 1,
            total: 5,
            log_path: PathBuf::from("/tmp/test.log"),
            receiver: rx,
            cancel_flag,
            child_handle: Arc::new(Mutex::new(None)),
        };

        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: Some(existing_impl),
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        assert!(app.implementation.is_some());
        app.stop_running_implementation();
        assert!(app.implementation.is_none());
        assert!(cancel_flag_clone.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn test_s_key_noop_when_no_implementation() {
        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        // Should not panic when no implementation is running
        app.stop_running_implementation();
        assert!(app.implementation.is_none());
    }

    #[test]
    fn test_s_key_works_from_artifact_view() {
        use std::sync::atomic::AtomicBool;
        use std::sync::{Arc, Mutex, mpsc};

        let (_tx, rx) = mpsc::channel();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_flag_clone = cancel_flag.clone();
        let existing_impl = crate::runner::ImplState {
            change_name: "test-change".to_string(),
            completed: 2,
            total: 5,
            log_path: PathBuf::from("/tmp/test.log"),
            receiver: rx,
            cancel_flag,
            child_handle: Arc::new(Mutex::new(None)),
        };

        let mut app = App {
            screen: Screen::ArtifactView {
                title: "Test".to_string(),
                content: "content".to_string(),
                scroll: 0,
                is_plain_text: false,
                file_path: None,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: Some(existing_impl),
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.stop_running_implementation();
        assert!(app.implementation.is_none());
        assert!(cancel_flag_clone.load(std::sync::atomic::Ordering::Relaxed));
        // Screen should remain unchanged
        assert!(matches!(app.screen, Screen::ArtifactView { .. }));
    }

    #[test]
    fn test_s_key_works_from_artifact_menu() {
        use std::sync::atomic::AtomicBool;
        use std::sync::{Arc, Mutex, mpsc};

        let (_tx, rx) = mpsc::channel();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_flag_clone = cancel_flag.clone();
        let existing_impl = crate::runner::ImplState {
            change_name: "test-change".to_string(),
            completed: 3,
            total: 5,
            log_path: PathBuf::from("/tmp/test.log"),
            receiver: rx,
            cancel_flag,
            child_handle: Arc::new(Mutex::new(None)),
        };

        let mut app = App {
            screen: Screen::ArtifactMenu {
                change_name: "test-change".to_string(),
                change_dir: PathBuf::from("/tmp"),
                items: vec![],
                selected: 0,
                is_archived: false,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: Some(existing_impl),
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.stop_running_implementation();
        assert!(app.implementation.is_none());
        assert!(cancel_flag_clone.load(std::sync::atomic::Ordering::Relaxed));
        // Screen should remain unchanged
        assert!(matches!(app.screen, Screen::ArtifactMenu { .. }));
    }

    #[test]
    fn test_poll_implementation_updates_progress() {
        use std::sync::atomic::AtomicBool;
        use std::sync::{Arc, Mutex, mpsc};

        let (tx, rx) = mpsc::channel();
        let impl_state = crate::runner::ImplState {
            change_name: "test-change".to_string(),
            completed: 0,
            total: 5,
            log_path: PathBuf::from("/tmp/test.log"),
            receiver: rx,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            child_handle: Arc::new(Mutex::new(None)),
        };

        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: Some(impl_state),
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        // Send progress updates
        tx.send(crate::runner::ImplUpdate::Progress {
            completed: 2,
            total: 5,
        })
        .unwrap();
        tx.send(crate::runner::ImplUpdate::Progress {
            completed: 3,
            total: 5,
        })
        .unwrap();

        app.poll_implementation();

        // Should have consumed all messages and applied the latest progress
        let state = app.implementation.as_ref().unwrap();
        assert_eq!(state.completed, 3);
        assert_eq!(state.total, 5);
    }

    #[test]
    fn test_poll_implementation_clears_on_finished() {
        use std::sync::atomic::AtomicBool;
        use std::sync::{Arc, Mutex, mpsc};

        let (tx, rx) = mpsc::channel();
        let impl_state = crate::runner::ImplState {
            change_name: "test-change".to_string(),
            completed: 3,
            total: 5,
            log_path: PathBuf::from("/tmp/test.log"),
            receiver: rx,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            child_handle: Arc::new(Mutex::new(None)),
        };

        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: Some(impl_state),
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        tx.send(crate::runner::ImplUpdate::Finished { success: true })
            .unwrap();

        app.poll_implementation();

        assert!(app.implementation.is_none());
    }

    #[test]
    fn test_poll_implementation_progress_then_finished() {
        use std::sync::atomic::AtomicBool;
        use std::sync::{Arc, Mutex, mpsc};

        let (tx, rx) = mpsc::channel();
        let impl_state = crate::runner::ImplState {
            change_name: "test-change".to_string(),
            completed: 0,
            total: 5,
            log_path: PathBuf::from("/tmp/test.log"),
            receiver: rx,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            child_handle: Arc::new(Mutex::new(None)),
        };

        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: Some(impl_state),
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        // Send progress then finished
        tx.send(crate::runner::ImplUpdate::Progress {
            completed: 5,
            total: 5,
        })
        .unwrap();
        tx.send(crate::runner::ImplUpdate::Finished { success: true })
            .unwrap();

        app.poll_implementation();

        // Finished should clear the implementation
        assert!(app.implementation.is_none());
    }

    #[test]
    fn test_poll_implementation_noop_when_none() {
        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        // Should not panic
        app.poll_implementation();
        assert!(app.implementation.is_none());
    }

    #[test]
    fn test_poll_implementation_no_messages() {
        use std::sync::atomic::AtomicBool;
        use std::sync::{Arc, Mutex, mpsc};

        let (_tx, rx) = mpsc::channel();
        let impl_state = crate::runner::ImplState {
            change_name: "test-change".to_string(),
            completed: 2,
            total: 5,
            log_path: PathBuf::from("/tmp/test.log"),
            receiver: rx,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            child_handle: Arc::new(Mutex::new(None)),
        };

        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: Some(impl_state),
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.poll_implementation();

        // Should still be running with unchanged progress
        let state = app.implementation.as_ref().unwrap();
        assert_eq!(state.completed, 2);
        assert_eq!(state.total, 5);
    }

    #[test]
    fn test_build_artifact_menu_items_with_implementation_log() {
        let dir = std::env::temp_dir().join("openspec-tui-test-menu-with-log");
        std::fs::create_dir_all(&dir).unwrap();
        // Create the implementation.log file
        std::fs::write(dir.join("implementation.log"), "log content").unwrap();

        let status = make_status(vec![
            ("proposal", "done"),
            ("design", "done"),
            ("tasks", "done"),
            ("specs", "pending"),
        ]);
        let items = build_artifact_menu_items(&status, &dir, false);

        // Should have: Proposal, Design, Tasks, Specs header, Implementation Log, Dependencies [0]
        let log_item = items
            .iter()
            .find(|i| i.label == "Implementation Log")
            .unwrap();
        assert!(log_item.available);
        assert_eq!(log_item.file_path, Some(dir.join("implementation.log")));
        assert!(!log_item.is_spec_header);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_build_artifact_menu_items_without_implementation_log() {
        let dir = std::env::temp_dir().join("openspec-tui-test-menu-no-log");
        std::fs::create_dir_all(&dir).unwrap();
        // Do NOT create implementation.log

        let status = make_status(vec![
            ("proposal", "done"),
            ("design", "done"),
            ("tasks", "done"),
            ("specs", "pending"),
        ]);
        let items = build_artifact_menu_items(&status, &dir, false);

        // No item should have the "Implementation Log" label
        assert!(
            !items.iter().any(|i| i.label == "Implementation Log"),
            "Implementation Log should not appear when file does not exist"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_change_list_empty_navigation() {
        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        // Navigation on empty list shouldn't panic
        app.handle_change_list_input(KeyCode::Down);
        app.handle_change_list_input(KeyCode::Up);
        app.handle_change_list_input(KeyCode::Enter);
        assert!(matches!(app.screen, Screen::ChangeList { .. }));
    }

    #[test]
    fn test_tab_switch_active_to_archived() {
        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![ChangeEntry {
                    name: "active-change".to_string(),
                    completed_tasks: 0,
                    total_tasks: 1,
                }],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_change_list_input(KeyCode::Right);
        if let Screen::ChangeList { tab, selected, .. } = &app.screen {
            assert_eq!(*tab, ChangeTab::Archived);
            assert_eq!(*selected, 0);
        } else {
            panic!("Expected ChangeList screen");
        }
    }

    #[test]
    fn test_tab_switch_archived_to_active() {
        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Archived,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_change_list_input(KeyCode::Left);
        if let Screen::ChangeList { tab, .. } = &app.screen {
            assert_eq!(*tab, ChangeTab::Active);
        } else {
            panic!("Expected ChangeList screen");
        }
    }

    #[test]
    fn test_tab_switch_already_on_active_left_noop() {
        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![ChangeEntry {
                    name: "test".to_string(),
                    completed_tasks: 0,
                    total_tasks: 1,
                }],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_change_list_input(KeyCode::Left);
        if let Screen::ChangeList { tab, changes, .. } = &app.screen {
            assert_eq!(*tab, ChangeTab::Active);
            // Changes should not be reloaded (still has the original entry)
            assert_eq!(changes.len(), 1);
            assert_eq!(changes[0].name, "test");
        } else {
            panic!("Expected ChangeList screen");
        }
    }

    #[test]
    fn test_tab_switch_already_on_archived_right_noop() {
        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Archived,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_change_list_input(KeyCode::Right);
        if let Screen::ChangeList { tab, .. } = &app.screen {
            assert_eq!(*tab, ChangeTab::Archived);
        } else {
            panic!("Expected ChangeList screen");
        }
    }

    #[test]
    fn test_tab_switch_with_h_l_keys() {
        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        // Switch to archived with 'l'
        app.handle_change_list_input(KeyCode::Char('l'));
        if let Screen::ChangeList { tab, .. } = &app.screen {
            assert_eq!(*tab, ChangeTab::Archived);
        }

        // Switch back to active with 'h'
        app.handle_change_list_input(KeyCode::Char('h'));
        if let Screen::ChangeList { tab, .. } = &app.screen {
            assert_eq!(*tab, ChangeTab::Active);
        }
    }

    #[test]
    fn test_tab_switch_resets_selection() {
        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![
                    ChangeEntry {
                        name: "a".to_string(),
                        completed_tasks: 0,
                        total_tasks: 1,
                    },
                    ChangeEntry {
                        name: "b".to_string(),
                        completed_tasks: 0,
                        total_tasks: 1,
                    },
                ],
                selected: 1,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_change_list_input(KeyCode::Right);
        if let Screen::ChangeList { selected, .. } = &app.screen {
            assert_eq!(*selected, 0, "Selection should reset to 0 on tab switch");
        }
    }

    #[test]
    fn test_r_key_ignored_on_archived_change() {
        let mut app = App {
            screen: Screen::ArtifactMenu {
                change_name: "archived-change".to_string(),
                change_dir: PathBuf::from("/tmp"),
                items: vec![],
                selected: 0,
                is_archived: true,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_artifact_menu_input(KeyCode::Char('R'));
        assert!(
            app.implementation.is_none(),
            "Implementation runner should not start for archived changes"
        );
    }

    #[test]
    fn test_r_key_works_on_active_change() {
        let mut app = App {
            screen: Screen::ArtifactMenu {
                change_name: "active-change".to_string(),
                change_dir: PathBuf::from("/tmp"),
                items: vec![],
                selected: 0,
                is_archived: false,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_artifact_menu_input(KeyCode::Char('R'));
        assert!(
            app.implementation.is_some(),
            "Implementation runner should start for active changes"
        );
    }

    #[test]
    fn test_find_change_dir_active() {
        let app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        let dir = app.find_change_dir("my-change", false);
        assert!(dir.ends_with("openspec/changes/my-change"));
        assert!(!dir.to_string_lossy().contains("archive"));
    }

    #[test]
    fn test_find_change_dir_archived() {
        let app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Archived,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        let dir = app.find_change_dir("2026-03-06-my-change", true);
        assert!(dir.ends_with("openspec/changes/archive/2026-03-06-my-change"));
    }

    #[test]
    fn test_l_key_opens_log_when_exists() {
        let dir = std::env::temp_dir().join("openspec-tui-test-l-key");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("implementation.log"), "log line 1\nlog line 2").unwrap();

        let mut app = App {
            screen: Screen::ArtifactMenu {
                change_name: "test-change".to_string(),
                change_dir: dir.clone(),
                items: vec![],
                selected: 0,
                is_archived: false,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_artifact_menu_input(KeyCode::Char('L'));

        if let Screen::ArtifactView {
            title,
            content,
            is_plain_text,
            ..
        } = &app.screen
        {
            assert_eq!(title, "Implementation Log");
            assert!(content.contains("log line 1"));
            assert!(*is_plain_text);
        } else {
            panic!("Expected ArtifactView screen after pressing L");
        }
        assert_eq!(app.screen_stack.len(), 1);
        assert!(matches!(app.screen_stack[0], Screen::ArtifactMenu { .. }));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_l_key_noop_when_log_missing() {
        let dir = std::env::temp_dir().join("openspec-tui-test-l-key-noop");
        std::fs::create_dir_all(&dir).unwrap();
        // No implementation.log created

        let mut app = App {
            screen: Screen::ArtifactMenu {
                change_name: "test-change".to_string(),
                change_dir: dir.clone(),
                items: vec![],
                selected: 0,
                is_archived: false,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_artifact_menu_input(KeyCode::Char('L'));

        assert!(
            matches!(app.screen, Screen::ArtifactMenu { .. }),
            "Screen should remain ArtifactMenu when log does not exist"
        );
        assert!(app.screen_stack.is_empty());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_l_key_works_for_archived_changes() {
        let dir = std::env::temp_dir().join("openspec-tui-test-l-key-archived");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("implementation.log"), "archived log").unwrap();

        let mut app = App {
            screen: Screen::ArtifactMenu {
                change_name: "archived-change".to_string(),
                change_dir: dir.clone(),
                items: vec![],
                selected: 0,
                is_archived: true,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_artifact_menu_input(KeyCode::Char('L'));

        if let Screen::ArtifactView {
            content,
            is_plain_text,
            ..
        } = &app.screen
        {
            assert!(content.contains("archived log"));
            assert!(*is_plain_text);
        } else {
            panic!("Expected ArtifactView screen after pressing L on archived change");
        }

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_r_key_navigates_to_log_view() {
        let mut app = App {
            screen: Screen::ArtifactMenu {
                change_name: "active-change".to_string(),
                change_dir: PathBuf::from("/tmp"),
                items: vec![],
                selected: 0,
                is_archived: false,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_artifact_menu_input(KeyCode::Char('R'));

        // Runner should have started
        assert!(app.implementation.is_some());
        // Should navigate to ArtifactView with plain text
        if let Screen::ArtifactView {
            title,
            is_plain_text,
            ..
        } = &app.screen
        {
            assert_eq!(title, "Implementation Log");
            assert!(*is_plain_text);
        } else {
            panic!("Expected ArtifactView after pressing R");
        }
        // Previous screen should be on the stack
        assert_eq!(app.screen_stack.len(), 1);
        assert!(matches!(app.screen_stack[0], Screen::ArtifactMenu { .. }));
    }

    #[test]
    fn test_r_key_esc_returns_to_artifact_menu() {
        let mut app = App {
            screen: Screen::ArtifactMenu {
                change_name: "active-change".to_string(),
                change_dir: PathBuf::from("/tmp"),
                items: vec![],
                selected: 0,
                is_archived: false,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_artifact_menu_input(KeyCode::Char('R'));
        assert!(matches!(app.screen, Screen::ArtifactView { .. }));

        // Press Esc to go back
        app.handle_artifact_view_input(KeyCode::Esc);
        assert!(
            matches!(app.screen, Screen::ArtifactMenu { .. }),
            "Esc should return to ArtifactMenu"
        );
    }

    // --- Config screen tests ---

    fn make_config_app() -> App {
        App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig {
                command: "test-tool {prompt}".to_string(),
                prompt: "test prompt {name}".to_string(),
                ..Default::default()
            },
            config_path: std::env::temp_dir().join("openspec-tui-test-config.yaml"),
        }
    }

    #[test]
    fn test_c_key_pushes_config_screen_from_change_list() {
        let mut app = make_config_app();
        app.handle_change_list_input(KeyCode::Char('C'));
        assert!(matches!(app.screen, Screen::Config { .. }));
        assert_eq!(app.screen_stack.len(), 1);
        assert!(matches!(app.screen_stack[0], Screen::ChangeList { .. }));
    }

    #[test]
    fn test_c_key_pushes_config_screen_from_artifact_menu() {
        let mut app = App {
            screen: Screen::ArtifactMenu {
                change_name: "test".to_string(),
                change_dir: PathBuf::from("/tmp"),
                items: vec![],
                selected: 0,
                is_archived: false,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };
        app.handle_artifact_menu_input(KeyCode::Char('C'));
        assert!(matches!(app.screen, Screen::Config { .. }));
    }

    #[test]
    fn test_c_key_pushes_config_screen_from_artifact_view() {
        let mut app = App {
            screen: Screen::ArtifactView {
                title: "Test".to_string(),
                content: "content".to_string(),
                scroll: 0,
                is_plain_text: false,
                file_path: None,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };
        app.handle_artifact_view_input(KeyCode::Char('C'));
        assert!(matches!(app.screen, Screen::Config { .. }));
    }

    #[test]
    fn test_config_screen_has_cloned_config_values() {
        let mut app = make_config_app();
        app.push_config_screen();
        if let Screen::Config {
            command, prompt, ..
        } = &app.screen
        {
            assert_eq!(command, "test-tool {prompt}");
            assert_eq!(prompt, "test prompt {name}");
        } else {
            panic!("Expected Config screen");
        }
    }

    #[test]
    fn test_config_screen_cursor_starts_at_end() {
        let mut app = make_config_app();
        app.push_config_screen();
        if let Screen::Config {
            cursor_position,
            command,
            ..
        } = &app.screen
        {
            assert_eq!(*cursor_position, command.len());
        } else {
            panic!("Expected Config screen");
        }
    }

    #[test]
    fn test_config_screen_focused_on_command() {
        let mut app = make_config_app();
        app.push_config_screen();
        if let Screen::Config { focused_field, .. } = &app.screen {
            assert_eq!(*focused_field, ConfigField::Command);
        } else {
            panic!("Expected Config screen");
        }
    }

    #[test]
    fn test_config_esc_discards_changes() {
        let mut app = make_config_app();
        app.push_config_screen();

        // Type a character
        app.handle_config_input(KeyCode::Char('X'));

        // Press Esc to discard
        app.handle_config_input(KeyCode::Esc);
        assert!(matches!(app.screen, Screen::ChangeList { .. }));
        // Original config should be unchanged
        assert_eq!(app.config.command, "test-tool {prompt}");
    }

    #[test]
    fn test_config_tab_switches_focus() {
        let mut app = make_config_app();
        app.push_config_screen();

        // Start on Command
        if let Screen::Config { focused_field, .. } = &app.screen {
            assert_eq!(*focused_field, ConfigField::Command);
        }

        // Tab -> Prompt
        app.handle_config_input(KeyCode::Tab);
        if let Screen::Config { focused_field, .. } = &app.screen {
            assert_eq!(*focused_field, ConfigField::Prompt);
        }

        // Tab -> PostImplementationPrompt
        app.handle_config_input(KeyCode::Tab);
        if let Screen::Config { focused_field, .. } = &app.screen {
            assert_eq!(*focused_field, ConfigField::PostImplementationPrompt);
        }

        // Tab -> InteractiveCommand
        app.handle_config_input(KeyCode::Tab);
        if let Screen::Config { focused_field, .. } = &app.screen {
            assert_eq!(*focused_field, ConfigField::InteractiveCommand);
        }

        // Tab -> RunFinishedCommand
        app.handle_config_input(KeyCode::Tab);
        if let Screen::Config { focused_field, .. } = &app.screen {
            assert_eq!(*focused_field, ConfigField::RunFinishedCommand);
        }

        // Tab -> Command (wraps around)
        app.handle_config_input(KeyCode::Tab);
        if let Screen::Config { focused_field, .. } = &app.screen {
            assert_eq!(*focused_field, ConfigField::Command);
        }
    }

    #[test]
    fn test_config_typing_in_command_field() {
        let mut app = make_config_app();
        app.push_config_screen();

        // Enter edit mode, then move cursor to beginning
        app.handle_config_input(KeyCode::Enter);
        app.handle_config_input(KeyCode::Home);
        // Type characters
        app.handle_config_input(KeyCode::Char('A'));
        app.handle_config_input(KeyCode::Char('B'));

        if let Screen::Config {
            command,
            cursor_position,
            ..
        } = &app.screen
        {
            assert_eq!(command, "ABtest-tool {prompt}");
            assert_eq!(*cursor_position, 2);
        } else {
            panic!("Expected Config screen");
        }
    }

    #[test]
    fn test_config_backspace_in_command_field() {
        let mut app = make_config_app();
        app.push_config_screen();

        // Enter edit mode, then backspace deletes last char
        app.handle_config_input(KeyCode::Enter);
        app.handle_config_input(KeyCode::Backspace);
        if let Screen::Config { command, .. } = &app.screen {
            assert_eq!(command, "test-tool {prompt");
        }
    }

    #[test]
    fn test_config_delete_in_command_field() {
        let mut app = make_config_app();
        app.push_config_screen();

        // Enter edit mode, move to start, delete first char
        app.handle_config_input(KeyCode::Enter);
        app.handle_config_input(KeyCode::Home);
        app.handle_config_input(KeyCode::Delete);
        if let Screen::Config { command, .. } = &app.screen {
            assert_eq!(command, "est-tool {prompt}");
        }
    }

    #[test]
    fn test_config_cursor_movement() {
        let mut app = make_config_app();
        app.push_config_screen();

        let cmd_len = if let Screen::Config { command, .. } = &app.screen {
            command.len()
        } else {
            0
        };

        // Enter edit mode first
        app.handle_config_input(KeyCode::Enter);

        // Home -> cursor at 0
        app.handle_config_input(KeyCode::Home);
        if let Screen::Config {
            cursor_position, ..
        } = &app.screen
        {
            assert_eq!(*cursor_position, 0);
        }

        // Right -> cursor at 1
        app.handle_config_input(KeyCode::Right);
        if let Screen::Config {
            cursor_position, ..
        } = &app.screen
        {
            assert_eq!(*cursor_position, 1);
        }

        // End -> cursor at end
        app.handle_config_input(KeyCode::End);
        if let Screen::Config {
            cursor_position, ..
        } = &app.screen
        {
            assert_eq!(*cursor_position, cmd_len);
        }

        // Left -> cursor at end - 1
        app.handle_config_input(KeyCode::Left);
        if let Screen::Config {
            cursor_position, ..
        } = &app.screen
        {
            assert_eq!(*cursor_position, cmd_len - 1);
        }
    }

    #[test]
    fn test_config_save_updates_app_config() {
        let mut app = make_config_app();
        app.push_config_screen();

        // Enter edit mode, modify command
        app.handle_config_input(KeyCode::Enter);
        app.handle_config_input(KeyCode::Home);
        app.handle_config_input(KeyCode::Char('X'));

        // Return to navigation mode
        app.handle_config_input(KeyCode::Esc);

        // Save (S works in navigation mode regardless of focused field)
        app.handle_config_input(KeyCode::Char('S'));
        assert!(matches!(app.screen, Screen::ChangeList { .. }));
        assert_eq!(app.config.command, "Xtest-tool {prompt}");
    }

    #[test]
    fn test_config_reset_to_defaults() {
        let mut app = make_config_app();
        app.push_config_screen();

        // Switch to prompt field
        app.handle_config_input(KeyCode::Tab);

        // Reset to defaults
        app.handle_config_input(KeyCode::Char('D'));
        if let Screen::Config {
            command,
            prompt,
            focused_field,
            ..
        } = &app.screen
        {
            let defaults = TuiConfig::default();
            assert_eq!(command, &defaults.command);
            assert_eq!(prompt, &defaults.prompt);
            assert_eq!(*focused_field, ConfigField::Command);
        } else {
            panic!("Expected Config screen");
        }
    }

    #[test]
    fn test_config_enter_on_prompt_returns_true() {
        let mut app = make_config_app();
        app.push_config_screen();

        // Switch to prompt field
        app.handle_config_input(KeyCode::Tab);

        // Enter on prompt should signal editor
        let result = app.handle_config_input(KeyCode::Enter);
        assert!(
            result,
            "Enter on prompt field should return true for editor"
        );
        // Screen should still be Config
        assert!(matches!(app.screen, Screen::Config { .. }));
    }

    #[test]
    fn test_config_enter_on_command_doesnt_signal_editor() {
        let mut app = make_config_app();
        app.push_config_screen();

        // On command field, Enter should not signal editor
        let result = app.handle_config_input(KeyCode::Enter);
        assert!(!result, "Enter on command field should not signal editor");
    }

    #[test]
    fn test_config_s_in_navigation_mode_saves() {
        let mut app = make_config_app();
        app.push_config_screen();

        // S in navigation mode saves and exits, even with Command focused
        app.handle_config_input(KeyCode::Char('S'));
        assert!(matches!(app.screen, Screen::ChangeList { .. }));
    }

    #[test]
    fn test_config_d_in_navigation_mode_resets() {
        let mut app = make_config_app();
        app.push_config_screen();

        // D in navigation mode resets to defaults, even with Command focused
        app.handle_config_input(KeyCode::Char('D'));
        if let Screen::Config {
            command, prompt, ..
        } = &app.screen
        {
            let defaults = TuiConfig::default();
            assert_eq!(command, &defaults.command);
            assert_eq!(prompt, &defaults.prompt);
        } else {
            panic!("Expected Config screen");
        }
    }

    #[test]
    fn test_set_config_prompt() {
        let mut app = make_config_app();
        app.push_config_screen();

        app.set_config_prompt("new prompt text".to_string());
        if let Screen::Config { prompt, .. } = &app.screen {
            assert_eq!(prompt, "new prompt text");
        } else {
            panic!("Expected Config screen");
        }
    }

    #[test]
    fn test_config_char_keys_ignored_in_navigation_mode() {
        let mut app = make_config_app();
        app.push_config_screen();

        let original = if let Screen::Config { command, .. } = &app.screen {
            command.clone()
        } else {
            panic!("Expected Config screen");
        };

        // Character keys should be ignored in navigation mode
        app.handle_config_input(KeyCode::Char('x'));
        app.handle_config_input(KeyCode::Char('y'));
        app.handle_config_input(KeyCode::Char('z'));

        if let Screen::Config {
            command, editing, ..
        } = &app.screen
        {
            assert_eq!(
                command, &original,
                "Command should not change in navigation mode"
            );
            assert!(!editing, "Should still be in navigation mode");
        } else {
            panic!("Expected Config screen");
        }
    }

    #[test]
    fn test_config_enter_activates_edit_mode() {
        let mut app = make_config_app();
        app.push_config_screen();

        // Initially not editing
        if let Screen::Config { editing, .. } = &app.screen {
            assert!(!editing);
        }

        // Enter activates edit mode on Command field
        app.handle_config_input(KeyCode::Enter);
        if let Screen::Config { editing, .. } = &app.screen {
            assert!(*editing, "Enter should activate edit mode");
        } else {
            panic!("Expected Config screen");
        }
    }

    #[test]
    fn test_config_esc_exits_edit_mode() {
        let mut app = make_config_app();
        app.push_config_screen();

        // Enter edit mode
        app.handle_config_input(KeyCode::Enter);
        // Type something
        app.handle_config_input(KeyCode::Char('X'));

        // Esc should exit edit mode (not the config screen)
        app.handle_config_input(KeyCode::Esc);
        if let Screen::Config {
            editing, command, ..
        } = &app.screen
        {
            assert!(!editing, "Esc should return to navigation mode");
            assert!(command.contains('X'), "Edits should be preserved");
        } else {
            panic!("Expected Config screen, not exit");
        }
    }

    #[test]
    fn test_config_enter_exits_edit_mode() {
        let mut app = make_config_app();
        app.push_config_screen();

        // Enter edit mode
        app.handle_config_input(KeyCode::Enter);
        // Type something
        app.handle_config_input(KeyCode::Char('Z'));

        // Enter again should exit edit mode
        app.handle_config_input(KeyCode::Enter);
        if let Screen::Config {
            editing, command, ..
        } = &app.screen
        {
            assert!(!editing, "Enter should return to navigation mode");
            assert!(command.contains('Z'), "Edits should be preserved");
        } else {
            panic!("Expected Config screen");
        }
    }

    #[test]
    fn test_config_backspace_at_start_noop() {
        let mut app = make_config_app();
        app.push_config_screen();

        // Enter edit mode, move to start
        app.handle_config_input(KeyCode::Enter);
        app.handle_config_input(KeyCode::Home);
        let original = if let Screen::Config { command, .. } = &app.screen {
            command.clone()
        } else {
            String::new()
        };

        app.handle_config_input(KeyCode::Backspace);
        if let Screen::Config {
            command,
            cursor_position,
            ..
        } = &app.screen
        {
            assert_eq!(command, &original);
            assert_eq!(*cursor_position, 0);
        }
    }

    #[test]
    fn test_config_delete_at_end_noop() {
        let mut app = make_config_app();
        app.push_config_screen();

        // Enter edit mode (cursor starts at end)
        app.handle_config_input(KeyCode::Enter);
        let original = if let Screen::Config { command, .. } = &app.screen {
            command.clone()
        } else {
            String::new()
        };

        app.handle_config_input(KeyCode::Delete);
        if let Screen::Config { command, .. } = &app.screen {
            assert_eq!(command, &original);
        }
    }

    #[test]
    fn test_config_reset_includes_post_implementation_prompt() {
        let mut app = make_config_app();
        app.config.post_implementation_prompt = "commit {name}".to_string();
        app.push_config_screen();

        // Verify it's loaded
        if let Screen::Config {
            post_implementation_prompt,
            ..
        } = &app.screen
        {
            assert_eq!(post_implementation_prompt, "commit {name}");
        } else {
            panic!("Expected Config screen");
        }

        // Reset to defaults
        app.handle_config_input(KeyCode::Char('D'));
        if let Screen::Config {
            post_implementation_prompt,
            ..
        } = &app.screen
        {
            assert_eq!(
                post_implementation_prompt, "",
                "Post-impl prompt should be empty after reset"
            );
        } else {
            panic!("Expected Config screen");
        }
    }

    #[test]
    fn test_config_save_includes_post_implementation_prompt() {
        let mut app = make_config_app();
        app.config.post_implementation_prompt = "commit {name}".to_string();
        app.push_config_screen();

        // Save
        app.handle_config_input(KeyCode::Char('S'));
        assert_eq!(app.config.post_implementation_prompt, "commit {name}");
    }

    #[test]
    fn test_config_enter_on_post_prompt_opens_editor() {
        let mut app = make_config_app();
        app.push_config_screen();

        // Navigate to PostImplementationPrompt
        app.handle_config_input(KeyCode::Tab); // -> Prompt
        app.handle_config_input(KeyCode::Tab); // -> PostImplementationPrompt

        if let Screen::Config { focused_field, .. } = &app.screen {
            assert_eq!(*focused_field, ConfigField::PostImplementationPrompt);
        }

        // Enter on PostImplementationPrompt should signal editor
        let result = app.handle_config_input(KeyCode::Enter);
        assert!(
            result,
            "Enter on PostImplementationPrompt field should return true for editor"
        );
    }

    #[test]
    fn test_set_config_post_prompt() {
        let mut app = make_config_app();
        app.push_config_screen();

        app.set_config_post_prompt("commit all changes".to_string());
        if let Screen::Config {
            post_implementation_prompt,
            ..
        } = &app.screen
        {
            assert_eq!(post_implementation_prompt, "commit all changes");
        } else {
            panic!("Expected Config screen");
        }
    }

    #[test]
    fn test_config_tab_cycling_includes_interactive_command() {
        let mut app = make_config_app();
        app.push_config_screen();

        app.handle_config_input(KeyCode::Tab); // -> Prompt
        if let Screen::Config { focused_field, .. } = &app.screen {
            assert_eq!(*focused_field, ConfigField::Prompt);
        }
        app.handle_config_input(KeyCode::Tab); // -> PostImplementationPrompt
        if let Screen::Config { focused_field, .. } = &app.screen {
            assert_eq!(*focused_field, ConfigField::PostImplementationPrompt);
        }
        app.handle_config_input(KeyCode::Tab); // -> InteractiveCommand
        if let Screen::Config { focused_field, .. } = &app.screen {
            assert_eq!(*focused_field, ConfigField::InteractiveCommand);
        }
        app.handle_config_input(KeyCode::Tab); // -> RunFinishedCommand
        if let Screen::Config { focused_field, .. } = &app.screen {
            assert_eq!(*focused_field, ConfigField::RunFinishedCommand);
        }
        app.handle_config_input(KeyCode::Tab); // -> Command (wrap around)
        if let Screen::Config { focused_field, .. } = &app.screen {
            assert_eq!(*focused_field, ConfigField::Command);
        }
    }

    #[test]
    fn test_config_enter_on_interactive_command_activates_edit() {
        let mut app = make_config_app();
        app.push_config_screen();

        // Navigate to InteractiveCommand
        app.handle_config_input(KeyCode::Tab); // -> Prompt
        app.handle_config_input(KeyCode::Tab); // -> PostImplementationPrompt
        app.handle_config_input(KeyCode::Tab); // -> InteractiveCommand

        let result = app.handle_config_input(KeyCode::Enter);
        assert!(
            !result,
            "Enter on InteractiveCommand should not signal editor"
        );
        if let Screen::Config {
            editing,
            focused_field,
            ..
        } = &app.screen
        {
            assert!(*editing, "Should be in edit mode");
            assert_eq!(*focused_field, ConfigField::InteractiveCommand);
        }
    }

    #[test]
    fn test_config_typing_in_interactive_command_field() {
        let mut app = make_config_app();
        app.push_config_screen();

        // Navigate to InteractiveCommand and enter edit mode
        app.handle_config_input(KeyCode::Tab); // -> Prompt
        app.handle_config_input(KeyCode::Tab); // -> PostImplementationPrompt
        app.handle_config_input(KeyCode::Tab); // -> InteractiveCommand
        app.handle_config_input(KeyCode::Enter); // edit mode

        // Type some chars
        app.handle_config_input(KeyCode::Char(' '));
        app.handle_config_input(KeyCode::Char('-'));
        app.handle_config_input(KeyCode::Char('-'));
        app.handle_config_input(KeyCode::Char('v'));

        if let Screen::Config {
            interactive_command,
            ..
        } = &app.screen
        {
            assert_eq!(interactive_command, "claude --v");
        } else {
            panic!("Expected Config screen");
        }
    }

    #[test]
    fn test_config_save_persists_interactive_command() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("config.yaml");

        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: config_path.clone(),
        };

        app.push_config_screen();

        // Navigate to InteractiveCommand and edit
        app.handle_config_input(KeyCode::Tab); // -> Prompt
        app.handle_config_input(KeyCode::Tab); // -> PostImplementationPrompt
        app.handle_config_input(KeyCode::Tab); // -> InteractiveCommand
        app.handle_config_input(KeyCode::Enter); // edit mode

        // Clear and type "aider"
        // First select all and delete current content
        app.handle_config_input(KeyCode::Home);
        for _ in 0..10 {
            app.handle_config_input(KeyCode::Delete);
        }
        app.handle_config_input(KeyCode::Char('a'));
        app.handle_config_input(KeyCode::Char('i'));
        app.handle_config_input(KeyCode::Char('d'));
        app.handle_config_input(KeyCode::Char('e'));
        app.handle_config_input(KeyCode::Char('r'));
        app.handle_config_input(KeyCode::Esc); // exit edit mode

        // Save
        app.handle_config_input(KeyCode::Char('S'));

        assert_eq!(app.config.interactive_command, "aider");

        // Verify persisted
        let loaded = TuiConfig::load_from(&config_path).unwrap();
        assert_eq!(loaded.interactive_command, "aider");
    }

    #[test]
    fn test_config_reset_defaults_restores_interactive_command() {
        let mut app = make_config_app();
        app.push_config_screen();

        // Navigate to InteractiveCommand and edit it
        app.handle_config_input(KeyCode::Tab); // -> Prompt
        app.handle_config_input(KeyCode::Tab); // -> PostImplementationPrompt
        app.handle_config_input(KeyCode::Tab); // -> InteractiveCommand
        app.handle_config_input(KeyCode::Enter); // edit mode
        app.handle_config_input(KeyCode::Home);
        for _ in 0..10 {
            app.handle_config_input(KeyCode::Delete);
        }
        app.handle_config_input(KeyCode::Char('x'));
        app.handle_config_input(KeyCode::Esc); // exit edit mode

        if let Screen::Config {
            interactive_command,
            ..
        } = &app.screen
        {
            assert_eq!(interactive_command, "x");
        }

        // Reset to defaults
        app.handle_config_input(KeyCode::Char('D'));

        if let Screen::Config {
            interactive_command,
            ..
        } = &app.screen
        {
            assert_eq!(interactive_command, "claude");
        } else {
            panic!("Expected Config screen");
        }
    }

    #[test]
    fn test_config_enter_on_run_finished_command_activates_edit() {
        let mut app = make_config_app();
        app.push_config_screen();

        // Navigate to RunFinishedCommand
        app.handle_config_input(KeyCode::Tab); // -> Prompt
        app.handle_config_input(KeyCode::Tab); // -> PostImplementationPrompt
        app.handle_config_input(KeyCode::Tab); // -> InteractiveCommand
        app.handle_config_input(KeyCode::Tab); // -> RunFinishedCommand

        let result = app.handle_config_input(KeyCode::Enter);
        assert!(
            !result,
            "Enter on RunFinishedCommand should not signal editor"
        );
        if let Screen::Config {
            editing,
            focused_field,
            ..
        } = &app.screen
        {
            assert!(*editing, "Should be in edit mode");
            assert_eq!(*focused_field, ConfigField::RunFinishedCommand);
        }
    }

    #[test]
    fn test_config_typing_in_run_finished_command_field() {
        let mut app = make_config_app();
        app.push_config_screen();

        // Navigate to RunFinishedCommand and enter edit mode
        app.handle_config_input(KeyCode::Tab); // -> Prompt
        app.handle_config_input(KeyCode::Tab); // -> PostImplementationPrompt
        app.handle_config_input(KeyCode::Tab); // -> InteractiveCommand
        app.handle_config_input(KeyCode::Tab); // -> RunFinishedCommand
        app.handle_config_input(KeyCode::Enter); // edit mode

        // Type a command
        app.handle_config_input(KeyCode::Char('n'));
        app.handle_config_input(KeyCode::Char('t'));
        app.handle_config_input(KeyCode::Char('f'));
        app.handle_config_input(KeyCode::Char('y'));

        if let Screen::Config {
            run_finished_command,
            ..
        } = &app.screen
        {
            assert_eq!(run_finished_command, "ntfy");
        } else {
            panic!("Expected Config screen");
        }
    }

    #[test]
    fn test_config_save_persists_run_finished_command() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("config.yaml");

        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: config_path.clone(),
        };

        app.push_config_screen();

        // Navigate to RunFinishedCommand and edit
        app.handle_config_input(KeyCode::Tab); // -> Prompt
        app.handle_config_input(KeyCode::Tab); // -> PostImplementationPrompt
        app.handle_config_input(KeyCode::Tab); // -> InteractiveCommand
        app.handle_config_input(KeyCode::Tab); // -> RunFinishedCommand
        app.handle_config_input(KeyCode::Enter); // edit mode

        app.handle_config_input(KeyCode::Char('n'));
        app.handle_config_input(KeyCode::Char('t'));
        app.handle_config_input(KeyCode::Char('f'));
        app.handle_config_input(KeyCode::Char('y'));
        app.handle_config_input(KeyCode::Esc); // exit edit mode

        // Save
        app.handle_config_input(KeyCode::Char('S'));

        assert_eq!(app.config.run_finished_command, "ntfy");

        // Verify persisted
        let loaded = TuiConfig::load_from(&config_path).unwrap();
        assert_eq!(loaded.run_finished_command, "ntfy");
    }

    #[test]
    fn test_config_reset_defaults_restores_run_finished_command() {
        let mut app = make_config_app();
        app.push_config_screen();

        // Navigate to RunFinishedCommand and edit it
        app.handle_config_input(KeyCode::Tab); // -> Prompt
        app.handle_config_input(KeyCode::Tab); // -> PostImplementationPrompt
        app.handle_config_input(KeyCode::Tab); // -> InteractiveCommand
        app.handle_config_input(KeyCode::Tab); // -> RunFinishedCommand
        app.handle_config_input(KeyCode::Enter); // edit mode
        app.handle_config_input(KeyCode::Char('x'));
        app.handle_config_input(KeyCode::Esc); // exit edit mode

        if let Screen::Config {
            run_finished_command,
            ..
        } = &app.screen
        {
            assert_eq!(run_finished_command, "x");
        }

        // Reset to defaults
        app.handle_config_input(KeyCode::Char('D'));

        if let Screen::Config {
            run_finished_command,
            ..
        } = &app.screen
        {
            assert_eq!(run_finished_command, "");
        } else {
            panic!("Expected Config screen");
        }
    }

    #[test]
    fn test_spawn_run_finished_command_does_nothing_when_empty() {
        let app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/test.yaml"),
        };
        // Should not panic or error when command is empty
        app.spawn_run_finished_command();
    }

    #[test]
    fn test_spawn_run_finished_command_runs_when_set() {
        let tmp = tempfile::tempdir().unwrap();
        let marker = tmp.path().join("finished.txt");
        let cmd = format!("touch {}", marker.display());

        let app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig {
                run_finished_command: cmd,
                ..Default::default()
            },
            config_path: PathBuf::from("/tmp/test.yaml"),
        };

        app.spawn_run_finished_command();
        // Give the child process time to execute
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(
            marker.exists(),
            "Run finished command should have created the marker file"
        );
    }

    #[test]
    fn test_poll_implementation_fires_hook_on_single_run_finish() {
        use std::sync::mpsc;
        let tmp = tempfile::tempdir().unwrap();
        let marker = tmp.path().join("hook_fired.txt");
        let cmd = format!("touch {}", marker.display());

        let (tx, rx) = mpsc::channel();
        tx.send(runner::ImplUpdate::Finished { success: true })
            .unwrap();

        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: Some(ImplState {
                change_name: "test".to_string(),
                receiver: rx,
                log_path: PathBuf::from("/tmp/test.log"),
                cancel_flag: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                child_handle: std::sync::Arc::new(std::sync::Mutex::new(None)),
                completed: 0,
                total: 0,
            }),
            batch: None,
            config: TuiConfig {
                run_finished_command: cmd,
                ..Default::default()
            },
            config_path: PathBuf::from("/tmp/test.yaml"),
        };

        app.poll_implementation();
        assert!(app.implementation.is_none());

        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(marker.exists(), "Hook should fire on single run finish");
    }

    #[test]
    fn test_poll_implementation_does_not_fire_hook_mid_batch() {
        use std::sync::mpsc;
        let tmp = tempfile::tempdir().unwrap();
        let marker = tmp.path().join("hook_mid_batch.txt");
        let cmd = format!("touch {}", marker.display());

        let (tx, rx) = mpsc::channel();
        tx.send(runner::ImplUpdate::Finished { success: true })
            .unwrap();

        let batch = BatchImplState::new(
            vec!["change-a".to_string(), "change-b".to_string()],
            HashMap::new(),
        );

        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: Some(ImplState {
                change_name: "change-a".to_string(),
                receiver: rx,
                log_path: PathBuf::from("/tmp/test.log"),
                cancel_flag: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                child_handle: std::sync::Arc::new(std::sync::Mutex::new(None)),
                completed: 0,
                total: 0,
            }),
            batch: Some(batch),
            config: TuiConfig {
                run_finished_command: cmd,
                ..Default::default()
            },
            config_path: PathBuf::from("/tmp/test.yaml"),
        };

        app.poll_implementation();
        // A new implementation should have started (batch is not done yet)
        assert!(
            app.implementation.is_some(),
            "Batch should start next change"
        );

        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(!marker.exists(), "Hook should NOT fire mid-batch");
    }

    #[test]
    fn test_advance_batch_with_explicit_success() {
        let batch = BatchImplState::new(
            vec!["change-a".to_string(), "change-b".to_string()],
            HashMap::new(),
        );
        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: Some(batch),
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        // advance with success=true should mark change-a as completed
        app.advance_batch(true);
        let batch = app.batch.as_ref().unwrap();
        assert!(batch.completed.contains("change-a"));
        assert!(!batch.failed.contains("change-a"));

        // Clean up spawned implementation thread
        app.stop_running_implementation();
    }

    #[test]
    fn test_advance_batch_with_explicit_failure() {
        let mut deps = HashMap::new();
        deps.insert("change-b".to_string(), vec!["change-a".to_string()]);
        let batch = BatchImplState::new(vec!["change-a".to_string(), "change-b".to_string()], deps);
        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: Some(batch),
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        // advance with success=false should mark change-a as failed and skip change-b
        app.advance_batch(false);
        assert!(
            app.batch.is_none(),
            "Batch should be finished since change-b is skipped"
        );
    }

    // --- Dependency View Tests ---

    fn make_dependency_view_app(deps: Vec<&str>) -> App {
        App {
            screen: Screen::DependencyView {
                change_name: "test-change".to_string(),
                change_dir: PathBuf::from("/tmp/nonexistent"),
                dependencies: deps.into_iter().map(String::from).collect(),
                selected: 0,
                run_mode: RunMode::Normal,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        }
    }

    #[test]
    fn test_dependency_view_navigation() {
        let mut app = make_dependency_view_app(vec!["dep-a", "dep-b", "dep-c"]);

        app.handle_dependency_view_input(KeyCode::Char('j'));
        if let Screen::DependencyView { selected, .. } = &app.screen {
            assert_eq!(*selected, 1);
        }

        app.handle_dependency_view_input(KeyCode::Down);
        if let Screen::DependencyView { selected, .. } = &app.screen {
            assert_eq!(*selected, 2);
        }

        // At bottom, stays
        app.handle_dependency_view_input(KeyCode::Down);
        if let Screen::DependencyView { selected, .. } = &app.screen {
            assert_eq!(*selected, 2);
        }

        app.handle_dependency_view_input(KeyCode::Char('k'));
        if let Screen::DependencyView { selected, .. } = &app.screen {
            assert_eq!(*selected, 1);
        }

        app.handle_dependency_view_input(KeyCode::Up);
        if let Screen::DependencyView { selected, .. } = &app.screen {
            assert_eq!(*selected, 0);
        }

        // At top, stays
        app.handle_dependency_view_input(KeyCode::Up);
        if let Screen::DependencyView { selected, .. } = &app.screen {
            assert_eq!(*selected, 0);
        }
    }

    #[test]
    fn test_dependency_view_remove() {
        let dir = std::env::temp_dir().join("openspec-tui-test-dep-remove");
        std::fs::create_dir_all(&dir).unwrap();

        let mut app = App {
            screen: Screen::DependencyView {
                change_name: "test-change".to_string(),
                change_dir: dir.clone(),
                dependencies: vec![
                    "dep-a".to_string(),
                    "dep-b".to_string(),
                    "dep-c".to_string(),
                ],
                selected: 1,
                run_mode: RunMode::Normal,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        // Remove dep-b (selected=1)
        app.handle_dependency_view_input(KeyCode::Char('D'));
        if let Screen::DependencyView {
            dependencies,
            selected,
            ..
        } = &app.screen
        {
            assert_eq!(
                dependencies,
                &vec!["dep-a".to_string(), "dep-c".to_string()]
            );
            assert_eq!(*selected, 1); // stays at 1, now pointing to dep-c
        }

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_dependency_view_remove_last_adjusts_selection() {
        let dir = std::env::temp_dir().join("openspec-tui-test-dep-remove-last");
        std::fs::create_dir_all(&dir).unwrap();

        let mut app = App {
            screen: Screen::DependencyView {
                change_name: "test-change".to_string(),
                change_dir: dir.clone(),
                dependencies: vec!["dep-a".to_string(), "dep-b".to_string()],
                selected: 1,
                run_mode: RunMode::Normal,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        // Remove dep-b (last item, selected=1)
        app.handle_dependency_view_input(KeyCode::Char('D'));
        if let Screen::DependencyView {
            dependencies,
            selected,
            ..
        } = &app.screen
        {
            assert_eq!(dependencies, &vec!["dep-a".to_string()]);
            assert_eq!(*selected, 0); // adjusted back
        }

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_dependency_view_remove_on_empty_is_noop() {
        let mut app = make_dependency_view_app(vec![]);

        app.handle_dependency_view_input(KeyCode::Char('D'));
        if let Screen::DependencyView { dependencies, .. } = &app.screen {
            assert!(dependencies.is_empty());
        }
    }

    #[test]
    fn test_dependency_view_esc_goes_back() {
        let parent_screen = Screen::ArtifactMenu {
            change_name: "test-change".to_string(),
            change_dir: PathBuf::from("/tmp"),
            items: vec![],
            selected: 0,
            is_archived: false,
        };

        let mut app = App {
            screen: Screen::DependencyView {
                change_name: "test-change".to_string(),
                change_dir: PathBuf::from("/tmp"),
                dependencies: vec!["dep-a".to_string()],
                selected: 0,
                run_mode: RunMode::Normal,
            },
            screen_stack: vec![parent_screen],
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_dependency_view_input(KeyCode::Esc);
        assert!(matches!(app.screen, Screen::ArtifactMenu { .. }));
        assert!(app.screen_stack.is_empty());
    }

    // --- Dependency Add Tests ---

    #[test]
    fn test_dependency_add_navigation() {
        let mut app = App {
            screen: Screen::DependencyAdd {
                change_name: "test-change".to_string(),
                change_dir: PathBuf::from("/tmp"),
                available_changes: vec![
                    "change-a".to_string(),
                    "change-b".to_string(),
                    "change-c".to_string(),
                ],
                selected: 0,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_dependency_add_input(KeyCode::Char('j'));
        if let Screen::DependencyAdd { selected, .. } = &app.screen {
            assert_eq!(*selected, 1);
        }

        app.handle_dependency_add_input(KeyCode::Down);
        if let Screen::DependencyAdd { selected, .. } = &app.screen {
            assert_eq!(*selected, 2);
        }

        // At bottom, stays
        app.handle_dependency_add_input(KeyCode::Down);
        if let Screen::DependencyAdd { selected, .. } = &app.screen {
            assert_eq!(*selected, 2);
        }

        app.handle_dependency_add_input(KeyCode::Up);
        if let Screen::DependencyAdd { selected, .. } = &app.screen {
            assert_eq!(*selected, 1);
        }
    }

    #[test]
    fn test_dependency_add_enter_adds_and_returns() {
        let dir = std::env::temp_dir().join("openspec-tui-test-dep-add-enter");
        std::fs::create_dir_all(&dir).unwrap();

        let dep_view = Screen::DependencyView {
            change_name: "test-change".to_string(),
            change_dir: dir.clone(),
            dependencies: vec!["existing-dep".to_string()],
            selected: 0,
            run_mode: RunMode::Normal,
        };

        let mut app = App {
            screen: Screen::DependencyAdd {
                change_name: "test-change".to_string(),
                change_dir: dir.clone(),
                available_changes: vec!["new-dep".to_string()],
                selected: 0,
            },
            screen_stack: vec![dep_view],
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_dependency_add_input(KeyCode::Enter);

        // Should return to DependencyView with new dep added
        if let Screen::DependencyView { dependencies, .. } = &app.screen {
            assert_eq!(
                dependencies,
                &vec!["existing-dep".to_string(), "new-dep".to_string()]
            );
        } else {
            panic!("Expected DependencyView screen");
        }
        assert!(app.screen_stack.is_empty());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_dependency_add_esc_cancels() {
        let dep_view = Screen::DependencyView {
            change_name: "test-change".to_string(),
            change_dir: PathBuf::from("/tmp"),
            dependencies: vec!["existing-dep".to_string()],
            selected: 0,
            run_mode: RunMode::Normal,
        };

        let mut app = App {
            screen: Screen::DependencyAdd {
                change_name: "test-change".to_string(),
                change_dir: PathBuf::from("/tmp"),
                available_changes: vec!["new-dep".to_string()],
                selected: 0,
            },
            screen_stack: vec![dep_view],
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_dependency_add_input(KeyCode::Esc);

        // Should return to DependencyView without adding
        if let Screen::DependencyView { dependencies, .. } = &app.screen {
            assert_eq!(dependencies, &vec!["existing-dep".to_string()]);
        } else {
            panic!("Expected DependencyView screen");
        }
    }

    #[test]
    fn test_dependencies_item_appears_for_active_change() {
        let dir = std::env::temp_dir().join("openspec-tui-test-dep-item-active");
        std::fs::create_dir_all(&dir).unwrap();

        let status = make_status(vec![
            ("proposal", "done"),
            ("design", "done"),
            ("tasks", "done"),
            ("specs", "pending"),
        ]);
        let items = build_artifact_menu_items(&status, &dir, false);

        let dep_item = items.iter().find(|i| i.is_dependency_item);
        assert!(
            dep_item.is_some(),
            "Dependencies item should appear for active changes"
        );
        assert_eq!(dep_item.unwrap().label, "Dependencies [0]");
        assert!(dep_item.unwrap().available);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_dependencies_item_shows_count() {
        let dir = std::env::temp_dir().join("openspec-tui-test-dep-item-count");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("change-config.yaml"),
            "depends_on:\n  - change-a\n  - change-b\n",
        )
        .unwrap();

        let status = make_status(vec![
            ("proposal", "done"),
            ("design", "done"),
            ("tasks", "done"),
            ("specs", "pending"),
        ]);
        let items = build_artifact_menu_items(&status, &dir, false);

        let dep_item = items.iter().find(|i| i.is_dependency_item).unwrap();
        assert_eq!(dep_item.label, "Dependencies [2]");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_dependencies_item_not_shown_for_archived_change() {
        let dir = std::env::temp_dir().join("openspec-tui-test-dep-item-archived");
        std::fs::create_dir_all(&dir).unwrap();

        let status = make_status(vec![
            ("proposal", "done"),
            ("design", "done"),
            ("tasks", "done"),
            ("specs", "done"),
        ]);
        let items = build_artifact_menu_items(&status, &dir, true);

        let dep_item = items.iter().find(|i| i.is_dependency_item);
        assert!(
            dep_item.is_none(),
            "Dependencies item should not appear for archived changes"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_enter_on_dependencies_item_opens_dependency_view() {
        let dir = std::env::temp_dir().join("openspec-tui-test-dep-item-enter");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("change-config.yaml"), "depends_on:\n  - dep-one\n").unwrap();

        let mut app = App {
            screen: Screen::ArtifactMenu {
                change_name: "my-change".to_string(),
                change_dir: dir.clone(),
                items: vec![ArtifactMenuItem {
                    label: "Dependencies [1]".to_string(),
                    available: true,
                    file_path: None,
                    is_spec_header: false,
                    is_dependency_item: true,
                }],
                selected: 0,
                is_archived: false,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_artifact_menu_input(KeyCode::Enter);

        if let Screen::DependencyView {
            change_name,
            dependencies,
            ..
        } = &app.screen
        {
            assert_eq!(change_name, "my-change");
            assert_eq!(dependencies, &vec!["dep-one".to_string()]);
        } else {
            panic!("Expected DependencyView screen");
        }

        // Previous screen should be on the stack
        assert_eq!(app.screen_stack.len(), 1);
        assert!(matches!(app.screen_stack[0], Screen::ArtifactMenu { .. }));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_dependency_graph_scrolling() {
        let mut app = App {
            screen: Screen::DependencyGraph {
                graph_text: "line1\nline2\nline3\nline4\nline5".to_string(),
                scroll: 0,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        // Scroll down
        app.handle_dependency_graph_input(KeyCode::Char('j'));
        if let Screen::DependencyGraph { scroll, .. } = &app.screen {
            assert_eq!(*scroll, 1);
        }

        app.handle_dependency_graph_input(KeyCode::Down);
        if let Screen::DependencyGraph { scroll, .. } = &app.screen {
            assert_eq!(*scroll, 2);
        }

        // Scroll up
        app.handle_dependency_graph_input(KeyCode::Char('k'));
        if let Screen::DependencyGraph { scroll, .. } = &app.screen {
            assert_eq!(*scroll, 1);
        }

        app.handle_dependency_graph_input(KeyCode::Up);
        if let Screen::DependencyGraph { scroll, .. } = &app.screen {
            assert_eq!(*scroll, 0);
        }

        // At top, stays
        app.handle_dependency_graph_input(KeyCode::Up);
        if let Screen::DependencyGraph { scroll, .. } = &app.screen {
            assert_eq!(*scroll, 0);
        }
    }

    #[test]
    fn test_dependency_graph_esc_returns() {
        let parent = Screen::ChangeList {
            changes: vec![],
            selected: 0,
            error: None,
            tab: ChangeTab::Active,
            change_deps: HashMap::new(),
        };

        let mut app = App {
            screen: Screen::DependencyGraph {
                graph_text: "root".to_string(),
                scroll: 0,
            },
            screen_stack: vec![parent],
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_dependency_graph_input(KeyCode::Esc);
        assert!(matches!(app.screen, Screen::ChangeList { .. }));
        assert!(app.screen_stack.is_empty());
    }

    #[test]
    fn test_change_list_g_opens_dependency_graph() {
        let mut change_deps = HashMap::new();
        change_deps.insert("b".to_string(), vec!["a".to_string()]);

        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![
                    ChangeEntry {
                        name: "a".to_string(),
                        completed_tasks: 0,
                        total_tasks: 1,
                    },
                    ChangeEntry {
                        name: "b".to_string(),
                        completed_tasks: 0,
                        total_tasks: 1,
                    },
                ],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_change_list_input(KeyCode::Char('G'));
        assert!(matches!(app.screen, Screen::DependencyGraph { .. }));
        assert_eq!(app.screen_stack.len(), 1);
    }

    #[test]
    fn test_change_list_g_ignored_on_archived_tab() {
        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Archived,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_change_list_input(KeyCode::Char('G'));
        assert!(matches!(app.screen, Screen::ChangeList { .. }));
        assert!(app.screen_stack.is_empty());
    }

    #[test]
    fn test_change_list_i_signals_interactive_launch_on_active_tab() {
        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![ChangeEntry {
                    name: "test".to_string(),
                    completed_tasks: 0,
                    total_tasks: 5,
                }],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_change_list_input(KeyCode::Char('I'));
        assert!(app.launch_interactive);
    }

    #[test]
    fn test_change_list_i_ignored_on_archived_tab() {
        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Archived,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_change_list_input(KeyCode::Char('I'));
        assert!(!app.launch_interactive);
    }

    #[test]
    fn test_change_list_i_ignored_during_running_implementation() {
        use std::sync::atomic::AtomicBool;
        use std::sync::{Arc, Mutex, mpsc};

        let (_tx, rx) = mpsc::channel();
        let existing_impl = crate::runner::ImplState {
            change_name: "existing-change".to_string(),
            completed: 1,
            total: 5,
            log_path: PathBuf::from("/tmp/existing.log"),
            receiver: rx,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            child_handle: Arc::new(Mutex::new(None)),
        };

        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![ChangeEntry {
                    name: "test".to_string(),
                    completed_tasks: 0,
                    total_tasks: 5,
                }],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: Some(existing_impl),
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_change_list_input(KeyCode::Char('I'));
        assert!(!app.launch_interactive);
    }

    // --- RunAllSelection tests ---

    fn make_run_all_app(entries: Vec<RunAllEntry>) -> App {
        App {
            screen: Screen::RunAllSelection {
                entries,
                selected: 0,
                error: None,
            },
            screen_stack: vec![Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            }],
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        }
    }

    fn make_entry(name: &str, included: bool, blocked: bool) -> RunAllEntry {
        RunAllEntry {
            change_name: name.to_string(),
            included,
            blocked,
            blocked_by: if blocked {
                Some("some-dep".to_string())
            } else {
                None
            },
            completed_tasks: 0,
            total_tasks: 5,
        }
    }

    #[test]
    fn test_run_all_selection_navigation() {
        let entries = vec![
            make_entry("change-a", true, false),
            make_entry("change-b", true, false),
            make_entry("change-c", true, false),
        ];
        let mut app = make_run_all_app(entries);

        // Move down
        app.handle_run_all_selection_input(KeyCode::Char('j'));
        if let Screen::RunAllSelection { selected, .. } = &app.screen {
            assert_eq!(*selected, 1);
        }

        app.handle_run_all_selection_input(KeyCode::Down);
        if let Screen::RunAllSelection { selected, .. } = &app.screen {
            assert_eq!(*selected, 2);
        }

        // At bottom, stays
        app.handle_run_all_selection_input(KeyCode::Down);
        if let Screen::RunAllSelection { selected, .. } = &app.screen {
            assert_eq!(*selected, 2);
        }

        // Move up
        app.handle_run_all_selection_input(KeyCode::Char('k'));
        if let Screen::RunAllSelection { selected, .. } = &app.screen {
            assert_eq!(*selected, 1);
        }

        app.handle_run_all_selection_input(KeyCode::Up);
        if let Screen::RunAllSelection { selected, .. } = &app.screen {
            assert_eq!(*selected, 0);
        }

        // At top, stays
        app.handle_run_all_selection_input(KeyCode::Up);
        if let Screen::RunAllSelection { selected, .. } = &app.screen {
            assert_eq!(*selected, 0);
        }
    }

    #[test]
    fn test_run_all_selection_toggle() {
        let entries = vec![
            make_entry("change-a", true, false),
            make_entry("change-b", true, false),
        ];
        let mut app = make_run_all_app(entries);

        // Toggle off
        app.handle_run_all_selection_input(KeyCode::Char(' '));
        if let Screen::RunAllSelection { entries, .. } = &app.screen {
            assert!(!entries[0].included);
            assert!(entries[1].included);
        }

        // Toggle back on
        app.handle_run_all_selection_input(KeyCode::Char(' '));
        if let Screen::RunAllSelection { entries, .. } = &app.screen {
            assert!(entries[0].included);
        }
    }

    #[test]
    fn test_run_all_selection_toggle_blocked_is_noop() {
        let entries = vec![make_entry("change-a", false, true)];
        let mut app = make_run_all_app(entries);

        app.handle_run_all_selection_input(KeyCode::Char(' '));
        if let Screen::RunAllSelection { entries, .. } = &app.screen {
            assert!(!entries[0].included);
            assert!(entries[0].blocked);
        }
    }

    #[test]
    fn test_run_all_selection_esc_cancels() {
        let entries = vec![make_entry("change-a", true, false)];
        let mut app = make_run_all_app(entries);

        app.handle_run_all_selection_input(KeyCode::Esc);
        assert!(matches!(app.screen, Screen::ChangeList { .. }));
        assert!(app.screen_stack.is_empty());
    }

    #[test]
    fn test_run_all_selection_enter_empty_shows_error() {
        let entries = vec![make_entry("change-a", false, false)];
        let mut app = make_run_all_app(entries);

        app.handle_run_all_selection_input(KeyCode::Enter);
        if let Screen::RunAllSelection { error, .. } = &app.screen {
            assert!(error.is_some());
            assert!(error.as_ref().unwrap().contains("No changes selected"));
        }
    }

    #[test]
    fn test_run_all_selection_a_keybinding_opens_from_change_list() {
        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![ChangeEntry {
                    name: "test".to_string(),
                    completed_tasks: 0,
                    total_tasks: 5,
                }],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_change_list_input(KeyCode::Char('A'));
        assert!(matches!(app.screen, Screen::RunAllSelection { .. }));
        assert_eq!(app.screen_stack.len(), 1);
    }

    #[test]
    fn test_run_all_selection_a_ignored_on_archived_tab() {
        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Archived,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_change_list_input(KeyCode::Char('A'));
        assert!(matches!(app.screen, Screen::ChangeList { .. }));
        assert!(app.screen_stack.is_empty());
    }

    #[test]
    fn test_run_all_selection_a_ignored_when_implementation_running() {
        use std::sync::atomic::AtomicBool;
        use std::sync::{Arc, Mutex, mpsc};

        let (_tx, rx) = mpsc::channel();
        let existing_impl = crate::runner::ImplState {
            change_name: "existing".to_string(),
            completed: 1,
            total: 5,
            log_path: PathBuf::from("/tmp/test.log"),
            receiver: rx,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            child_handle: Arc::new(Mutex::new(None)),
        };

        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![ChangeEntry {
                    name: "test".to_string(),
                    completed_tasks: 0,
                    total_tasks: 5,
                }],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: Some(existing_impl),
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_change_list_input(KeyCode::Char('A'));
        assert!(matches!(app.screen, Screen::ChangeList { .. }));
        assert!(app.screen_stack.is_empty());
    }

    #[test]
    fn test_run_all_entry_fields() {
        let entry = RunAllEntry {
            change_name: "my-change".to_string(),
            included: true,
            blocked: false,
            blocked_by: None,
            completed_tasks: 3,
            total_tasks: 7,
        };
        assert_eq!(entry.change_name, "my-change");
        assert!(entry.included);
        assert!(!entry.blocked);
        assert!(entry.blocked_by.is_none());
        assert_eq!(entry.completed_tasks, 3);
        assert_eq!(entry.total_tasks, 7);
    }

    #[test]
    fn test_run_all_enter_starts_batch_and_implementation() {
        let entries = vec![
            make_entry("change-a", true, false),
            make_entry("change-b", true, false),
        ];
        let mut app = make_run_all_app(entries);

        app.handle_run_all_selection_input(KeyCode::Enter);

        // Should navigate back to ChangeList
        assert!(matches!(app.screen, Screen::ChangeList { .. }));

        // Batch should be set with the two changes
        assert!(app.batch.is_some());
        let batch = app.batch.as_ref().unwrap();
        assert_eq!(batch.queue.len(), 2);
        assert!(batch.queue.contains(&"change-a".to_string()));
        assert!(batch.queue.contains(&"change-b".to_string()));
        assert_eq!(batch.current_index, 0);

        // Implementation should be started for the first change in the queue
        assert!(app.implementation.is_some());
        let impl_state = app.implementation.as_ref().unwrap();
        assert_eq!(impl_state.change_name, batch.queue[0]);

        // Clean up: stop the implementation thread
        app.stop_running_implementation();
    }

    #[test]
    fn test_run_all_enter_no_batch_when_none_included() {
        let entries = vec![make_entry("change-a", false, false)];
        let mut app = make_run_all_app(entries);

        app.handle_run_all_selection_input(KeyCode::Enter);

        // Should show error, not start batch
        assert!(matches!(app.screen, Screen::RunAllSelection { .. }));
        assert!(app.batch.is_none());
        assert!(app.implementation.is_none());
    }

    #[test]
    fn test_poll_implementation_finished_clears_single_batch() {
        use std::sync::atomic::AtomicBool;
        use std::sync::{Arc, Mutex, mpsc};

        let (tx, rx) = mpsc::channel();
        let impl_state = crate::runner::ImplState {
            change_name: "change-a".to_string(),
            completed: 0,
            total: 5,
            log_path: PathBuf::from("/tmp/test.log"),
            receiver: rx,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            child_handle: Arc::new(Mutex::new(None)),
        };

        let batch = BatchImplState::new(vec!["change-a".to_string()], HashMap::new());

        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: Some(impl_state),
            batch: Some(batch),
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        tx.send(crate::runner::ImplUpdate::Finished { success: true })
            .unwrap();
        app.poll_implementation();

        // Implementation should be cleared
        assert!(app.implementation.is_none());
        // Batch should be cleared (only one change, now finished)
        assert!(app.batch.is_none());
    }

    #[test]
    fn test_poll_implementation_finished_advances_batch_to_next() {
        use std::sync::atomic::AtomicBool;
        use std::sync::{Arc, Mutex, mpsc};

        let (tx, rx) = mpsc::channel();
        let impl_state = crate::runner::ImplState {
            change_name: "change-a".to_string(),
            completed: 0,
            total: 5,
            log_path: PathBuf::from("/tmp/test.log"),
            receiver: rx,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            child_handle: Arc::new(Mutex::new(None)),
        };

        // Two independent changes
        let batch = BatchImplState::new(
            vec!["change-a".to_string(), "change-b".to_string()],
            HashMap::new(),
        );

        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: Some(impl_state),
            batch: Some(batch),
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        tx.send(crate::runner::ImplUpdate::Finished { success: true })
            .unwrap();
        app.poll_implementation();

        // Implementation should be started for change-b
        assert!(app.implementation.is_some());
        assert_eq!(app.implementation.as_ref().unwrap().change_name, "change-b");
        // Batch should still be active at index 1
        assert!(app.batch.is_some());
        assert_eq!(app.batch.as_ref().unwrap().current_index, 1);

        // Clean up spawned implementation thread
        app.stop_running_implementation();
    }

    #[test]
    fn test_poll_implementation_finished_skips_dependent_on_failure() {
        use std::sync::atomic::AtomicBool;
        use std::sync::{Arc, Mutex, mpsc};

        let (tx, rx) = mpsc::channel();
        let impl_state = crate::runner::ImplState {
            change_name: "change-a".to_string(),
            completed: 0,
            total: 5,
            log_path: PathBuf::from("/tmp/test.log"),
            receiver: rx,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            child_handle: Arc::new(Mutex::new(None)),
        };

        // change-b depends on change-a, change-c is independent
        let mut deps = HashMap::new();
        deps.insert("change-b".to_string(), vec!["change-a".to_string()]);
        let batch = BatchImplState::new(
            vec![
                "change-a".to_string(),
                "change-b".to_string(),
                "change-c".to_string(),
            ],
            deps,
        );

        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: Some(impl_state),
            batch: Some(batch),
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        // Send failure — change-a failed
        tx.send(crate::runner::ImplUpdate::Finished { success: false })
            .unwrap();
        app.poll_implementation();

        // change-b should be skipped (depends on failed change-a),
        // change-c should be started (independent)
        assert!(app.implementation.is_some());
        assert_eq!(app.implementation.as_ref().unwrap().change_name, "change-c");
        let batch = app.batch.as_ref().unwrap();
        assert!(batch.failed.contains("change-a"));
        assert!(batch.skipped.contains("change-b"));
        assert_eq!(batch.current_index, 2);

        // Clean up spawned implementation thread
        app.stop_running_implementation();
    }

    #[test]
    fn test_poll_implementation_finished_no_batch_unchanged() {
        use std::sync::atomic::AtomicBool;
        use std::sync::{Arc, Mutex, mpsc};

        let (tx, rx) = mpsc::channel();
        let impl_state = crate::runner::ImplState {
            change_name: "test-change".to_string(),
            completed: 5,
            total: 5,
            log_path: PathBuf::from("/tmp/test.log"),
            receiver: rx,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            child_handle: Arc::new(Mutex::new(None)),
        };

        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: Some(impl_state),
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        tx.send(crate::runner::ImplUpdate::Finished { success: true })
            .unwrap();
        app.poll_implementation();

        // Implementation cleared, no batch started
        assert!(app.implementation.is_none());
        assert!(app.batch.is_none());
    }

    #[test]
    fn test_run_all_enter_navigates_back() {
        let entries = vec![make_entry("change-a", true, false)];
        let mut app = make_run_all_app(entries);

        app.handle_run_all_selection_input(KeyCode::Enter);

        assert!(matches!(app.screen, Screen::ChangeList { .. }));
        assert!(app.screen_stack.is_empty());

        // Clean up
        app.stop_running_implementation();
    }

    #[test]
    fn test_stop_running_implementation_clears_batch() {
        use std::sync::atomic::AtomicBool;
        use std::sync::{Arc, Mutex, mpsc};

        let (_tx, rx) = mpsc::channel();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_flag_clone = cancel_flag.clone();
        let existing_impl = crate::runner::ImplState {
            change_name: "change-a".to_string(),
            completed: 1,
            total: 5,
            log_path: PathBuf::from("/tmp/test.log"),
            receiver: rx,
            cancel_flag,
            child_handle: Arc::new(Mutex::new(None)),
        };

        let batch = BatchImplState::new(
            vec![
                "change-a".to_string(),
                "change-b".to_string(),
                "change-c".to_string(),
            ],
            HashMap::new(),
        );

        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: Some(existing_impl),
            batch: Some(batch),
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        assert!(app.implementation.is_some());
        assert!(app.batch.is_some());

        app.stop_running_implementation();

        assert!(app.implementation.is_none());
        assert!(app.batch.is_none());
        assert!(cancel_flag_clone.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn test_stop_running_implementation_no_impl_clears_batch() {
        // Even without a running implementation, batch state should be cleared
        let batch = BatchImplState::new(vec!["change-a".to_string()], HashMap::new());

        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: Some(batch),
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.stop_running_implementation();

        assert!(app.implementation.is_none());
        assert!(app.batch.is_none());
    }

    #[test]
    fn test_stop_running_implementation_no_batch_unchanged() {
        use std::sync::atomic::AtomicBool;
        use std::sync::{Arc, Mutex, mpsc};

        let (_tx, rx) = mpsc::channel();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let existing_impl = crate::runner::ImplState {
            change_name: "change-a".to_string(),
            completed: 1,
            total: 5,
            log_path: PathBuf::from("/tmp/test.log"),
            receiver: rx,
            cancel_flag,
            child_handle: Arc::new(Mutex::new(None)),
        };

        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: Some(existing_impl),
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        // Stopping a single run without batch should work fine
        app.stop_running_implementation();
        assert!(app.implementation.is_none());
        assert!(app.batch.is_none());
    }

    #[test]
    fn test_poll_implementation_stalled_treats_as_failure_skips_dependents() {
        use std::sync::atomic::AtomicBool;
        use std::sync::{Arc, Mutex, mpsc};

        let (tx, rx) = mpsc::channel();
        let impl_state = crate::runner::ImplState {
            change_name: "change-a".to_string(),
            completed: 0,
            total: 5,
            log_path: PathBuf::from("/tmp/test.log"),
            receiver: rx,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            child_handle: Arc::new(Mutex::new(None)),
        };

        // change-b depends on change-a, change-c is independent
        let mut deps = HashMap::new();
        deps.insert("change-b".to_string(), vec!["change-a".to_string()]);
        let batch = BatchImplState::new(
            vec![
                "change-a".to_string(),
                "change-b".to_string(),
                "change-c".to_string(),
            ],
            deps,
        );

        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: Some(impl_state),
            batch: Some(batch),
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        // Send Stalled (not Finished) — should be treated as failure
        tx.send(crate::runner::ImplUpdate::Stalled).unwrap();
        app.poll_implementation();

        // Implementation should be cleared
        assert!(app.implementation.is_some());
        // change-b should be skipped (depends on failed change-a),
        // change-c should be started (independent)
        assert_eq!(app.implementation.as_ref().unwrap().change_name, "change-c");
        let batch = app.batch.as_ref().unwrap();
        assert!(batch.failed.contains("change-a"));
        assert!(batch.skipped.contains("change-b"));
        assert_eq!(batch.current_index, 2);

        // Clean up spawned implementation thread
        app.stop_running_implementation();
    }

    // --- Run Mode Tests ---

    #[test]
    fn test_r_key_dispatches_apply_mode() {
        let dir = std::env::temp_dir().join("openspec-tui-test-r-apply-mode");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("change-config.yaml"), "run_mode: apply\n").unwrap();

        let mut app = App {
            screen: Screen::ArtifactMenu {
                change_name: "apply-change".to_string(),
                change_dir: dir.clone(),
                items: vec![],
                selected: 0,
                is_archived: false,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_artifact_menu_input(KeyCode::Char('R'));
        assert!(app.implementation.is_some());
        assert_eq!(
            app.implementation.as_ref().unwrap().change_name,
            "apply-change"
        );
        // Apply mode sends no progress, so total should be 0
        assert_eq!(app.implementation.as_ref().unwrap().total, 0);

        app.stop_running_implementation();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_r_key_dispatches_normal_mode_by_default() {
        let dir = std::env::temp_dir().join("openspec-tui-test-r-normal-default");
        std::fs::create_dir_all(&dir).unwrap();
        // No change-config.yaml → defaults to normal mode

        let mut app = App {
            screen: Screen::ArtifactMenu {
                change_name: "normal-change".to_string(),
                change_dir: dir.clone(),
                items: vec![],
                selected: 0,
                is_archived: false,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_artifact_menu_input(KeyCode::Char('R'));
        assert!(app.implementation.is_some());
        assert_eq!(
            app.implementation.as_ref().unwrap().change_name,
            "normal-change"
        );

        app.stop_running_implementation();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_m_key_toggles_run_mode_normal_to_apply() {
        let dir = std::env::temp_dir().join("openspec-tui-test-m-toggle-to-apply");
        std::fs::create_dir_all(&dir).unwrap();

        let mut app = App {
            screen: Screen::DependencyView {
                change_name: "test-change".to_string(),
                change_dir: dir.clone(),
                dependencies: vec![],
                selected: 0,
                run_mode: RunMode::Normal,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_dependency_view_input(KeyCode::Char('M'));

        if let Screen::DependencyView { run_mode, .. } = &app.screen {
            assert_eq!(*run_mode, RunMode::Apply);
        } else {
            panic!("Expected DependencyView screen");
        }

        // Verify it was persisted
        let saved = data::read_run_mode(&dir).unwrap();
        assert_eq!(saved, RunMode::Apply);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_m_key_toggles_run_mode_apply_to_normal() {
        let dir = std::env::temp_dir().join("openspec-tui-test-m-toggle-to-normal");
        std::fs::create_dir_all(&dir).unwrap();

        let mut app = App {
            screen: Screen::DependencyView {
                change_name: "test-change".to_string(),
                change_dir: dir.clone(),
                dependencies: vec![],
                selected: 0,
                run_mode: RunMode::Apply,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_dependency_view_input(KeyCode::Char('M'));

        if let Screen::DependencyView { run_mode, .. } = &app.screen {
            assert_eq!(*run_mode, RunMode::Normal);
        } else {
            panic!("Expected DependencyView screen");
        }

        // Verify it was persisted
        let saved = data::read_run_mode(&dir).unwrap();
        assert_eq!(saved, RunMode::Normal);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_m_key_preserves_dependencies() {
        let dir = std::env::temp_dir().join("openspec-tui-test-m-preserves-deps");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("change-config.yaml"),
            "depends_on:\n  - dep-a\n  - dep-b\nrun_mode: normal\n",
        )
        .unwrap();

        let mut app = App {
            screen: Screen::DependencyView {
                change_name: "test-change".to_string(),
                change_dir: dir.clone(),
                dependencies: vec!["dep-a".to_string(), "dep-b".to_string()],
                selected: 0,
                run_mode: RunMode::Normal,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_dependency_view_input(KeyCode::Char('M'));

        // Dependencies should be preserved
        let config = data::read_change_config(&dir).unwrap();
        assert_eq!(
            config.depends_on,
            vec!["dep-a".to_string(), "dep-b".to_string()]
        );
        assert_eq!(config.run_mode, RunMode::Apply);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_enter_on_dependency_item_loads_run_mode() {
        let dir = std::env::temp_dir().join("openspec-tui-test-dep-item-loads-mode");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("change-config.yaml"),
            "depends_on:\n  - dep-one\nrun_mode: apply\n",
        )
        .unwrap();

        let mut app = App {
            screen: Screen::ArtifactMenu {
                change_name: "my-change".to_string(),
                change_dir: dir.clone(),
                items: vec![ArtifactMenuItem {
                    label: "Dependencies [1]".to_string(),
                    available: true,
                    file_path: None,
                    is_spec_header: false,
                    is_dependency_item: true,
                }],
                selected: 0,
                is_archived: false,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_artifact_menu_input(KeyCode::Enter);

        if let Screen::DependencyView {
            run_mode,
            dependencies,
            ..
        } = &app.screen
        {
            assert_eq!(*run_mode, RunMode::Apply);
            assert_eq!(dependencies, &vec!["dep-one".to_string()]);
        } else {
            panic!("Expected DependencyView screen");
        }

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // Task 1.3: Tests for ArtifactView file_path

    #[test]
    fn test_artifact_view_constructed_with_file_path() {
        let dir = std::env::temp_dir().join("openspec-tui-test-artifact-view-filepath");
        std::fs::create_dir_all(&dir).unwrap();
        let proposal_path = dir.join("proposal.md");
        std::fs::write(&proposal_path, "# Proposal\nContent here").unwrap();

        let items = vec![ArtifactMenuItem {
            label: "Proposal".to_string(),
            available: true,
            file_path: Some(proposal_path.clone()),
            is_spec_header: false,
            is_dependency_item: false,
        }];

        let mut app = App {
            screen: Screen::ArtifactMenu {
                change_name: "test-change".to_string(),
                change_dir: dir.clone(),
                items,
                selected: 0,
                is_archived: false,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_artifact_menu_input(KeyCode::Enter);

        if let Screen::ArtifactView { file_path, .. } = &app.screen {
            assert_eq!(*file_path, Some(proposal_path));
        } else {
            panic!("Expected ArtifactView screen");
        }

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_artifact_view_log_has_file_path() {
        let dir = std::env::temp_dir().join("openspec-tui-test-log-filepath");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("implementation.log"), "log content").unwrap();

        let mut app = App {
            screen: Screen::ArtifactMenu {
                change_name: "test-change".to_string(),
                change_dir: dir.clone(),
                items: vec![],
                selected: 0,
                is_archived: false,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.handle_artifact_menu_input(KeyCode::Char('L'));

        if let Screen::ArtifactView { file_path, .. } = &app.screen {
            assert_eq!(*file_path, Some(dir.join("implementation.log")));
        } else {
            panic!("Expected ArtifactView screen after pressing L");
        }

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // Task 2.9: Tests for refresh_screen()

    #[test]
    fn test_refresh_screen_artifact_view_reloads_content() {
        let dir = std::env::temp_dir().join("openspec-tui-test-refresh-artifact-view");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("test.md");
        std::fs::write(&file, "original content").unwrap();

        let mut app = App {
            screen: Screen::ArtifactView {
                title: "Test".to_string(),
                content: "original content".to_string(),
                scroll: 3,
                is_plain_text: false,
                file_path: Some(file.clone()),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        // Modify file on disk
        std::fs::write(&file, "updated content").unwrap();

        app.refresh_screen();

        if let Screen::ArtifactView {
            content, scroll, ..
        } = &app.screen
        {
            assert_eq!(content, "updated content");
            // Scroll position preserved
            assert_eq!(*scroll, 3);
        } else {
            panic!("Expected ArtifactView screen");
        }

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_refresh_screen_artifact_view_no_file_path_is_noop() {
        let mut app = App {
            screen: Screen::ArtifactView {
                title: "Test".to_string(),
                content: "original".to_string(),
                scroll: 0,
                is_plain_text: false,
                file_path: None,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.refresh_screen();

        if let Screen::ArtifactView { content, .. } = &app.screen {
            assert_eq!(content, "original");
        } else {
            panic!("Expected ArtifactView screen");
        }
    }

    #[test]
    fn test_refresh_screen_change_list_clamps_selection() {
        // Start with a list of 3, selected at index 2
        let mut app = App {
            screen: Screen::ChangeList {
                changes: vec![
                    ChangeEntry {
                        name: "a".to_string(),
                        completed_tasks: 0,
                        total_tasks: 1,
                    },
                    ChangeEntry {
                        name: "b".to_string(),
                        completed_tasks: 0,
                        total_tasks: 1,
                    },
                    ChangeEntry {
                        name: "c".to_string(),
                        completed_tasks: 0,
                        total_tasks: 1,
                    },
                ],
                selected: 2,
                error: None,
                tab: ChangeTab::Active,
                change_deps: HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        // After refresh, the real change list is loaded from the CLI.
        // We can't control the external data, but we can verify the method
        // doesn't panic and the selected index is valid.
        app.refresh_screen();

        if let Screen::ChangeList {
            selected, changes, ..
        } = &app.screen
        {
            if !changes.is_empty() {
                assert!(*selected < changes.len());
            } else {
                assert_eq!(*selected, 0);
            }
        } else {
            panic!("Expected ChangeList screen");
        }
    }

    #[test]
    fn test_refresh_screen_config_is_noop() {
        let mut app = App {
            screen: Screen::Config {
                command: "test-cmd".to_string(),
                prompt: "test-prompt".to_string(),
                post_implementation_prompt: "".to_string(),
                interactive_command: "".to_string(),
                run_finished_command: "".to_string(),
                cursor_position: 5,
                focused_field: ConfigField::Command,
                editing: false,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.refresh_screen();

        // Config screen should remain unchanged
        if let Screen::Config {
            command,
            cursor_position,
            ..
        } = &app.screen
        {
            assert_eq!(command, "test-cmd");
            assert_eq!(*cursor_position, 5);
        } else {
            panic!("Expected Config screen");
        }
    }

    #[test]
    fn test_refresh_screen_dependency_view_reloads() {
        let dir = std::env::temp_dir().join("openspec-tui-test-refresh-depview");
        std::fs::create_dir_all(&dir).unwrap();
        // Write a change-config.yaml with one dependency
        std::fs::write(
            dir.join("change-config.yaml"),
            "depends_on:\n  - dep-a\n  - dep-b\nrun_mode: normal\n",
        )
        .unwrap();

        let mut app = App {
            screen: Screen::DependencyView {
                change_name: "test".to_string(),
                change_dir: dir.clone(),
                dependencies: vec!["old-dep".to_string()],
                selected: 0,
                run_mode: data::RunMode::Normal,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: TuiConfig::default(),
            config_path: PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        app.refresh_screen();

        if let Screen::DependencyView { dependencies, .. } = &app.screen {
            assert_eq!(
                dependencies,
                &vec!["dep-a".to_string(), "dep-b".to_string()]
            );
        } else {
            panic!("Expected DependencyView screen");
        }

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
