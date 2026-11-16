//! The ticket detail: a pane beside the list at 120 columns or wider, in place of the list below
//! that. It shows the ticket's fields, its description rendered from Jira wiki markup and its
//! comments, scrolled by the keys in `main`'s `handle_detail_keys`; the move picker and the
//! forms dock under it (`dock`).

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
};

use crate::app::{App, DetailMode};
use crate::cache::{ActivityKind, Epic, StatusRules, Ticket};
use crate::mouse::Target;
use crate::style;
use crate::views::common::{epic_bar, fit, truncate, EPIC_BAR_WIDTH};
use crate::widgets::activity::format_timestamp;
use crate::widgets::{form, markup};

/// Width of the field names ("Assignee", ...) in the header.
const FIELD_WIDTH: usize = 10;

/// The narrowest content area the detail sits beside the list in. Narrower, it replaces it.
pub const SPLIT_MIN_WIDTH: u16 = 120;
/// Cells of the list beside the detail, which shows only keys and summaries.
pub const LIST_WIDTH: u16 = 50;

/// The hints under the detail while it shows a ticket.
const VIEW_HINTS: &[(&str, &str)] = &[
    ("↑↓", "scroll"),
    ("←→", "prev/next"),
    ("m", "move"),
    ("C", "comment"),
    ("a", "assign"),
    ("e", "edit"),
    ("h", "activity"),
    ("o", "browser"),
    ("z", "zoom"),
    ("Esc", "close"),
];

const EPIC_HINTS: &[(&str, &str)] = &[
    ("↑↓", "scroll"),
    ("←→", "prev/next"),
    ("o", "browser"),
    ("z", "zoom"),
    ("Esc", "close"),
];

const HISTORY_HINTS: &[(&str, &str)] = &[("↑↓", "scroll"), ("Esc", "back")];

/// The hints the status line shows while the detail has the keys.
pub fn hints(app: &App) -> &'static [(&'static str, &'static str)] {
    if app.detail_epic_key.is_some() {
        EPIC_HINTS
    } else if matches!(app.detail_mode, DetailMode::History { .. }) {
        HISTORY_HINTS
    } else {
        VIEW_HINTS
    }
}

/// Where the detail and the list go in the content area.
pub struct Panes {
    /// The list beside the detail, narrowed to `LIST_WIDTH`; none when the detail replaces it.
    pub list: Option<Rect>,
    /// The dim rule between them.
    pub divider: Option<Rect>,
    pub detail: Rect,
}

