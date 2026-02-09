use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, BulkUploadState};

use super::form;

fn truncate(input: &str, max: usize) -> String {
    if input.chars().count() <= max {
        return input.to_string();
    }
    input
        .chars()
        .take(max.saturating_sub(3))
        .collect::<String>()
        + "..."
}

fn preview_window(total: usize, selected: usize, max_rows: usize) -> (usize, usize) {
    if total <= max_rows {
        return (0, total);
    }
    let half = max_rows / 2;
    let mut start = selected.saturating_sub(half);
    let mut end = start + max_rows;
    if end > total {
        end = total;
        start = end.saturating_sub(max_rows);
    }
    (start, end)
}

pub fn render(f: &mut ratatui::Frame, app: &App) {
    let Some(state) = app.bulk_upload_state.as_ref() else {
        return;
    };

    let (title, percent_x, percent_y) = match state {
        BulkUploadState::PathInput { .. } => ("Bulk CSV Upload", 72, 34),
        BulkUploadState::Preview { .. } => ("Bulk CSV Preview", 90, 80),
        BulkUploadState::Running { .. } => ("Bulk Upload Running", 68, 36),
        BulkUploadState::Result { .. } => ("Bulk Upload Results", 72, 54),
    };
    let inner = form::render_modal_frame(f, title, percent_x, percent_y);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));

    match state {
        BulkUploadState::PathInput { path, loading } => {
            form::render_text_input(&mut lines, "CSV Path", path, true);
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Headers: summary (required), type, assignee_email, epic_key, labels, description",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(Span::styled(
                "Labels use '|' separators (example: frontend|urgent). Max rows: 500.",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(""));
            if *loading {
                lines.push(Line::from(Span::styled(
                    "Loading preview...",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "[Enter] preview  [Esc] cancel",
                Style::default().fg(Color::DarkGray),
            )));
        }
        BulkUploadState::Preview { preview, selected } => {
            lines.push(Line::from(format!("Path: {}", preview.source_path)));
            lines.push(Line::from(format!(
                "Rows: {}  Valid: {}  Invalid: {}  Warnings: {}",
                preview.total_rows, preview.valid_rows, preview.invalid_rows, preview.warning_count
            )));
            lines.push(Line::from(""));

            let gate_style = if preview.can_submit() {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Red)
            };
            let gate_message = if preview.can_submit() {
                "Ready to submit."
            } else {
                "Submission blocked: fix invalid rows and reload preview."
            };
            lines.push(Line::from(Span::styled(gate_message, gate_style)));
            lines.push(Line::from(""));

            if preview.rows.is_empty() {
                lines.push(Line::from(Span::styled(
                    "No rows found in CSV.",
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                let (start, end) = preview_window(preview.rows.len(), *selected, 14);
                for (idx, row) in preview.rows[start..end].iter().enumerate() {
                    let absolute_idx = start + idx;
                    let marker = if absolute_idx == *selected { ">" } else { " " };
                    let status = if !row.errors.is_empty() {
                        "ERR"
                    } else if !row.warnings.is_empty() {
                        "WARN"
                    } else {
                        "OK"
                    };
                    let color = if !row.errors.is_empty() {
                        Color::Red
                    } else if !row.warnings.is_empty() {
                        Color::Yellow
                    } else {
                        Color::Green
                    };
                    lines.push(Line::from(Span::styled(
                        format!(
                            "{} {:<4} row {:<4} {:<6} {}",
                            marker,
                            status,
                            row.row_number,
                            row.issue_type,
                            truncate(&row.summary, 68)
                        ),
                        Style::default().fg(color),
                    )));
                }
                if end < preview.rows.len() {
                    lines.push(Line::from(Span::styled(
                        format!("... {} more rows", preview.rows.len().saturating_sub(end)),
                        Style::default().fg(Color::DarkGray),
                    )));
                }
            }

            lines.push(Line::from(""));
            if let Some(row) = preview.rows.get(*selected) {
                if !row.errors.is_empty() {
                    lines.push(Line::from(Span::styled(
                        format!("Row {} errors: {}", row.row_number, row.errors.join("; ")),
                        Style::default().fg(Color::Red),
                    )));
                } else if !row.warnings.is_empty() {
                    lines.push(Line::from(Span::styled(
                        format!(
                            "Row {} warnings: {}",
                            row.row_number,
                            row.warnings.join("; ")
                        ),
                        Style::default().fg(Color::Yellow),
                    )));
                }
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "[j/k] inspect rows  [r] reload CSV  [Enter/y] submit  [Esc] close",
                Style::default().fg(Color::DarkGray),
            )));
        }
        BulkUploadState::Running { preview } => {
            lines.push(Line::from(Span::styled(
                "Creating tickets from CSV...",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(format!("Path: {}", preview.source_path)));
            lines.push(Line::from(format!(
                "Rows: {} ({} valid)",
                preview.total_rows, preview.valid_rows
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Upload is running in background. Press Esc to close this modal.",
                Style::default().fg(Color::DarkGray),
            )));
        }
        BulkUploadState::Result { summary } => {
            lines.push(Line::from(Span::styled(
                "Bulk upload complete",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(format!("Path: {}", summary.source_path)));
            lines.push(Line::from(format!("Total rows: {}", summary.total_rows)));
            lines.push(Line::from(format!("Attempted: {}", summary.attempted)));
            lines.push(Line::from(format!("Succeeded: {}", summary.succeeded)));
            lines.push(Line::from(format!("Failed: {}", summary.failed)));
            if !summary.created_keys.is_empty() {
                let sample = summary
                    .created_keys
                    .iter()
                    .take(5)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                lines.push(Line::from(format!("Created keys: {}", sample)));
            }
            if !summary.failed_details.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "Failures:",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )));
                for (row_number, row_summary, err) in summary.failed_details.iter().take(5) {
                    lines.push(Line::from(Span::styled(
                        format!(
                            "  row {} ({}): {}",
                            row_number,
                            truncate(row_summary, 30),
                            truncate(err, 80)
                        ),
                        Style::default().fg(Color::Red),
                    )));
                }
                if summary.failed_details.len() > 5 {
                    lines.push(Line::from(Span::styled(
                        format!(
                            "  ... and {} more",
                            summary.failed_details.len().saturating_sub(5)
                        ),
                        Style::default().fg(Color::Red),
                    )));
                }
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "[r] rerun from same CSV  [Enter/Esc] close",
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    let body = Paragraph::new(lines);
    f.render_widget(body, inner);
}
