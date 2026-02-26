use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{LazyLock, OnceLock, RwLock};

use crate::*;

static REQUEST_SEND: OnceLock<Sender<DialogMessageRequest>> = OnceLock::new();
static SILENT: AtomicBool = AtomicBool::new(false);
static NEXT_ID: AtomicUsize = AtomicUsize::new(1);
static RESULT_MAP: LazyLock<RwLock<HashMap<usize, XDialogResult>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

pub fn set_silent(silent: bool) {
    SILENT.store(silent, Ordering::Relaxed);
}

pub fn get_silent() -> bool {
    SILENT.load(Ordering::Relaxed)
}

pub fn insert_result(key: usize, result: XDialogResult) {
    let mut map = RESULT_MAP.write().unwrap();
    if map.contains_key(&key) {
        return; // don't overwrite existing results
    }
    map.insert(key, result);
}

pub fn get_result(key: usize) -> Option<XDialogResult> {
    let map = RESULT_MAP.read().unwrap();
    map.get(&key).cloned()
}

pub fn get_next_id() -> usize {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

pub fn init_sender(sender: Sender<DialogMessageRequest>) {
    REQUEST_SEND.set(sender).unwrap();
}

pub fn send_request(message: DialogMessageRequest) -> Result<(), XDialogError> {
    let once = REQUEST_SEND.get();
    if once.is_none() {
        return Err(XDialogError::NotInitialized);
    }

    once.unwrap().send(message).map_err(|e| XDialogError::SendFailed(e))?;
    Ok(())
}
