use crossterm::event::KeyCode;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, BulkUploadState};
use crate::style;
use crate::views::common::truncate;

use super::form::{self, Row};

/// A form's buttons and the hints beside them.
type Footer = (
    &'static [(&'static str, KeyCode)],
    &'static [(&'static str, &'static str)],
);

/// Preview rows shown at most.
const MAX_PREVIEW_ROWS: usize = 14;
/// The lines of the preview that aren't ticket rows: path, counts, gate, the "more" line, the
/// selected row's problem and the blank lines between.
const PREVIEW_CHROME: usize = 9;

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

/// The title of the panel for `state`.
fn title(state: &BulkUploadState) -> &'static str {
    match state {
        BulkUploadState::PathInput { .. } => "Bulk CSV Upload",
        BulkUploadState::Preview { .. } => "Bulk CSV Preview",
        BulkUploadState::Running { .. } => "Bulk Upload Running",
        BulkUploadState::Result { .. } => "Bulk Upload Results",
    }
}

/// The path's body: its editor, and a line under it.
const PATH_ROWS: [Row; 2] = [Row::fixed(1), Row::fixed(1)];

/// The rows the panel's body needs: for the path, its editor and a line; otherwise the lines.
pub fn need(app: &App) -> form::Need {
    match app.bulk_upload_state.as_ref() {
        None => form::need(&[]),
        Some(BulkUploadState::PathInput { .. }) => form::need(&PATH_ROWS),
        Some(state) => form::need(&[Row::flex(lines(state, MAX_PREVIEW_ROWS).0.len() as u16, 1)]),
    }
}

/// The text of a state other than the path: its lines, and the line each preview row is on.
fn lines(state: &BulkUploadState, max_rows: usize) -> (Vec<Line<'static>>, Vec<(usize, usize)>) {
    let mut lines: Vec<Line> = Vec::new();
    let mut choices = Vec::new();
    match state {
        BulkUploadState::PathInput { .. } => {}
        BulkUploadState::Preview { preview, selected } => {
            lines.push(Line::from(format!("Path: {}", preview.source_path)));
            lines.push(Line::from(format!(
                "Rows: {}  Valid: {}  Invalid: {}  Warnings: {}",
                preview.total_rows, preview.valid_rows, preview.invalid_rows, preview.warning_count
            )));
            lines.push(Line::from(""));

            let (gate_style, gate_message) = if preview.can_submit() {
                (style::success(), "Ready to submit.")
            } else {
                (
                    style::error(),
                    "Submission blocked: fix invalid rows and reload preview.",
                )
            };
            lines.push(Line::from(Span::styled(gate_message, gate_style)));
            lines.push(Line::from(""));

            if preview.rows.is_empty() {
                lines.push(Line::from(Span::styled(
                    "No rows found in CSV.",
                    style::secondary(),
                )));
            } else {
                let (start, end) = preview_window(preview.rows.len(), *selected, max_rows);
                for (idx, row) in preview.rows[start..end].iter().enumerate() {
                    let absolute_idx = start + idx;
                    choices.push((lines.len(), absolute_idx));
                    let marker = if absolute_idx == *selected { ">" } else { " " };
                    let (status, row_style) = if !row.errors.is_empty() {
                        ("ERR", style::error())
                    } else if !row.warnings.is_empty() {
                        ("WARN", style::warning())
                    } else {
                        ("OK", style::success())
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
                        row_style,
                    )));
                }
                if end < preview.rows.len() {
                    lines.push(Line::from(Span::styled(
                        format!("... {} more rows", preview.rows.len().saturating_sub(end)),
                        style::secondary(),
                    )));
                }
            }

            lines.push(Line::from(""));
            if let Some(row) = preview.rows.get(*selected) {
                if !row.errors.is_empty() {
                    lines.push(Line::from(Span::styled(
                        format!("Row {} errors: {}", row.row_number, row.errors.join("; ")),
                        style::error(),
                    )));
                } else if !row.warnings.is_empty() {
                    lines.push(Line::from(Span::styled(
                        format!(
                            "Row {} warnings: {}",
                            row.row_number,
                            row.warnings.join("; ")
                        ),
                        style::warning(),
                    )));
                }
            }
        }
        BulkUploadState::Running { preview } => {
            lines.push(Line::from(Span::styled(
                "Creating tickets from CSV...",
                style::warning().patch(style::heading()),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(format!("Path: {}", preview.source_path)));
            lines.push(Line::from(format!(
                "Rows: {} ({} valid)",
                preview.total_rows, preview.valid_rows
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Upload is running in background. Press Esc to close this panel.",
                style::secondary(),
            )));
        }
        BulkUploadState::Result { summary } => {
            lines.push(Line::from(Span::styled(
                "Bulk upload complete",
                style::success().patch(style::heading()),
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
                    style::error().patch(style::heading()),
                )));
                for (row_number, row_summary, err) in summary.failed_details.iter().take(5) {
                    lines.push(Line::from(Span::styled(
                        format!(
                            "  row {} ({}): {}",
                            row_number,
                            truncate(row_summary, 30),
                            truncate(err, 80)
                        ),
                        style::error(),
                    )));
                }
                if summary.failed_details.len() > 5 {
                    lines.push(Line::from(Span::styled(
                        format!(
                            "  ... and {} more",
                            summary.failed_details.len().saturating_sub(5)
                        ),
                        style::error(),
                    )));
                }
            }
        }
    }
    (lines, choices)
}

pub fn render(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let Some(state) = app.bulk_upload_state.as_ref() else {
        return;
    };
    let (body, footer) = form::render_panel_frame(f, app, area, form::title(title(state), ""));

    let (buttons, hints): Footer = match state {
        BulkUploadState::PathInput { .. } => (
            &[
                ("Preview", KeyCode::Enter),
                ("Cancel", KeyCode::Esc),
                ("Editor", KeyCode::F(4)),
            ],
            &[("Ctrl+E", "external editor")],
        ),
        BulkUploadState::Preview { .. } => (
            &[
                ("Submit", KeyCode::Enter),
                ("Reload", KeyCode::Char('r')),
                ("Close", KeyCode::Esc),
            ],
            &[("j/k", "inspect rows")],
        ),
        BulkUploadState::Running { .. } => (&[("Close", KeyCode::Esc)], &[]),
        BulkUploadState::Result { .. } => (
            &[("Close", KeyCode::Enter), ("Reload", KeyCode::Char('r'))],
            &[("Esc", "close")],
        ),
    };
    form::button_row(f, app, footer, buttons, hints);

    if let BulkUploadState::PathInput { path, loading } = state {
        let [editor, note] = form::split(PATH_ROWS, body);
        form::render_editor(f, app, editor, "CSV path", path, !loading, 0);
        f.render_widget(
            Paragraph::new(Span::styled(
                if *loading {
                    "Loading preview..."
                } else {
                    "Columns: summary, type, assignee_email, epic_key, labels, description"
                },
                style::secondary(),
            )),
            note,
        );
        return;
    }
    let max_rows = (body.height as usize)
        .saturating_sub(PREVIEW_CHROME)
        .clamp(1, MAX_PREVIEW_ROWS);
    let (lines, choices) = lines(state, max_rows);
    form::choice_targets(app, body, 0, &choices);
    f.render_widget(Paragraph::new(lines), body);
}
