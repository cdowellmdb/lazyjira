use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, VisibleGroup};
use crate::cache::{Epic, Ticket};
use crate::views::common::{
    color_marks, column_width, fit, fold_indicator, group_marker, highlight_row, panel,
    status_color, status_width, ticket_cells, ticket_marker, truncate, KEY_WIDTH, SEPARATOR_WIDTH,
};

/// Status and Assignee are as wide as the longest value on screen (Assignee at most 24), so
/// "Waiting for Approval" or a long name doesn't push its row's summary out of line; Summary
/// takes the rest.
fn child_column_widths(
    area: Rect,
    epics: &[VisibleGroup<'_, &Epic>],
) -> (usize, usize, usize, usize) {
    let key_w = KEY_WIDTH;
    let mut status_w = status_width(epics);
    let mut assignee_w = column_width(epics, "ASSIGNEE", assignee).min(24);
    let inner = panel().inner(area).width as usize;
    let room = inner.saturating_sub(4 + key_w + 3 * SEPARATOR_WIDTH);

    // Narrow terminals: Assignee, then Status, give up columns (down to 8) until Summary has 18.
    let short =
        |status_w: usize, assignee_w: usize| (status_w + assignee_w + 18).saturating_sub(room);
    assignee_w -= short(status_w, assignee_w).min(assignee_w.saturating_sub(8));
    status_w -= short(status_w, assignee_w).min(status_w.saturating_sub(8));

    (
        key_w,
        status_w,
        assignee_w,
        room.saturating_sub(status_w + assignee_w),
    )
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
    let (key_w, status_w, assignee_w, summary_w) = child_column_widths(area, &visible_epics);
    let heading_style = Style::default()
        .fg(Color::Reset)
        .add_modifier(Modifier::BOLD);

    let mut lines: Vec<Line> = Vec::new();
    let mut selected_visual_line: Option<usize> = None;
    let mut mouse_rows = Vec::new();

    if !visible_epics.is_empty() {
        let header_w = 4 + key_w + 3 * SEPARATOR_WIDTH + status_w + assignee_w + summary_w;
        lines.push(Line::from(vec![
            Span::styled(format!("    {:<key_w$}", "SEL KEY"), heading_style),
            Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<status_w$}", "STATUS"), heading_style),
            Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<assignee_w$}", "ASSIGNEE"), heading_style),
            Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<summary_w$}", "SUMMARY"), heading_style),
        ]));
        lines.push(Line::from(Span::styled(
            "─".repeat(header_w),
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
        let meta = if blocked > 0 {
            format!(
                "{}  ({} / {} done, {:.1}%, blocked {})",
                progress, done, total, pct, blocked
            )
        } else {
            format!("{}  ({} / {} done, {:.1}%)", progress, done, total, pct)
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
            Span::styled(
                format!(
                    "{:<epic_summary_w$}",
                    truncate(&epic.summary, epic_summary_w)
                ),
                header_summary_style,
            ),
        ]));
        let meta_style = if is_header_selected {
            Style::default().fg(Color::Gray)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(meta, meta_style),
        ]));

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
                    Span::styled(fit(ticket.status.as_str(), status_w), status_style),
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
