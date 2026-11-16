//! Anti-blokir: proxy HTTP lokal untuk WebView2 (seperti SpoofDPI/GoodbyeDPI).
//!
//! Pemblokiran situs di banyak ISP Indonesia bekerja di dua tempat:
//! 1. DNS: jawaban DNS ISP diganti ke halaman blokir. Proxy ini mencari alamat situs
//!    sendiri lewat DNS-over-HTTPS (Cloudflare 1.1.1.1, cadangan Google 8.8.8.8), jadi DNS
//!    ISP tidak dipakai sama sekali.
//! 2. SNI: nama situs terlihat jelas di salam pembuka TLS (ClientHello). Proxy ini memecah
//!    ClientHello menjadi dua rekaman TLS dan beberapa paket TCP tepat di tengah nama situs,
//!    sehingga penyaring yang membaca satu paket tidak menemukan nama utuhnya. Server tetap
//!    menerimanya karena TLS mengizinkan pesan handshake dipecah ke beberapa rekaman.
//!
//! Isi koneksi tetap terenkripsi ujung-ke-ujung antara WebView2 dan situs; proxy hanya
//! meneruskan byte dan tidak mencatat apa pun.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const STACK: usize = 128 * 1024;

/// Statistik sederhana untuk halaman diagnostik.
pub static CONNECTIONS: AtomicU64 = AtomicU64::new(0);
pub static DOH_OK: AtomicU64 = AtomicU64::new(0);
pub static DOH_FAIL: AtomicU64 = AtomicU64::new(0);

/// Jalankan proxy di 127.0.0.1 dengan port bebas. Mengembalikan port-nya.
pub fn start() -> std::io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    std::thread::Builder::new().name("antiblokir".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = std::thread::Builder::new().stack_size(STACK).spawn(move || {
                CONNECTIONS.fetch_add(1, Ordering::Relaxed);
                let _ = handle(stream);
            });
        }
    })?;
    Ok(port)
}

/// Argumen WebView2 agar semua lalu lintas web lewat proxy ini.
pub fn browser_args(port: u16) -> String {
    format!(" --proxy-server=http://127.0.0.1:{port} --proxy-bypass-list=<-loopback>")
}

fn handle(mut client: TcpStream) -> std::io::Result<()> {
    client.set_read_timeout(Some(Duration::from_secs(30)))?;
    let (head, rest) = read_head(&mut client)?;
    let text = String::from_utf8_lossy(&head).to_string();
    let first = text.lines().next().unwrap_or("").to_string();
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("").to_string();
    if method.eq_ignore_ascii_case("CONNECT") {
        let (host, port) = split_host_port(&target, 443);
        let mut upstream = match connect(&host, port) {
            Ok(s) => s,
            Err(_) => {
                let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n");
                return Ok(());
            }
        };
        client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")?;
        // Byte pertama dari browser setelah terowongan terbuka: ClientHello.
        let mut hello = rest;
        if hello.is_empty() {
            let mut buf = vec![0u8; 16 * 1024];
            client.set_read_timeout(Some(Duration::from_secs(10)))?;
            let n = client.read(&mut buf)?;
            buf.truncate(n);
            hello = buf;
        }
        send_fragmented(&mut upstream, &hello)?;
        client.set_read_timeout(None)?;
        pipe(client, upstream);
    } else {
        // HTTP biasa: "GET http://situs/jalan HTTP/1.1" → "GET /jalan HTTP/1.1" ke situsnya.
        let Some((host, port, path)) = parse_absolute(&target) else {
            let _ = client.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n");
            return Ok(());
        };
        let mut upstream = match connect(&host, port) {
            Ok(s) => s,
            Err(_) => {
                let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n");
                return Ok(());
            }
        };
        let req = rewrite_http_head(&text, &method, &path);
        // Pecah di tengah nilai Host: supaya penyaring HTTP juga tidak membacanya utuh.
        let bytes = req.as_bytes();
        let cut = req.to_ascii_lowercase().find("\r\nhost:").map(|i| i + 9).unwrap_or(1).min(bytes.len());
        upstream.write_all(&bytes[..cut])?;
        upstream.flush()?;
        std::thread::sleep(Duration::from_millis(3));
        upstream.write_all(&bytes[cut..])?;
        upstream.write_all(&rest)?;
        client.set_read_timeout(None)?;
        pipe(client, upstream);
    }
    Ok(())
}

