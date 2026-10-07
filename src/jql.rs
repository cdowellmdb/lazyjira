//! Building JQL safely: quoting a value, and keeping only what is shaped like a ticket key in a
//! list, so config, saved filters and keys from a cache can't change the query they go in.

/// How many keys one `key in (…)`, `parent in (…)` or `"Epic Link" in (…)` search holds.
pub const KEYS_PER_SEARCH: usize = 50;

/// Whether `key` looks like a Jira key, safe to put in a JQL list.
pub fn is_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// `keys` as the inside of a JQL list (`A-1,B-2`). Keys that don't look like Jira keys are left
/// out so they can't break the query; `None` when none are left.
pub fn key_list(keys: &[String]) -> Option<String> {
    let keys: Vec<&str> = keys
        .iter()
        .map(String::as_str)
        .filter(|key| is_key(key))
        .collect();
    (!keys.is_empty()).then(|| keys.join(","))
}

/// `text` as a JQL string literal, quotes included: a `"` or `\` in it can't end the string or
/// change the query.
pub fn quote(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_keeps_a_value_inside_its_string() {
        assert_eq!(quote("Platform Team"), "\"Platform Team\"");
        assert_eq!(quote(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(quote(r"back\slash"), r#""back\\slash""#);
        // The backslash is escaped before the quote, so an escaped quote can't be forged.
        assert_eq!(quote(r#"\""#), r#""\\\"""#);
    }

    #[test]
    fn only_keys_shaped_like_jira_keys_reach_a_jql_list() {
        let keys = |keys: &[&str]| keys.iter().map(|k| k.to_string()).collect::<Vec<_>>();
        assert_eq!(
            key_list(&keys(&["DSCI-1", "x\") OR 1=1", "", "AB_2"])).as_deref(),
            Some("DSCI-1,AB_2")
        );
        assert_eq!(key_list(&keys(&["no good"])), None);
        assert_eq!(key_list(&[]), None);
        assert!(is_key("DSCI-3244") && !is_key("DSCI 1") && !is_key(""));
    }
}
