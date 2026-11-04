//! Whether live mode runs or the presentation window is open, readable without any lock.
//!
//! Their latency budget is 30 ms (SPEC §4.2), so background work (the handwriting indexer)
//! waits while either runs. It asks often, and from a thread that must never wait for — nor
//! hold up — the locks of live mode or of the interface (the hub's state, Tauri's window and
//! state managers): hence atomics, set by the hub at every change of its session and by the
//! presentation window's creation and destruction.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// The flags, shared by clones (the hub, the presentation window, the indexer).
#[derive(Debug, Clone, Default)]
pub struct Activity {
    flags: Arc<Flags>,
}

#[derive(Debug, Default)]
struct Flags {
    live: AtomicBool,
    /// Token of the presentation window open now; 0 when there is none.
    presentation: AtomicU64,
    /// Last token given.
    last_token: AtomicU64,
}

impl Activity {
    /// True while live mode runs or the presentation window is open.
    pub fn busy(&self) -> bool {
        self.live() || self.presentation_open()
    }

    pub fn live(&self) -> bool {
        self.flags.live.load(Ordering::Acquire)
    }

    pub fn presentation_open(&self) -> bool {
        self.flags.presentation.load(Ordering::Acquire) != 0
    }

    /// Live mode started or stopped (the hub, after every change of its session). Returns what it
    /// was before.
    pub(crate) fn set_live(&self, running: bool) -> bool {
        self.flags.live.swap(running, Ordering::AcqRel)
    }

    /// A presentation window is being created: it counts as open until
    /// [`presentation_closed`](Self::presentation_closed) with the token returned here.
    pub(crate) fn presentation_opened(&self) -> u64 {
        let token = self.flags.last_token.fetch_add(1, Ordering::Relaxed) + 1;
        self.flags.presentation.store(token, Ordering::Release);
        token
    }

    /// The window of `token` is gone (closed by the user, by the app, or never created). A
    /// window created since keeps the flag: the destruction of the previous one may be reported
    /// after the next one was created.
    pub(crate) fn presentation_closed(&self, token: u64) {
        let _ =
            self.flags
                .presentation
                .compare_exchange(token, 0, Ordering::AcqRel, Ordering::Acquire);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_mode_and_the_presentation_make_it_busy() {
        let activity = Activity::default();
        let indexer = activity.clone();
        assert!(!indexer.busy());
        activity.set_live(true);
        assert!(indexer.busy() && indexer.live());
        activity.set_live(false);
        assert!(!indexer.busy());

        let window = activity.presentation_opened();
        assert!(indexer.busy() && indexer.presentation_open());
        activity.presentation_closed(window);
        assert!(!indexer.busy());

        // Both at once: busy until both ended.
        activity.set_live(true);
        let window = activity.presentation_opened();
        activity.set_live(false);
        assert!(indexer.busy());
        activity.presentation_closed(window);
        assert!(!indexer.busy());
    }

    #[test]
    fn a_late_report_of_an_old_window_keeps_the_new_one() {
        let activity = Activity::default();
        let first = activity.presentation_opened();
        // Closed and opened again before the first window's destruction is reported.
        let second = activity.presentation_opened();
        assert_ne!(first, second);
        activity.presentation_closed(first);
        assert!(activity.presentation_open());
        activity.presentation_closed(second);
        assert!(!activity.presentation_open());
        // Reported twice: no harm.
        activity.presentation_closed(second);
        assert!(!activity.busy());
    }
}
