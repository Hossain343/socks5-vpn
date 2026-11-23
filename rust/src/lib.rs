mod control;
mod dns;
mod engine;
mod ffi; // C ABI used by the iOS Packet Tunnel extension (compiled on every target)
mod socks;
mod stats;

#[cfg(target_os = "android")]
mod jni_android;
#[cfg(target_os = "android")]
mod tunfd;
