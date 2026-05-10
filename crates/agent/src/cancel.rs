use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::sync::Notify;

/// One-shot cancellation flag, shared by `clone()`. Cheap to await: the future
/// resolves immediately if cancellation already happened, otherwise it waits
/// on a `Notify`.
#[derive(Clone, Default)]
pub struct Cancel(Arc<Inner>);

#[derive(Default)]
struct Inner {
    flagged: AtomicBool,
    notify: Notify,
}

impl Cancel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        // Release: pair with the Acquire load below so observers see the flag
        // before they check `is_cancelled`.
        self.0.flagged.store(true, Ordering::Release);
        self.0.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.flagged.load(Ordering::Acquire)
    }

    /// Resolves once `cancel()` has been called. Re-checks the flag after
    /// registering with `Notify` to close the wake-before-await race.
    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        let notified = self.0.notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if self.is_cancelled() {
            return;
        }
        notified.await;
    }
}
