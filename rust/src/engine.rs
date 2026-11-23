use crate::{
    dns::{self, FakeDns},
    socks::{self, ProxyCfg, Target},
    stats,
};
use futures::{SinkExt, Stream, StreamExt};
use std::{
    collections::{HashMap, HashSet},
    io,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{
        atomic::{AtomicU64, Ordering::Relaxed},
        Arc, Mutex, MutexGuard,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::UdpSocket,
    sync::mpsc::{self, error::TrySendError},
    time::{interval, timeout, Instant},
};
use tokio_util::sync::CancellationToken;

const UDP_IDLE: Duration = Duration::from_secs(180); // same as udp.rs on the server
const TCP_IDLE: Duration = Duration::from_secs(600);
// iOS Network Extensions are capped at ~50 MB, so per-flow buffers are smaller there.
#[cfg(target_os = "ios")]
const PUMP_BUF: usize = 8 * 1024;
#[cfg(not(target_os = "ios"))]
const PUMP_BUF: usize = 32 * 1024;
#[cfg(target_os = "ios")]
const UDP_BUF: usize = 8 * 1024;
#[cfg(not(target_os = "ios"))]
const UDP_BUF: usize = 65536;

type UdpOut = (Vec<u8>, SocketAddr, SocketAddr); // (payload, from, to)
type SessTx = mpsc::Sender<(Vec<u8>, SocketAddr)>; // (payload, tunnel destination)
type SessMap = Arc<Mutex<HashMap<SocketAddr, SessTx>>>;

fn lk<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn other(m: String) -> io::Error {
    io::Error::new(io::ErrorKind::Other, m)
}

/// Packet pipes between the platform tunnel and the engine.
pub struct TunIo {
    pub rx: mpsc::Receiver<Vec<u8>>, // device -> engine
    pub tx: mpsc::Sender<Vec<u8>>,   // engine -> device
}

pub async fn run_core(cfg: ProxyCfg, io: TunIo, cancel: CancellationToken) -> io::Result<()> {
    let (stack, runner, udp, tcp) = netstack_smoltcp::StackBuilder::default()
        .enable_tcp(true)
        .enable_udp(true)
        .build()
        .map_err(|e| other(format!("netstack: {e}")))?;
    let (mut sink, mut stream) = StreamExt::split::<Vec<u8>>(stack);

    if let Some(r) = runner {
        tokio::spawn(async move {
            let _ = r.await;
        });
    }

    let cfg = Arc::new(cfg);
    let fake = Arc::new(FakeDns::new());
    let TunIo { mut rx, tx } = io;

    let c = cancel.clone();
    let mut feed = tokio::spawn(async move {
        loop {
            let it = tokio::select! { p = rx.recv() => p, _ = c.cancelled() => None };
            match it {
                Some(p) => {
                    if sink.send(p).await.is_err() {
                        break;
                    }
                }
                None => break,
            }
        }
    });

    let c = cancel.clone();
    let mut drain = tokio::spawn(async move {
        loop {
            let it = tokio::select! { i = stream.next() => i, _ = c.cancelled() => None };
            match it {
                Some(Ok(p)) => {
                    if tx.send(p).await.is_err() {
                        break;
                    }
                }
                Some(Err(_)) => continue,
                None => break,
            }
        }
    });

    if let Some(l) = tcp {
        tokio::spawn(tcp_accept(l, cfg.clone(), fake.clone(), cancel.clone()));
    }
    if let Some(u) = udp {
        let (urx, mut utx) = u.split();
        let (out_tx, mut out_rx) = mpsc::channel::<UdpOut>(2048);
        let c = cancel.clone();
        tokio::spawn(async move {
            loop {
                let it = tokio::select! { i = out_rx.recv() => i, _ = c.cancelled() => None };
                match it {
                    Some(p) => {
                        if utx.send(p).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
        });
        tokio::spawn(udp_loop(urx, out_tx, cfg.clone(), fake.clone(), cancel.clone()));
    }

    tokio::select! {
        _ = cancel.cancelled() => {}
        _ = &mut feed => {}
        _ = &mut drain => {}
    }
    cancel.cancel();
    Ok(())
}

// -------------------------------------------------------------------- TCP

async fn pump<R, W>(r: &mut R, w: &mut W, ctr: &AtomicU64) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buf = vec![0u8; PUMP_BUF];
    loop {
        let n = match timeout(TCP_IDLE, r.read(&mut buf)).await {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err(other("idle timeout".into())),
        };
        if n == 0 {
            let _ = w.shutdown().await;
            return Ok(());
        }
        w.write_all(&buf[..n]).await?;
        ctr.fetch_add(n as u64, Relaxed);
    }
}

async fn tcp_accept<L, T>(mut l: L, cfg: Arc<ProxyCfg>, dns: Arc<FakeDns>, cancel: CancellationToken)
where
    L: Stream<Item = (T, SocketAddr, SocketAddr)> + Unpin,
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    loop {
        let it = tokio::select! { i = l.next() => i, _ = cancel.cancelled() => None };
        let Some((stream, _src, dst)) = it else { break };
        tokio::spawn(handle_tcp(stream, dst, cfg.clone(), dns.clone(), cancel.clone()));
    }
}

async fn handle_tcp<T>(app: T, dst: SocketAddr, cfg: Arc<ProxyCfg>, dns: Arc<FakeDns>, cancel: CancellationToken)
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let _g = stats::SessionGuard::new();
    let target = socks::resolve(&dns, dst);
    let proxy = match socks::tcp_connect(&cfg, &target).await {
        Ok(p) => p,
        Err(e) => {
            stats::error(format!("tcp {:?}: {e}", target));
            return; // dropping `app` resets the client connection
        }
    };
    let (mut ar, mut aw) = tokio::io::split(app);
    let (mut pr, mut pw) = proxy.into_split();
    tokio::select! {
        _ = cancel.cancelled() => {}
        _ = async {
            let _ = tokio::try_join!(
                pump(&mut ar, &mut pw, &stats::UP),
                pump(&mut pr, &mut aw, &stats::DOWN),
            );
        } => {}
    }
}

// -------------------------------------------------------------------- UDP

async fn udp_loop<R>(mut rx: R, out: mpsc::Sender<UdpOut>, cfg: Arc<ProxyCfg>, dns: Arc<FakeDns>, cancel: CancellationToken)
where
    R: Stream<Item = UdpOut> + Unpin,
{
    let map: SessMap = Arc::new(Mutex::new(HashMap::new()));
    loop {
        let it = tokio::select! { i = rx.next() => i, _ = cancel.cancelled() => None };
        let Some((data, src, dst)) = it else { break };

        // Local DNS: any UDP/53 query is answered right here from the fake pool.
        if dst.port() == 53 {
            if let Some(resp) = dns::answer(&dns, &data) {
                let _ = out.try_send((resp, dst, src));
            }
            continue;
        }

        let mut pkt = (data, dst);
        for _ in 0..2 {
            let existing = lk(&map).get(&src).cloned();
            let tx = match existing {
                Some(t) => t,
                None => {
                    let (t, r) = mpsc::channel(512);
                    lk(&map).insert(src, t.clone());
                    tokio::spawn(udp_session(src, r, map.clone(), out.clone(), cfg.clone(), dns.clone(), cancel.clone()));
                    t
                }
            };
            match tx.try_send(pkt) {
                Ok(()) => break,
                Err(TrySendError::Full(_)) => break, // UDP: drop under pressure
                Err(TrySendError::Closed(p)) => {
                    lk(&map).remove(&src);
                    pkt = p;
                }
            }
        }
    }
}

async fn udp_session(
    src: SocketAddr,
    mut rx: mpsc::Receiver<(Vec<u8>, SocketAddr)>,
    map: SessMap,
    out: mpsc::Sender<UdpOut>,
    cfg: Arc<ProxyCfg>,
    dns: Arc<FakeDns>,
    cancel: CancellationToken,
) {
    let _g = stats::SessionGuard::new();
    if let Err(e) = udp_session_inner(src, &mut rx, &out, &cfg, &dns, &cancel).await {
        stats::error(format!("udp {src}: {e}"));
    }
    lk(&map).remove(&src);
}

async fn udp_session_inner(
    src: SocketAddr,
    rx: &mut mpsc::Receiver<(Vec<u8>, SocketAddr)>,
    out: &mpsc::Sender<UdpOut>,
    cfg: &ProxyCfg,
    dns: &FakeDns,
    cancel: &CancellationToken,
) -> io::Result<()> {
    let (mut ctrl, relay) = socks::udp_associate(cfg).await?;
    let sock = UdpSocket::bind(if relay.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" }).await?;
    sock.connect(relay).await?;

    let mut last_fake: Option<SocketAddr> = None;
    let mut fake_by_name: HashMap<String, SocketAddr> = HashMap::new();
    let mut direct: HashSet<SocketAddr> = HashSet::new();
    let mut last = Instant::now();
    let mut tick = interval(Duration::from_secs(15));
    let mut rbuf = vec![0u8; UDP_BUF];
    let mut cbuf = [0u8; 1];

    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Ok(()),
            _ = tick.tick() => {
                if last.elapsed() > UDP_IDLE { return Ok(()); }
            }
            _ = ctrl.read(&mut cbuf) => {
                // The association lives exactly as long as the control socket.
                return Ok(());
            }
            m = rx.recv() => {
                let Some((payload, dst)) = m else { return Ok(()) };
                let target = socks::resolve(dns, dst);
                match &target {
                    Target::Domain(n, _) => { fake_by_name.insert(n.clone(), dst); last_fake = Some(dst); }
                    Target::Ip(a) => { direct.insert(*a); }
                }
                let pkt = socks::wrap_udp(&target, &payload)?;
                sock.send(&pkt).await?;
                stats::UP.fetch_add(payload.len() as u64, Relaxed);
                last = Instant::now();
            }
            r = sock.recv(&mut rbuf) => {
                let n = match r {
                    Ok(n) => n,
                    Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => continue,
                    Err(e) => return Err(e),
                };
                if let Some((t, payload)) = socks::unwrap_udp(&rbuf[..n]) {
                    // Make replies look like they come from the address the app talked to.
                    let from = match t {
                        Target::Domain(nm, p) => fake_by_name
                            .get(&nm)
                            .copied()
                            .unwrap_or_else(|| SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), p)),
                        Target::Ip(a) => {
                            if direct.contains(&a) { a } else if let Some(f) = last_fake { f } else { a }
                        }
                    };
                    stats::DOWN.fetch_add(payload.len() as u64, Relaxed);
                    let _ = out.try_send((payload.to_vec(), from, src));
                    last = Instant::now();
                }
            }
        }
    }
}