/// Baca kepala permintaan sampai baris kosong. Mengembalikan (kepala, sisa byte setelahnya).
fn read_head(s: &mut TcpStream) -> std::io::Result<(Vec<u8>, Vec<u8>)> {
    let mut buf = Vec::with_capacity(2048);
    let mut chunk = [0u8; 4096];
    loop {
        let n = s.read(&mut chunk)?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = find(&buf, b"\r\n\r\n") {
            let rest = buf.split_off(i + 4);
            return Ok((buf, rest));
        }
        if buf.len() > 64 * 1024 {
            return Err(std::io::ErrorKind::InvalidData.into());
        }
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// "situs.id:443", "[::1]:8080", "1.2.3.4" → (host, port).
pub fn split_host_port(target: &str, default: u16) -> (String, u16) {
    if let Some(rest) = target.strip_prefix('[') {
        let (h, p) = rest.split_once(']').unwrap_or((rest, ""));
        let port = p.strip_prefix(':').and_then(|p| p.parse().ok()).unwrap_or(default);
        return (h.to_string(), port);
    }
    match target.rsplit_once(':') {
        Some((h, p)) if !h.contains(':') => (h.to_string(), p.parse().unwrap_or(default)),
        _ => (target.to_string(), default),
    }
}

/// "http://situs.id:8080/a?b" → ("situs.id", 8080, "/a?b").
pub fn parse_absolute(target: &str) -> Option<(String, u16, String)> {
    let rest = target.strip_prefix("http://")?;
    let (hostport, path) = match rest.find('/') {
        Some(i) => (&rest[..i], rest[i..].to_string()),
        None => (rest, "/".to_string()),
    };
    let (host, port) = split_host_port(hostport, 80);
    if host.is_empty() {
        return None;
    }
    Some((host, port, path))
}

/// Kepala HTTP untuk server asal: baris pertama berbentuk origin, tanpa tajuk proxy, dan
/// koneksi ditutup setelah satu respons (proxy tidak memakai ulang koneksi lintas situs).
pub fn rewrite_http_head(head: &str, method: &str, path: &str) -> String {
    let mut lines = head.split("\r\n");
    let first = lines.next().unwrap_or("");
    let version = first.split_whitespace().nth(2).unwrap_or("HTTP/1.1");
    let mut out = format!("{method} {path} {version}\r\n");
    for l in lines {
        if l.is_empty() {
            continue;
        }
        let lower = l.to_ascii_lowercase();
        if lower.starts_with("proxy-connection:") || lower.starts_with("connection:") || lower.starts_with("keep-alive:") {
            continue;
        }
        out.push_str(l);
        out.push_str("\r\n");
    }
    out.push_str("Connection: close\r\n\r\n");
    out
}

fn connect(host: &str, port: u16) -> std::io::Result<TcpStream> {
    let ips = resolve(host);
    let mut last = std::io::Error::from(std::io::ErrorKind::NotFound);
    for ip in ips {
        match TcpStream::connect_timeout(&SocketAddr::new(ip, port), Duration::from_secs(10)) {
            Ok(s) => {
                let _ = s.set_nodelay(true);
                return Ok(s);
            }
            Err(e) => last = e,
        }
    }
    Err(last)
}

static CACHE: Mutex<Option<HashMap<String, (Vec<IpAddr>, Instant)>>> = Mutex::new(None);

/// Alamat IP situs: lewat DoH (dengan cache), cadangan DNS Windows bila DoH tidak bisa.
pub fn resolve(host: &str) -> Vec<IpAddr> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return vec![ip];
    }
    let key = host.to_ascii_lowercase();
    if let Some((ips, until)) = CACHE.lock().ok().and_then(|c| c.as_ref().and_then(|m| m.get(&key).cloned())) {
        if Instant::now() < until {
            return ips;
        }
    }
    let found = doh(&key);
    if let Some((ips, ttl)) = found.filter(|(ips, _)| !ips.is_empty()) {
        DOH_OK.fetch_add(1, Ordering::Relaxed);
        let ttl = Duration::from_secs(ttl.clamp(60, 3600));
        if let Ok(mut c) = CACHE.lock() {
            let m = c.get_or_insert_with(HashMap::new);
            if m.len() > 4000 {
                m.clear();
            }
            m.insert(key, (ips.clone(), Instant::now() + ttl));
        }
        return ips;
    }
    DOH_FAIL.fetch_add(1, Ordering::Relaxed);
    (host, 0).to_socket_addrs().map(|a| a.map(|s| s.ip()).collect()).unwrap_or_default()
}

