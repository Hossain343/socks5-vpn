use crate::{
    control::{self, Tun},
    socks::ProxyCfg,
    stats,
};
use jni::{
    objects::{JClass, JString},
    sys::{jboolean, jint, jlongArray, jstring, JNI_FALSE, JNI_TRUE},
    JNIEnv,
};
use std::{ptr, sync::atomic::Ordering::Relaxed};

fn jstr(env: &mut JNIEnv, s: &JString) -> String {
    env.get_string(s).map(String::from).unwrap_or_default()
}

#[no_mangle]
pub extern "system" fn Java_com_vpnshare_client_Native_start<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    fd: jint,
    host: JString<'l>,
    port: jint,
    user: JString<'l>,
    pass: JString<'l>,
) -> jboolean {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("vpnclient"),
    );
    let cfg = ProxyCfg {
        host: jstr(&mut env, &host),
        port: port as u16,
        user: jstr(&mut env, &user),
        pass: jstr(&mut env, &pass),
    };
    match control::start(cfg, Tun::Fd(fd)) {
        Ok(()) => JNI_TRUE,
        Err(e) => {
            stats::error(format!("start: {e}"));
            JNI_FALSE
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_com_vpnshare_client_Native_stop<'l>(_env: JNIEnv<'l>, _c: JClass<'l>) {
    control::stop();
}

/// [connected(0/1), bytesUp, bytesDown, activeSessions, errors]
#[no_mangle]
pub extern "system" fn Java_com_vpnshare_client_Native_stats<'l>(mut env: JNIEnv<'l>, _c: JClass<'l>) -> jlongArray {
    let vals: [i64; 5] = [
        stats::CONNECTED.load(Relaxed) as i64,
        stats::UP.load(Relaxed) as i64,
        stats::DOWN.load(Relaxed) as i64,
        stats::SESSIONS.load(Relaxed),
        stats::ERRORS.load(Relaxed) as i64,
    ];
    match env.new_long_array(5) {
        Ok(arr) => {
            if env.set_long_array_region(&arr, 0, &vals).is_err() {
                return ptr::null_mut();
            }
            arr.into_raw()
        }
        Err(_) => ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "system" fn Java_com_vpnshare_client_Native_lastError<'l>(mut env: JNIEnv<'l>, _c: JClass<'l>) -> jstring {
    match env.new_string(stats::last_error()) {
        Ok(s) => s.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}
