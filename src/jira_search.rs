//! Searching Jira a few queries at a time, and reading ticket details through those searches:
//! `search_in_order` runs a read's queries four at a time and returns their tickets in query
//! order, `read_details` reads many tickets' details a chunk of keys per search, and
//! `fetch_ticket_detail` reads one ticket as the issue itself. Which queries to send is `jql`'s
//! business, and what a detail read returns reaches the screen through `App::enrich_ticket`.

use std::collections::HashMap;
use std::future::Future;
use std::ops::ControlFlow;

use anyhow::{anyhow, Result};

use crate::bounded::for_each_bounded;
use crate::cache::Ticket;
use crate::jira_rest::describe;
use crate::jql::{is_key, key_chunks};

/// How many of a read's searches run at once: a project with 400 epics needs about fifty,
/// which one after another is a minute, and a few at a time is easy on Jira.
const SEARCHES_AT_ONCE: usize = 4;

/// Runs `search` on each of `jqls`, `SEARCHES_AT_ONCE` at a time, and gives each answer to `done`
/// with the position of its query in `jqls`, as it arrives rather than in order. A search that
/// panics answers with an error. `done` can answer `Break` to send no more queries; the ones
/// already sent still answer.
async fn search_each<S, F>(
    jqls: Vec<String>,
    search: S,
    mut done: impl FnMut(usize, Result<Vec<Ticket>>) -> ControlFlow<()>,
) where
    S: Fn(String) -> F,
    F: Future<Output = Result<Vec<Ticket>>> + Send + 'static,
{
    for_each_bounded(SEARCHES_AT_ONCE, jqls, search, |at, joined| {
        done(
            at,
            joined.unwrap_or_else(|e| Err(anyhow!("A search task failed: {e}"))),
        )
    })
    .await
}

/// The tickets every one of `jqls` finds, in the order of the queries. A search that fails stops
/// the queued ones from being sent, and the first failure in query order fails them all (a query
/// that wasn't sent has no answer to fail with).
async fn search_in_order<S, F>(jqls: Vec<String>, search: S) -> Result<Vec<Ticket>>
where
    S: Fn(String) -> F,
    F: Future<Output = Result<Vec<Ticket>>> + Send + 'static,
{
    let mut answers = Vec::new();
    search_each(jqls, search, |at, found| {
        let flow = match found {
            Ok(_) => ControlFlow::Continue(()),
            Err(_) => ControlFlow::Break(()),
        };
        answers.push((at, found));
        flow
    })
    .await;
    answers.sort_by_key(|(at, _)| *at);
    let found = answers
        .into_iter()
        .map(|(_, found)| found)
        .collect::<Result<Vec<_>>>()?;
    Ok(found.concat())
}

/// Every ticket `jqls` find (see `search_in_order`), asking Jira for `fields`.
pub async fn search_all(jqls: Vec<String>, fields: &'static [&'static str]) -> Result<Vec<Ticket>> {
    search_in_order(jqls, |jql| async move {
        crate::jira_rest::search(&jql, fields).await
    })
    .await
}

/// What a detail adds to a list row's fields: what the detail overlay shows. `enrich_ticket`
/// copies the rest over the row.
const DETAIL_ONLY_FIELDS: &[&str] = &["reporter", "description", "comment"];

/// Reads one ticket's detail fresh from Jira: when it's opened, and after a move. It reads the
/// issue itself rather than searching, so it can't miss a change made a moment ago.
pub async fn fetch_ticket_detail(key: &str) -> Result<Ticket> {
    Ok(as_detail(
        crate::jira_rest::issue(key, &detail_fields()).await?,
    ))
}

/// What a detail read asks for: the list fields and the detail-only ones.
fn detail_fields() -> Vec<&'static str> {
    [LIST_FIELDS, DETAIL_ONLY_FIELDS].concat()
}

/// Reads the details of `keys` over Jira's REST search, a chunk of tickets per request. See
/// `read_details`.
pub async fn fetch_ticket_details(
    keys: &[String],
    deliver: impl FnMut(String, Result<Ticket, String>),
) {
    let fields = detail_fields();
    read_details(
        keys,
        |jql| {
            let fields = fields.clone();
            async move { crate::jira_rest::search(&jql, &fields).await }
        },
        deliver,
    )
    .await
}

/// A ticket read with the detail fields, which only a detail read gets, so it counts as loaded.
fn as_detail(mut ticket: Ticket) -> Ticket {
    ticket.detail_loaded = true;
    ticket
}

