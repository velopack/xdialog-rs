use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

static SILENT: AtomicBool = AtomicBool::new(false);
static NEXT_ID: AtomicUsize = AtomicUsize::new(1);

pub fn set_silent(silent: bool) {
    SILENT.store(silent, Ordering::Relaxed);
}

pub fn get_silent() -> bool {
    SILENT.load(Ordering::Relaxed)
}

pub fn get_next_id() -> usize {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}