/// Jawaban DoH (format JSON Cloudflare/Google) → (IPv4, TTL terkecil).
pub fn parse_doh_json(text: &str) -> Option<(Vec<IpAddr>, u64)> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    if v["Status"].as_u64() != Some(0) {
        return Some((Vec::new(), 60));
    }
    let mut ips = Vec::new();
    let mut ttl = u64::MAX;
    for a in v["Answer"].as_array().into_iter().flatten() {
        if a["type"].as_u64() == Some(1) {
            if let Some(ip) = a["data"].as_str().and_then(|d| d.parse::<IpAddr>().ok()) {
                ips.push(ip);
                ttl = ttl.min(a["TTL"].as_u64().unwrap_or(300));
            }
        }
    }
    Some((ips, if ttl == u64::MAX { 300 } else { ttl }))
}

fn doh(host: &str) -> Option<(Vec<IpAddr>, u64)> {
    let q: String = url::form_urlencoded::byte_serialize(host.as_bytes()).collect();
    for (server, path) in [("1.1.1.1", format!("/dns-query?name={q}&type=A")), ("8.8.8.8", format!("/resolve?name={q}&type=A"))] {
        if let Some(r) = https_get(server, &path).and_then(|t| parse_doh_json(&t)) {
            return Some(r);
        }
    }
    None
}

/// Uji cepat untuk halaman diagnostik: (berhasil, keterangan).
pub fn self_test() -> (bool, String) {
    let t = Instant::now();
    match doh("example.com") {
        Some((ips, _)) if !ips.is_empty() => (true, format!("DoH OK: example.com → {} ({} ms)", ips[0], t.elapsed().as_millis())),
        _ => (false, "DoH gagal (1.1.1.1 dan 8.8.8.8 tidak terjangkau)".into()),
    }
}

#[cfg(windows)]
fn https_get(server: &str, path: &str) -> Option<String> {
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::Networking::WinHttp::*;
    unsafe {
        // Ikuti setelan proxy Windows (jaringan kantor).
        let session = WinHttpOpen(w!("Ovner"), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0);
        if session.is_null() {
            return None;
        }
        let _ = WinHttpSetTimeouts(session, 4000, 4000, 4000, 4000);
        let mut out = None;
        let conn = WinHttpConnect(session, &HSTRING::from(server), 443, 0);
        if !conn.is_null() {
            let req = WinHttpOpenRequest(conn, w!("GET"), &HSTRING::from(path), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE);
            if !req.is_null() {
                let header: Vec<u16> = "Accept: application/dns-json".encode_utf16().collect();
                let _ = WinHttpAddRequestHeaders(req, &header, WINHTTP_ADDREQ_FLAG_ADD);
                if WinHttpSendRequest(req, None, None, 0, 0, 0).is_ok() && WinHttpReceiveResponse(req, std::ptr::null_mut()).is_ok() {
                    let mut body = Vec::new();
                    let mut buf = [0u8; 8192];
                    loop {
                        let mut n = 0u32;
                        if WinHttpReadData(req, buf.as_mut_ptr() as *mut _, buf.len() as u32, &mut n).is_err() || n == 0 {
                            break;
                        }
                        body.extend_from_slice(&buf[..n as usize]);
                        if body.len() > 256 * 1024 {
                            break;
                        }
                    }
                    out = String::from_utf8(body).ok();
                }
                let _ = WinHttpCloseHandle(req);
            }
            let _ = WinHttpCloseHandle(conn);
        }
        let _ = WinHttpCloseHandle(session);
        out
    }
}