/// Reads the details of `keys`, `KEYS_PER_SEARCH` at a time (usually one page of results), with
/// `search`, which runs a JQL query, a few chunks at once (`SEARCHES_AT_ONCE`). Each key's
/// outcome goes to `deliver` as its chunk is read. A chunk that fails fails only its keys, so the
/// rest are still read and the failed ones are asked for again later. A key that isn't shaped
/// like a ticket key is refused without being searched, and takes no place in a chunk.
async fn read_details<S, F>(
    keys: &[String],
    search: S,
    mut deliver: impl FnMut(String, Result<Ticket, String>),
) where
    S: Fn(String) -> F,
    F: Future<Output = Result<Vec<Ticket>>> + Send + 'static,
{
    for key in keys.iter().filter(|key| !is_key(key)) {
        deliver(key.clone(), Err(format!("{key:?} isn't a ticket key")));
    }

    let chunks = key_chunks(keys);
    let jqls = chunks
        .iter()
        .map(|chunk| format!("key in ({})", chunk.join(",")))
        .collect();
    search_each(jqls, search, |at, found| {
        match found {
            Ok(tickets) => {
                let mut found: HashMap<_, _> = tickets
                    .into_iter()
                    .map(|ticket| (ticket.key.clone(), ticket))
                    .collect();
                for key in &chunks[at] {
                    let result = found
                        .remove(*key)
                        .map(as_detail)
                        .ok_or_else(|| "Jira didn't return this ticket".to_string());
                    deliver(key.to_string(), result);
                }
            }
            Err(e) => {
                let error = describe(&e);
                for key in &chunks[at] {
                    deliver(key.to_string(), Err(error.clone()));
                }
            }
        }
        // A failed chunk fails only its keys; the other chunks are still read.
        ControlFlow::Continue(())
    })
    .await
}

