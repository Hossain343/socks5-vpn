//! C ABI for the iOS Packet Tunnel extension (see ios/include/vpnclient.h).
use crate::{
    control::{self, PacketSink, Tun},
    socks::ProxyCfg,
    stats,
};
use std::{
    ffi::{c_char, c_void, CStr},
    ptr, slice,
    sync::atomic::Ordering::Relaxed,
};

pub type PacketCb = extern "C" fn(data: *const u8, len: usize, ctx: *mut c_void);

struct CbCtx {
    cb: PacketCb,
    ctx: *mut c_void,
}
// The callback and context are provided by Swift and are valid until vpn_stop().
unsafe impl Send for CbCtx {}
unsafe impl Sync for CbCtx {}

impl CbCtx {
    fn call(&self, p: &[u8]) {
        (self.cb)(p.as_ptr(), p.len(), self.ctx);
    }
}

fn cstr(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// Returns 0 on success, -1 on failure (see vpn_last_error).
#[no_mangle]
pub extern "C" fn vpn_start(
    host: *const c_char,
    port: u16,
    user: *const c_char,
    pass: *const c_char,
    cb: PacketCb,
    ctx: *mut c_void,
) -> i32 {
    let cfg = ProxyCfg { host: cstr(host), port, user: cstr(user), pass: cstr(pass) };
    if cfg.host.is_empty() || cfg.port == 0 {
        stats::error("invalid server settings".to_string());
        return -1;
    }
    let c = CbCtx { cb, ctx };
    let sink: PacketSink = Box::new(move |p: &[u8]| c.call(p));
    match control::start(cfg, Tun::Callback(sink)) {
        Ok(()) => 0,
        Err(e) => {
            stats::error(format!("start: {e}"));
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn vpn_input(data: *const u8, len: usize) {
    if data.is_null() || len == 0 {
        return;
    }
    control::input(unsafe { slice::from_raw_parts(data, len) });
}

#[no_mangle]
pub extern "C" fn vpn_stop() {
    control::stop();
}

/// Writes 5 values: connected(0/1), bytesUp, bytesDown, activeSessions, errors.
#[no_mangle]
pub extern "C" fn vpn_stats(out: *mut i64) {
    if out.is_null() {
        return;
    }
    let v: [i64; 5] = [
        stats::CONNECTED.load(Relaxed) as i64,
        stats::UP.load(Relaxed) as i64,
        stats::DOWN.load(Relaxed) as i64,
        stats::SESSIONS.load(Relaxed),
        stats::ERRORS.load(Relaxed) as i64,
    ];
    unsafe { ptr::copy_nonoverlapping(v.as_ptr(), out, 5) };
}

/// Copies the last error (NUL-terminated, truncated to cap-1) into buf. Returns bytes written.
#[no_mangle]
pub extern "C" fn vpn_last_error(buf: *mut c_char, cap: usize) -> usize {
    if buf.is_null() || cap == 0 {
        return 0;
    }
    let s = stats::last_error();
    let b = s.as_bytes();
    let n = b.len().min(cap - 1);
    unsafe {
        ptr::copy_nonoverlapping(b.as_ptr(), buf as *mut u8, n);
        *buf.add(n) = 0;
    }
    n
}
