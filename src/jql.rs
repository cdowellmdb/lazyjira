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

/// The keys of `keys` that look like Jira keys, `KEYS_PER_SEARCH` to a chunk, each chunk for one
/// search's list (`.join(",")`). A key that doesn't look like one is left out before the keys
/// are counted, so it can't break the query or take a place from a real key.
pub fn key_chunks(keys: &[String]) -> Vec<Vec<&str>> {
    let usable: Vec<&str> = keys
        .iter()
        .map(String::as_str)
        .filter(|key| is_key(key))
        .collect();
    usable
        .chunks(KEYS_PER_SEARCH)
        .map(<[&str]>::to_vec)
        .collect()
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

    fn keys(count: usize) -> Vec<String> {
        (1..=count).map(|n| format!("DSCI-{n}")).collect()
    }

    #[test]
    fn only_keys_shaped_like_jira_keys_reach_a_jql_list() {
        let keys: Vec<String> = ["DSCI-1", "x\") OR 1=1", "", "AB_2"]
            .map(String::from)
            .into();
        assert_eq!(key_chunks(&keys), [["DSCI-1", "AB_2"]]);
        assert!(key_chunks(&["no good".to_string()]).is_empty());
        assert!(key_chunks(&[]).is_empty());
        assert!(is_key("DSCI-3244") && !is_key("DSCI 1") && !is_key(""));
    }

    #[test]
    fn keys_are_chunked_by_the_search_limit_with_malformed_ones_taking_no_place() {
        let sizes =
            |keys: &[String]| -> Vec<usize> { key_chunks(keys).iter().map(Vec::len).collect() };
        assert_eq!(sizes(&keys(50)), [50]);
        let fifty_one = keys(51);
        assert_eq!(sizes(&fifty_one), [50, 1]);
        assert_eq!(key_chunks(&fifty_one)[1], ["DSCI-51"]);
        // A malformed key among 50 real ones doesn't push the 50th into a second search.
        let mut with_bad = keys(50);
        with_bad.insert(10, "bad key".to_string());
        assert_eq!(sizes(&with_bad), [50]);
    }
}