/// The panes in `area`: side by side from `SPLIT_MIN_WIDTH` columns, unless `z` zoomed the
/// detail to the whole area.
pub fn panes(app: &App, area: Rect) -> Panes {
    if area.width < SPLIT_MIN_WIDTH || app.detail_fullscreen {
        return Panes {
            list: None,
            divider: None,
            detail: area,
        };
    }
    let [list, divider, detail] = Layout::horizontal([
        Constraint::Length(LIST_WIDTH),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(area);
    Panes {
        list: Some(list),
        divider: Some(divider),
        detail,
    }
}

/// The dim `│` between the list and the detail.
pub fn render_divider(f: &mut ratatui::Frame, area: Rect) {
    f.render_widget(
        Block::default()
            .borders(Borders::LEFT)
            .border_style(style::secondary()),
        area,
    );
}

/// Draws the detail of the ticket or epic the app has open in `area`: a header line with the
/// key, its place among the list's rows and a `[×]`, then the body.
pub fn render(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let ticket = app
        .detail_ticket_key
        .as_ref()
        .and_then(|key| app.find_ticket(key));
    let epic = app
        .detail_epic_key
        .as_ref()
        .and_then(|key| app.cache.epics.iter().find(|epic| &epic.key == key));
    let key = match (ticket, epic) {
        (Some(ticket), _) => &ticket.key,
        (None, Some(epic)) => &epic.key,
        (None, None) => return,
    };
    let [head, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
    let mut title = vec![Span::raw(" "), Span::styled(key.clone(), style::heading())];
    if let Some((index, count)) = app.detail_position() {
        title.push(Span::styled(
            format!("  {index} of {count}"),
            style::secondary(),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(title)), head);
    form::close_button(f, app, head, Target::CloseDetail);
    // A drag selects the header and the text, not the scrollbar's column.
    crate::mouse::selectable(
        app,
        Rect::new(
            area.x + 1,
            area.y,
            area.width.saturating_sub(2),
            area.height,
        ),
    );
    match (ticket, epic) {
        (Some(ticket), _) => match &app.detail_mode {
            // The move picker docks under the detail, which keeps showing the ticket.
            DetailMode::History { scroll } => render_history(f, app, body, ticket, *scroll),
            _ => render_view(f, app, body, ticket),
        },
        (None, Some(epic)) => render_epic_view(f, app, body, epic),
        (None, None) => {}
    }
}

fn render_history(f: &mut ratatui::Frame, app: &App, body: Rect, ticket: &Ticket, scroll: u16) {
    let [text, buttons] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(u16::from(body.height > 1)),
    ])
    .areas(body);
    let text = Rect::new(
        text.x + 1,
        text.y,
        text.width.saturating_sub(2),
        text.height,
    );
    crate::widgets::activity::render(f, text, &ticket.activity, scroll);
    form::buttons(
        f,
        app,
        Rect::new(
            buttons.x + 1,
            buttons.y,
            buttons.width.saturating_sub(1),
            buttons.height,
        ),
        &[("Back", crossterm::event::KeyCode::Esc)],
    );
}

/// Draws `lines` in `area`, a cell in from the left and leaving the right column for a
/// scrollbar, at the detail's scroll position, and records the scroll limits for the scroll
/// keys.
fn render_scrollable(
    f: &mut ratatui::Frame,
    app: &App,
    area: Rect,
    build: impl FnOnce(usize) -> Vec<Line<'static>>,
) {
    let body = Rect::new(
        area.x + u16::from(area.width > 0),
        area.y,
        area.width.saturating_sub(2),
        area.height,
    );
    let lines = build(body.width as usize);
    let max = lines
        .len()
        .saturating_sub(body.height as usize)
        .min(u16::MAX as usize) as u16;
    app.detail_scroll_max.set(max);
    app.detail_page_height.set(body.height.max(1));
    let scroll = app.detail_scroll.min(max);

    f.render_widget(
        Paragraph::new(lines)
            .scroll((scroll, 0))
            .wrap(Wrap { trim: false }),
        body,
    );

    if max > 0 {
        let mut state = ScrollbarState::new(max as usize).position(scroll as usize);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .symbols(symbols::scrollbar::VERTICAL)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(None)
                .thumb_style(style::primary()),
            area,
            &mut state,
        );
    }
}

fn rule(width: usize) -> Line<'static> {
    Line::from(Span::styled("─".repeat(width), style::secondary()))
}

/// A rule with `title` set into it: `── Comments (2) ─────`.
fn section(title: &str, width: usize) -> Line<'static> {
    let lead = "── ";
    let used = lead.chars().count() + title.chars().count() + 1;
    Line::from(vec![
        Span::styled(lead, style::secondary()),
        Span::styled(title.to_string(), style::heading()),
        Span::styled(
            format!(" {}", "─".repeat(width.saturating_sub(used))),
            style::secondary(),
        ),
    ])
}

/// A header row: the field name, then `value` wrapped under itself.
fn push_field(lines: &mut Vec<Line<'static>>, name: &str, value: Vec<Span<'static>>, width: usize) {
    lines.extend(markup::wrap(
        vec![Span::styled(
            format!("{:<FIELD_WIDTH$}", name),
            style::secondary(),
        )],
        vec![Span::raw(" ".repeat(FIELD_WIDTH))],
        value,
        width,
    ));
}

fn title_lines(summary: &str, width: usize) -> Vec<Line<'static>> {
    markup::wrap(
        vec![],
        vec![],
        vec![Span::styled(summary.to_string(), style::heading())],
        width,
    )
}

fn indented(line: Line<'static>, indent: &str) -> Line<'static> {
    let mut spans = vec![Span::raw(indent.to_string())];
    spans.extend(line.spans);
    Line::from(spans)
}

fn header_lines(ticket: &Ticket, rules: &StatusRules, width: usize) -> Vec<Line<'static>> {
    let mut lines = title_lines(&ticket.summary, width);
    lines.push(Line::from(""));

    push_field(
        &mut lines,
        "Status",
        vec![Span::styled(
            format!("● {}", ticket.status),
            style::status(&ticket.status, rules).patch(style::heading()),
        )],
        width,
    );

    let assignee = match &ticket.assignee {
        Some(name) => Span::styled(name.clone(), style::primary()),
        None => Span::styled("Unassigned", style::aside()),
    };
    push_field(&mut lines, "Assignee", vec![assignee], width);

    if let Some(reporter) = &ticket.reporter {
        push_field(
            &mut lines,
            "Reporter",
            vec![Span::styled(reporter.clone(), style::primary())],
            width,
        );
    }

    if let Some(epic_key) = &ticket.epic_key {
        let mut value = vec![Span::styled(epic_key.clone(), style::primary())];
        if let Some(name) = &ticket.epic_name {
            value.push(Span::styled(format!(" · {}", name), style::primary()));
        }
        push_field(&mut lines, "Epic", value, width);
    }

    if !ticket.labels.is_empty() {
        let chips = ticket
            .labels
            .iter()
            .map(|label| label.replace(' ', "\u{a0}"))
            .collect::<Vec<_>>()
            .join("  ");
        push_field(
            &mut lines,
            "Labels",
            vec![Span::styled(chips, style::primary())],
            width,
        );
    }

    lines
}