#[cfg(not(windows))]
fn https_get(server: &str, path: &str) -> Option<String> {
    let out = std::process::Command::new("curl")
        .args(["-fsS", "--max-time", "4", "-H", "Accept: application/dns-json"])
        .arg(format!("https://{server}{path}"))
        .output()
        .ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

/// Posisi nama situs (SNI) di dalam data rekaman TLS pertama (setelah 5 byte kepala).
pub fn sni_offset(record: &[u8]) -> Option<(usize, usize)> {
    if record.len() < 5 || record[0] != 0x16 || record[1] != 0x03 {
        return None;
    }
    let len = u16::from_be_bytes([record[3], record[4]]) as usize;
    let p = record.get(5..5 + len.min(record.len() - 5))?;
    if p.first() != Some(&1) {
        return None;
    }
    let mut i = 4 + 2 + 32;
    let sid = *p.get(i)? as usize;
    i += 1 + sid;
    let cs = u16::from_be_bytes([*p.get(i)?, *p.get(i + 1)?]) as usize;
    i += 2 + cs;
    let cm = *p.get(i)? as usize;
    i += 1 + cm;
    let ext_end = i + 2 + u16::from_be_bytes([*p.get(i)?, *p.get(i + 1)?]) as usize;
    i += 2;
    while i + 4 <= ext_end.min(p.len()) {
        let ty = u16::from_be_bytes([p[i], p[i + 1]]);
        let el = u16::from_be_bytes([p[i + 2], p[i + 3]]) as usize;
        if ty == 0 {
            // server_name: panjang daftar (2), jenis (1), panjang nama (2), nama.
            let name_len = u16::from_be_bytes([*p.get(i + 7)?, *p.get(i + 8)?]) as usize;
            return Some((i + 9, name_len));
        }
        i += 4 + el;
    }
    None
}

/// Pecah rekaman ClientHello menjadi dua rekaman TLS di tengah nama situs.
pub fn split_client_hello(data: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let (off, name_len) = sni_offset(data)?;
    let len = u16::from_be_bytes([data[3], data[4]]) as usize;
    if data.len() < 5 + len {
        return None;
    }
    let payload = &data[5..5 + len];
    let cut = (off + name_len / 2).clamp(1, payload.len() - 1);
    let rec = |part: &[u8]| {
        let mut r = vec![0x16, data[1], data[2]];
        r.extend_from_slice(&(part.len() as u16).to_be_bytes());
        r.extend_from_slice(part);
        r
    };
    let mut second = rec(&payload[cut..]);
    second.extend_from_slice(&data[5 + len..]);
    Some((rec(&payload[..cut]), second))
}

fn send_fragmented(up: &mut TcpStream, data: &[u8]) -> std::io::Result<()> {
    let Some((a, b)) = split_client_hello(data) else {
        return up.write_all(data);
    };
    // Rekaman pertama juga dikirim dalam dua paket TCP.
    up.write_all(&a[..1])?;
    up.flush()?;
    std::thread::sleep(Duration::from_millis(2));
    up.write_all(&a[1..])?;
    up.flush()?;
    std::thread::sleep(Duration::from_millis(2));
    up.write_all(&b)
}

fn pipe(client: TcpStream, upstream: TcpStream) {
    let (Ok(mut c2), Ok(mut u2)) = (client.try_clone(), upstream.try_clone()) else { return };
    let up = std::thread::Builder::new().stack_size(STACK).spawn(move || {
        let mut c = client;
        let _ = std::io::copy(&mut c, &mut u2);
        let _ = u2.shutdown(Shutdown::Write);
    });
    let mut u = upstream;
    let _ = std::io::copy(&mut u, &mut c2);
    let _ = c2.shutdown(Shutdown::Write);
    if let Ok(h) = up {
        let _ = h.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ClientHello minimal dengan SNI "situs-diblokir.id".
    fn hello(name: &str) -> Vec<u8> {
        let mut ext = Vec::new();
        let n = name.as_bytes();
        ext.extend_from_slice(&[0, 0]);
        ext.extend_from_slice(&((n.len() + 5) as u16).to_be_bytes());
        ext.extend_from_slice(&((n.len() + 3) as u16).to_be_bytes());
        ext.push(0);
        ext.extend_from_slice(&(n.len() as u16).to_be_bytes());
        ext.extend_from_slice(n);
        let mut body = vec![3, 3];
        body.extend_from_slice(&[7u8; 32]);
        body.push(0);
        body.extend_from_slice(&[0, 2, 0x13, 0x01]);
        body.extend_from_slice(&[1, 0]);
        body.extend_from_slice(&(ext.len() as u16).to_be_bytes());
        body.extend_from_slice(&ext);
        let mut hs = vec![1];
        hs.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
        hs.extend_from_slice(&body);
        let mut rec = vec![0x16, 3, 1];
        rec.extend_from_slice(&(hs.len() as u16).to_be_bytes());
        rec.extend_from_slice(&hs);
        rec
    }

    #[test]
    fn splits_client_hello_inside_sni() {
        let h = hello("situs-diblokir.id");
        let (off, len) = sni_offset(&h).unwrap();
        assert_eq!(&h[5 + off..5 + off + len], b"situs-diblokir.id");
        let (a, b) = split_client_hello(&h).unwrap();
        // Tidak ada rekaman yang memuat nama situs utuh.
        for r in [&a, &b] {
            assert_eq!(r[0], 0x16);
            assert!(find(r, b"situs-diblokir.id").is_none());
        }
        // Isi kedua rekaman bila digabung = isi asli.
        let mut joined = a[5..].to_vec();
        joined.extend_from_slice(&b[5..]);
        assert_eq!(joined, h[5..]);
        assert_eq!(u16::from_be_bytes([a[3], a[4]]) as usize, a.len() - 5);
        assert!(split_client_hello(b"GET / HTTP/1.1\r\n\r\n").is_none());
    }

    #[test]
    fn parses_proxy_requests_and_doh() {
        assert_eq!(split_host_port("situs.id:443", 80), ("situs.id".into(), 443));
        assert_eq!(split_host_port("[::1]:8080", 80), ("::1".into(), 8080));
        assert_eq!(split_host_port("1.2.3.4", 443), ("1.2.3.4".into(), 443));
        assert_eq!(parse_absolute("http://a.id:8080/x?y=1"), Some(("a.id".into(), 8080, "/x?y=1".into())));
        assert_eq!(parse_absolute("http://a.id"), Some(("a.id".into(), 80, "/".into())));
        let head = "GET http://a.id/x HTTP/1.1\r\nHost: a.id\r\nProxy-Connection: keep-alive\r\nAccept: */*\r\n\r\n";
        let r = rewrite_http_head(head, "GET", "/x");
        assert_eq!(r, "GET /x HTTP/1.1\r\nHost: a.id\r\nAccept: */*\r\nConnection: close\r\n\r\n");
        let j = r#"{"Status":0,"Answer":[{"name":"a.id","type":5,"TTL":30,"data":"b.id."},{"name":"b.id","type":1,"TTL":120,"data":"104.21.5.9"}]}"#;
        assert_eq!(parse_doh_json(j), Some((vec!["104.21.5.9".parse().unwrap()], 120)));
        assert_eq!(parse_doh_json(r#"{"Status":3}"#).unwrap().0.len(), 0);
    }

    #[test]
    fn proxies_plain_http_end_to_end() {
        // Server HTTP palsu di 127.0.0.1, lalu permintaan lewat proxy.
        let srv = TcpListener::bind("127.0.0.1:0").unwrap();
        let sp = srv.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut s, _) = srv.accept().unwrap();
            let mut buf = vec![0u8; 4096];
            let mut got = Vec::new();
            while find(&got, b"\r\n\r\n").is_none() {
                let n = s.read(&mut buf).unwrap();
                got.extend_from_slice(&buf[..n]);
            }
            let req = String::from_utf8_lossy(&got).to_string();
            assert!(req.starts_with("GET /halo HTTP/1.1\r\n"), "{req}");
            s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok").unwrap();
        });
        let port = start().unwrap();
        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(c, "GET http://127.0.0.1:{sp}/halo HTTP/1.1\r\nHost: 127.0.0.1:{sp}\r\nProxy-Connection: keep-alive\r\n\r\n").unwrap();
        let mut resp = String::new();
        c.read_to_string(&mut resp).unwrap();
        assert!(resp.starts_with("HTTP/1.1 200 OK") && resp.ends_with("ok"), "{resp}");
    }
}
