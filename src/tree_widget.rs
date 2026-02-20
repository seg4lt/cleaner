use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, StatefulWidget, Widget},
};

use crate::tree::Tree;

// Lazygit-inspired colors
const COLOR_SELECTED_BG: Color = Color::Blue;
const COLOR_MARKED: Color = Color::Red;
const COLOR_DEFAULT: Color = Color::Reset;
const COLOR_BORDER_ACTIVE: Color = Color::Green;
const COLOR_BORDER_INACTIVE: Color = Color::Reset;
const COLOR_TEXT: Color = Color::Reset;
const COLOR_DIR: Color = Color::Blue;
const COLOR_HELP_TEXT: Color = Color::Blue;

/// State for the tree widget
#[derive(Debug, Default)]
pub struct TreeWidgetState {
    pub selected_index: usize,
    pub show_confirmation: bool,
    pub confirmation_message: String,
}

impl TreeWidgetState {
    pub fn new() -> Self {
        Self {
            selected_index: 0,
            show_confirmation: false,
            confirmation_message: String::new(),
        }
    }

    pub fn select_next(&mut self, max_index: usize) {
        if self.selected_index < max_index.saturating_sub(1) {
            self.selected_index += 1;
        }
    }

    pub fn select_previous(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }
}

/// The tree widget for rendering the file tree
pub struct TreeWidget<'a> {
    tree: &'a Tree,
    title: String,
}

impl<'a> TreeWidget<'a> {
    pub fn new(tree: &'a Tree, title: String) -> Self {
        Self { tree, title }
    }
}

impl<'a> StatefulWidget for TreeWidget<'a> {
    type State = TreeWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        // Render the main block with border
        let block = Block::default()
            .title(self.title.clone())
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COLOR_BORDER_ACTIVE));

        let inner_area = block.inner(area);
        block.render(area, buf);

        // Get visible nodes
        let visible_nodes: Vec<_> = self.tree.flatten_visible();

        // Render tree items
        let max_items = inner_area.height as usize;
        let start_index = if state.selected_index >= max_items {
            state.selected_index - max_items + 1
        } else {
            0
        };

        for (i, node) in visible_nodes
            .iter()
            .skip(start_index)
            .take(max_items)
            .enumerate()
        {
            let row = inner_area.y + i as u16;
            if row >= inner_area.y + inner_area.height {
                break;
            }

            let actual_index = start_index + i;
            let is_selected = actual_index == state.selected_index;

            // Build the line content
            let mut spans = Vec::new();

            // Indentation
            let indent = "  ".repeat(node.depth);
            spans.push(Span::styled(indent, Style::default().fg(COLOR_TEXT)));

            // Radio button (selected or not)
            let radio = if is_selected { "◉ " } else { "○ " };
            spans.push(Span::styled(radio, Style::default().fg(COLOR_TEXT)));

            // Mark indicator (if marked for deletion)
            if node.is_marked {
                spans.push(Span::styled("✗ ", Style::default().fg(COLOR_MARKED)));
            } else {
                spans.push(Span::styled("  ", Style::default()));
            }

            // Directory/File icon and name
            let name_style = if node.is_dir {
                Style::default().fg(COLOR_DIR)
            } else {
                Style::default().fg(COLOR_TEXT)
            };

            let icon = if node.is_dir {
                if node.is_expanded {
                    "📂 "
                } else {
                    "📁 "
                }
            } else {
                "📄 "
            };

            spans.push(Span::styled(icon, name_style));
            spans.push(Span::styled(node.label.clone(), name_style));

            // Create the line
            let line = Line::from(spans);

            // Render with background if selected
            let style = if is_selected {
                Style::default().bg(COLOR_SELECTED_BG)
            } else {
                Style::default()
            };

            // Clear the line first
            for x in inner_area.x..inner_area.x + inner_area.width {
                buf[(x, row)].set_char(' ').set_style(style);
            }

            // Render the line
            let mut current_x = inner_area.x;
            for span in line.iter() {
                let content = span.content.as_ref();
                for ch in content.chars() {
                    if current_x >= inner_area.x + inner_area.width {
                        break;
                    }
                    let cell_style = if is_selected {
                        span.style.patch(style)
                    } else {
                        span.style
                    };
                    buf[(current_x, row)].set_char(ch).set_style(cell_style);
                    current_x += 1;
                }
            }
        }

        // Render confirmation dialog if needed
        if state.show_confirmation {
            render_confirmation_dialog(area, buf, &state.confirmation_message);
        }
    }
}

