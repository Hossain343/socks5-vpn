//! Fake-DNS: every A query gets a unique address from 198.18.0.0/16.
//! When the app later connects to that address we send the real *domain*
//! to the vpnshare-rs server (SOCKS5 ATYP=3), so the server's domain rules
//! (iran-hosted-domains, *.ir, ads, ...) keep working exactly as designed.
use std::{collections::HashMap, net::Ipv4Addr, sync::Mutex};

const BASE: u32 = 0xC612_0000; // 198.18.0.0

struct Inner {
    names: Vec<Option<String>>,
    index: HashMap<String, u16>,
    next: u32,
}

pub struct FakeDns {
    inner: Mutex<Inner>,
}

impl FakeDns {
    pub fn new() -> Self {
        FakeDns {
            inner: Mutex::new(Inner {
                names: vec![None; 65536],
                index: HashMap::new(),
                next: 1,
            }),
        }
    }

    pub fn alloc(&self, name: &str) -> Ipv4Addr {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(&i) = g.index.get(name) {
            return Ipv4Addr::from(BASE + i as u32);
        }
        let i = g.next;
        g.next = if i >= 65534 { 1 } else { i + 1 };
        if let Some(old) = g.names[i as usize].take() {
            g.index.remove(&old);
        }
        g.names[i as usize] = Some(name.to_string());
        g.index.insert(name.to_string(), i as u16);
        Ipv4Addr::from(BASE + i)
    }

    pub fn lookup(&self, ip: Ipv4Addr) -> Option<String> {
        let v = u32::from(ip);
        if v & 0xFFFF_0000 != BASE {
            return None;
        }
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.names.get((v & 0xFFFF) as usize).and_then(|n| n.clone())
    }
}

/// Answers a DNS query packet. A -> fake IPv4 (TTL 30). Everything else -> NOERROR, empty.
pub fn answer(fake: &FakeDns, q: &[u8]) -> Option<Vec<u8>> {
    if q.len() < 12 || q[2] & 0x80 != 0 {
        return None;
    }
    if u16::from_be_bytes([q[4], q[5]]) != 1 {
        return None;
    }
    let mut pos = 12usize;
    let mut name = String::new();
    loop {
        let l = *q.get(pos)? as usize;
        pos += 1;
        if l == 0 {
            break;
        }
        if l & 0xC0 != 0 {
            return None;
        }
        let lab = q.get(pos..pos + l)?;
        if !name.is_empty() {
            name.push('.');
        }
        name.push_str(&String::from_utf8_lossy(lab).to_lowercase());
        pos += l;
    }
    let qtype = u16::from_be_bytes([*q.get(pos)?, *q.get(pos + 1)?]);
    pos += 4;
    let question = q.get(12..pos)?;
    let is_a = qtype == 1 && !name.is_empty();

    let mut r = Vec::with_capacity(pos + 16);
    r.extend_from_slice(&q[0..2]);
    r.push(0x80 | (q[2] & 0x01)); // QR + RD copied
    r.push(0x80); // RA, rcode 0
    r.extend_from_slice(&[0, 1, 0, if is_a { 1 } else { 0 }, 0, 0, 0, 0]);
    r.extend_from_slice(question);
    if is_a {
        let ip = fake.alloc(&name);
        r.extend_from_slice(&[0xC0, 0x0C, 0, 1, 0, 1, 0, 0, 0, 30, 0, 4]);
        r.extend_from_slice(&ip.octets());
    }
    Some(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(name: &str, qtype: u16) -> Vec<u8> {
        let mut q = vec![0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
        for l in name.split('.') {
            q.push(l.len() as u8);
            q.extend_from_slice(l.as_bytes());
        }
        q.push(0);
        q.extend_from_slice(&qtype.to_be_bytes());
        q.extend_from_slice(&[0, 1]);
        q
    }

    #[test]
    fn a_query_gets_fake_ip_and_roundtrips() {
        let f = FakeDns::new();
        let r = answer(&f, &query("Example.COM", 1)).expect("answer");
        assert_eq!(&r[0..2], &[0x12, 0x34]);
        assert_eq!(r[2] & 0x80, 0x80);
        assert_eq!(u16::from_be_bytes([r[6], r[7]]), 1);
        let ip = Ipv4Addr::new(r[r.len() - 4], r[r.len() - 3], r[r.len() - 2], r[r.len() - 1]);
        assert_eq!(ip.octets()[0..2], [198, 18]);
        assert_eq!(f.lookup(ip).as_deref(), Some("example.com"));
        let again = answer(&f, &query("example.com", 1)).expect("answer");
        assert_eq!(&r[r.len() - 4..], &again[again.len() - 4..]);
    }

    #[test]
    fn aaaa_gets_empty_noerror() {
        let f = FakeDns::new();
        let r = answer(&f, &query("example.com", 28)).expect("answer");
        assert_eq!(u16::from_be_bytes([r[6], r[7]]), 0);
        assert_eq!(r[3] & 0x0F, 0);
    }

    #[test]
    fn garbage_is_ignored() {
        let f = FakeDns::new();
        assert!(answer(&f, &[1, 2, 3]).is_none());
    }
}
