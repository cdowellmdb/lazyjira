use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use std::time::Instant;

use crate::app::{App, VisibleGroup};
use crate::cache::{Epic, Ticket};
use crate::views::common::{
    color_marks, column_width, fit, fold_indicator, group_marker, highlight_row,
    narrow_for_summary, panel, status_color, status_label, status_width, ticket_cells,
    ticket_marker, Ages, KEY_WIDTH, SEPARATOR_WIDTH,
};

/// Status and Assignee are as wide as the longest value on screen (Assignee at most 24), so
/// "Waiting for Approval" or a long name doesn't push its row's summary out of line. Summary takes
/// the rest; on narrow terminals Assignee, then Status, give up columns, down to 8, so it keeps 18.
fn child_column_widths(area: Rect, epics: &[VisibleGroup<'_, &Epic>]) -> (usize, usize, usize) {
    let mut status_w = status_width(epics);
    let mut assignee_w = column_width(epics, "ASSIGNEE", assignee).min(24);
    let inner = panel().inner(area).width as usize;
    let room = inner.saturating_sub(4 + KEY_WIDTH + 3 * SEPARATOR_WIDTH);
    let columns = &mut [(&mut assignee_w, 8), (&mut status_w, 8)];
    let summary_w = narrow_for_summary(room, 18, columns);
    (status_w, assignee_w, summary_w)
}

fn assignee(ticket: &Ticket) -> &str {
    ticket.assignee.as_deref().unwrap_or("-")
}

fn progress_bar(done: usize, total: usize, width: usize) -> String {
    if total == 0 || width == 0 {
        return format!("[{}]", "-".repeat(width));
    }

    let filled = ((done * width) + (total / 2)) / total;
    format!(
        "[{}{}]",
        "#".repeat(filled.min(width)),
        "-".repeat(width.saturating_sub(filled.min(width)))
    )
}

pub fn render(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let visible_epics = app.epics_visible_epics();
    let ages = Ages::of(app);
    let key_w = KEY_WIDTH;
    let (status_w, assignee_w, summary_w) = child_column_widths(area, &visible_epics);
    let heading_style = Style::default()
        .fg(Color::Reset)
        .add_modifier(Modifier::BOLD);

    let mut lines: Vec<Line> = Vec::new();
    let mut selected_visual_line: Option<usize> = None;
    let mut mouse_rows = Vec::new();

    if !visible_epics.is_empty() {
        let header = Line::from(vec![
            Span::styled(format!("    {:<key_w$}", "SEL KEY"), heading_style),
            Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<status_w$}", "STATUS"), heading_style),
            Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<assignee_w$}", "ASSIGNEE"), heading_style),
            Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<summary_w$}", "SUMMARY"), heading_style),
        ]);
        let rule = "─".repeat(header.width());
        lines.push(header);
        lines.push(Line::from(Span::styled(
            rule,
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(""));
    }

    for group in visible_epics {
        let epic = group.header;
        let indicator = fold_indicator(group.tickets.is_none());
        let marker = group_marker(app.group_selection_state(&epic.key));

        mouse_rows.push((lines.len(), group.index, true));
        let is_header_selected = group.index == app.selected_index;
        if is_header_selected {
            selected_visual_line = Some(lines.len());
        }

        let total = epic.total();
        let done = epic.done_count(app.status_rules());
        let pct = epic.progress_pct(app.status_rules());
        let blocked = epic.blocked_count();
        let bar_width = 18usize;
        let progress = progress_bar(done, total, bar_width);
        let counts = if blocked > 0 {
            format!(
                "  ({} / {} done, {:.1}%, blocked {})",
                done, total, pct, blocked
            )
        } else {
            format!("  ({} / {} done, {:.1}%)", done, total, pct)
        };
        let inner = panel().inner(area).width as usize;
        let prefix = 2 + 1 + 1 + 10 + 2; // mark + indicator + space + key + gap
        let epic_summary_w = inner.saturating_sub(prefix).max(12);

        let header_key_style = if is_header_selected {
            Style::default()
                .add_modifier(Modifier::BOLD)
                .bg(Color::DarkGray)
        } else {
            Style::default().add_modifier(Modifier::BOLD)
        };
        let header_summary_style = if is_header_selected {
            Style::default().bg(Color::DarkGray)
        } else {
            Style::default()
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} {} {:<10}", marker, indicator, epic.key),
                header_key_style,
            ),
            Span::raw("  "),
            Span::styled(fit(&epic.summary, epic_summary_w), header_summary_style),
        ]));
        let meta_style = if is_header_selected {
            Style::default().fg(Color::Gray)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let mut meta = vec![Span::raw("  ")];
        match app
            .delight
            .rainbow_bar(&epic.key, &progress, meta_style, Instant::now())
        {
            Some(bar) => meta.extend(bar),
            None => meta.push(Span::styled(progress, meta_style)),
        }
        meta.push(Span::styled(counts, meta_style));
        lines.push(Line::from(meta));

        let Some(children) = &group.tickets else {
            lines.push(Line::from(""));
            continue;
        };

        if children.is_empty() {
            lines.push(Line::from(Span::styled(
                "    (no related tickets)",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            for (index, ticket) in children {
                mouse_rows.push((lines.len(), *index, false));
                let is_selected = *index == app.selected_index;
                if is_selected {
                    selected_visual_line = Some(lines.len());
                }

                let base = if is_selected {
                    Style::default().bg(Color::DarkGray)
                } else {
                    Style::default()
                };

                let base = ages.fade(ticket, base);
                let status_style = if is_selected {
                    Style::default()
                        .fg(status_color(&ticket.status, app.status_rules()))
                        .bg(Color::DarkGray)
                } else {
                    Style::default().fg(status_color(&ticket.status, app.status_rules()))
                };
                let marker = ticket_marker(app.is_ticket_selected(&ticket.key));
                let (key_cell, summary) = ticket_cells(
                    app,
                    group.family.get(index).copied(),
                    ticket,
                    marker,
                    summary_w,
                    base,
                );

                let mut row = vec![
                    Span::styled(format!("    {:<key_w$}", key_cell), base),
                    Span::styled(" │ ", base.fg(Color::DarkGray)),
                    Span::styled(
                        fit(status_label(app, &ticket.status), status_w),
                        status_style,
                    ),
                    Span::styled(" │ ", base.fg(Color::DarkGray)),
                    Span::styled(fit(assignee(ticket), assignee_w), base),
                    Span::styled(" │ ", base.fg(Color::DarkGray)),
                ];
                row.extend(summary);
                lines.push(Line::from(row));
            }
        }

        // Blank line between epics
        lines.push(Line::from(""));
    }

    if lines.is_empty() {
        let empty_text = if app.epics_refreshing {
            "  Loading epics... (assembling the relationship map)"
        } else if let Some(search) = app.search.as_ref().filter(|s| !s.is_empty()) {
            lines.push(Line::from(Span::styled(
                format!("  No epics match \"{}\"", search),
                Style::default().fg(Color::DarkGray),
            )));
            ""
        } else {
            "  No epics found"
        };

        if !empty_text.is_empty() {
            lines.push(Line::from(Span::styled(
                empty_text,
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    highlight_row(&mut lines, selected_visual_line, panel().inner(area).width);
    color_marks(&mut lines);

    // Scroll to keep selected row visible
    let visible = area.height.saturating_sub(2) as usize;
    let scroll_y = match selected_visual_line {
        Some(line) if line >= visible => (line - visible + 1) as u16,
        _ => 0,
    };

    crate::mouse::register_rows(app, area, &mouse_rows, scroll_y, &lines);
    let widget = Paragraph::new(lines).block(panel()).scroll((scroll_y, 0));
    f.render_widget(widget, area);
}

#[cfg(test)]
mod tests {
    use crate::app::{App, Tab};
    use crate::cache::{Epic, Ticket};
    use crate::views::tests::{column_of, draw, line_with};
    use ratatui::style::Color;

    /// The Epics tab drawn `width` wide with one child assigned to `assignee`, and the drawn
    /// widths of its Status, Assignee and Summary columns, read from the headings.
    fn epic_columns(width: u16, assignee: &str) -> (Vec<(String, Color)>, [usize; 3]) {
        let mut waiting = Ticket::for_test("DEMO-1", "Waiting for Approval");
        waiting.assignee = Some(assignee.into());
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::Epics;
        app.cache.epics = vec![Epic {
            key: "EPIC-9".into(),
            summary: "Epic".into(),
            children: vec![waiting, Ticket::for_test("DEMO-2", "In Progress")],
        }];
        let rows = draw(&app, Tab::Epics, width);
        let header = &line_with(&rows, "SEL KEY").unwrap().0;
        let [status, assignee, summary] =
            ["STATUS", "ASSIGNEE", "SUMMARY"].map(|heading| column_of(header, heading));
        // The panel's border and padding take two columns on each side.
        let widths = [
            assignee - status - 3,
            summary - assignee - 3,
            width as usize - 2 - summary,
        ];
        (rows, widths)
    }

    #[test]
    fn epic_children_show_their_assignee_in_a_column_of_at_most_24() {
        let (rows, widths) = epic_columns(160, "Sam Doe");
        assert_eq!(widths[..2], [20, 8]);
        assert!(line_with(&rows, "DEMO-1")
            .unwrap()
            .0
            .contains("Waiting for Approval │ Sam Doe  │"));
        assert!(line_with(&rows, "DEMO-2")
            .unwrap()
            .0
            .contains("In Progress          │ -        │"));

        let name = "Alexandria Montgomery-Jo";
        let (rows, widths) = epic_columns(160, name);
        assert_eq!(widths[1], 24);
        assert!(line_with(&rows, "DEMO-1")
            .unwrap()
            .0
            .contains(&format!("│ {name} │")));

        let (rows, widths) = epic_columns(160, "Alexandria Montgomery-Jon");
        assert_eq!(widths[1], 24);
        assert!(line_with(&rows, "DEMO-1")
            .unwrap()
            .0
            .contains("│ Alexandria Montgomery... │"));

        // Wide characters count by display width: 12 characters are 24 columns.
        let (_, widths) = epic_columns(160, "山田太郎山田太郎山田太郎");
        assert_eq!(widths[1], 24);
    }

    #[test]
    fn narrow_epics_trim_assignee_then_status_to_8_so_summary_keeps_18() {
        // 95 columns leave Summary exactly 18 beside Status 20 and Assignee 24.
        let name = "Alexandria Montgomery-Jo";
        assert_eq!(epic_columns(95, name).1, [20, 24, 18]);
        assert_eq!(epic_columns(94, name).1, [20, 23, 18]);
        // Assignee reaches 8 first, then Status gives up its columns.
        assert_eq!(epic_columns(68, name).1, [9, 8, 18]);
        assert_eq!(epic_columns(67, name).1, [8, 8, 18]);
        // Neither goes below 8, so Summary shrinks instead of the row overflowing.
        assert_eq!(epic_columns(66, name).1, [8, 8, 17]);
    }

    #[test]
    fn a_folded_epic_leaves_status_and_assignee_as_wide_as_the_drawn_rows() {
        let mut waiting = Ticket::for_test("DEMO-1", "Waiting for Approval");
        waiting.assignee = Some("Alexandria Montgomery-Jo".into());
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::Epics;
        app.cache.epics = vec![
            Epic {
                key: "EPIC-1".into(),
                summary: "Folded".into(),
                children: vec![waiting],
            },
            Epic {
                key: "EPIC-2".into(),
                summary: "Open".into(),
                children: vec![Ticket::for_test("DEMO-2", "Open")],
            },
        ];
        app.collapsed_epics.insert("EPIC-1".into());
        app.mark_cache_changed();
        let rows = draw(&app, Tab::Epics, 160);
        let header = &line_with(&rows, "SEL KEY").unwrap().0;
        let [status, assignee, summary] =
            ["STATUS", "ASSIGNEE", "SUMMARY"].map(|heading| column_of(header, heading));
        // "STATUS" and "ASSIGNEE" are the widest drawn values: the folded epic's don't count.
        assert_eq!([assignee - status - 3, summary - assignee - 3], [6, 8]);
        assert!(line_with(&rows, "DEMO-1").is_none());
    }
}
