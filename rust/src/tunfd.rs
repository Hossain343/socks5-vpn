//! Android: bridge a non-blocking TUN fd (from VpnService) to the engine's packet channels.
use crate::{
    engine::{self, TunIo},
    socks::ProxyCfg,
};
use std::{
    io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    sync::Arc,
};
use tokio::{io::unix::AsyncFd, sync::mpsc};
use tokio_util::sync::CancellationToken;

fn flatten(r: Result<io::Result<()>, tokio::task::JoinError>) -> io::Result<()> {
    match r {
        Ok(v) => v,
        Err(e) => Err(io::Error::new(io::ErrorKind::Other, e.to_string())),
    }
}

pub async fn run(cfg: ProxyCfg, raw_fd: i32, cancel: CancellationToken) -> io::Result<()> {
    // Rust owns the TUN fd from here on; it is closed when the last Arc is dropped.
    let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };
    unsafe {
        let fl = libc::fcntl(fd.as_raw_fd(), libc::F_GETFL);
        libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, fl | libc::O_NONBLOCK);
    }
    let afd = Arc::new(AsyncFd::new(fd)?);

    let (in_tx, in_rx) = mpsc::channel::<Vec<u8>>(2048);
    let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>(2048);
    let mut rd = tokio::spawn(tun_read(afd.clone(), in_tx, cancel.clone()));
    let mut wr = tokio::spawn(tun_write(afd, out_rx, cancel.clone()));
    let core = engine::run_core(cfg, TunIo { rx: in_rx, tx: out_tx }, cancel.clone());
    tokio::pin!(core);

    let res = tokio::select! {
        r = &mut core => r,
        r = &mut rd => flatten(r),
        r = &mut wr => flatten(r),
    };
    cancel.cancel();
    res
}

async fn tun_read(afd: Arc<AsyncFd<OwnedFd>>, tx: mpsc::Sender<Vec<u8>>, cancel: CancellationToken) -> io::Result<()> {
    let mut buf = vec![0u8; 65536];
    loop {
        let mut g = tokio::select! {
            r = afd.readable() => r?,
            _ = cancel.cancelled() => return Ok(()),
        };
        let res = g.try_io(|i| {
            let n = unsafe { libc::read(i.as_raw_fd(), buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
            if n < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(n as usize)
            }
        });
        match res {
            Ok(Ok(0)) => return Ok(()),
            Ok(Ok(n)) => {
                if tx.send(buf[..n].to_vec()).await.is_err() {
                    return Ok(());
                }
            }
            Ok(Err(e)) if e.kind() == io::ErrorKind::Interrupted => continue,
            Ok(Err(e)) => return Err(e),
            Err(_would_block) => continue,
        }
    }
}

async fn tun_write(afd: Arc<AsyncFd<OwnedFd>>, mut rx: mpsc::Receiver<Vec<u8>>, cancel: CancellationToken) -> io::Result<()> {
    loop {
        let item = tokio::select! { p = rx.recv() => p, _ = cancel.cancelled() => None };
        let Some(pkt) = item else { return Ok(()) };
        loop {
            let mut g = afd.writable().await?;
            let res = g.try_io(|i| {
                let n = unsafe { libc::write(i.as_raw_fd(), pkt.as_ptr() as *const libc::c_void, pkt.len()) };
                if n < 0 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(n as usize)
                }
            });
            match res {
                Ok(Ok(_)) => break,
                Ok(Err(e)) if e.kind() == io::ErrorKind::Interrupted => continue,
                Ok(Err(_)) => break, // drop this packet, keep the tunnel alive
                Err(_would_block) => continue,
            }
        }
    }
}
