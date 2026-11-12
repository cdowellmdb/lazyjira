use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::app::App;
use crate::views::common::{
    fold_indicator, group_marker, panel, status_color, status_label, ticket_cells, ticket_marker,
    Ages, Columns, ListLines, Shared, KEY_WIDTH, SEPARATOR_WIDTH, UPDATED_WIDTH,
};

fn my_work_column_widths(area: Rect) -> (usize, usize, usize, usize) {
    let key_w = KEY_WIDTH;
    let mut summary_w = 34usize;
    let mut epic_w = 24usize;
    let mut labels_w = 22usize;
    let inner = panel().inner(area).width as usize;
    let prefix_and_separators = 2 + key_w + 4 * SEPARATOR_WIDTH + UPDATED_WIDTH;
    let mut overflow = prefix_and_separators + summary_w + epic_w + labels_w;

    if overflow > inner {
        let mut to_trim = overflow - inner;
        if summary_w > 16 {
            let cut = to_trim.min(summary_w - 16);
            summary_w -= cut;
            to_trim -= cut;
        }
        if to_trim > 0 && epic_w > 12 {
            let cut = to_trim.min(epic_w - 12);
            epic_w -= cut;
            to_trim -= cut;
        }
        if to_trim > 0 && labels_w > 10 {
            let cut = to_trim.min(labels_w - 10);
            labels_w -= cut;
            to_trim -= cut;
        }
        if to_trim > 0 && summary_w > 12 {
            let cut = to_trim.min(summary_w - 12);
            summary_w -= cut;
        }
    }

    overflow = prefix_and_separators + summary_w + epic_w + labels_w;
    if overflow < inner {
        summary_w += inner - overflow;
    }

    (key_w, summary_w, epic_w, labels_w)
}

/// What My Work shows with no rows: a scene when nothing is left to do (no active ticket, and no
/// search or status focus hiding any), otherwise "No tickets found".
fn empty_lines(app: &App) -> Vec<Line<'static>> {
    let rules = app.status_rules();
    let all_done = app
        .cache
        .my_tickets
        .iter()
        .all(|t| rules.is_done(&t.status));
    let scene = (all_done && app.search.is_none() && app.status_focus.is_none())
        .then(|| app.delight.inbox_zero())
        .flatten();
    let Some((art, caption)) = scene else {
        return vec![Line::from(Span::styled(
            "  No tickets found",
            Style::default().fg(Color::DarkGray),
        ))];
    };
    let mut lines = vec![Line::from("")];
    lines.extend(art.iter().map(|row| {
        Line::from(Span::styled(
            format!("    {row}"),
            Style::default().fg(Color::Cyan),
        ))
    }));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!("    {caption}"),
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )));
    lines
}

pub fn render(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let grouped = app.my_work_visible_by_status();
    let ages = Ages::of(app);
    let shared = Shared::of(&grouped, Columns::EpicAndLabels);
    let (key_w, summary_w, epic_w, labels_w) = my_work_column_widths(area);
    let summary_w = shared.summary_width(summary_w, epic_w, labels_w);
    let heading_style = Style::default()
        .fg(Color::Reset)
        .add_modifier(Modifier::BOLD);
    let separator = |style: Style| Span::styled(" │ ", style.fg(Color::DarkGray));

    let mut lines = ListLines::default();
    let mut has_rows = false;

    for group in &grouped {
        let status = &group.header;
        if !has_rows {
            let mut header = Line::from(vec![
                Span::styled(format!("  {:<key_w$}", "SEL KEY"), heading_style),
                separator(Style::default()),
                Span::styled(format!("{:<summary_w$}", "SUMMARY"), heading_style),
            ]);
            shared.push_trailing_headings(&mut header, heading_style, epic_w, labels_w);
            let header_w = header.width();
            lines.push(header);
            lines.extend(shared.line(header_w));
            lines.push(Span::styled(
                "─".repeat(header_w),
                Style::default().fg(Color::DarkGray),
            ));
            lines.push("");
        }

        let total_count = group.total;
        has_rows = true;

        let indicator = fold_indicator(group.tickets.is_none());
        let marker = group_marker(app.group_selection_state(status.as_str()));

        let is_header_selected = group.index == app.selected_index;
        let header = format!(
            "{} {} {} ({})",
            marker,
            indicator,
            status_label(app, status).to_uppercase(),
            total_count
        );
        let header_style = if is_header_selected {
            Style::default()
                .fg(status_color(status, app.status_rules()))
                .add_modifier(Modifier::BOLD)
                .bg(Color::DarkGray)
        } else {
            Style::default()
                .fg(status_color(status, app.status_rules()))
                .add_modifier(Modifier::BOLD)
        };
        lines.header(
            group.index,
            is_header_selected,
            Span::styled(header, header_style),
        );

        let Some(tickets) = &group.tickets else {
            lines.push("");
            continue;
        };

        // Ticket rows
        for (index, ticket) in tickets {
            let is_selected = *index == app.selected_index;

            let base = if is_selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            let base = ages.fade(ticket, base);
            let marker = ticket_marker(app.is_ticket_selected(&ticket.key));
            let (key_cell, summary) = ticket_cells(
                app,
                group.family.get(index).copied(),
                ticket,
                marker,
                summary_w,
                base,
            );

            let cells = vec![Span::styled(format!("  {:<key_w$}", key_cell), base)];
            let row = lines.ticket(*index, is_selected, cells, base, summary);
            shared.push_trailing_cells(ages, row, ticket, base, epic_w, labels_w);
        }

        // Blank line between groups
        lines.push("");
    }

    if lines.is_empty() {
        lines.extend(empty_lines(app));
    }

    lines.render(f, area, app, panel());
}
