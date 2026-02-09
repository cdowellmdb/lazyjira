use std::collections::{HashMap, HashSet};

use anyhow::{bail, Context, Result};

use crate::app::{BulkUploadPreview, BulkUploadRow, ISSUE_TYPES};

pub const MAX_BULK_UPLOAD_ROWS: usize = 500;

#[derive(Debug, Clone)]
pub struct BulkUploadContext {
    pub known_epic_keys: HashSet<String>,
    pub existing_summaries: HashSet<String>,
    pub issue_types: Vec<String>,
}

impl BulkUploadContext {
    pub fn new(known_epic_keys: HashSet<String>, existing_summaries: HashSet<String>) -> Self {
        Self {
            known_epic_keys,
            existing_summaries,
            issue_types: ISSUE_TYPES.iter().map(|s| s.to_string()).collect(),
        }
    }
}

pub fn normalize_summary(summary: &str) -> String {
    summary.trim().to_ascii_lowercase()
}

pub fn parse_csv_preview(path: &str, context: &BulkUploadContext) -> Result<BulkUploadPreview> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(path)
        .with_context(|| format!("Failed to open CSV file: {}", path))?;

    let raw_headers = reader
        .headers()
        .context("Failed to read CSV headers")?
        .iter()
        .map(|s| s.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();

    if raw_headers.is_empty() {
        bail!("CSV file is empty");
    }

    let mut headers = HashMap::new();
    for (idx, header) in raw_headers.iter().enumerate() {
        headers.entry(header.clone()).or_insert(idx);
    }

    if !headers.contains_key("summary") {
        bail!("Missing required 'summary' header");
    }

    let allowed_types: HashMap<String, String> = context
        .issue_types
        .iter()
        .map(|t| (t.to_ascii_lowercase(), t.clone()))
        .collect();

    let mut rows = Vec::new();
    let mut summary_seen: HashMap<String, usize> = HashMap::new();

    for (idx, record_result) in reader.records().enumerate() {
        if idx >= MAX_BULK_UPLOAD_ROWS {
            bail!(
                "Row limit exceeded. Maximum supported rows per upload is {}",
                MAX_BULK_UPLOAD_ROWS
            );
        }

        let row_number = idx + 2;
        let record = record_result.with_context(|| {
            format!(
                "Failed to parse CSV row {}. Check quoting and delimiters.",
                row_number
            )
        })?;

        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        let summary = field_value(&record, &headers, "summary").unwrap_or_default();
        if summary.is_empty() {
            errors.push("summary is required".to_string());
        }

        let issue_type_input = field_value(&record, &headers, "type");
        let issue_type = match issue_type_input {
            Some(ref t) if !t.is_empty() => {
                let normalized = t.to_ascii_lowercase();
                if let Some(found) = allowed_types.get(&normalized) {
                    found.clone()
                } else {
                    errors.push(format!(
                        "invalid type '{}'; expected one of {}",
                        t,
                        context.issue_types.join(", ")
                    ));
                    t.clone()
                }
            }
            _ => "Task".to_string(),
        };

        let assignee_email = field_value(&record, &headers, "assignee_email");
        if let Some(ref email) = assignee_email {
            if !is_valid_email(email) {
                errors.push(format!("invalid assignee_email '{}'", email));
            }
        }

        let epic_key = field_value(&record, &headers, "epic_key");
        if let Some(ref key) = epic_key {
            if !is_valid_jira_key(key) {
                errors.push(format!("invalid epic_key '{}'", key));
            } else if !context.known_epic_keys.contains(&key.to_ascii_uppercase()) {
                errors.push(format!("unknown epic_key '{}'", key));
            }
        }

        let labels = field_value(&record, &headers, "labels")
            .unwrap_or_default()
            .split('|')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect::<Vec<_>>();

        let description = field_value(&record, &headers, "description");

        if !summary.is_empty() {
            let normalized_summary = normalize_summary(&summary);

            if context.existing_summaries.contains(&normalized_summary) {
                warnings.push("possible duplicate: summary matches an existing ticket".to_string());
            }

            let seen = summary_seen.entry(normalized_summary).or_insert(0);
            if *seen > 0 {
                warnings.push("duplicate summary in this CSV".to_string());
            }
            *seen += 1;
        }

        rows.push(BulkUploadRow {
            row_number,
            issue_type,
            summary,
            assignee_email,
            epic_key,
            labels,
            description,
            errors,
            warnings,
        });
    }

    let total_rows = rows.len();
    let invalid_rows = rows.iter().filter(|r| !r.errors.is_empty()).count();
    let valid_rows = total_rows.saturating_sub(invalid_rows);
    let warning_count = rows.iter().map(|r| r.warnings.len()).sum();

    Ok(BulkUploadPreview {
        source_path: path.to_string(),
        rows,
        total_rows,
        valid_rows,
        invalid_rows,
        warning_count,
    })
}

