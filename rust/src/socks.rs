//! SOCKS5 client: RFC 1928 (CONNECT, UDP ASSOCIATE) + RFC 1929 user/pass.
use crate::dns::FakeDns;
use std::{
    future::Future,
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};

const HS: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct ProxyCfg {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub pass: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Ip(SocketAddr),
    Domain(String, u16),
}

fn err(m: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::Other, m.into())
}

async fn tmo<T>(f: impl Future<Output = io::Result<T>>) -> io::Result<T> {
    match timeout(HS, f).await {
        Ok(r) => r,
        Err(_) => Err(err("socks timeout")),
    }
}

/// Fake IP -> real domain (so the server applies its domain rules), else plain IP.
pub fn resolve(dns: &FakeDns, a: SocketAddr) -> Target {
    if let IpAddr::V4(v4) = a.ip() {
        if let Some(n) = dns.lookup(v4) {
            return Target::Domain(n, a.port());
        }
    }
    Target::Ip(a)
}

pub fn encode_addr(buf: &mut Vec<u8>, t: &Target) -> io::Result<()> {
    match t {
        Target::Ip(SocketAddr::V4(a)) => {
            buf.push(1);
            buf.extend_from_slice(&a.ip().octets());
            buf.extend_from_slice(&a.port().to_be_bytes());
        }
        Target::Ip(SocketAddr::V6(a)) => {
            buf.push(4);
            buf.extend_from_slice(&a.ip().octets());
            buf.extend_from_slice(&a.port().to_be_bytes());
        }
        Target::Domain(n, p) => {
            if n.is_empty() || n.len() > 255 {
                return Err(err("bad domain"));
            }
            buf.push(3);
            buf.push(n.len() as u8);
            buf.extend_from_slice(n.as_bytes());
            buf.extend_from_slice(&p.to_be_bytes());
        }
    }
    Ok(())
}

/// `b` starts at ATYP. Returns (target, bytes consumed).
pub fn parse_addr(b: &[u8]) -> Option<(Target, usize)> {
    match *b.first()? {
        1 => {
            let x = b.get(1..7)?;
            let ip = Ipv4Addr::new(x[0], x[1], x[2], x[3]);
            let port = u16::from_be_bytes([x[4], x[5]]);
            Some((Target::Ip(SocketAddr::new(IpAddr::V4(ip), port)), 7))
        }
        4 => {
            let x = b.get(1..19)?;
            let mut o = [0u8; 16];
            o.copy_from_slice(&x[..16]);
            let port = u16::from_be_bytes([x[16], x[17]]);
            Some((Target::Ip(SocketAddr::new(IpAddr::V6(Ipv6Addr::from(o)), port)), 19))
        }
        3 => {
            let l = *b.get(1)? as usize;
            let x = b.get(2..2 + l + 2)?;
            let name = String::from_utf8_lossy(&x[..l]).to_string();
            Some((Target::Domain(name, u16::from_be_bytes([x[l], x[l + 1]])), 2 + l + 2))
        }
        _ => None,
    }
}

async fn connect_ctrl(cfg: &ProxyCfg) -> io::Result<TcpStream> {
    let mut s = tmo(TcpStream::connect((cfg.host.as_str(), cfg.port))).await?;
    let _ = s.set_nodelay(true);
    let auth = !cfg.user.is_empty();
    if auth {
        tmo(s.write_all(&[5, 2, 0, 2])).await?;
    } else {
        tmo(s.write_all(&[5, 1, 0])).await?;
    }
    let mut r = [0u8; 2];
    tmo(s.read_exact(&mut r)).await?;
    if r[0] != 5 {
        return Err(err("not a socks5 server"));
    }
    match r[1] {
        0 => {}
        2 => {
            if !auth {
                return Err(err("server requires username/password"));
            }
            let u = cfg.user.as_bytes();
            let p = cfg.pass.as_bytes();
            if u.len() > 255 || p.len() > 255 {
                return Err(err("credentials too long"));
            }
            let mut m = vec![1u8, u.len() as u8];
            m.extend_from_slice(u);
            m.push(p.len() as u8);
            m.extend_from_slice(p);
            tmo(s.write_all(&m)).await?;
            let mut a = [0u8; 2];
            tmo(s.read_exact(&mut a)).await?;
            if a[1] != 0 {
                return Err(err("socks auth failed"));
            }
        }
        _ => return Err(err("no acceptable socks auth method")),
    }
    Ok(s)
}

