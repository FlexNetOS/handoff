use std::collections::HashMap;

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use crate::runner::{BatchImplState, ImplState};

use crate::app::{App, ChangeTab, ConfigField, Screen};

pub fn draw(frame: &mut Frame, app: &App) {
    let (content_area, status_area) = if let Some(ref impl_state) = app.implementation {
        let chunks =
            Layout::vertical([Constraint::Min(0), Constraint::Length(4)]).split(frame.area());
        draw_status_bar(frame, impl_state, app.batch.as_ref(), chunks[1]);
        (chunks[0], Some(chunks[1]))
    } else {
        (frame.area(), None)
    };
    let _ = status_area;

    match &app.screen {
        Screen::ChangeList {
            changes,
            selected,
            error,
            tab,
            change_deps,
        } => draw_change_list(
            frame,
            changes,
            *selected,
            error.as_deref(),
            tab,
            change_deps,
            content_area,
        ),
        Screen::ArtifactMenu {
            change_name,
            items,
            selected,
            ..
        } => draw_artifact_menu(frame, change_name, items, *selected, content_area),
        Screen::ArtifactView {
            title,
            content,
            scroll,
            is_plain_text,
            ..
        } => draw_artifact_view(frame, title, content, *scroll, *is_plain_text, content_area),
        Screen::Config {
            command,
            prompt,
            post_implementation_prompt,
            interactive_command,
            run_finished_command,
            cursor_position,
            focused_field,
            editing,
        } => draw_config_screen(
            frame,
            &ConfigScreenParams {
                command,
                prompt,
                post_implementation_prompt,
                interactive_command,
                run_finished_command,
                cursor_position: *cursor_position,
                focused_field,
                editing: *editing,
            },
            content_area,
        ),
        Screen::DependencyView {
            change_name,
            dependencies,
            selected,
            run_mode,
            ..
        } => draw_dependency_view(
            frame,
            change_name,
            dependencies,
            *selected,
            run_mode,
            content_area,
        ),
        Screen::DependencyAdd {
            change_name,
            available_changes,
            selected,
            ..
        } => draw_dependency_add(
            frame,
            change_name,
            available_changes,
            *selected,
            content_area,
        ),
        Screen::DependencyGraph { graph_text, scroll } => {
            draw_dependency_graph(frame, graph_text, *scroll, content_area)
        }
        Screen::RunAllSelection {
            entries,
            selected,
            error,
        } => draw_run_all_selection(frame, entries, *selected, error.as_deref(), content_area),
    }
}

fn tab_title(tab: &ChangeTab) -> Line<'static> {
    let (active_style, archived_style) = match tab {
        ChangeTab::Active => (
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            Style::default().fg(Color::DarkGray),
        ),
        ChangeTab::Archived => (
            Style::default().fg(Color::DarkGray),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    };

    Line::from(vec![
        Span::raw(" OpenSpec TUI ["),
        Span::styled("Active", active_style),
        Span::raw(" | "),
        Span::styled("Archived", archived_style),
        Span::raw("] "),
    ])
}

