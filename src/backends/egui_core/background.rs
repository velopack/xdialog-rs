//! A value produced on a background thread; readers wait a bounded time for it (Linux: the
//! appearance portal and the system font scan).

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

pub(crate) struct Background<T> {
    /// (thread started, value)
    state: Mutex<(bool, Option<T>)>,
    cv: Condvar,
}

impl<T: Clone + Default + Send + 'static> Background<T> {
    pub(crate) const fn new() -> Self {
        Background { state: Mutex::new((false, None)), cv: Condvar::new() }
    }

    fn lock(&self) -> MutexGuard<'_, (bool, Option<T>)> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Run `f` on a new thread `name`, once per process; `f` publishes with [`Background::set`].
    /// If the thread can't start, or `f` ends (or panics) without a value, the value becomes
    /// `T::default()` so readers stop waiting.
    pub(crate) fn start(&'static self, name: &str, f: impl FnOnce() + Send + 'static) {
        if std::mem::replace(&mut self.lock().0, true) {
            return;
        }
        let spawned = std::thread::Builder::new().name(name.into()).spawn(move || {
            if catch_unwind(AssertUnwindSafe(f)).is_err() {
                warn!("xdialog: a background thread panicked");
            }
            self.set_if_none();
        });
        if let Err(e) = spawned {
            warn!("xdialog: could not start thread {name}: {e}");
            self.set_if_none();
        }
    }

    fn set_if_none(&self) {
        self.lock().1.get_or_insert_with(T::default);
        self.cv.notify_all();
    }

    /// Store `v`; returns the previous value.
    pub(crate) fn set(&self, v: T) -> Option<T> {
        let old = self.lock().1.replace(v);
        self.cv.notify_all();
        old
    }

    /// The value, waiting up to `wait` for the first one (`None`: not started, or none yet).
    pub(crate) fn get(&self, wait: Duration) -> Option<T> {
        let deadline = Instant::now() + wait;
        let mut st = self.lock();
        loop {
            let now = Instant::now();
            if st.1.is_some() || !st.0 || now >= deadline {
                return st.1.clone();
            }
            st = self.cv.wait_timeout(st, deadline - now).unwrap_or_else(|e| e.into_inner()).0;
        }
    }

    /// Whether a value is available.
    pub(crate) fn done(&self) -> bool {
        self.lock().1.is_some()
    }
}