async fn read_reply(s: &mut TcpStream) -> io::Result<SocketAddr> {
    let mut h = [0u8; 4];
    tmo(s.read_exact(&mut h)).await?;
    if h[0] != 5 {
        return Err(err("bad socks reply"));
    }
    if h[1] != 0 {
        return Err(err(format!("socks reply code {}", h[1])));
    }
    match h[3] {
        1 => {
            let mut b = [0u8; 6];
            tmo(s.read_exact(&mut b)).await?;
            Ok(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(b[0], b[1], b[2], b[3])),
                u16::from_be_bytes([b[4], b[5]]),
            ))
        }
        4 => {
            let mut b = [0u8; 18];
            tmo(s.read_exact(&mut b)).await?;
            let mut o = [0u8; 16];
            o.copy_from_slice(&b[..16]);
            Ok(SocketAddr::new(IpAddr::V6(Ipv6Addr::from(o)), u16::from_be_bytes([b[16], b[17]])))
        }
        3 => {
            let mut l = [0u8; 1];
            tmo(s.read_exact(&mut l)).await?;
            let mut b = vec![0u8; l[0] as usize + 2];
            tmo(s.read_exact(&mut b)).await?;
            let n = b.len();
            Ok(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), u16::from_be_bytes([b[n - 2], b[n - 1]])))
        }
        _ => Err(err("bad socks reply atyp")),
    }
}

pub async fn tcp_connect(cfg: &ProxyCfg, target: &Target) -> io::Result<TcpStream> {
    let mut s = connect_ctrl(cfg).await?;
    let mut req = vec![5u8, 1, 0];
    encode_addr(&mut req, target)?;
    tmo(s.write_all(&req)).await?;
    read_reply(&mut s).await?;
    Ok(s)
}

/// Returns the control stream (must stay open for the association's lifetime)
/// and the relay address. If the server answers 0.0.0.0 / :: (or loopback while
/// the server itself is remote), the relay IP is the server we connected to.
pub async fn udp_associate(cfg: &ProxyCfg) -> io::Result<(TcpStream, SocketAddr)> {
    let mut s = connect_ctrl(cfg).await?;
    tmo(s.write_all(&[5, 3, 0, 1, 0, 0, 0, 0, 0, 0])).await?;
    let bnd = read_reply(&mut s).await?;
    let peer = s.peer_addr()?;
    let relay = if bnd.ip().is_unspecified() || (bnd.ip().is_loopback() && !peer.ip().is_loopback()) {
        SocketAddr::new(peer.ip(), bnd.port())
    } else {
        bnd
    };
    Ok((s, relay))
}

pub fn wrap_udp(t: &Target, payload: &[u8]) -> io::Result<Vec<u8>> {
    let mut v = Vec::with_capacity(payload.len() + 24);
    v.extend_from_slice(&[0, 0, 0]);
    encode_addr(&mut v, t)?;
    v.extend_from_slice(payload);
    Ok(v)
}

pub fn unwrap_udp(b: &[u8]) -> Option<(Target, &[u8])> {
    if b.len() < 4 || b[2] != 0 {
        return None; // fragmented datagrams are not supported (same as the server)
    }
    let (t, n) = parse_addr(&b[3..])?;
    Some((t, b.get(3 + n..)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn udp_wrap_unwrap_domain() {
        let t = Target::Domain("example.com".into(), 443);
        let w = wrap_udp(&t, &[1, 2, 3]).expect("wrap");
        let (back, payload) = unwrap_udp(&w).expect("unwrap");
        assert_eq!(back, t);
        assert_eq!(payload, &[1, 2, 3]);
    }

    #[test]
    fn udp_wrap_unwrap_ipv4_and_v6() {
        for a in ["1.2.3.4:53", "[2001:db8::1]:443"] {
            let t = Target::Ip(a.parse().expect("addr"));
            let w = wrap_udp(&t, b"x").expect("wrap");
            let (back, payload) = unwrap_udp(&w).expect("unwrap");
            assert_eq!(back, t);
            assert_eq!(payload, b"x");
        }
    }

    #[test]
    fn fragmented_datagram_rejected() {
        assert!(unwrap_udp(&[0, 0, 1, 1, 1, 2, 3, 4, 0, 80]).is_none());
    }
}
