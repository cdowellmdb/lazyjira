//! What ends the event loop's wait between frames: terminal input, a background task's result,
//! or the next confetti frame. The loop sleeps until one of them, so a Jira answer is applied
//! when it arrives instead of at the next poll, and an idle app doesn't wake at all.

use std::future::poll_fn;
use std::io;
use std::pin::Pin;
use std::time::Duration;

use crossterm::event::Event;
use futures_core::Stream;
use tokio::sync::mpsc::UnboundedReceiver;

/// How long a confetti frame stays on screen: about 30 frames a second.
const FRAME: Duration = Duration::from_millis(33);

pub enum Wake<M> {
    Input(Event),
    Background(M),
    /// Confetti is falling and its next frame is due.
    Frame,
}

/// Waits for the next thing the event loop must act on. `events` is the terminal's input
/// (crossterm's `EventStream`), `background` the channel background tasks answer on, and
/// `animating` whether confetti is falling: only then does the wait end by itself, after `FRAME`.
pub async fn next<M>(
    events: &mut (impl Stream<Item = io::Result<Event>> + Unpin),
    background: &mut UnboundedReceiver<M>,
    animating: bool,
) -> io::Result<Wake<M>> {
    let input = poll_fn(|cx| Pin::new(&mut *events).poll_next(cx));
    tokio::select! {
        event = input => event
            .unwrap_or_else(|| Err(io::Error::new(io::ErrorKind::UnexpectedEof, "Terminal input ended")))
            .map(Wake::Input),
        Some(message) = background.recv() => Ok(Wake::Background(message)),
        () = tokio::time::sleep(FRAME), if animating => Ok(Wake::Frame),
    }
}

/// The next background message to apply before drawing: `woke_with`, the one that ended the last
/// wait, then every other that has arrived since, in the order they were sent.
pub fn arrived<M>(woke_with: &mut Option<M>, background: &mut UnboundedReceiver<M>) -> Option<M> {
    woke_with.take().or_else(|| background.try_recv().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent};
    use std::task::{Context, Poll};
    use tokio::sync::mpsc::unbounded_channel;
    use tokio::time::Instant;

    /// Terminal input that answers its first poll with this, then stays quiet.
    struct Typing(Poll<Option<Event>>);

    impl Stream for Typing {
        type Item = io::Result<Event>;

        fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            std::mem::replace(&mut self.0, Poll::Pending).map(|event| event.map(Ok))
        }
    }

    fn quiet() -> Typing {
        Typing(Poll::Pending)
    }

    /// A background task that answers `after` from now.
    fn answer_in(after: Duration) -> UnboundedReceiver<&'static str> {
        let (tx, rx) = unbounded_channel();
        tokio::spawn(async move {
            tokio::time::sleep(after).await;
            let _ = tx.send("transitions");
        });
        rx
    }

    #[tokio::test(start_paused = true)]
    async fn a_background_answer_ends_the_wait_the_moment_it_arrives() {
        for animating in [false, true] {
            let mut background = answer_in(Duration::from_millis(7));
            let started = Instant::now();
            let wake = next(&mut quiet(), &mut background, animating).await;
            assert!(matches!(wake, Ok(Wake::Background("transitions"))));
            assert_eq!(started.elapsed(), Duration::from_millis(7));
        }
    }

    #[tokio::test]
    async fn the_answer_that_ended_the_wait_is_applied_first_then_the_rest_in_order() {
        let (tx, mut background) = unbounded_channel();
        for message in ["first", "second", "third"] {
            tx.send(message).unwrap();
        }
        let wake = next(&mut quiet(), &mut background, false).await;
        let Ok(Wake::Background(first)) = wake else {
            panic!("a background answer ends the wait");
        };
        let mut woke_with = Some(first);
        let applied: Vec<_> =
            std::iter::from_fn(|| arrived(&mut woke_with, &mut background)).collect();
        assert_eq!(applied, ["first", "second", "third"]);
        assert_eq!(arrived(&mut woke_with, &mut background), None);
    }

    #[tokio::test(start_paused = true)]
    async fn confetti_ends_the_wait_after_one_frame_and_nothing_else_does() {
        let mut background = answer_in(Duration::from_secs(7200));
        let started = Instant::now();
        let wake = next(&mut quiet(), &mut background, true).await;
        assert!(matches!(wake, Ok(Wake::Frame)));
        assert_eq!(started.elapsed(), Duration::from_millis(33));

        // With no confetti an idle app is still waiting an hour later.
        let hour = Duration::from_secs(3600);
        let idle = tokio::time::timeout(hour, next(&mut quiet(), &mut background, false)).await;
        assert!(idle.is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn terminal_input_ends_the_wait_and_its_end_is_an_error() {
        let (_tx, mut background) = unbounded_channel::<&str>();
        let key = Event::Key(KeyEvent::from(KeyCode::Char('j')));
        let mut typed = Typing(Poll::Ready(Some(key.clone())));
        let wake = next(&mut typed, &mut background, false).await;
        assert!(matches!(wake, Ok(Wake::Input(event)) if event == key));

        let mut closed = Typing(Poll::Ready(None));
        let error = next(&mut closed, &mut background, false).await.err();
        assert_eq!(error.map(|e| e.kind()), Some(io::ErrorKind::UnexpectedEof));
    }
}