/// What a list row needs. `key` always comes back, and the Epic Link field is added by the
/// search. `issuetype` tells a sub-task's parent from an epic.
pub const LIST_FIELDS: &[&str] = &[
    "summary",
    "status",
    "assignee",
    "labels",
    "parent",
    "issuetype",
    "updated",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ticket(key: &str, status: &str) -> Ticket {
        Ticket {
            summary: format!("Summary for {}", key),
            ..Ticket::for_test(key, status)
        }
    }

    fn ticket_keys(tickets: &[Ticket]) -> Vec<&str> {
        tickets.iter().map(|t| t.key.as_str()).collect()
    }

    fn keys(range: std::ops::RangeInclusive<u32>) -> Vec<String> {
        range.map(|n| format!("DEMO-{n}")).collect()
    }

    /// Runs `read_details` with a Jira that answers each search with `answer(jql)`, returning
    /// the searches made and what each key got.
    async fn read(
        keys: &[String],
        answer: impl Fn(&str) -> Result<Vec<Ticket>>,
    ) -> (Vec<String>, Vec<(String, Result<Ticket, String>)>) {
        let searches = std::cell::RefCell::new(Vec::new());
        let mut delivered = Vec::new();
        read_details(
            keys,
            |jql| {
                let result = answer(&jql);
                searches.borrow_mut().push(jql);
                async move { result }
            },
            |key, result| delivered.push((key, result)),
        )
        .await;
        (searches.into_inner(), delivered)
    }

    #[tokio::test]
    async fn details_are_read_with_one_key_search_and_marked_loaded() {
        let (searches, delivered) = read(&keys(1..=2), |_| {
            Ok(vec![
                test_ticket("DEMO-2", "Done"),
                test_ticket("DEMO-1", "To Do"),
            ])
        })
        .await;

        assert_eq!(searches, ["key in (DEMO-1,DEMO-2)"]);
        // Each key gets its own result, in the order asked, whatever order Jira answered in.
        let got: Vec<_> = delivered
            .iter()
            .map(|(key, result)| (key.as_str(), result.as_ref().unwrap().detail_loaded))
            .collect();
        assert_eq!(got, [("DEMO-1", true), ("DEMO-2", true)]);
        assert_eq!(delivered[1].1.as_ref().unwrap().status, "Done");
    }

    /// A Jira that has every ticket asked for.
    fn has_all(jql: &str) -> Result<Vec<Ticket>> {
        let list = jql
            .strip_prefix("key in (")
            .unwrap()
            .strip_suffix(')')
            .unwrap();
        Ok(list
            .split(',')
            .map(|key| test_ticket(key, "To Do"))
            .collect())
    }

    #[tokio::test]
    async fn details_are_searched_fifty_keys_at_a_time() {
        // Exactly 50 keys fit one search.
        let (searches, delivered) = read(&keys(1..=50), has_all).await;
        assert_eq!(searches, [format!("key in ({})", keys(1..=50).join(","))]);
        assert_eq!(delivered.len(), 50);

        // The 51st key starts a second search that holds only it.
        let (searches, delivered) = read(&keys(1..=51), has_all).await;
        assert_eq!(
            searches,
            [
                format!("key in ({})", keys(1..=50).join(",")),
                "key in (DEMO-51)".to_string()
            ]
        );
        assert_eq!(delivered.len(), 51);
        assert!(delivered.iter().all(|(_, result)| result.is_ok()));

        // Nothing to read, nothing to search.
        let (searches, delivered) = read(&[], has_all).await;
        assert!(searches.is_empty() && delivered.is_empty());
    }

    #[tokio::test]
    async fn a_malformed_key_takes_no_place_in_a_chunk() {
        // 50 usable keys and a malformed one: still one search of exactly the 50.
        let mut asked = keys(1..=50);
        asked.insert(10, "no good".to_string());
        let (searches, delivered) = read(&asked, has_all).await;
        assert_eq!(searches, [format!("key in ({})", keys(1..=50).join(","))]);
        assert_eq!(delivered.len(), 51);

        // The 51st usable key starts the second search, alone.
        asked.push("DEMO-51".to_string());
        let (searches, _) = read(&asked, has_all).await;
        assert_eq!(
            searches,
            [
                format!("key in ({})", keys(1..=50).join(",")),
                "key in (DEMO-51)".to_string()
            ]
        );
    }

    /// A search for jql "n" that answers one ticket, T-n, after a while: the earlier the query
    /// the longer it takes, so answers arrive out of order. `running` and `most` count how many
    /// searches are at it at once, and the most there were.
    fn slow_search(
        running: &std::sync::Arc<std::sync::atomic::AtomicUsize>,
        most: &std::sync::Arc<std::sync::atomic::AtomicUsize>,
        jql: String,
    ) -> impl Future<Output = Result<Vec<Ticket>>> + Send + 'static {
        use std::sync::atomic::Ordering::SeqCst;
        let (running, most) = (running.clone(), most.clone());
        async move {
            let now = running.fetch_add(1, SeqCst) + 1;
            most.fetch_max(now, SeqCst);
            let n: u64 = jql.parse().unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(4 * (12 - n.min(11)))).await;
            running.fetch_sub(1, SeqCst);
            if jql == "7" {
                anyhow::bail!("Jira answered 503.");
            }
            Ok(vec![test_ticket(&format!("T-{jql}"), "To Do")])
        }
    }

    /// Runs `count` queries ("0", "1", ...) through `search_in_order`, skipping the one that
    /// fails unless `count` is past it, returning what it found and how many ran at once.
    async fn run_queries(count: usize) -> (Result<Vec<Ticket>>, usize) {
        let running = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let most = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let jqls = (0..count).map(|n| n.to_string()).collect();
        let found = search_in_order(jqls, |jql| slow_search(&running, &most, jql)).await;
        (found, most.load(std::sync::atomic::Ordering::SeqCst))
    }

    #[tokio::test(start_paused = true)]
    async fn searches_run_four_at_a_time_and_answer_in_the_order_asked() {
        // Fewer queries than the limit all run at once; exactly the limit does too.
        assert_eq!(run_queries(3).await.1, 3);
        assert_eq!(run_queries(4).await.1, 4);
        // The fifth waits for one of the four to finish, so never more than four are at it.
        let (found, most) = run_queries(5).await;
        assert_eq!(most, 4);
        assert_eq!(
            ticket_keys(&found.unwrap()),
            ["T-0", "T-1", "T-2", "T-3", "T-4"],
            "in query order although the earlier ones finish last"
        );
        // Seven queries (0 to 6) are answered in order too, still four at a time.
        let (found, most) = run_queries(7).await;
        assert_eq!(most, 4);
        assert_eq!(
            ticket_keys(&found.unwrap()),
            ["T-0", "T-1", "T-2", "T-3", "T-4", "T-5", "T-6"]
        );
        // No queries, nothing to run.
        assert!(run_queries(0).await.0.unwrap().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn one_failed_search_fails_them_all_and_the_first_in_query_order_is_reported() {
        // Query "7" fails after a while, and nothing is found for the read.
        let (found, _) = run_queries(10).await;
        assert_eq!(found.unwrap_err().to_string(), "Jira answered 503.");
    }

    /// A search for jql "n" that records it was sent, then answers T-n after 20 ms, except "1",
    /// which fails after 5 ms.
    fn recorded_search(
        sent: &std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        jql: String,
    ) -> impl Future<Output = Result<Vec<Ticket>>> + Send + 'static {
        let sent = sent.clone();
        async move {
            sent.lock().unwrap().push(jql.clone());
            if jql == "1" {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                anyhow::bail!("Jira answered 401.");
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            Ok(vec![test_ticket(&format!("T-{jql}"), "To Do")])
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_search_stops_the_queued_ones_from_being_sent() {
        let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let jqls = (0..10).map(|n| n.to_string()).collect();

        let found = search_in_order(jqls, |jql| recorded_search(&sent, jql)).await;

        assert_eq!(found.unwrap_err().to_string(), "Jira answered 401.");
        // The first four were out together when "1" failed; the other six never were.
        let mut sent = sent.lock().unwrap().clone();
        sent.sort();
        assert_eq!(sent, ["0", "1", "2", "3"]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_search_that_panics_stops_the_queued_ones_too() {
        let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let jqls = (0..10).map(|n| n.to_string()).collect();

        let found = search_in_order(jqls, |jql| {
            let sent = sent.clone();
            async move {
                sent.lock().unwrap().push(jql.clone());
                if jql == "1" {
                    panic!("a parser bug");
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                Ok(vec![test_ticket(&format!("T-{jql}"), "To Do")])
            }
        })
        .await;

        assert!(found.is_err());
        let mut sent = sent.lock().unwrap().clone();
        sent.sort();
        assert_eq!(sent, ["0", "1", "2", "3"]);
    }

    #[tokio::test(start_paused = true)]
    async fn when_two_searches_fail_the_earlier_query_is_the_one_reported() {
        // Query 1 fails first (5 ms), query 0 later (20 ms): the report is query 0's.
        let found = search_in_order(vec!["0".to_string(), "1".to_string()], |jql| async move {
            let (wait, error) = if jql == "0" {
                (20, "query 0 failed")
            } else {
                (5, "query 1 failed")
            };
            tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
            anyhow::bail!(error)
        })
        .await;

        assert_eq!(found.unwrap_err().to_string(), "query 0 failed");
    }

    #[tokio::test]
    async fn a_search_that_panics_fails_the_read_instead_of_hanging_it() {
        let jqls = vec!["0".to_string(), "1".to_string()];

        let found = search_in_order(jqls, |jql| async move {
            if jql == "1" {
                panic!("a parser bug");
            }
            Ok(vec![test_ticket("T-0", "To Do")])
        })
        .await;

        let error = format!("{:#}", found.unwrap_err());
        assert!(error.contains("search task"), "{error}");
    }

    #[tokio::test]
    async fn a_failed_chunk_fails_only_its_keys_and_the_next_chunk_is_still_read() {
        let (searches, delivered) = read(&keys(1..=51), |jql| {
            if jql.contains("DEMO-1,") {
                Err(anyhow::anyhow!("Jira answered 503."))
            } else {
                has_all(jql)
            }
        })
        .await;

        assert_eq!(searches.len(), 2);
        for (key, result) in &delivered[..50] {
            assert_eq!(result.as_ref().unwrap_err(), "Jira answered 503.", "{key}");
        }
        assert_eq!(delivered[50].0, "DEMO-51");
        assert!(delivered[50].1.is_ok());
    }

    #[tokio::test]
    async fn a_key_jira_leaves_out_fails_alone_and_a_malformed_key_is_never_searched() {
        let (searches, delivered) = read(
            &["DEMO-1".into(), "DEMO-2".into(), "x\") OR 1=1".into()],
            |_| Ok(vec![test_ticket("DEMO-1", "To Do")]),
        )
        .await;

        assert_eq!(searches, ["key in (DEMO-1,DEMO-2)"]);
        // The malformed key is refused before anything is searched.
        assert_eq!(
            delivered[0].1.as_ref().unwrap_err(),
            "\"x\\\") OR 1=1\" isn't a ticket key"
        );
        assert_eq!(delivered[1].0, "DEMO-1");
        assert!(delivered[1].1.is_ok());
        assert_eq!(delivered[2].0, "DEMO-2");
        assert_eq!(
            delivered[2].1.as_ref().unwrap_err(),
            "Jira didn't return this ticket"
        );

        // A chunk of nothing but malformed keys makes no search at all.
        let (searches, delivered) = read(&["no good".into()], has_all).await;
        assert!(searches.is_empty());
        assert_eq!(
            delivered[0].1.as_ref().unwrap_err(),
            "\"no good\" isn't a ticket key"
        );
    }
}