fn draw_change_list(
    frame: &mut Frame,
    changes: &[crate::data::ChangeEntry],
    selected: usize,
    error: Option<&str>,
    tab: &ChangeTab,
    change_deps: &HashMap<String, Vec<String>>,
    area: Rect,
) {
    if let Some(err) = error {
        let paragraph = Paragraph::new(err)
            .style(Style::default().fg(Color::Red))
            .block(
                Block::default()
                    .title(" OpenSpec TUI - Error ")
                    .borders(Borders::ALL),
            );
        frame.render_widget(paragraph, area);
        return;
    }

    let title = tab_title(tab);

    if changes.is_empty() {
        let empty_msg = match tab {
            ChangeTab::Active => "No active changes found.",
            ChangeTab::Archived => "No archived changes found.",
        };
        let paragraph = Paragraph::new(empty_msg)
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().title(title).borders(Borders::ALL));
        frame.render_widget(paragraph, area);
        return;
    }

    // Available width inside the border (2 chars for borders)
    let inner_width = area.width.saturating_sub(2) as usize;

    let items: Vec<ListItem> = changes
        .iter()
        .enumerate()
        .map(|(i, change)| {
            let style = if i == selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let progress = format!("  ({}/{})", change.completed_tasks, change.total_tasks);
            let left_len = change.name.len() + progress.len();

            let mut spans = vec![
                Span::styled(&change.name, style),
                Span::styled(progress, Style::default().fg(Color::DarkGray)),
            ];

            if let Some(deps) = change_deps.get(&change.name)
                && !deps.is_empty()
            {
                let dep_str = format!("<- {}", deps.join(", "));
                // Need at least 3 chars of space before dep string (for "   ")
                let available = inner_width.saturating_sub(left_len + 3);
                let truncated = if dep_str.len() > available && available > 3 {
                    format!("{}...", &dep_str[..available - 3])
                } else if dep_str.len() > available {
                    String::new()
                } else {
                    dep_str
                };
                if !truncated.is_empty() {
                    spans.push(Span::styled(
                        format!("   {}", truncated),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    let mut bottom_hints = vec![Span::styled(
        " [C] Config ",
        Style::default().fg(Color::DarkGray),
    )];
    if *tab == ChangeTab::Active {
        bottom_hints.push(Span::styled(
            "[I] Interactive ",
            Style::default().fg(Color::DarkGray),
        ));
    }
    bottom_hints.push(Span::styled(
        "[q] Quit ",
        Style::default().fg(Color::DarkGray),
    ));

    let list = List::new(items).block(
        Block::default()
            .title(title)
            .title_bottom(Line::from(bottom_hints))
            .borders(Borders::ALL),
    );
    frame.render_widget(list, area);
}

fn draw_artifact_menu(
    frame: &mut Frame,
    change_name: &str,
    items: &[crate::app::ArtifactMenuItem],
    selected: usize,
    area: Rect,
) {
    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let style = if !item.available {
                Style::default().fg(Color::DarkGray)
            } else if i == selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let indicator = if i == selected { "> " } else { "  " };
            ListItem::new(Line::from(Span::styled(
                format!("{}{}", indicator, item.label),
                style,
            )))
        })
        .collect();

    let list = List::new(list_items).block(
        Block::default()
            .title(format!(" {} - Artifacts ", change_name))
            .title_bottom(Line::from(vec![
                Span::styled(" [C] Config ", Style::default().fg(Color::DarkGray)),
                Span::styled("[R] Run ", Style::default().fg(Color::DarkGray)),
                Span::styled("[L] Log ", Style::default().fg(Color::DarkGray)),
                Span::styled("[Esc] Back ", Style::default().fg(Color::DarkGray)),
            ]))
            .borders(Borders::ALL),
    );
    frame.render_widget(list, area);
}

pub fn draw_artifact_view(
    frame: &mut Frame,
    title: &str,
    content: &str,
    scroll: usize,
    is_plain_text: bool,
    area: Rect,
) {
    let text = if is_plain_text {
        ratatui::text::Text::from(content)
    } else {
        tui_markdown::from_str(content)
    };
    let total_lines = text.lines.len();

    let paragraph = Paragraph::new(text)
        .wrap(Wrap { trim: false })
        .scroll((scroll as u16, 0))
        .block(
            Block::default()
                .title(format!(" {} [{}/{}] ", title, scroll + 1, total_lines))
                .title_bottom(Line::from(vec![
                    Span::styled(" [C] Config ", Style::default().fg(Color::DarkGray)),
                    Span::styled("[Esc] Back ", Style::default().fg(Color::DarkGray)),
                ]))
                .borders(Borders::ALL),
        );
    frame.render_widget(paragraph, area);
}

pub struct ConfigScreenParams<'a> {
    pub command: &'a str,
    pub prompt: &'a str,
    pub post_implementation_prompt: &'a str,
    pub interactive_command: &'a str,
    pub run_finished_command: &'a str,
    pub cursor_position: usize,
    pub focused_field: &'a ConfigField,
    pub editing: bool,
}

pub fn draw_config_screen(frame: &mut Frame, params: &ConfigScreenParams, area: Rect) {
    let command = params.command;
    let prompt = params.prompt;
    let post_implementation_prompt = params.post_implementation_prompt;
    let interactive_command = params.interactive_command;
    let run_finished_command = params.run_finished_command;
    let cursor_position = params.cursor_position;
    let focused_field = params.focused_field;
    let editing = params.editing;
    let chunks = Layout::vertical([
        Constraint::Length(3), // Command field
        Constraint::Min(3),    // Prompt preview
        Constraint::Length(3), // Post-implementation prompt
        Constraint::Length(3), // Interactive command
        Constraint::Length(3), // Run finished command
        Constraint::Length(1), // Keybinding hints
    ])
    .split(area);

    // Command field
    let cmd_style = if *focused_field == ConfigField::Command {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let cmd_block = Block::default()
        .title(" Command ")
        .borders(Borders::ALL)
        .border_style(cmd_style);

    let cmd_text = if editing && *focused_field == ConfigField::Command {
        // Show cursor only in edit mode
        let before = &command[..cursor_position];
        let cursor_char = command
            .get(cursor_position..cursor_position + 1)
            .unwrap_or(" ");
        let after = if cursor_position < command.len() {
            &command[cursor_position + 1..]
        } else {
            ""
        };
        Line::from(vec![
            Span::raw(before),
            Span::styled(
                cursor_char,
                Style::default().fg(Color::Black).bg(Color::White),
            ),
            Span::raw(after),
        ])
    } else {
        Line::from(command)
    };

    let cmd_paragraph = Paragraph::new(cmd_text).block(cmd_block);
    frame.render_widget(cmd_paragraph, chunks[0]);

    // Prompt preview
    let prompt_style = if *focused_field == ConfigField::Prompt {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let prompt_title = if *focused_field == ConfigField::Prompt {
        " Prompt (Enter to edit in $EDITOR) "
    } else {
        " Prompt "
    };
    let prompt_block = Block::default()
        .title(prompt_title)
        .borders(Borders::ALL)
        .border_style(prompt_style);

    let prompt_text = prompt
        .lines()
        .map(|line| Line::from(line.to_string()))
        .collect::<Vec<_>>();
    let prompt_paragraph = Paragraph::new(prompt_text)
        .wrap(Wrap { trim: false })
        .block(prompt_block);
    frame.render_widget(prompt_paragraph, chunks[1]);

    // Post-implementation prompt
    let post_prompt_style = if *focused_field == ConfigField::PostImplementationPrompt {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let post_prompt_title = if *focused_field == ConfigField::PostImplementationPrompt {
        " Post-Implementation Prompt (Enter to edit in $EDITOR) "
    } else {
        " Post-Implementation Prompt "
    };
    let post_prompt_block = Block::default()
        .title(post_prompt_title)
        .borders(Borders::ALL)
        .border_style(post_prompt_style);

    let post_prompt_display = if post_implementation_prompt.is_empty() {
        "(disabled)".to_string()
    } else {
        post_implementation_prompt.to_string()
    };
    let post_prompt_paragraph = Paragraph::new(post_prompt_display)
        .wrap(Wrap { trim: false })
        .block(post_prompt_block);
    frame.render_widget(post_prompt_paragraph, chunks[2]);

    // Interactive command field
    let interactive_style = if *focused_field == ConfigField::InteractiveCommand {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let interactive_block = Block::default()
        .title(" Interactive Command ")
        .borders(Borders::ALL)
        .border_style(interactive_style);

    let interactive_text = if editing && *focused_field == ConfigField::InteractiveCommand {
        let before = &interactive_command[..cursor_position];
        let cursor_char = interactive_command
            .get(cursor_position..cursor_position + 1)
            .unwrap_or(" ");
        let after = if cursor_position < interactive_command.len() {
            &interactive_command[cursor_position + 1..]
        } else {
            ""
        };
        Line::from(vec![
            Span::raw(before),
            Span::styled(
                cursor_char,
                Style::default().fg(Color::Black).bg(Color::White),
            ),
            Span::raw(after),
        ])
    } else {
        Line::from(interactive_command)
    };

    let interactive_paragraph = Paragraph::new(interactive_text).block(interactive_block);
    frame.render_widget(interactive_paragraph, chunks[3]);

    // Run finished command field
    let run_finished_style = if *focused_field == ConfigField::RunFinishedCommand {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let run_finished_block = Block::default()
        .title(" Run Finished Command ")
        .borders(Borders::ALL)
        .border_style(run_finished_style);

    let run_finished_text = if editing && *focused_field == ConfigField::RunFinishedCommand {
        let before = &run_finished_command[..cursor_position];
        let cursor_char = run_finished_command
            .get(cursor_position..cursor_position + 1)
            .unwrap_or(" ");
        let after = if cursor_position < run_finished_command.len() {
            &run_finished_command[cursor_position + 1..]
        } else {
            ""
        };
        Line::from(vec![
            Span::raw(before),
            Span::styled(
                cursor_char,
                Style::default().fg(Color::Black).bg(Color::White),
            ),
            Span::raw(after),
        ])
    } else if run_finished_command.is_empty() {
        Line::from(Span::styled(
            "(disabled)",
            Style::default().fg(Color::DarkGray),
        ))
    } else {
        Line::from(run_finished_command)
    };

    let run_finished_paragraph = Paragraph::new(run_finished_text).block(run_finished_block);
    frame.render_widget(run_finished_paragraph, chunks[4]);

    // Warning if {prompt} is missing from command
    let has_prompt_placeholder = command.contains("{prompt}");

    // Keybinding hints
    let mut hints = if editing {
        vec![Span::styled(
            " [Esc] Done editing ",
            Style::default().fg(Color::DarkGray),
        )]
    } else {
        vec![
            Span::styled(" [Enter] Edit ", Style::default().fg(Color::DarkGray)),
            Span::styled(" [Tab] Switch field ", Style::default().fg(Color::DarkGray)),
            Span::styled(" [S] Save ", Style::default().fg(Color::DarkGray)),
            Span::styled(" [D] Reset defaults ", Style::default().fg(Color::DarkGray)),
            Span::styled(" [Esc] Cancel ", Style::default().fg(Color::DarkGray)),
        ]
    };

    if !has_prompt_placeholder {
        hints.push(Span::styled(
            " ⚠ {prompt} missing in command! ",
            Style::default().fg(Color::Red),
        ));
    }

    let hints_line = Line::from(hints);
    let hints_paragraph = Paragraph::new(hints_line);
    frame.render_widget(hints_paragraph, chunks[5]);
}

pub fn draw_dependency_view(
    frame: &mut Frame,
    change_name: &str,
    dependencies: &[String],
    selected: usize,
    run_mode: &crate::data::RunMode,
    area: Rect,
) {
    let mode_label = match run_mode {
        crate::data::RunMode::Normal => "Normal",
        crate::data::RunMode::Apply => "Apply",
    };
    let title = format!(" {} - Config  [Mode: {}] ", change_name, mode_label);

    if dependencies.is_empty() {
        let paragraph = Paragraph::new("No dependencies configured.")
            .style(Style::default().fg(Color::DarkGray))
            .block(
                Block::default()
                    .title(title)
                    .title_bottom(Line::from(vec![
                        Span::styled(" [A] Add ", Style::default().fg(Color::DarkGray)),
                        Span::styled("[M] Toggle Mode ", Style::default().fg(Color::DarkGray)),
                        Span::styled("[Esc] Back ", Style::default().fg(Color::DarkGray)),
                    ]))
                    .borders(Borders::ALL),
            );
        frame.render_widget(paragraph, area);
        return;
    }

    let items: Vec<ListItem> = dependencies
        .iter()
        .enumerate()
        .map(|(i, dep)| {
            let style = if i == selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let indicator = if i == selected { "> " } else { "  " };
            ListItem::new(Line::from(Span::styled(
                format!("{}{}", indicator, dep),
                style,
            )))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(title)
            .title_bottom(Line::from(vec![
                Span::styled(" [A] Add ", Style::default().fg(Color::DarkGray)),
                Span::styled("[D] Remove ", Style::default().fg(Color::DarkGray)),
                Span::styled("[M] Toggle Mode ", Style::default().fg(Color::DarkGray)),
                Span::styled("[Esc] Back ", Style::default().fg(Color::DarkGray)),
            ]))
            .borders(Borders::ALL),
    );
    frame.render_widget(list, area);
}

pub fn draw_dependency_add(
    frame: &mut Frame,
    change_name: &str,
    available_changes: &[String],
    selected: usize,
    area: Rect,
) {
    let items: Vec<ListItem> = available_changes
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let style = if i == selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let indicator = if i == selected { "> " } else { "  " };
            ListItem::new(Line::from(Span::styled(
                format!("{}{}", indicator, name),
                style,
            )))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(format!(" {} - Add Dependency ", change_name))
            .title_bottom(Line::from(vec![
                Span::styled(" [Enter] Select ", Style::default().fg(Color::DarkGray)),
                Span::styled("[Esc] Cancel ", Style::default().fg(Color::DarkGray)),
            ]))
            .borders(Borders::ALL),
    );
    frame.render_widget(list, area);
}

pub fn draw_status_bar(
    frame: &mut Frame,
    impl_state: &ImplState,
    batch: Option<&BatchImplState>,
    area: Rect,
) {
    let progress_pct = if impl_state.total > 0 {
        (impl_state.completed as f64 / impl_state.total as f64 * 100.0) as u16
    } else {
        0
    };

    // Build progress bar: use the available inner width minus the text portions
    // Line 1: ⟳ change-name  completed/total  [████░░] pct%  Change X/Y  (batch info)
    // Line 2: Log: /path/to/log  [S] Stop
    let bar_width = area.width.saturating_sub(2) as usize; // account for borders

    // Build batch suffix for line 1 (e.g., "  Change 2/4  1 failed, 2 skipped")
    let batch_suffix = if let Some(batch) = batch {
        let change_progress = format!("  Change {}/{}", batch.current_index + 1, batch.total());
        let mut parts = Vec::new();
        let failed_count = batch.failed.len();
        let skipped_count = batch.skipped.len();
        if failed_count > 0 {
            parts.push(format!("{} failed", failed_count));
        }
        if skipped_count > 0 {
            parts.push(format!("{} skipped", skipped_count));
        }
        if parts.is_empty() {
            change_progress
        } else {
            format!("{}  {}", change_progress, parts.join(", "))
        }
    } else {
        String::new()
    };

    let prefix = format!(
        " ⟳ {}  {}/{}  ",
        impl_state.change_name, impl_state.completed, impl_state.total
    );
    let suffix = format!(" {}%", progress_pct);
    let bar_space = bar_width
        .saturating_sub(prefix.len())
        .saturating_sub(suffix.len())
        .saturating_sub(batch_suffix.len());
    let filled = (bar_space as u32 * impl_state.completed)
        .checked_div(impl_state.total)
        .unwrap_or(0) as usize;
    let empty = bar_space.saturating_sub(filled);

    let mut line1_spans = vec![
        Span::styled(&prefix, Style::default().fg(Color::Cyan)),
        Span::styled("█".repeat(filled), Style::default().fg(Color::Green)),
        Span::styled("░".repeat(empty), Style::default().fg(Color::DarkGray)),
        Span::styled(&suffix, Style::default().fg(Color::Cyan)),
    ];
    if !batch_suffix.is_empty() {
        line1_spans.push(Span::styled(
            batch_suffix,
            Style::default().fg(Color::Yellow),
        ));
    }
    let line1 = Line::from(line1_spans);

    let log_display = impl_state.log_path.display().to_string();
    let line2 = Line::from(vec![
        Span::styled(
            format!(" Log: {}", log_display),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled("  [S] Stop", Style::default().fg(Color::Yellow)),
    ]);

    let paragraph = Paragraph::new(vec![line1, line2]).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(paragraph, area);
}

pub fn draw_dependency_graph(frame: &mut Frame, graph_text: &str, scroll: usize, area: Rect) {
    let total_lines = graph_text.lines().count();

    let paragraph = Paragraph::new(graph_text).scroll((scroll as u16, 0)).block(
        Block::default()
            .title(format!(
                " Dependency Graph [{}/{}] ",
                scroll + 1,
                total_lines.max(1)
            ))
            .title_bottom(Line::from(vec![Span::styled(
                " [Esc] Back ",
                Style::default().fg(Color::DarkGray),
            )]))
            .borders(Borders::ALL),
    );
    frame.render_widget(paragraph, area);
}

pub fn draw_run_all_selection(
    frame: &mut Frame,
    entries: &[crate::app::RunAllEntry],
    selected: usize,
    error: Option<&str>,
    area: Rect,
) {
    if entries.is_empty() {
        let paragraph = Paragraph::new("No eligible changes found (no tasks.md).")
            .style(Style::default().fg(Color::DarkGray))
            .block(
                Block::default()
                    .title(" Run All - Select Changes ")
                    .title_bottom(Line::from(vec![Span::styled(
                        " [Esc] Cancel ",
                        Style::default().fg(Color::DarkGray),
                    )]))
                    .borders(Borders::ALL),
            );
        frame.render_widget(paragraph, area);
        return;
    }

    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let checkbox = if entry.blocked {
                "[~]"
            } else if entry.included {
                "[x]"
            } else {
                "[ ]"
            };

            let progress = format!("({}/{})", entry.completed_tasks, entry.total_tasks);

            let style = if entry.blocked {
                Style::default().fg(Color::DarkGray)
            } else if i == selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let mut spans = vec![
                Span::styled(format!(" {} ", checkbox), style),
                Span::styled(&entry.change_name, style),
                Span::styled(
                    format!("  {}", progress),
                    Style::default().fg(Color::DarkGray),
                ),
            ];

            if let Some(ref blocker) = entry.blocked_by {
                spans.push(Span::styled(
                    format!("  blocked by: {}", blocker),
                    Style::default().fg(Color::Red),
                ));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    let mut title_bottom_spans = vec![
        Span::styled(" [Space] Toggle ", Style::default().fg(Color::DarkGray)),
        Span::styled("[Enter] Start ", Style::default().fg(Color::DarkGray)),
        Span::styled("[Esc] Cancel ", Style::default().fg(Color::DarkGray)),
    ];

    if let Some(err) = error {
        title_bottom_spans.push(Span::styled(
            format!(" {} ", err),
            Style::default().fg(Color::Red),
        ));
    }

    let list = List::new(items).block(
        Block::default()
            .title(" Run All - Select Changes ")
            .title_bottom(Line::from(title_bottom_spans))
            .borders(Borders::ALL),
    );
    frame.render_widget(list, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend, layout::Rect};

    fn render_artifact_view(width: u16, height: u16, content: &str, scroll: usize) -> String {
        render_artifact_view_with_mode(width, height, content, scroll, false)
    }

    fn render_artifact_view_with_mode(
        width: u16,
        height: u16,
        content: &str,
        scroll: usize,
        is_plain_text: bool,
    ) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                draw_artifact_view(frame, "test", content, scroll, is_plain_text, area);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut lines = Vec::new();
        for y in 0..height {
            let mut line = String::new();
            for x in 0..width {
                line.push_str(buffer.cell((x, y)).unwrap().symbol());
            }
            lines.push(line.trim_end().to_string());
        }
        lines.join("\n")
    }

    #[test]
    fn test_long_lines_are_wrapped() {
        // Terminal is 20 wide; border takes 2 chars, leaving 18 content chars.
        // A line longer than 18 chars should wrap onto the next rendered line.
        let content = "aaa bbb ccc ddd eee fff ggg";
        let rendered = render_artifact_view(20, 6, content, 0);
        // The content area is lines 1..5 (between top and bottom border).
        // With wrapping, the long line should span multiple rendered lines.
        // With wrapping enabled, the long line should span multiple rendered lines.
        // Verify that both early and later words appear in the render.
        assert!(rendered.contains("aaa"), "First word should be visible");
        assert!(
            rendered.contains("fff"),
            "Later word should be visible via wrapping"
        );
    }

    #[test]
    fn test_short_lines_remain_unchanged() {
        let content = "short line";
        let rendered = render_artifact_view(40, 5, content, 0);
        let content_lines: Vec<&str> = rendered.lines().collect();
        // Content line 1 (after top border) should contain the text
        assert!(content_lines[1].contains("short line"));
        // Content line 2 should be empty (no wrapping occurred)
        let line2_content = content_lines[2]
            .trim_start_matches('│')
            .trim_end_matches('│')
            .trim();
        assert!(
            line2_content.is_empty(),
            "Second content line should be empty for short text"
        );
    }

    #[test]
    fn test_markdown_headers_rendered_as_formatted_text() {
        let content = "# Main Header\n\nSome body text.";
        let rendered = render_artifact_view(40, 8, content, 0);
        // Header text and body text should both be visible
        assert!(
            rendered.contains("Main Header"),
            "Header text should be visible"
        );
        assert!(
            rendered.contains("Some body text"),
            "Body text should be visible"
        );

        // Verify header has bold styling applied
        let backend = TestBackend::new(40, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                draw_artifact_view(frame, "test", content, 0, false, area);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        // Find a cell in the header line and check it has bold modifier
        let mut found_bold = false;
        for x in 0..40u16 {
            let cell = buffer.cell((x, 1)).unwrap();
            if cell.symbol() == "M" && cell.modifier.contains(Modifier::BOLD) {
                found_bold = true;
            }
        }
        assert!(
            found_bold,
            "Header text should be rendered with bold styling"
        );
    }

    #[test]
    fn test_code_blocks_rendered_with_highlighting() {
        let content = "```rust\nfn main() {}\n```";
        let rendered = render_artifact_view(40, 8, content, 0);
        // Code content should be visible
        assert!(rendered.contains("fn"), "Code keyword should be visible");
        assert!(rendered.contains("main"), "Function name should be visible");

        // Verify syntax highlighting: check that the code content cell has non-default styling
        let backend = TestBackend::new(40, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                draw_artifact_view(frame, "test", content, 0, false, area);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        // Find a cell containing "fn" and check it has a foreground color set (syntax highlighting)
        let mut found_styled = false;
        for y in 0..8u16 {
            for x in 0..40u16 {
                let cell = buffer.cell((x, y)).unwrap();
                if cell.symbol() == "f"
                    && x + 1 < 40
                    && buffer.cell((x + 1, y)).unwrap().symbol() == "n"
                {
                    // "fn" keyword found; check if it has non-default foreground color
                    if cell.fg != Color::Reset && cell.fg != Color::default() {
                        found_styled = true;
                    }
                }
            }
        }
        assert!(
            found_styled,
            "Code keyword 'fn' should have syntax highlighting (non-default color)"
        );
    }

    #[test]
    fn test_indented_code_block_rendered() {
        // 4-space indented text is rendered as a code block in Markdown
        let content = "    indented code";
        let rendered = render_artifact_view(30, 6, content, 0);
        // The indented text should still be visible in the rendered output
        assert!(
            rendered.contains("indented code"),
            "Code block content should be visible"
        );
    }

    fn render_status_bar(width: u16, height: u16, impl_state: &ImplState) -> String {
        render_status_bar_with_batch(width, height, impl_state, None)
    }

    fn render_status_bar_with_batch(
        width: u16,
        height: u16,
        impl_state: &ImplState,
        batch: Option<&BatchImplState>,
    ) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, width, height);
                draw_status_bar(frame, impl_state, batch, area);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut lines = Vec::new();
        for y in 0..height {
            let mut line = String::new();
            for x in 0..width {
                line.push_str(buffer.cell((x, y)).unwrap().symbol());
            }
            lines.push(line.trim_end().to_string());
        }
        lines.join("\n")
    }

    fn make_impl_state(change_name: &str, completed: u32, total: u32) -> ImplState {
        use std::path::PathBuf;
        use std::sync::atomic::AtomicBool;
        use std::sync::{Arc, Mutex, mpsc};

        let (_tx, rx) = mpsc::channel();
        ImplState {
            change_name: change_name.to_string(),
            completed,
            total,
            log_path: PathBuf::from(format!(
                "openspec/changes/{}/implementation.log",
                change_name
            )),
            receiver: rx,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            child_handle: Arc::new(Mutex::new(None)),
        }
    }

    #[test]
    fn test_status_bar_shows_change_name() {
        let state = make_impl_state("my-change", 3, 7);
        let rendered = render_status_bar(60, 4, &state);
        assert!(
            rendered.contains("my-change"),
            "Change name should be displayed"
        );
    }

    #[test]
    fn test_status_bar_shows_task_counts() {
        let state = make_impl_state("test", 3, 7);
        let rendered = render_status_bar(60, 4, &state);
        assert!(rendered.contains("3/7"), "Task counts should be displayed");
    }

    #[test]
    fn test_status_bar_shows_progress_bar() {
        let state = make_impl_state("test", 5, 10);
        let rendered = render_status_bar(60, 4, &state);
        assert!(
            rendered.contains("█"),
            "Progress bar should have filled blocks"
        );
        assert!(
            rendered.contains("░"),
            "Progress bar should have empty blocks"
        );
        assert!(rendered.contains("50%"), "Percentage should be displayed");
    }

    #[test]
    fn test_status_bar_shows_stop_hint() {
        let state = make_impl_state("test", 0, 5);
        let rendered = render_status_bar(60, 4, &state);
        assert!(
            rendered.contains("[S] Stop"),
            "Stop hint should be displayed"
        );
    }

    #[test]
    fn test_status_bar_shows_log_path() {
        let state = make_impl_state("test", 0, 5);
        let rendered = render_status_bar(80, 4, &state);
        assert!(
            rendered.contains("Log:"),
            "Log path label should be displayed"
        );
        assert!(
            rendered.contains("openspec/changes/test/implementation.log"),
            "Log path should show the change-local path"
        );
    }

    #[test]
    fn test_status_bar_zero_total() {
        let state = make_impl_state("test", 0, 0);
        let rendered = render_status_bar(60, 4, &state);
        assert!(
            rendered.contains("0/0"),
            "Zero progress should be displayed"
        );
        assert!(
            rendered.contains("0%"),
            "Zero percentage should be displayed"
        );
    }

    #[test]
    fn test_status_bar_all_complete() {
        let state = make_impl_state("test", 5, 5);
        let rendered = render_status_bar(60, 4, &state);
        assert!(
            rendered.contains("5/5"),
            "Complete progress should be displayed"
        );
        assert!(rendered.contains("100%"), "100% should be displayed");
    }

    #[test]
    fn test_status_bar_batch_progress() {
        let state = make_impl_state("change-b", 3, 7);
        let batch = BatchImplState::new(
            vec![
                "change-a".to_string(),
                "change-b".to_string(),
                "change-c".to_string(),
                "change-d".to_string(),
            ],
            std::collections::HashMap::new(),
        );
        // current_index=0 means we're on change 1/4, but we want to simulate being on change 2
        let mut batch = batch;
        batch.current_index = 1;
        batch.completed.insert("change-a".to_string());

        let rendered = render_status_bar_with_batch(100, 4, &state, Some(&batch));
        assert!(
            rendered.contains("3/7"),
            "Task progress should be displayed"
        );
        assert!(
            rendered.contains("Change 2/4"),
            "Batch change progress should be displayed"
        );
    }

    #[test]
    fn test_status_bar_batch_with_failures() {
        let state = make_impl_state("change-c", 1, 5);
        let mut batch = BatchImplState::new(
            vec![
                "change-a".to_string(),
                "change-b".to_string(),
                "change-c".to_string(),
            ],
            std::collections::HashMap::new(),
        );
        batch.current_index = 2;
        batch.failed.insert("change-a".to_string());
        batch.completed.insert("change-b".to_string());

        let rendered = render_status_bar_with_batch(100, 4, &state, Some(&batch));
        assert!(
            rendered.contains("Change 3/3"),
            "Batch change progress should be displayed"
        );
        assert!(
            rendered.contains("1 failed"),
            "Failed count should be displayed"
        );
    }

    #[test]
    fn test_status_bar_batch_with_skips() {
        let state = make_impl_state("change-d", 0, 3);
        let mut batch = BatchImplState::new(
            vec![
                "change-a".to_string(),
                "change-b".to_string(),
                "change-c".to_string(),
                "change-d".to_string(),
            ],
            std::collections::HashMap::new(),
        );
        batch.current_index = 3;
        batch.failed.insert("change-a".to_string());
        batch.skipped.insert("change-b".to_string());
        batch.skipped.insert("change-c".to_string());

        let rendered = render_status_bar_with_batch(100, 4, &state, Some(&batch));
        assert!(
            rendered.contains("Change 4/4"),
            "Batch change progress should be displayed"
        );
        assert!(
            rendered.contains("1 failed"),
            "Failed count should be displayed"
        );
        assert!(
            rendered.contains("2 skipped"),
            "Skipped count should be displayed"
        );
    }

    #[test]
    fn test_status_bar_no_batch_unchanged() {
        // Without batch state, the status bar should not show any batch info
        let state = make_impl_state("test", 3, 7);
        let rendered = render_status_bar_with_batch(80, 4, &state, None);
        assert!(
            rendered.contains("3/7"),
            "Task progress should be displayed"
        );
        assert!(
            !rendered.contains("Change"),
            "No batch progress should be displayed without batch state"
        );
    }

    fn render_draw(width: u16, height: u16, app: &crate::app::App) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                draw(frame, app);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut lines = Vec::new();
        for y in 0..height {
            let mut line = String::new();
            for x in 0..width {
                line.push_str(buffer.cell((x, y)).unwrap().symbol());
            }
            lines.push(line.trim_end().to_string());
        }
        lines.join("\n")
    }

    #[test]
    fn test_layout_split_when_implementation_running() {
        let app = crate::app::App {
            screen: crate::app::Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: crate::app::ChangeTab::Active,
                change_deps: std::collections::HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: Some(make_impl_state("my-change", 2, 5)),
            batch: None,
            config: crate::config::TuiConfig::default(),
            config_path: std::path::PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        let rendered = render_draw(80, 14, &app);
        // Status bar should be visible at the bottom
        assert!(
            rendered.contains("my-change"),
            "Status bar should show change name"
        );
        assert!(rendered.contains("2/5"), "Status bar should show progress");
        assert!(
            rendered.contains("[S] Stop"),
            "Status bar should show stop hint"
        );
        // Main content should also be visible
        assert!(
            rendered.contains("OpenSpec TUI"),
            "Main content header should be visible"
        );
    }

    #[test]
    fn test_layout_not_split_when_no_implementation() {
        let app = crate::app::App {
            screen: crate::app::Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: crate::app::ChangeTab::Active,
                change_deps: std::collections::HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: crate::config::TuiConfig::default(),
            config_path: std::path::PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };

        let rendered = render_draw(60, 14, &app);
        // No status bar content should be present
        assert!(
            !rendered.contains("[S] Stop"),
            "Stop hint should not appear without implementation"
        );
        // Main content should use the full area
        assert!(
            rendered.contains("OpenSpec TUI"),
            "Main content header should be visible"
        );
    }

    fn render_change_list(width: u16, height: u16, tab: &crate::app::ChangeTab) -> String {
        let empty_deps = HashMap::new();
        render_change_list_with_deps(width, height, &[], tab, &empty_deps)
    }

    fn render_change_list_with_deps(
        width: u16,
        height: u16,
        changes: &[crate::data::ChangeEntry],
        tab: &crate::app::ChangeTab,
        deps: &HashMap<String, Vec<String>>,
    ) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                draw_change_list(frame, changes, 0, None, tab, deps, area);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut lines = Vec::new();
        for y in 0..height {
            let mut line = String::new();
            for x in 0..width {
                line.push_str(buffer.cell((x, y)).unwrap().symbol());
            }
            lines.push(line.trim_end().to_string());
        }
        lines.join("\n")
    }

    #[test]
    fn test_title_rendering_active_tab() {
        let rendered = render_change_list(50, 5, &crate::app::ChangeTab::Active);
        assert!(
            rendered.contains("OpenSpec TUI"),
            "Title should contain OpenSpec TUI"
        );
        assert!(
            rendered.contains("Active"),
            "Title should contain Active tab label"
        );
        assert!(
            rendered.contains("Archived"),
            "Title should contain Archived tab label"
        );
    }

    #[test]
    fn test_title_rendering_archived_tab() {
        let rendered = render_change_list(50, 5, &crate::app::ChangeTab::Archived);
        assert!(
            rendered.contains("OpenSpec TUI"),
            "Title should contain OpenSpec TUI"
        );
        assert!(
            rendered.contains("Active"),
            "Title should contain Active tab label"
        );
        assert!(
            rendered.contains("Archived"),
            "Title should contain Archived tab label"
        );
    }

    #[test]
    fn test_empty_message_active_tab() {
        let rendered = render_change_list(50, 5, &crate::app::ChangeTab::Active);
        assert!(
            rendered.contains("No active changes found"),
            "Should show active-specific empty message"
        );
    }

    #[test]
    fn test_empty_message_archived_tab() {
        let rendered = render_change_list(50, 5, &crate::app::ChangeTab::Archived);
        assert!(
            rendered.contains("No archived changes found"),
            "Should show archived-specific empty message"
        );
    }

    #[test]
    fn test_plain_text_preserves_single_newlines() {
        let content = "line one\nline two\nline three";
        let rendered = render_artifact_view_with_mode(40, 8, content, 0, true);
        assert!(
            rendered.contains("line one"),
            "First line should be visible"
        );
        assert!(
            rendered.contains("line two"),
            "Second line should be visible"
        );
        assert!(
            rendered.contains("line three"),
            "Third line should be visible"
        );
    }

    #[test]
    fn test_plain_text_preserves_separator_lines() {
        let content = "Header\n══════════════\nContent\n──────────────";
        let rendered = render_artifact_view_with_mode(40, 8, content, 0, true);
        assert!(
            rendered.contains("══════"),
            "Double-line separator should render verbatim"
        );
        assert!(
            rendered.contains("──────"),
            "Single-line separator should render verbatim"
        );
    }

    #[test]
    fn test_non_log_files_use_markdown_rendering() {
        let content = "# Header\n\nBody text";
        let rendered = render_artifact_view_with_mode(40, 8, content, 0, false);
        assert!(rendered.contains("Header"), "Header text should be visible");
        assert!(
            rendered.contains("Body text"),
            "Body text should be visible"
        );

        // Verify markdown formatting is applied (bold on header)
        let backend = TestBackend::new(40, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                draw_artifact_view(frame, "test", content, 0, false, area);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut found_bold = false;
        for x in 0..40u16 {
            let cell = buffer.cell((x, 1)).unwrap();
            if cell.symbol() == "H" && cell.modifier.contains(Modifier::BOLD) {
                found_bold = true;
            }
        }
        assert!(found_bold, "Header should be bold in markdown mode");
    }

    fn render_config_screen(
        width: u16,
        height: u16,
        command: &str,
        prompt: &str,
        cursor_position: usize,
        focused_field: &ConfigField,
    ) -> String {
        render_config_screen_with_editing(
            width,
            height,
            command,
            prompt,
            cursor_position,
            focused_field,
            false,
        )
    }

    fn render_config_screen_with_editing(
        width: u16,
        height: u16,
        command: &str,
        prompt: &str,
        cursor_position: usize,
        focused_field: &ConfigField,
        editing: bool,
    ) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                draw_config_screen(
                    frame,
                    &ConfigScreenParams {
                        command,
                        prompt,
                        post_implementation_prompt: "",
                        interactive_command: "claude",
                        run_finished_command: "",
                        cursor_position,
                        focused_field,
                        editing,
                    },
                    area,
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut lines = Vec::new();
        for y in 0..height {
            let mut line = String::new();
            for x in 0..width {
                line.push_str(buffer.cell((x, y)).unwrap().symbol());
            }
            lines.push(line.trim_end().to_string());
        }
        lines.join("\n")
    }

    #[test]
    fn test_config_screen_shows_command() {
        let rendered = render_config_screen(
            60,
            15,
            "my-tool {prompt}",
            "my prompt",
            0,
            &ConfigField::Command,
        );
        assert!(
            rendered.contains("my-tool"),
            "Command text should be visible"
        );
    }

    #[test]
    fn test_config_screen_shows_prompt() {
        let rendered = render_config_screen(
            60,
            15,
            "cmd {prompt}",
            "implement {name}",
            0,
            &ConfigField::Command,
        );
        assert!(
            rendered.contains("implement"),
            "Prompt text should be visible"
        );
    }

    #[test]
    fn test_config_screen_shows_command_title() {
        let rendered =
            render_config_screen(60, 15, "cmd {prompt}", "prompt", 0, &ConfigField::Command);
        assert!(
            rendered.contains("Command"),
            "Command title should be visible"
        );
    }

    #[test]
    fn test_config_screen_shows_prompt_title() {
        let rendered =
            render_config_screen(60, 15, "cmd {prompt}", "prompt", 0, &ConfigField::Command);
        assert!(
            rendered.contains("Prompt"),
            "Prompt title should be visible"
        );
    }

    #[test]
    fn test_config_screen_shows_keybinding_hints() {
        let rendered =
            render_config_screen(80, 15, "cmd {prompt}", "prompt", 0, &ConfigField::Command);
        assert!(rendered.contains("[Tab]"), "Tab hint should be visible");
        assert!(rendered.contains("[S] Save"), "Save hint should be visible");
        assert!(
            rendered.contains("[Esc] Cancel"),
            "Cancel hint should be visible"
        );
        assert!(
            rendered.contains("[D] Reset"),
            "Reset hint should be visible"
        );
    }

    #[test]
    fn test_config_screen_warns_missing_prompt_placeholder() {
        let rendered =
            render_config_screen(100, 15, "cmd --flag", "prompt", 0, &ConfigField::Command);
        assert!(
            rendered.contains("missing"),
            "Warning should show when {{prompt}} is missing"
        );
    }

    #[test]
    fn test_config_screen_no_warning_with_prompt_placeholder() {
        let rendered =
            render_config_screen(100, 15, "cmd {prompt}", "prompt", 0, &ConfigField::Command);
        assert!(
            !rendered.contains("missing"),
            "No warning when {{prompt}} is present"
        );
    }

    #[test]
    fn test_config_screen_shows_editor_hint_when_prompt_focused() {
        let rendered =
            render_config_screen(80, 15, "cmd {prompt}", "prompt", 0, &ConfigField::Prompt);
        assert!(
            rendered.contains("$EDITOR"),
            "Editor hint should show when prompt is focused"
        );
    }

    #[test]
    fn test_draw_full_app_with_config_screen() {
        let app = crate::app::App {
            screen: crate::app::Screen::Config {
                command: "claude --print {prompt}".to_string(),
                prompt: "implement {name}".to_string(),
                post_implementation_prompt: String::new(),
                interactive_command: "claude".to_string(),
                run_finished_command: String::new(),
                cursor_position: 0,
                focused_field: ConfigField::Command,
                editing: false,
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: crate::config::TuiConfig::default(),
            config_path: std::path::PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };
        let rendered = render_draw(60, 15, &app);
        assert!(
            rendered.contains("Command"),
            "Config screen should render in draw()"
        );
        assert!(
            rendered.contains("claude"),
            "Command text should be visible"
        );
    }

    #[test]
    fn test_config_screen_no_cursor_in_navigation_mode() {
        // In navigation mode (editing=false), no block cursor should be shown
        let backend = TestBackend::new(60, 15);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                draw_config_screen(
                    frame,
                    &ConfigScreenParams {
                        command: "cmd {prompt}",
                        prompt: "prompt",
                        post_implementation_prompt: "",
                        interactive_command: "claude",
                        run_finished_command: "",
                        cursor_position: 0,
                        focused_field: &ConfigField::Command,
                        editing: false,
                    },
                    area,
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        // In navigation mode, no cell in the command row should have bg=White (cursor indicator)
        let mut found_cursor = false;
        for x in 0..60u16 {
            let cell = buffer.cell((x, 1)).unwrap(); // row 1 is inside the Command block
            if cell.bg == Color::White {
                found_cursor = true;
            }
        }
        assert!(
            !found_cursor,
            "No cursor should be visible in navigation mode"
        );
    }

    #[test]
    fn test_config_screen_cursor_visible_in_edit_mode() {
        let backend = TestBackend::new(60, 15);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                draw_config_screen(
                    frame,
                    &ConfigScreenParams {
                        command: "cmd {prompt}",
                        prompt: "prompt",
                        post_implementation_prompt: "",
                        interactive_command: "claude",
                        run_finished_command: "",
                        cursor_position: 0,
                        focused_field: &ConfigField::Command,
                        editing: true,
                    },
                    area,
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        // In edit mode, a cell in the command row should have bg=White (cursor indicator)
        let mut found_cursor = false;
        for x in 0..60u16 {
            let cell = buffer.cell((x, 1)).unwrap();
            if cell.bg == Color::White {
                found_cursor = true;
            }
        }
        assert!(found_cursor, "Cursor should be visible in edit mode");
    }

    #[test]
    fn test_config_screen_navigation_hints() {
        let rendered = render_config_screen_with_editing(
            100,
            15,
            "cmd {prompt}",
            "prompt",
            0,
            &ConfigField::Command,
            false,
        );
        assert!(
            rendered.contains("[Enter] Edit"),
            "Enter hint should show in navigation mode"
        );
        assert!(
            rendered.contains("[Tab]"),
            "Tab hint should show in navigation mode"
        );
        assert!(
            rendered.contains("[S] Save"),
            "Save hint should show in navigation mode"
        );
        assert!(
            rendered.contains("[D] Reset"),
            "Reset hint should show in navigation mode"
        );
        assert!(
            rendered.contains("[Esc] Cancel"),
            "Cancel hint should show in navigation mode"
        );
        assert!(
            !rendered.contains("Done editing"),
            "Edit-mode hint should not show in navigation mode"
        );
    }

    #[test]
    fn test_config_screen_edit_mode_hints() {
        let rendered = render_config_screen_with_editing(
            100,
            15,
            "cmd {prompt}",
            "prompt",
            0,
            &ConfigField::Command,
            true,
        );
        assert!(
            rendered.contains("[Esc] Done editing"),
            "Done editing hint should show in edit mode"
        );
        assert!(
            !rendered.contains("[S] Save"),
            "Save hint should not show in edit mode"
        );
        assert!(
            !rendered.contains("[D] Reset"),
            "Reset hint should not show in edit mode"
        );
        assert!(
            !rendered.contains("[Enter] Edit"),
            "Enter-edit hint should not show in edit mode"
        );
    }

    #[test]
    fn test_config_screen_prompt_warning_in_both_modes() {
        // Navigation mode without {prompt}
        let rendered_nav = render_config_screen_with_editing(
            100,
            15,
            "cmd --flag",
            "prompt",
            0,
            &ConfigField::Command,
            false,
        );
        assert!(
            rendered_nav.contains("missing"),
            "Warning should show in navigation mode"
        );

        // Edit mode without {prompt}
        let rendered_edit = render_config_screen_with_editing(
            100,
            15,
            "cmd --flag",
            "prompt",
            0,
            &ConfigField::Command,
            true,
        );
        assert!(
            rendered_edit.contains("missing"),
            "Warning should show in edit mode"
        );
    }

    #[test]
    fn test_config_screen_shows_post_implementation_prompt_title() {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                draw_config_screen(
                    frame,
                    &ConfigScreenParams {
                        command: "cmd {prompt}",
                        prompt: "prompt",
                        post_implementation_prompt: "commit changes",
                        interactive_command: "claude",
                        run_finished_command: "",
                        cursor_position: 0,
                        focused_field: &ConfigField::Command,
                        editing: false,
                    },
                    area,
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut lines = Vec::new();
        for y in 0..20u16 {
            let mut line = String::new();
            for x in 0..80u16 {
                line.push_str(buffer.cell((x, y)).unwrap().symbol());
            }
            lines.push(line.trim_end().to_string());
        }
        let rendered = lines.join("\n");
        assert!(
            rendered.contains("Post-Implementation"),
            "Post-Implementation Prompt title should be visible"
        );
        assert!(
            rendered.contains("commit changes"),
            "Post-implementation prompt content should be visible"
        );
    }

    #[test]
    fn test_config_screen_shows_disabled_when_empty_post_prompt() {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                draw_config_screen(
                    frame,
                    &ConfigScreenParams {
                        command: "cmd {prompt}",
                        prompt: "prompt",
                        post_implementation_prompt: "",
                        interactive_command: "claude",
                        run_finished_command: "",
                        cursor_position: 0,
                        focused_field: &ConfigField::Command,
                        editing: false,
                    },
                    area,
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut lines = Vec::new();
        for y in 0..20u16 {
            let mut line = String::new();
            for x in 0..80u16 {
                line.push_str(buffer.cell((x, y)).unwrap().symbol());
            }
            lines.push(line.trim_end().to_string());
        }
        let rendered = lines.join("\n");
        assert!(
            rendered.contains("(disabled)"),
            "Empty post-implementation prompt should show (disabled)"
        );
    }

    #[test]
    fn test_config_screen_post_prompt_editor_hint_when_focused() {
        let backend = TestBackend::new(100, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                draw_config_screen(
                    frame,
                    &ConfigScreenParams {
                        command: "cmd {prompt}",
                        prompt: "prompt",
                        post_implementation_prompt: "",
                        interactive_command: "claude",
                        run_finished_command: "",
                        cursor_position: 0,
                        focused_field: &ConfigField::PostImplementationPrompt,
                        editing: false,
                    },
                    area,
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut lines = Vec::new();
        for y in 0..20u16 {
            let mut line = String::new();
            for x in 0..100u16 {
                line.push_str(buffer.cell((x, y)).unwrap().symbol());
            }
            lines.push(line.trim_end().to_string());
        }
        let rendered = lines.join("\n");
        assert!(
            rendered.contains("$EDITOR"),
            "Editor hint should show when post-implementation prompt is focused"
        );
    }

    // --- Dependency View Rendering Tests ---

    fn render_dependency_view(width: u16, height: u16, deps: &[String], selected: usize) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                draw_dependency_view(
                    frame,
                    "test-change",
                    deps,
                    selected,
                    &crate::data::RunMode::Normal,
                    area,
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut lines = Vec::new();
        for y in 0..height {
            let mut line = String::new();
            for x in 0..width {
                line.push_str(buffer.cell((x, y)).unwrap().symbol());
            }
            lines.push(line.trim_end().to_string());
        }
        lines.join("\n")
    }

    #[test]
    fn test_dependency_view_shows_title() {
        let deps = vec!["dep-a".to_string()];
        let rendered = render_dependency_view(60, 6, &deps, 0);
        assert!(
            rendered.contains("test-change"),
            "Change name should be in title"
        );
        assert!(
            rendered.contains("Config"),
            "Config label should be in title"
        );
        assert!(
            rendered.contains("Mode: Normal"),
            "Run mode should be in title"
        );
    }

    #[test]
    fn test_dependency_view_shows_dependencies() {
        let deps = vec!["dep-a".to_string(), "dep-b".to_string()];
        let rendered = render_dependency_view(50, 6, &deps, 0);
        assert!(
            rendered.contains("dep-a"),
            "First dependency should be visible"
        );
        assert!(
            rendered.contains("dep-b"),
            "Second dependency should be visible"
        );
    }

    #[test]
    fn test_dependency_view_shows_empty_message() {
        let deps: Vec<String> = vec![];
        let rendered = render_dependency_view(50, 6, &deps, 0);
        assert!(
            rendered.contains("No dependencies"),
            "Empty message should show"
        );
    }

    #[test]
    fn test_dependency_view_shows_keybinding_hints() {
        let deps = vec!["dep-a".to_string()];
        let rendered = render_dependency_view(80, 6, &deps, 0);
        assert!(rendered.contains("[A] Add"), "Add hint should be visible");
        assert!(
            rendered.contains("[D] Remove"),
            "Remove hint should be visible"
        );
        assert!(
            rendered.contains("[M] Toggle Mode"),
            "Toggle Mode hint should be visible"
        );
        assert!(
            rendered.contains("[Esc] Back"),
            "Back hint should be visible"
        );
    }

    #[test]
    fn test_dependency_view_selection_highlight() {
        let deps = vec!["dep-a".to_string(), "dep-b".to_string()];
        let backend = TestBackend::new(50, 6);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                draw_dependency_view(
                    frame,
                    "test-change",
                    &deps,
                    1,
                    &crate::data::RunMode::Normal,
                    area,
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();

        // Find "dep-b" in the buffer and check it has yellow foreground (selected)
        let mut found_yellow = false;
        for y in 0..6u16 {
            for x in 0..50u16 {
                let cell = buffer.cell((x, y)).unwrap();
                if cell.symbol() == "d" && x + 4 < 50 {
                    let next = buffer.cell((x + 1, y)).unwrap();
                    if next.symbol() == "e" {
                        let third = buffer.cell((x + 2, y)).unwrap();
                        let fourth = buffer.cell((x + 3, y)).unwrap();
                        if third.symbol() == "p" && fourth.symbol() == "-" {
                            let fifth = buffer.cell((x + 4, y)).unwrap();
                            if fifth.symbol() == "b" && cell.fg == Color::Yellow {
                                found_yellow = true;
                            }
                        }
                    }
                }
            }
        }
        assert!(
            found_yellow,
            "Selected dependency should be highlighted in yellow"
        );
    }

    fn render_dependency_add(
        width: u16,
        height: u16,
        changes: &[String],
        selected: usize,
    ) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                draw_dependency_add(frame, "test-change", changes, selected, area);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut lines = Vec::new();
        for y in 0..height {
            let mut line = String::new();
            for x in 0..width {
                line.push_str(buffer.cell((x, y)).unwrap().symbol());
            }
            lines.push(line.trim_end().to_string());
        }
        lines.join("\n")
    }

    #[test]
    fn test_dependency_add_shows_title() {
        let changes = vec!["change-a".to_string()];
        let rendered = render_dependency_add(50, 6, &changes, 0);
        assert!(
            rendered.contains("Add Dependency"),
            "Add Dependency should be in title"
        );
    }

    #[test]
    fn test_dependency_add_shows_available_changes() {
        let changes = vec!["change-a".to_string(), "change-b".to_string()];
        let rendered = render_dependency_add(50, 6, &changes, 0);
        assert!(
            rendered.contains("change-a"),
            "First change should be visible"
        );
        assert!(
            rendered.contains("change-b"),
            "Second change should be visible"
        );
    }

    #[test]
    fn test_dependency_add_shows_keybinding_hints() {
        let changes = vec!["change-a".to_string()];
        let rendered = render_dependency_add(60, 6, &changes, 0);
        assert!(
            rendered.contains("[Enter] Select"),
            "Select hint should be visible"
        );
        assert!(
            rendered.contains("[Esc] Cancel"),
            "Cancel hint should be visible"
        );
    }

    #[test]
    fn test_change_list_shows_inline_dependencies() {
        let changes = vec![
            crate::data::ChangeEntry {
                name: "add-api".to_string(),
                completed_tasks: 2,
                total_tasks: 5,
            },
            crate::data::ChangeEntry {
                name: "add-auth".to_string(),
                completed_tasks: 0,
                total_tasks: 7,
            },
        ];
        let mut deps = HashMap::new();
        deps.insert(
            "add-auth".to_string(),
            vec!["add-api".to_string(), "add-user".to_string()],
        );
        let rendered =
            render_change_list_with_deps(80, 6, &changes, &crate::app::ChangeTab::Active, &deps);
        assert!(
            rendered.contains("<- add-api, add-user"),
            "Dependencies should be displayed inline: {}",
            rendered,
        );
    }

    #[test]
    fn test_change_list_no_deps_no_arrow() {
        let changes = vec![crate::data::ChangeEntry {
            name: "simple-change".to_string(),
            completed_tasks: 1,
            total_tasks: 3,
        }];
        let deps = HashMap::new();
        let rendered =
            render_change_list_with_deps(60, 5, &changes, &crate::app::ChangeTab::Active, &deps);
        assert!(
            !rendered.contains("<-"),
            "No dependency arrow should appear for changes without deps: {}",
            rendered,
        );
    }

    #[test]
    fn test_change_list_deps_truncated_when_long() {
        let changes = vec![crate::data::ChangeEntry {
            name: "my-change".to_string(),
            completed_tasks: 0,
            total_tasks: 1,
        }];
        let mut deps = HashMap::new();
        deps.insert(
            "my-change".to_string(),
            vec![
                "very-long-dependency-name-one".to_string(),
                "very-long-dependency-name-two".to_string(),
                "very-long-dependency-name-three".to_string(),
            ],
        );
        // Use a narrow width to force truncation
        let rendered =
            render_change_list_with_deps(50, 5, &changes, &crate::app::ChangeTab::Active, &deps);
        // Should either show truncated "..." or not show deps at all if no space
        let has_ellipsis = rendered.contains("...");
        let has_no_arrow = !rendered.contains("<-");
        assert!(
            has_ellipsis || has_no_arrow,
            "Long deps should be truncated with ... or omitted: {}",
            rendered,
        );
    }

    fn render_dependency_graph(width: u16, height: u16, graph_text: &str, scroll: usize) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                draw_dependency_graph(frame, graph_text, scroll, area);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut lines = Vec::new();
        for y in 0..height {
            let mut line = String::new();
            for x in 0..width {
                line.push_str(buffer.cell((x, y)).unwrap().symbol());
            }
            lines.push(line.trim_end().to_string());
        }
        lines.join("\n")
    }

    #[test]
    fn test_draw_dependency_graph_shows_content() {
        let graph = "root\n├── child-a\n└── child-b";
        let rendered = render_dependency_graph(40, 8, graph, 0);
        assert!(rendered.contains("Dependency Graph"), "Should show title");
        assert!(rendered.contains("root"), "Should show graph content");
        assert!(rendered.contains("child-a"), "Should show child-a");
        assert!(rendered.contains("child-b"), "Should show child-b");
    }

    #[test]
    fn test_draw_dependency_graph_shows_scroll_position() {
        let graph = "line1\nline2\nline3";
        let rendered = render_dependency_graph(40, 6, graph, 1);
        assert!(rendered.contains("[2/3]"), "Should show scroll position");
    }

    #[test]
    fn test_draw_dependency_graph_shows_back_hint() {
        let graph = "root";
        let rendered = render_dependency_graph(40, 6, graph, 0);
        assert!(rendered.contains("[Esc] Back"), "Should show back hint");
    }

    // --- RunAllSelection rendering tests ---

    fn render_run_all_selection(
        width: u16,
        height: u16,
        entries: &[crate::app::RunAllEntry],
        selected: usize,
        error: Option<&str>,
    ) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                draw_run_all_selection(frame, entries, selected, error, area);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut lines = Vec::new();
        for y in 0..height {
            let mut line = String::new();
            for x in 0..width {
                line.push_str(buffer.cell((x, y)).unwrap().symbol());
            }
            lines.push(line.trim_end().to_string());
        }
        lines.join("\n")
    }

    #[test]
    fn test_run_all_selection_shows_title() {
        let entries = vec![crate::app::RunAllEntry {
            change_name: "test".to_string(),
            included: true,
            blocked: false,
            blocked_by: None,
            completed_tasks: 1,
            total_tasks: 3,
        }];
        let rendered = render_run_all_selection(60, 6, &entries, 0, None);
        assert!(rendered.contains("Run All"), "Should show Run All title");
    }

    #[test]
    fn test_run_all_selection_shows_checkboxes() {
        let entries = vec![
            crate::app::RunAllEntry {
                change_name: "included-change".to_string(),
                included: true,
                blocked: false,
                blocked_by: None,
                completed_tasks: 1,
                total_tasks: 3,
            },
            crate::app::RunAllEntry {
                change_name: "excluded-change".to_string(),
                included: false,
                blocked: false,
                blocked_by: None,
                completed_tasks: 0,
                total_tasks: 5,
            },
        ];
        let rendered = render_run_all_selection(60, 6, &entries, 0, None);
        assert!(
            rendered.contains("[x]"),
            "Should show checked checkbox for included"
        );
        assert!(
            rendered.contains("[ ]"),
            "Should show unchecked checkbox for excluded"
        );
    }

    #[test]
    fn test_run_all_selection_shows_blocked() {
        let entries = vec![crate::app::RunAllEntry {
            change_name: "blocked-change".to_string(),
            included: false,
            blocked: true,
            blocked_by: Some("some-dep".to_string()),
            completed_tasks: 0,
            total_tasks: 5,
        }];
        let rendered = render_run_all_selection(80, 6, &entries, 0, None);
        assert!(rendered.contains("[~]"), "Should show blocked checkbox");
        assert!(
            rendered.contains("blocked by"),
            "Should show blocked reason"
        );
    }

    #[test]
    fn test_run_all_selection_shows_keybinding_hints() {
        let entries = vec![crate::app::RunAllEntry {
            change_name: "test".to_string(),
            included: true,
            blocked: false,
            blocked_by: None,
            completed_tasks: 0,
            total_tasks: 3,
        }];
        let rendered = render_run_all_selection(80, 6, &entries, 0, None);
        assert!(
            rendered.contains("[Space] Toggle"),
            "Should show toggle hint"
        );
        assert!(rendered.contains("[Enter] Start"), "Should show start hint");
        assert!(rendered.contains("[Esc] Cancel"), "Should show cancel hint");
    }

    #[test]
    fn test_run_all_selection_shows_error() {
        let entries = vec![crate::app::RunAllEntry {
            change_name: "test".to_string(),
            included: false,
            blocked: false,
            blocked_by: None,
            completed_tasks: 0,
            total_tasks: 3,
        }];
        let rendered = render_run_all_selection(80, 6, &entries, 0, Some("No changes selected."));
        assert!(
            rendered.contains("No changes selected"),
            "Should show error message"
        );
    }

    #[test]
    fn test_run_all_selection_shows_progress() {
        let entries = vec![crate::app::RunAllEntry {
            change_name: "test".to_string(),
            included: true,
            blocked: false,
            blocked_by: None,
            completed_tasks: 2,
            total_tasks: 7,
        }];
        let rendered = render_run_all_selection(60, 6, &entries, 0, None);
        assert!(rendered.contains("(2/7)"), "Should show progress");
    }

    #[test]
    fn test_run_all_selection_empty_shows_message() {
        let entries: Vec<crate::app::RunAllEntry> = vec![];
        let rendered = render_run_all_selection(60, 6, &entries, 0, None);
        assert!(
            rendered.contains("No eligible changes"),
            "Should show empty message"
        );
    }

    #[test]
    fn test_config_screen_shows_interactive_command_field() {
        let backend = TestBackend::new(80, 25);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                draw_config_screen(
                    frame,
                    &ConfigScreenParams {
                        command: "cmd {prompt}",
                        prompt: "prompt",
                        post_implementation_prompt: "",
                        interactive_command: "aider --model gpt4",
                        run_finished_command: "",
                        cursor_position: 0,
                        focused_field: &ConfigField::Command,
                        editing: false,
                    },
                    area,
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut lines = Vec::new();
        for y in 0..25u16 {
            let mut line = String::new();
            for x in 0..80u16 {
                line.push_str(buffer.cell((x, y)).unwrap().symbol());
            }
            lines.push(line.trim_end().to_string());
        }
        let rendered = lines.join("\n");
        assert!(
            rendered.contains("Interactive Command"),
            "Interactive Command field should be visible"
        );
        assert!(
            rendered.contains("aider --model gpt4"),
            "Interactive command value should be visible"
        );
    }

    #[test]
    fn test_change_list_shows_interactive_hint_on_active_tab() {
        let app = crate::app::App {
            screen: crate::app::Screen::ChangeList {
                changes: vec![crate::data::ChangeEntry {
                    name: "test-change".to_string(),
                    completed_tasks: 1,
                    total_tasks: 3,
                }],
                selected: 0,
                error: None,
                tab: crate::app::ChangeTab::Active,
                change_deps: std::collections::HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: crate::config::TuiConfig::default(),
            config_path: std::path::PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };
        let rendered = render_draw(80, 10, &app);
        assert!(
            rendered.contains("[I] Interactive"),
            "Active tab should show [I] Interactive hint"
        );
    }

    #[test]
    fn test_change_list_hides_interactive_hint_on_archived_tab() {
        let app = crate::app::App {
            screen: crate::app::Screen::ChangeList {
                changes: vec![],
                selected: 0,
                error: None,
                tab: crate::app::ChangeTab::Archived,
                change_deps: std::collections::HashMap::new(),
            },
            screen_stack: Vec::new(),
            should_quit: false,
            launch_interactive: false,
            implementation: None,
            batch: None,
            config: crate::config::TuiConfig::default(),
            config_path: std::path::PathBuf::from("/tmp/openspec-tui-test-config.yaml"),
        };
        let rendered = render_draw(80, 10, &app);
        assert!(
            !rendered.contains("[I] Interactive"),
            "Archived tab should not show [I] Interactive hint"
        );
    }
}
