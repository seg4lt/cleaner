use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, StatefulWidget, Widget},
};

use crate::tree::Tree;

const COLOR_SELECTED_BG: Color = Color::Blue;
const COLOR_BORDER_ACTIVE: Color = Color::Green;
const COLOR_TEXT: Color = Color::Reset;
const COLOR_DIR: Color = Color::Blue;
const COLOR_HELP_TEXT: Color = Color::Blue;
const COLOR_TREE_LINES: Color = Color::DarkGray;
const COLOR_MARKED: Color = Color::Red;

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
        let block = Block::default()
            .title(self.title.clone())
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COLOR_BORDER_ACTIVE));

        let inner_area = block.inner(area);
        block.render(area, buf);

        let visible_nodes: Vec<_> = self.tree.flatten_visible();

        // Ensure selected_index is always valid
        if !visible_nodes.is_empty() && state.selected_index >= visible_nodes.len() {
            state.selected_index = visible_nodes.len() - 1;
        }

        let max_items = inner_area.height as usize;
        let half_height = max_items / 2;

        let start_index = if visible_nodes.len() <= max_items {
            0
        } else if state.selected_index <= half_height {
            0
        } else if state.selected_index >= visible_nodes.len() - half_height {
            visible_nodes.len() - max_items
        } else {
            state.selected_index - half_height
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

            let mut spans = Vec::new();

            let tree_lines = calculate_tree_lines(&visible_nodes, actual_index);
            spans.push(Span::styled(
                tree_lines,
                Style::default().fg(COLOR_TREE_LINES),
            ));

            let checkbox = if node.is_marked { "☑ " } else { "☐ " };
            spans.push(Span::styled(checkbox, Style::default().fg(COLOR_TEXT)));

            let name_style = if node.is_dir {
                Style::default().fg(COLOR_DIR)
            } else {
                Style::default().fg(COLOR_TEXT)
            };

            spans.push(Span::styled(node.label.clone(), name_style));

            let line = Line::from(spans);

            let style = if is_selected {
                Style::default().bg(COLOR_SELECTED_BG)
            } else {
                Style::default()
            };

            for x in inner_area.x..inner_area.x + inner_area.width {
                buf[(x, row)].set_char(' ').set_style(style);
            }

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

    Clear.render(dialog_area, buf);

    let block = Block::default()
        .title("Confirm")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(COLOR_BORDER_ACTIVE));

    let inner_area = block.inner(dialog_area);
    block.render(dialog_area, buf);

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

fn calculate_tree_lines(visible_nodes: &[&crate::tree::TreeNode], current_index: usize) -> String {
    let node = &visible_nodes[current_index];
    let depth = node.depth;

    if depth == 0 {
        return String::new();
    }

    let mut lines = String::new();

    for d in 0..depth - 1 {
        let parent_at_depth = find_parent_at_depth(visible_nodes, current_index, d);

        if let Some(parent_idx) = parent_at_depth {
            if has_more_siblings(visible_nodes, parent_idx, current_index) {
                lines.push('│');
                lines.push(' ');
            } else {
                lines.push(' ');
                lines.push(' ');
            }
        } else {
            lines.push(' ');
            lines.push(' ');
        }
    }

    let parent_at_last_depth = find_parent_at_depth(visible_nodes, current_index, depth - 1);
    if let Some(parent_idx) = parent_at_last_depth {
        if has_more_siblings(visible_nodes, parent_idx, current_index) {
            lines.push_str("├─");
        } else {
            lines.push_str("└─");
        }
    } else {
        lines.push_str("──");
    }

    lines.push(' ');
    lines
}

fn find_parent_at_depth(
    visible_nodes: &[&crate::tree::TreeNode],
    node_index: usize,
    target_depth: usize,
) -> Option<usize> {
    let node = visible_nodes[node_index];

    for i in (0..node_index).rev() {
        let candidate = visible_nodes[i];
        if candidate.depth == target_depth && is_ancestor(candidate, node) {
            return Some(i);
        }
    }

    None
}

fn is_ancestor(parent: &crate::tree::TreeNode, child: &crate::tree::TreeNode) -> bool {
    if parent.id.is_empty() {
        return true;
    }
    child.id.starts_with(&parent.id) && child.id.len() > parent.id.len()
}

fn has_more_siblings(
    visible_nodes: &[&crate::tree::TreeNode],
    parent_index: usize,
    current_child_index: usize,
) -> bool {
    let parent = visible_nodes[parent_index];
    let parent_depth = parent.depth;
    let child_depth = parent_depth + 1;

    let mut found_current = false;
    let mut has_more = false;

    for i in (parent_index + 1)..visible_nodes.len() {
        let node = visible_nodes[i];

        if node.depth <= parent_depth {
            break;
        }

        if node.depth == child_depth && is_direct_child(parent, node) {
            if i == current_child_index {
                found_current = true;
            } else if found_current {
                has_more = true;
                break;
            }
        }
    }

    has_more
}

fn is_direct_child(parent: &crate::tree::TreeNode, child: &crate::tree::TreeNode) -> bool {
    if child.depth != parent.depth + 1 {
        return false;
    }

    let parent_prefix = if parent.id.is_empty() {
        String::new()
    } else {
        format!("{}/", parent.id)
    };

    if child.id.starts_with(&parent_prefix) {
        let remainder = &child.id[parent_prefix.len()..];
        !remainder.contains('/')
    } else if parent.id.is_empty() {
        !child.id.contains('/')
    } else {
        false
    }
}
