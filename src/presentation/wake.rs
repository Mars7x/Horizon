//! The UI half of `services::worker`: turning a worker's [`Notify`] into a
//! run of a UI-thread handler.
//!
//! [`ui_notify`] registers a handler on the UI thread (it may hold `Rc`
//! controllers) and returns a `Notify` any thread can call. Calls are
//! coalesced: however many results land together, the handler runs once on
//! the next event-loop turn and drains everything waiting.

use std::{
    cell::{OnceCell, RefCell},
    rc::{Rc, Weak},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::services::worker::Notify;

thread_local! {
    static HANDLERS: RefCell<Vec<Rc<dyn Fn()>>> = const { RefCell::new(Vec::new()) };
}

/// Call on the UI thread. `handler` runs on the UI thread after the returned
/// `Notify` is called, once per burst of calls.
pub fn ui_notify(handler: impl Fn() + 'static) -> Notify {
    let index = HANDLERS.with(|handlers| {
        let mut handlers = handlers.borrow_mut();
        handlers.push(Rc::new(handler));
        handlers.len() - 1
    });
    let scheduled = Arc::new(AtomicBool::new(false));
    Arc::new(move || {
        if scheduled.swap(true, Ordering::AcqRel) {
            return;
        }
        let scheduled = Arc::clone(&scheduled);
        // Fails only once the event loop has quit; nothing is left to wake.
        let _ = slint::invoke_from_event_loop(move || {
            // Cleared before running, so results sent while the handler
            // drains schedule one more run instead of being missed.
            scheduled.store(false, Ordering::Release);
            let handler = HANDLERS.with(|handlers| handlers.borrow().get(index).cloned());
            if let Some(handler) = handler {
                handler();
            }
        });
    })
}

/// A [`ui_notify`] whose handler drains a controller built afterwards (the
/// controller needs the `Notify` to start its workers). Call [`Drain::bind`]
/// once the controller exists; wake-ups before then are ignored.
pub struct Drain<C> {
    target: Rc<OnceCell<Weak<C>>>,
}

pub fn drain_notify<C: 'static>(drain: impl Fn(&C) + 'static) -> (Notify, Drain<C>) {
    let target: Rc<OnceCell<Weak<C>>> = Rc::new(OnceCell::new());
    let handler_target = Rc::clone(&target);
    let notify = ui_notify(move || {
        if let Some(controller) = handler_target.get().and_then(Weak::upgrade) {
            drain(&controller);
        }
    });
    (notify, Drain { target })
}

impl<C> Drain<C> {
    pub fn bind(self, controller: &Rc<C>) {
        let _ = self.target.set(Rc::downgrade(controller));
    }
}
