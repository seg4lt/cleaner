use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, StatefulWidget, Widget},
};

use crate::tree::{NodeAction, Tree};

const COLOR_SELECTED_BG: Color = Color::Blue;
const COLOR_SELECTED_FG: Color = Color::White;
const COLOR_BORDER_ACTIVE: Color = Color::Green;
const COLOR_TEXT: Color = Color::Reset;
const COLOR_DIR: Color = Color::Blue;
const COLOR_HELP_TEXT: Color = Color::Blue;
const COLOR_TREE_LINES: Color = Color::DarkGray;
const COLOR_MARKED: Color = Color::Red;
const COLOR_ACTION_CLEAN: Color = Color::Green;
const COLOR_ACTION_DELETE: Color = Color::Red;

#[derive(Debug, Default)]
pub struct TreeWidgetState {
    pub selected_index: usize,
    pub show_confirmation: bool,
    pub confirmation_message: String,
    pub loading_message: Option<String>,
    pub loading_frame: usize,
}

impl TreeWidgetState {
    pub fn new() -> Self {
        Self {
            selected_index: 0,
            show_confirmation: false,
            confirmation_message: String::new(),
            loading_message: None,
            loading_frame: 0,
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

            let (action_text, action_style) = if !node.can_be_marked() {
                ("         ".to_string(), Style::default().fg(COLOR_TEXT))
            } else {
                match node.action {
                    NodeAction::None => ("[      ] ".to_string(), Style::default().fg(Color::DarkGray)),
                    NodeAction::Clean => (
                        "[clean ] ".to_string(),
                        Style::default().fg(COLOR_ACTION_CLEAN),
                    ),
                    NodeAction::Delete => (
                        "[delete] ".to_string(),
                        Style::default().fg(COLOR_ACTION_DELETE),
                    ),
                }
            };
            spans.push(Span::styled(action_text, action_style));

            let name_style = if node.is_dir {
                Style::default().fg(COLOR_DIR)
            } else {
                Style::default().fg(COLOR_TEXT)
            };

            spans.push(Span::styled(node.label.clone(), name_style));

            // In repo-list mode, show the full repo path in muted text.
            if node.depth == 0 {
                spans.push(Span::styled(" ", Style::default()));
                spans.push(Span::styled(
                    format!("{}", node.repo_path.display()),
                    Style::default().fg(Color::DarkGray),
                ));
            }

            // Hide synthetic root sizes in repo-list mode (they're not useful here)
            let size_str = node.size_str();
            if node.depth > 0 && !size_str.is_empty() {
                spans.push(Span::styled(" ", Style::default()));
                spans.push(Span::styled(
                    format!("({})", size_str),
                    Style::default().fg(Color::DarkGray),
                ));
            }

            let line = Line::from(spans);

            let style = if is_selected {
                Style::default().bg(COLOR_SELECTED_BG).fg(COLOR_SELECTED_FG)
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

        if let Some(message) = &state.loading_message {
            render_loading_dialog(area, buf, message, state.loading_frame);
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

fn render_loading_dialog(area: Rect, buf: &mut Buffer, message: &str, frame: usize) {
    const SPINNER: &[char] = &['|', '/', '-', '\\'];
    let spinner = SPINNER[frame % SPINNER.len()];

    let dialog_width = 60;
    let dialog_height = 8;

    let dialog_x = (area.width.saturating_sub(dialog_width)) / 2;
    let dialog_y = (area.height.saturating_sub(dialog_height)) / 2;
    let dialog_area = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

    Clear.render(dialog_area, buf);

    let block = Block::default()
        .title("Running")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(COLOR_BORDER_ACTIVE));

    let inner_area = block.inner(dialog_area);
    block.render(dialog_area, buf);

    let text = Paragraph::new(format!("{} {}", spinner, message))
        .alignment(Alignment::Center)
        .wrap(ratatui::widgets::Wrap { trim: true });

    let text_area = Rect::new(
        inner_area.x,
        inner_area.y,
        inner_area.width,
        inner_area.height - 3,
    );
    text.render(text_area, buf);

    let bar_width = inner_area.width.saturating_sub(8) as usize;
    let bar = format!("[{}]", animated_bar(bar_width, frame));
    let bar_paragraph = Paragraph::new(bar)
        .alignment(Alignment::Center)
        .style(Style::default().fg(COLOR_HELP_TEXT));
    let bar_area = Rect::new(inner_area.x, inner_area.y + inner_area.height - 2, inner_area.width, 1);
    bar_paragraph.render(bar_area, buf);
}

fn animated_bar(width: usize, frame: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let mut chars = vec!['-'; width];
    let segment_len = (width / 4).max(1);
    let cycle = width + segment_len;
    let offset = frame % cycle;

    for i in 0..segment_len {
        let pos = offset + i;
        if pos < width {
            chars[pos] = '=';
        }
    }

    chars.into_iter().collect()
}

pub struct HelpWidget;

impl Widget for HelpWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let help_text =
            "j/k: Navigate | Space: Cycle [clean/delete] | m: Mode | r: Refresh | Enter: Run | q: Quit";
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
            .title("Run Results")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COLOR_BORDER_ACTIVE));

        let inner_area = block.inner(area);
        block.render(area, buf);

        let mut lines = Vec::new();

        if !self.deleted.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(
                    "✓ Succeeded: ",
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
                "No commands were run.",
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

    // Build the prefix by checking each ancestor level
    for d in 0..depth {
        let ancestor_at_d = find_ancestor_at_depth(visible_nodes, current_index, d);

        if let Some(ancestor_idx) = ancestor_at_d {
            // Check if this ancestor has more children after the current node
            let has_more =
                ancestor_has_more_children_after(visible_nodes, ancestor_idx, current_index);

            if d == depth - 1 {
                // Last level - branch connector
                if has_more {
                    lines.push_str("├── ");
                } else {
                    lines.push_str("└── ");
                }
            } else {
                // Intermediate level
                if has_more {
                    lines.push_str("│   ");
                } else {
                    lines.push_str("    ");
                }
            }
        }
    }

    lines
}

fn find_ancestor_at_depth(
    visible_nodes: &[&crate::tree::TreeNode],
    node_index: usize,
    target_depth: usize,
) -> Option<usize> {
    let node = visible_nodes[node_index];

    // Walk backwards to find ancestor at target_depth
    for i in (0..node_index).rev() {
        let candidate = visible_nodes[i];
        if candidate.depth == target_depth {
            // Verify this is actually an ancestor by checking path
            if is_ancestor_of_path(candidate, node) {
                return Some(i);
            }
        }
    }

    None
}

fn is_ancestor_of_path(parent: &crate::tree::TreeNode, child: &crate::tree::TreeNode) -> bool {
    if parent.id.is_empty() {
        return true; // Root is ancestor of all
    }

    // Child's path should start with parent's path + "/" or be equal
    let parent_prefix = format!("{}/", parent.id);
    child.id.starts_with(&parent_prefix) || child.id == parent.id
}

fn ancestor_has_more_children_after(
    visible_nodes: &[&crate::tree::TreeNode],
    ancestor_idx: usize,
    current_idx: usize,
) -> bool {
    let ancestor = visible_nodes[ancestor_idx];
    let current = visible_nodes[current_idx];

    // Find all direct children of this ancestor
    let mut children_ranges: Vec<(usize, usize)> = Vec::new(); // (start, end) indices for each child
    let mut current_child_start: Option<usize> = None;

    for i in (ancestor_idx + 1)..visible_nodes.len() {
        let node = visible_nodes[i];

        // If we've gone past the ancestor's subtree, stop
        if node.depth <= ancestor.depth {
            break;
        }

        // Direct child of ancestor
        if node.depth == ancestor.depth + 1 {
            if let Some(start) = current_child_start {
                children_ranges.push((start, i - 1));
            }
            current_child_start = Some(i);
        }
    }

    // Close the last child range
    if let Some(start) = current_child_start {
        // Find where this child's subtree ends
        let mut end = start;
        for i in (start + 1)..visible_nodes.len() {
            if visible_nodes[i].depth <= ancestor.depth + 1 {
                break;
            }
            end = i;
        }
        children_ranges.push((start, end));
    }

    // Check which child contains current_idx
    for (i, (start, end)) in children_ranges.iter().enumerate() {
        if *start <= current_idx && current_idx <= *end {
            // Found the child containing current - check if there are more children after
            return i < children_ranges.len() - 1;
        }
    }

    false
}
