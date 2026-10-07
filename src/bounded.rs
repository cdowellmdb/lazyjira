//! Running many Jira calls a few at a time, for the reads (`jira_reads`) and the bulk actions.

use std::future::Future;

use tokio::task::{JoinError, JoinSet};

/// Runs `task` on each of `items`, at most `limit` at a time, and hands each output to `done`
/// with its item's position in `items`, as it finishes rather than in order. A task that panics
/// reaches `done` as its `JoinError` instead of ending the others.
pub async fn for_each_bounded<I, T, F, Fut>(
    limit: usize,
    items: Vec<I>,
    task: F,
    mut done: impl FnMut(usize, Result<T, JoinError>),
) where
    F: Fn(I) -> Fut,
    Fut: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let mut waiting = items.into_iter().enumerate();
    let mut running = JoinSet::new();
    loop {
        while running.len() < limit {
            let Some((at, item)) = waiting.next() else {
                break;
            };
            // The task has a spawn of its own so a panic comes back to this loop with its
            // position, rather than as an anonymous error from the set.
            let work = tokio::spawn(task(item));
            running.spawn(async move { (at, work.await) });
        }
        match running.join_next().await {
            Some(Ok((at, output))) => done(at, output),
            // The wrapper only awaits, so this is it being cancelled as the runtime shuts down.
            Some(Err(_)) => {}
            None => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
    use std::sync::Arc;
    use std::time::Duration;

    /// Runs `count` tasks, each of which takes a moment, `limit` at a time, returning the most
    /// that were at it at once and the positions `done` heard of.
    async fn run(limit: usize, count: usize) -> (usize, Vec<usize>) {
        let running = Arc::new(AtomicUsize::new(0));
        let most = Arc::new(AtomicUsize::new(0));
        let mut positions = Vec::new();
        let (r, m) = (running.clone(), most.clone());
        for_each_bounded(
            limit,
            (0..count).collect(),
            move |_| {
                let (running, most) = (r.clone(), m.clone());
                async move {
                    most.fetch_max(running.fetch_add(1, SeqCst) + 1, SeqCst);
                    tokio::time::sleep(Duration::from_millis(5)).await;
                    running.fetch_sub(1, SeqCst);
                }
            },
            |at, done| {
                done.unwrap();
                positions.push(at);
            },
        )
        .await;
        positions.sort();
        (most.load(SeqCst), positions)
    }

    #[tokio::test]
    async fn no_more_than_the_limit_run_at_once() {
        // Fewer than the limit all run at once, and so do exactly as many as the limit.
        assert_eq!(run(3, 2).await, (2, vec![0, 1]));
        assert_eq!(run(3, 3).await, (3, vec![0, 1, 2]));
        // The fourth waits for one of the three to finish.
        assert_eq!(run(3, 4).await, (3, vec![0, 1, 2, 3]));
        assert_eq!(run(3, 0).await, (0, vec![]));
    }

    #[tokio::test]
    async fn a_task_that_panics_is_an_error_at_its_position_and_the_others_still_run() {
        let mut heard = Vec::new();
        for_each_bounded(
            2,
            vec![0, 1, 2],
            |n| async move {
                if n == 1 {
                    panic!("task {n} failed");
                }
                n * 10
            },
            |at, done| heard.push((at, done.map_err(|e| e.is_panic()))),
        )
        .await;
        heard.sort_by_key(|(at, _)| *at);
        assert_eq!(heard, [(0, Ok(0)), (1, Err(true)), (2, Ok(20))]);
    }
}
