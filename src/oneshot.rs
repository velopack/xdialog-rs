//! A write-once value from a backend to a caller: the reply to a dialog request. The receiver can
//! check for it, block on it or poll it as a future, as often as it likes (every read returns a
//! clone). Replaces `std::sync::mpsc` because a [`MessageDialogProxy`](crate::MessageDialogProxy)
//! has to be woken.
//!
//! Every delivery (a value, or the sender dropped without one) also wakes xdialog's event loop, so
//! a host app that checks a proxy in `about_to_wait` gets an iteration that sees it.

use std::sync::mpsc::RecvError;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Waker};
use std::time::Instant;

/// A write-once channel.
pub(crate) fn channel<T: Clone>() -> (Sender<T>, Receiver<T>) {
    let shared = Arc::new(Shared { state: Mutex::new(State { value: None, closed: false, waker: None }), cv: Condvar::new() });
    (Sender(shared.clone()), Receiver(shared))
}

struct Shared<T> {
    state: Mutex<State<T>>,
    cv: Condvar,
}

struct State<T> {
    value: Option<T>,
    /// The sender is gone.
    closed: bool,
    /// The task awaiting the value.
    waker: Option<Waker>,
}

impl<T> Shared<T> {
    fn lock(&self) -> MutexGuard<'_, State<T>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Set the value unless there is one. `Err(value)` if there was.
    fn set(&self, value: T) -> Result<(), T> {
        let mut s = self.lock();
        if s.value.is_some() {
            return Err(value);
        }
        s.value = Some(value);
        self.notify(s);
        Ok(())
    }

    /// Wake everything waiting for the value: blocked threads, the awaiting task, the event loop.
    fn notify(&self, mut s: MutexGuard<'_, State<T>>) {
        let waker = s.waker.take();
        drop(s);
        self.cv.notify_all();
        if let Some(w) = waker {
            w.wake();
        }
        crate::channel::wake_ui();
    }
}

/// Sets the value; the first value wins.
pub(crate) struct Sender<T>(Arc<Shared<T>>);

impl<T> Sender<T> {
    /// `Err(value)` if the value was set already.
    pub(crate) fn send(&self, value: T) -> Result<(), T> {
        self.0.set(value)
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        let mut s = self.0.lock();
        s.closed = true;
        if s.value.is_none() {
            self.0.notify(s); // disconnected: receivers get `RecvError`
        }
    }
}

/// How long [`Receiver::recv_with`] waits.
pub(crate) enum Wait<'a, 'b> {
    /// Not at all.
    Now,
    /// Block until the deadline (`None`: forever).
    Until(Option<Instant>),
    /// Register the task's waker instead of blocking.
    Poll(&'a mut Context<'b>),
}

pub(crate) struct Receiver<T>(Arc<Shared<T>>);

impl<T: Clone> Receiver<T> {
    /// The value, `Err` if the sender is gone without one, `None` if it's not there (yet) after
    /// waiting as `wait` says.
    pub(crate) fn recv_with(&self, wait: &mut Wait<'_, '_>) -> Option<Result<T, RecvError>> {
        let mut s = self.0.lock();
        loop {
            if let Some(v) = &s.value {
                return Some(Ok(v.clone()));
            }
            if s.closed {
                return Some(Err(RecvError));
            }
            s = match wait {
                Wait::Now => return None,
                Wait::Poll(cx) => {
                    s.waker = Some(cx.waker().clone());
                    return None;
                }
                Wait::Until(None) => self.0.cv.wait(s).unwrap_or_else(PoisonError::into_inner),
                Wait::Until(Some(deadline)) => {
                    let left = deadline.checked_duration_since(Instant::now()).filter(|d| !d.is_zero())?;
                    self.0.cv.wait_timeout(s, left).unwrap_or_else(PoisonError::into_inner).0
                }
            };
        }
    }

    /// Set the value from the receiving side (e.g. a timeout), unless the sender set one first.
    pub(crate) fn set(&self, value: T) {
        let _ = self.0.set(value);
    }

    #[cfg(test)]
    pub(crate) fn try_recv(&self) -> Result<T, std::sync::mpsc::TryRecvError> {
        use std::sync::mpsc::TryRecvError;
        match self.recv_with(&mut Wait::Now) {
            Some(r) => r.map_err(|_| TryRecvError::Disconnected),
            None => Err(TryRecvError::Empty),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::TryRecvError;
    use std::task::Wake;
    use std::time::Duration;

    use super::*;

    #[test]
    fn first_value_wins_and_is_kept() {
        let (tx, rx) = channel();
        assert_eq!(tx.send(1), Ok(()));
        assert_eq!(tx.send(2), Err(2));
        rx.set(3);
        drop(tx);
        assert_eq!(rx.try_recv(), Ok(1));
        assert_eq!(rx.try_recv(), Ok(1), "reads don't consume");
    }

    #[test]
    fn dropped_sender_disconnects() {
        let (tx, rx) = channel::<u32>();
        assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));
        drop(tx);
        assert_eq!(rx.recv_with(&mut Wait::Until(None)), Some(Err(RecvError)));
    }

    #[test]
    fn blocking_recv_and_deadline() {
        let (tx, rx) = channel();
        let soon = Instant::now() + Duration::from_millis(20);
        assert!(rx.recv_with(&mut Wait::Until(Some(soon))).is_none());
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            tx.send(5).unwrap();
        });
        assert_eq!(rx.recv_with(&mut Wait::Until(None)), Some(Ok(5)));
        t.join().unwrap();
    }

    #[test]
    fn poll_registers_the_waker() {
        struct Flag(std::sync::atomic::AtomicBool);
        impl Wake for Flag {
            fn wake(self: Arc<Self>) {
                self.0.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let flag = Arc::new(Flag(false.into()));
        let waker = Waker::from(flag.clone());
        let mut cx = Context::from_waker(&waker);
        let (tx, rx) = channel();
        assert!(rx.recv_with(&mut Wait::Poll(&mut cx)).is_none());
        tx.send(()).unwrap();
        assert!(flag.0.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(rx.recv_with(&mut Wait::Poll(&mut cx)), Some(Ok(())));
    }
}
