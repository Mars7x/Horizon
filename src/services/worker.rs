//! Handing results from worker threads to the UI thread without polling.
//!
//! Every background job (artwork, achievements, Album media, system status)
//! sends its results through a [`WorkerSender`]. Each send also calls the
//! channel's [`Notify`], which the presentation layer turns into one
//! coalesced wake-up of the UI thread (`presentation::wake::ui_notify`). The
//! UI thread then drains the receiver. Nothing checks a channel on a timer,
//! so an idle Horizon does no work.

use std::sync::{
    Arc,
    mpsc::{self, Receiver, SendError, Sender},
};

/// Wakes whoever drains a channel. Cheap, callable from any thread, and safe
/// to call many times in a row (the UI side coalesces).
pub type Notify = Arc<dyn Fn() + Send + Sync>;

/// A `Notify` that does nothing: tests, and receivers drained some other way.
pub fn no_notify() -> Notify {
    Arc::new(|| {})
}

/// The sending half of a worker channel: `send` delivers, then notifies.
pub struct WorkerSender<T> {
    sender: Sender<T>,
    notify: Notify,
}

impl<T> WorkerSender<T> {
    pub fn send(&self, value: T) -> Result<(), SendError<T>> {
        self.sender.send(value)?;
        (self.notify)();
        Ok(())
    }
}

impl<T> Clone for WorkerSender<T> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            notify: Arc::clone(&self.notify),
        }
    }
}

/// Starts a named background thread. A thread that can't start (the system
/// is out of threads) is logged and skipped, never a panic: callers treat it
/// like a job that produced nothing. Returns whether it started.
pub fn spawn(name: &str, job: impl FnOnce() + Send + 'static) -> bool {
    match std::thread::Builder::new().name(name.to_owned()).spawn(job) {
        Ok(_) => true,
        Err(error) => {
            tracing::warn!(%error, thread = name, "background thread did not start");
            false
        }
    }
}

/// A channel whose every send calls `notify`.
pub fn worker_channel<T>(notify: Notify) -> (WorkerSender<T>, Receiver<T>) {
    let (sender, receiver) = mpsc::channel();
    (WorkerSender { sender, notify }, receiver)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn every_send_notifies_after_delivering() {
        let count = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&count);
        let (sender, receiver) = worker_channel(Arc::new(move || {
            seen.fetch_add(1, Ordering::SeqCst);
        }));
        let worker = sender.clone();
        std::thread::spawn(move || {
            worker.send(1).unwrap();
            worker.send(2).unwrap();
        })
        .join()
        .unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 2);
        assert_eq!(receiver.try_iter().collect::<Vec<_>>(), [1, 2]);
    }

    #[test]
    fn a_closed_channel_does_not_notify() {
        let count = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&count);
        let (sender, receiver) = worker_channel(Arc::new(move || {
            seen.fetch_add(1, Ordering::SeqCst);
        }));
        drop(receiver);
        assert!(sender.send(1).is_err());
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }
}