fn description_lines(app: &App, ticket: &Ticket, width: usize) -> Vec<Line<'static>> {
    if ticket.detail_loaded {
        return match ticket.description.as_deref() {
            Some(desc) if !desc.trim().is_empty() => markup::render(desc, width as u16),
            _ => vec![Line::from(Span::styled("No description.", style::aside()))],
        };
    }
    match app.detail_fetch_error(&ticket.key) {
        Some(error) => {
            let mut lines = markup::wrap(
                vec![],
                vec![],
                vec![Span::styled(
                    format!("Couldn't load details: {}", error.trim()),
                    style::error(),
                )],
                width,
            );
            lines.push(Line::from(Span::styled(
                "Close and reopen the ticket to try again.",
                style::secondary(),
            )));
            lines
        }
        None => vec![Line::from(Span::styled(
            "Loading details…",
            style::warning(),
        ))],
    }
}

/// The ticket's comments, oldest first.
fn comment_lines(ticket: &Ticket, width: usize) -> Vec<Line<'static>> {
    // Activity is newest first.
    let comments: Vec<_> = ticket
        .activity
        .iter()
        .rev()
        .map(|entry| {
            let ActivityKind::Comment { body } = &entry.kind;
            (entry, body)
        })
        .collect();

    let title = match comments.len() {
        0 => "Comments".to_string(),
        n => format!("Comments ({})", n),
    };
    let mut lines = vec![Line::from(""), section(&title, width)];
    if comments.is_empty() {
        lines.push(Line::from(Span::styled(
            "No comments yet. Press C to add one.",
            style::aside(),
        )));
        return lines;
    }

    let indent = "  ";
    for (entry, body) in comments {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(entry.author.clone(), style::heading()),
            Span::styled(
                format!("  {}", format_timestamp(&entry.timestamp)),
                style::secondary(),
            ),
        ]));
        let body_width = width.saturating_sub(indent.len()) as u16;
        lines.extend(
            markup::render(body, body_width)
                .into_iter()
                .map(|line| indented(line, indent)),
        );
    }
    lines
}

fn render_view(f: &mut ratatui::Frame, app: &App, area: Rect, ticket: &Ticket) {
    render_scrollable(f, app, area, |width| {
        let mut lines = header_lines(ticket, app.status_rules(), width);
        let now = crate::local_cache::now_unix_secs() as i64;
        if let Some(quip) = app.delight.stale_quip(app.status_rules(), ticket, now) {
            let quip = Span::styled(quip, style::aside());
            lines.extend(markup::wrap(vec![], vec![], vec![quip], width));
        }
        lines.push(Line::from(""));
        lines.push(rule(width));
        lines.push(Line::from(""));
        lines.extend(description_lines(app, ticket, width));
        if ticket.detail_loaded {
            lines.extend(comment_lines(ticket, width));
        }
        lines
    });
}

