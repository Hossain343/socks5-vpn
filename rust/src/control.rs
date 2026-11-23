//! Platform-neutral start/stop of the engine. Android feeds it a TUN fd,
//! iOS feeds it packets through a callback (NEPacketTunnelProvider has no fd).
use crate::{
    engine::{self, TunIo},
    socks::ProxyCfg,
    stats,
};
use std::{
    sync::{atomic::Ordering::Relaxed, Mutex, MutexGuard},
    time::Duration,
};
use tokio::{
    runtime::{Builder, Runtime},
    sync::mpsc,
};
use tokio_util::sync::CancellationToken;

/// Called for every IP packet the engine wants to write to the device.
pub type PacketSink = Box<dyn Fn(&[u8]) + Send + Sync + 'static>;

pub enum Tun {
    #[cfg(target_os = "android")]
    Fd(i32),
    Callback(PacketSink),
}

struct Engine {
    rt: Runtime,
    cancel: CancellationToken,
    input: Option<mpsc::Sender<Vec<u8>>>,
}

static ENGINE: Mutex<Option<Engine>> = Mutex::new(None);

fn engine() -> MutexGuard<'static, Option<Engine>> {
    ENGINE.lock().unwrap_or_else(|p| p.into_inner())
}

pub fn stop() {
    let e = engine().take();
    if let Some(e) = e {
        e.cancel.cancel();
        e.rt.shutdown_timeout(Duration::from_millis(800));
    }
    stats::CONNECTED.store(false, Relaxed);
}

fn finish(r: std::io::Result<()>) {
    stats::CONNECTED.store(false, Relaxed);
    if let Err(e) = r {
        stats::error(format!("engine: {e}"));
    }
}

pub fn start(cfg: ProxyCfg, tun: Tun) -> Result<(), String> {
    stop();
    stats::reset();
    let workers = if cfg!(target_os = "android") { 3 } else { 2 };
    let rt = Builder::new_multi_thread()
        .worker_threads(workers)
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let cancel = CancellationToken::new();
    stats::CONNECTED.store(true, Relaxed);
    let mut input = None;

    match tun {
        #[cfg(target_os = "android")]
        Tun::Fd(fd) => {
            let c = cancel.clone();
            rt.spawn(async move {
                finish(crate::tunfd::run(cfg, fd, c).await);
            });
        }
        Tun::Callback(sink) => {
            let (in_tx, in_rx) = mpsc::channel::<Vec<u8>>(2048);
            let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(2048);
            let c1 = cancel.clone();
            rt.spawn(async move {
                loop {
                    let it = tokio::select! { i = out_rx.recv() => i, _ = c1.cancelled() => None };
                    match it {
                        Some(p) => sink(&p),
                        None => break,
                    }
                }
            });
            let c2 = cancel.clone();
            rt.spawn(async move {
                finish(engine::run_core(cfg, TunIo { rx: in_rx, tx: out_tx }, c2).await);
            });
            input = Some(in_tx);
        }
    }

    *engine() = Some(Engine { rt, cancel, input });
    Ok(())
}

/// iOS path: a packet read from `packetFlow`. Never blocks; drops when overloaded.
pub fn input(data: &[u8]) {
    let tx = engine().as_ref().and_then(|e| e.input.clone());
    if let Some(tx) = tx {
        let _ = tx.try_send(data.to_vec());
    }
}
