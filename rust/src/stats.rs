use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering::Relaxed};
use std::sync::Mutex;

pub static CONNECTED: AtomicBool = AtomicBool::new(false);
pub static UP: AtomicU64 = AtomicU64::new(0);
pub static DOWN: AtomicU64 = AtomicU64::new(0);
pub static ERRORS: AtomicU64 = AtomicU64::new(0);
pub static SESSIONS: AtomicI64 = AtomicI64::new(0);
static LAST_ERROR: Mutex<String> = Mutex::new(String::new());

pub fn reset() {
    UP.store(0, Relaxed);
    DOWN.store(0, Relaxed);
    ERRORS.store(0, Relaxed);
    SESSIONS.store(0, Relaxed);
    if let Ok(mut g) = LAST_ERROR.lock() {
        g.clear();
    }
}

pub fn error(msg: String) {
    log::warn!("{msg}");
    ERRORS.fetch_add(1, Relaxed);
    if let Ok(mut g) = LAST_ERROR.lock() {
        *g = msg;
    }
}

pub fn last_error() -> String {
    LAST_ERROR.lock().map(|g| g.clone()).unwrap_or_default()
}

pub struct SessionGuard;

impl SessionGuard {
    pub fn new() -> Self {
        SESSIONS.fetch_add(1, Relaxed);
        SessionGuard
    }
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        SESSIONS.fetch_sub(1, Relaxed);
    }
}