fn render_epic_view(f: &mut ratatui::Frame, app: &App, area: Rect, epic: &Epic) {
    render_scrollable(f, app, area, |width| {
        let rules = app.status_rules();

        let mut lines = title_lines(&epic.summary, width);
        lines.push(Line::from(""));

        let total = epic.total();
        let (done, scope) = epic.progress(rules);
        let mut progress = epic_bar(epic, rules, EPIC_BAR_WIDTH, style::primary());
        if let Some(percent) = epic.percent_done(rules) {
            progress.push(Span::styled(
                format!("\u{a0}\u{a0}{done}\u{a0}/\u{a0}{scope}\u{a0}({percent}%)"),
                style::success(),
            ));
        }
        // Dropped children aren't in the fraction, so name them to make it add up to the total.
        if scope < total {
            let lead = if scope > 0 { "," } else { "\u{a0}" };
            progress.push(Span::styled(
                format!("{lead}\u{a0}{}\u{a0}dropped", total - scope),
                style::secondary(),
            ));
        }
        push_field(&mut lines, "Progress", progress, width);

        let mut counts = Vec::new();
        for (status, count) in epic.status_counts(rules) {
            if !counts.is_empty() {
                counts.push(Span::raw("  "));
            }
            counts.push(Span::styled(
                format!("{}:\u{a0}{}", status.as_str(), count).replace(' ', "\u{a0}"),
                style::status(status.as_str(), rules),
            ));
        }
        if counts.is_empty() {
            counts.push(Span::styled("No related tickets", style::secondary()));
        }
        push_field(&mut lines, "Status", counts, width);

        lines.push(Line::from(""));
        lines.push(section(&format!("Related tickets ({})", total), width));
        lines.push(Line::from(""));

        let mut children: Vec<_> = epic.children.iter().collect();
        rules.sort_tickets(&mut children);
        if children.is_empty() {
            lines.push(Line::from(Span::styled(
                "(no related tickets)",
                style::secondary(),
            )));
        }
        let summary_width = width.saturating_sub(12 + 15 + 2).max(10);
        for ticket in children {
            lines.push(Line::from(vec![
                Span::styled(format!("{:<12}", ticket.key), style::primary()),
                Span::styled(
                    fit(&ticket.status, 15),
                    style::status(&ticket.status, rules),
                ),
                Span::raw("  "),
                Span::styled(truncate(&ticket.summary, summary_width), style::primary()),
            ]));
        }
        lines
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::ActivityEntry;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn ticket() -> Ticket {
        Ticket {
            key: "DEMO-3228".to_string(),
            summary: "PAY: Payments Platform Revamp".to_string(),
            status: "In Progress".to_string(),
            assignee: Some("Alex Rivera".to_string()),
            reporter: Some("Alex Rivera".to_string()),
            description: Some(
                "*Stakeholders:* Alex Rivera (owner)\n\nh2. Goals\n# Replay the changes\n# Grade the result"
                    .to_string(),
            ),
            labels: vec!["DEMO".to_string(), "data-science".to_string()],
            detail_loaded: true,
            activity: vec![
                comment("2026-09-02T10:00:00.000+0000", "Sam Lee", "Second *reply*"),
                comment("2026-09-01T09:30:00.000+0000", "Alex Rivera", "First"),
            ],
            ..Ticket::default()
        }
    }

    fn comment(timestamp: &str, author: &str, body: &str) -> ActivityEntry {
        ActivityEntry {
            timestamp: timestamp.to_string(),
            author: author.to_string(),
            author_email: None,
            kind: ActivityKind::Comment {
                body: body.to_string(),
            },
        }
    }

    fn app_showing(ticket: Ticket) -> App {
        let mut app = App::new();
        app.loading = false;
        app.open_detail(ticket.key.clone());
        app.cache.my_tickets = vec![ticket];
        app
    }

    fn draw(app: &App, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| render(f, app, f.area())).unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().replace('\u{a0}', " "))
                    .collect()
            })
            .collect()
    }

    fn row(lines: &[String], text: &str) -> usize {
        lines
            .iter()
            .position(|l| l.contains(text))
            .unwrap_or_else(|| panic!("{:?} not drawn:\n{}", text, lines.join("\n")))
    }

    #[test]
    fn epic_detail_names_dropped_children_so_the_counts_add_up() {
        let showing = |children: &[(&str, usize)]| {
            let mut app = App::new();
            app.loading = false;
            app.cache.epics = vec![Epic::for_test("DEMO-3228", children)];
            app.detail_epic_key = Some("DEMO-3228".into());
            draw(&app, 120, 40)
        };
        let line = |lines: &[String], text| lines[row(lines, text)].clone();

        let lines = showing(&[("Closed", 108), ("In Progress", 3), ("Won't Do", 10)]);
        let progress = line(&lines, "Progress");
        assert!(
            progress.contains("108 / 111 (97%), 10 dropped"),
            "{progress}"
        );
        let status = line(&lines, "Status");
        assert!(
            status.contains("Closed: 108  In Progress: 3  Won't Do: 10"),
            "{status}"
        );
        row(&lines, "Related tickets (121)");

        // With nothing dropped, the fraction is the whole epic, rounded down like the header.
        let lines = showing(&[("Closed", 1999), ("In Progress", 1)]);
        let progress = line(&lines, "Progress");
        assert!(progress.contains("1999 / 2000 (99%)"), "{progress}");
        assert!(!progress.contains("dropped"), "{progress}");
        row(&lines, "Related tickets (2000)");

        // With every child dropped, there's no fraction to show.
        let lines = showing(&[("Won't Do", 2)]);
        let progress = line(&lines, "Progress");
        assert!(progress.contains("░  2 dropped"), "{progress}");
        assert!(!progress.contains('/'), "{progress}");
    }

    #[tokio::test]
    async fn the_close_button_ends_one_cell_before_the_right_edge_of_the_header_and_closes() {
        let mut config = toml::from_str("[jira]\nproject = 'DEMO'\nteam_name = 'Demo'\n").unwrap();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        for (width, height) in [(55, 50), (40, 18), (80, 24), (160, 40)] {
            for mode in [
                DetailMode::View,
                DetailMode::History { scroll: 0 },
                DetailMode::MoveLoading { request: 1 },
            ] {
                let mut app = app_showing(ticket());
                app.detail_mode = mode;
                let lines = draw(&app, width, height);
                let at = lines[0]
                    .find("[×]")
                    .map(|byte| lines[0][..byte].chars().count());
                assert_eq!(
                    at,
                    Some(width as usize - 5),
                    "{width}x{height}: {}",
                    lines[0]
                );
                let close = Rect::new(width - 6, 0, 5, 1);
                assert!(app
                    .mouse_targets
                    .borrow()
                    .contains(&(close, crate::mouse::Target::CloseDetail)));
                for (x, closes) in [
                    (width - 7, false),
                    (width - 6, true),
                    (width - 2, true),
                    (width - 1, false),
                ] {
                    let mut app = app_showing(ticket());
                    draw(&app, width, height);
                    for kind in [
                        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                        crossterm::event::MouseEventKind::Up(crossterm::event::MouseButton::Left),
                    ] {
                        crate::mouse::handle(
                            &mut app,
                            crossterm::event::MouseEvent {
                                kind,
                                column: x,
                                row: 0,
                                modifiers: crossterm::event::KeyModifiers::NONE,
                            },
                            &tx,
                            &mut config,
                        )
                        .await;
                    }
                    assert_eq!(
                        !app.is_detail_open(),
                        closes,
                        "{width}x{height} click at {x}"
                    );
                }
            }
        }
    }

    #[test]
    fn header_fields_and_comments_oldest_first() {
        let lines = draw(&app_showing(ticket()), 70, 34);
        assert!(lines[row(&lines, "Status")].contains("● In Progress"));
        assert!(lines[row(&lines, "Labels")].contains("Labels    DEMO  data-science"));
        assert!(lines[row(&lines, "── Comments (2) ─")].contains('─'));
        assert!(row(&lines, "Alex Rivera  2026-09-01 09:30") < row(&lines, "Sam Lee"));
        assert!(lines[row(&lines, "Second reply")].contains("   Second reply"));
    }

    #[test]
    fn long_detail_scrolls_only_to_its_last_line() {
        let mut t = ticket();
        t.description = Some((1..=60).map(|n| format!("line {}\n", n)).collect());
        let mut app = app_showing(t);
        draw(&app, 70, 30);
        assert!(app.detail_scroll_max.get() > 0);

        app.detail_scroll = u16::MAX;
        let lines = draw(&app, 70, 30);
        let last = row(&lines, "Second reply");
        assert_eq!(last, 29, "the last line sits on the pane's last row");
        // The scrollbar is the pane's last column, its thumb at the bottom.
        assert!(lines[last].trim_end().ends_with('█'));
        // Only the thumb is drawn: the track is blank, so no vertical rule but the divider.
        assert!(lines
            .iter()
            .all(|line| matches!(line.chars().last(), Some('█' | ' '))));
    }

    #[test]
    fn unloaded_detail_says_loading_or_why_it_failed() {
        let mut t = ticket();
        t.detail_loaded = false;
        let mut app = app_showing(t);
        let lines = draw(&app, 70, 34);
        row(&lines, "Loading details…");
        assert!(!lines.iter().any(|l| l.contains("Comments")));

        app.fail_detail_fetch("DEMO-3228", "jira: timed out".to_string());
        let lines = draw(&app, 70, 34);
        row(&lines, "Couldn't load details: jira: timed out");
    }
}