fn render_confirmation_dialog(area: Rect, buf: &mut Buffer, message: &str) {
    let dialog_width = 50;
    let dialog_height = 7;

    let dialog_x = (area.width.saturating_sub(dialog_width)) / 2;
    let dialog_y = (area.height.saturating_sub(dialog_height)) / 2;

    let dialog_area = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

    // Clear the area
    Clear.render(dialog_area, buf);

    // Render dialog block
    let block = Block::default()
        .title("Confirm")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(COLOR_BORDER_ACTIVE));

    let inner_area = block.inner(dialog_area);
    block.render(dialog_area, buf);

    // Render message
    let text = Paragraph::new(message)
        .alignment(Alignment::Center)
        .wrap(ratatui::widgets::Wrap { trim: true });

    let text_area = Rect::new(
        inner_area.x,
        inner_area.y + 1,
        inner_area.width,
        inner_area.height - 2,
    );
    text.render(text_area, buf);

    // Render help text
    let help = Paragraph::new("(y)es / (n)o")
        .alignment(Alignment::Center)
        .style(Style::default().fg(COLOR_HELP_TEXT));

    let help_area = Rect::new(
        inner_area.x,
        inner_area.y + inner_area.height - 1,
        inner_area.width,
        1,
    );
    help.render(help_area, buf);
}

/// Help widget for the bottom of the screen
pub struct HelpWidget;

impl Widget for HelpWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let help_text =
            "j/k: Navigate | h/l: Collapse/Expand | Space: Mark | Enter: Delete | q: Quit";
        let help = Paragraph::new(help_text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(COLOR_HELP_TEXT));
        help.render(area, buf);
    }
}

/// Results widget for showing deletion results
pub struct ResultsWidget {
    pub deleted: Vec<String>,
    pub failed: Vec<(String, String)>,
}

impl Widget for ResultsWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .title("Deletion Results")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COLOR_BORDER_ACTIVE));

        let inner_area = block.inner(area);
        block.render(area, buf);

        let mut lines = Vec::new();

        // Show successfully deleted
        if !self.deleted.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(
                    "✓ Deleted: ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{}", self.deleted.len()),
                    Style::default().fg(Color::Green),
                ),
            ]));
            for item in &self.deleted {
                lines.push(Line::from(Span::styled(
                    format!("  {}", item),
                    Style::default().fg(COLOR_TEXT),
                )));
            }
            lines.push(Line::from(""));
        }

        // Show failed deletions
        if !self.failed.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(
                    "✗ Failed: ",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{}", self.failed.len()),
                    Style::default().fg(Color::Red),
                ),
            ]));
            for (item, error) in &self.failed {
                lines.push(Line::from(vec![
                    Span::styled(format!("  {}", item), Style::default().fg(COLOR_TEXT)),
                    Span::styled(format!(" - {}", error), Style::default().fg(Color::Red)),
                ]));
            }
        }

        if lines.is_empty() {
            lines.push(Line::from(Span::styled(
                "No items were deleted.",
                Style::default().fg(COLOR_TEXT),
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Press any key to exit...",
            Style::default().fg(COLOR_HELP_TEXT),
        )));

        let text = ratatui::text::Text::from(lines);
        let paragraph = Paragraph::new(text).wrap(ratatui::widgets::Wrap { trim: true });

        paragraph.render(inner_area, buf);
    }
}
