use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

pub fn render(f: &mut ratatui::Frame) {
    let area = centered_rect(64, 66, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Keybindings ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let lines = vec![
        Line::from(Span::styled(
            "Navigation",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("  Tab: switch tab"),
        Line::from("  j / k or Up / Down: move selection"),
        Line::from("  Space: toggle ticket or group selection"),
        Line::from("  A: select all visible tickets"),
        Line::from("  u: clear selected tickets"),
        Line::from("  B: open bulk actions (move/assign)"),
        Line::from("  Enter: open ticket detail"),
        Line::from("  z: fold/unfold group"),
        Line::from("  Z: fold/unfold all groups"),
        Line::from(""),
        Line::from(Span::styled(
            "Filtering (My Work + Team)",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("  d: toggle Done tickets"),
        Line::from("  p: focus In Progress"),
        Line::from("  w: focus Ready for Work"),
        Line::from("  n: focus Needs Triage"),
        Line::from("  v: focus In Review"),
        Line::from("  /: search (tickets, labels, and team member names)"),
        Line::from("  (while searching) Up/Down or Ctrl+j/Ctrl+k: navigate"),
        Line::from("  Unassigned tab: tickets are grouped by epic"),
        Line::from(""),
        Line::from(Span::styled(
            "Actions",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("  c: create ticket"),
        Line::from("  r: refresh tickets"),
        Line::from(""),
        Line::from(Span::styled(
            "Detail View",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("  Esc: close detail"),
        Line::from("  Up / Down: scroll detail"),
        Line::from("  o: open ticket in browser"),
        Line::from("  m: move ticket"),
        Line::from("  C: add comment"),
        Line::from("  a: assign/reassign ticket"),
        Line::from("  e: edit summary and labels"),
        Line::from("  h: view activity history"),
        Line::from("  (in move picker) j/k or Up/Down: choose status"),
        Line::from("  (in move picker) p/w/n/t/v/b/c: choose + confirm prompt"),
        Line::from("  (in move picker) Shift+key: move immediately"),
        Line::from("  (in move picker) Enter or y: confirm pending move"),
        Line::from(""),
        Line::from(Span::styled(
            "Filters Tab",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("  j / k: navigate filters or results"),
        Line::from("  Tab: switch to results / next tab"),
        Line::from("  Shift+Tab: switch back to sidebar"),
        Line::from("  Enter: run filter (sidebar) / open ticket (results)"),
        Line::from("  Space/A/u/B: select + bulk actions (results pane)"),
        Line::from("  n: new filter"),
        Line::from("  e: edit selected filter"),
        Line::from("  x: delete selected filter"),
        Line::from(""),
        Line::from(Span::styled(
            "Press ? or Esc to close",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let body = Paragraph::new(lines).block(Block::default());
    f.render_widget(body, inner);
}