fn field_value(
    record: &csv::StringRecord,
    headers: &HashMap<String, usize>,
    name: &str,
) -> Option<String> {
    let idx = *headers.get(name)?;
    let value = record.get(idx)?.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn is_valid_email(email: &str) -> bool {
    let mut parts = email.split('@');
    let local = parts.next().unwrap_or_default();
    let domain = parts.next().unwrap_or_default();
    let extra = parts.next();
    !local.is_empty() && !domain.is_empty() && extra.is_none() && domain.contains('.')
}

fn is_valid_jira_key(key: &str) -> bool {
    let Some((project, number)) = key.split_once('-') else {
        return false;
    };
    if project.is_empty() || number.is_empty() {
        return false;
    }
    project.chars().all(|c| c.is_ascii_alphanumeric()) && number.chars().all(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn write_temp_csv(content: &str) -> String {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("lazyjira_bulk_upload_{}.csv", stamp));
        fs::write(&path, content).expect("write temp csv");
        path.to_string_lossy().to_string()
    }

    fn sample_context() -> BulkUploadContext {
        let mut epics = HashSet::new();
        epics.insert("AMP-1".to_string());
        let mut summaries = HashSet::new();
        summaries.insert("existing summary".to_string());
        BulkUploadContext::new(epics, summaries)
    }

    #[test]
    fn parse_success_with_minimal_summary_only() {
        let path = write_temp_csv("summary\nFirst task\nSecond task\n");
        let preview = parse_csv_preview(&path, &sample_context()).expect("parse");
        assert_eq!(preview.total_rows, 2);
        assert_eq!(preview.valid_rows, 2);
        assert_eq!(preview.invalid_rows, 0);
        assert!(preview.can_submit());
    }

    #[test]
    fn parse_success_with_all_supported_fields() {
        let csv = "summary,type,assignee_email,epic_key,labels,description\n\
                   \"CSV task\",Bug,dev@example.com,AMP-1,\"frontend|urgent\",\"first line\nsecond line, with comma\"\n";
        let path = write_temp_csv(csv);
        let preview = parse_csv_preview(&path, &sample_context()).expect("parse");
        let row = &preview.rows[0];
        assert_eq!(row.issue_type, "Bug");
        assert_eq!(row.assignee_email.as_deref(), Some("dev@example.com"));
        assert_eq!(row.epic_key.as_deref(), Some("AMP-1"));
        assert_eq!(
            row.labels,
            vec!["frontend".to_string(), "urgent".to_string()]
        );
        assert!(row
            .description
            .as_deref()
            .unwrap_or("")
            .contains("with comma"));
        assert!(row.errors.is_empty());
    }

    #[test]
    fn parse_fails_when_summary_header_missing() {
        let path = write_temp_csv("type,assignee_email\nTask,dev@example.com\n");
        let err = parse_csv_preview(&path, &sample_context()).expect_err("must fail");
        assert!(err
            .to_string()
            .contains("Missing required 'summary' header"));
    }

    #[test]
    fn row_validation_catches_expected_errors() {
        let csv = "summary,type,assignee_email,epic_key\n\
                   ,Feature,bad-email,AMP-999\n";
        let path = write_temp_csv(csv);
        let preview = parse_csv_preview(&path, &sample_context()).expect("parse");
        let row = &preview.rows[0];
        assert!(row.errors.iter().any(|e| e.contains("summary is required")));
        assert!(row.errors.iter().any(|e| e.contains("invalid type")));
        assert!(row
            .errors
            .iter()
            .any(|e| e.contains("invalid assignee_email")));
        assert!(row.errors.iter().any(|e| e.contains("unknown epic_key")));
        assert_eq!(preview.invalid_rows, 1);
        assert!(!preview.can_submit());
    }

    #[test]
    fn duplicate_warnings_include_existing_and_in_csv() {
        let csv = "summary\nExisting Summary\nExisting Summary\n";
        let path = write_temp_csv(csv);
        let preview = parse_csv_preview(&path, &sample_context()).expect("parse");
        assert_eq!(preview.warning_count, 3);
        assert_eq!(preview.rows[0].warnings.len(), 1);
        assert_eq!(preview.rows[1].warnings.len(), 2);
    }

    #[test]
    fn row_limit_is_enforced() {
        let mut csv = String::from("summary\n");
        for i in 0..=MAX_BULK_UPLOAD_ROWS {
            csv.push_str(format!("task {}\n", i).as_str());
        }
        let path = write_temp_csv(&csv);
        let err = parse_csv_preview(&path, &sample_context()).expect_err("must fail");
        assert!(err.to_string().contains("Row limit exceeded"));
    }
}
