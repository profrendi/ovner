//! HTTP kecil untuk penerjemah dan baca keras: permintaan biasa (GET/POST dengan header dan isi)
//! dan WebSocket (suara Natural online seperti Edge). Windows memakai WinHTTP (ikut proxy
//! Windows/kantor); Linux hanya http:// polos untuk pengujian.

pub struct Resp {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Resp {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }
}

/// Pesan WebSocket yang diterima.
pub enum WsMsg {
    Text(String),
    Binary(Vec<u8>),
}

fn split_url(url: &str) -> Result<(bool, String, u16, String), String> {
    let u = url::Url::parse(url).map_err(|e| e.to_string())?;
    let secure = matches!(u.scheme(), "https" | "wss");
    let host = u.host_str().ok_or("alamat tanpa host")?.to_string();
    let port = u.port().unwrap_or(if secure { 443 } else { 80 });
    let mut object = u.path().to_string();
    if let Some(q) = u.query() {
        object.push('?');
        object.push_str(q);
    }
    Ok((secure, host, port, object))
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Networking::WinHttp::*;

    type H = *mut core::ffi::c_void;

    struct Handle(H);
    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    let _ = WinHttpCloseHandle(self.0);
                }
            }
        }
    }

    fn session() -> Result<H, String> {
        static S: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
        let h = *S.get_or_init(|| unsafe {
            let h = WinHttpOpen(&HSTRING::from("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36 Edg/140.0.0.0"), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0);
            if !h.is_null() {
                let _ = WinHttpSetTimeouts(h, 8_000, 10_000, 15_000, 30_000);
                let _ = WinHttpSetOption(Some(h as *const _), WINHTTP_OPTION_DISABLE_FEATURE, Some(&WINHTTP_DISABLE_COOKIES.to_le_bytes()[..]));
            }
            h as usize
        });
        if h == 0 {
            Err("WinHttpOpen gagal".into())
        } else {
            Ok(h as H)
        }
    }

    fn open(method: &str, url: &str, headers: &[(&str, &str)]) -> Result<(Handle, Handle), String> {
        let (secure, host, port, object) = split_url(url)?;
        unsafe {
            let conn = Handle(WinHttpConnect(session()?, &HSTRING::from(host), port, 0));
            if conn.0.is_null() {
                return Err("tidak bisa terhubung".into());
            }
            let flags = if secure { WINHTTP_FLAG_SECURE } else { WINHTTP_OPEN_REQUEST_FLAGS(0) };
            let req = Handle(WinHttpOpenRequest(conn.0, &HSTRING::from(method), &HSTRING::from(object), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), flags));
            if req.0.is_null() {
                return Err("WinHttpOpenRequest gagal".into());
            }
            let mut text = String::new();
            for (k, v) in headers {
                if !v.contains(['\r', '\n']) {
                    text.push_str(&format!("{k}: {v}\r\n"));
                }
            }
            if !text.is_empty() {
                let wide: Vec<u16> = text.encode_utf16().collect();
                let _ = WinHttpAddRequestHeaders(req.0, &wide, WINHTTP_ADDREQ_FLAG_ADD | WINHTTP_ADDREQ_FLAG_REPLACE);
            }
            Ok((conn, req))
        }
    }

    fn status(req: H) -> u16 {
        unsafe {
            let mut code = 0u32;
            let mut len = 4u32;
            let _ = WinHttpQueryHeaders(req, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, PCWSTR::null(), Some(&mut code as *mut u32 as *mut _), &mut len, std::ptr::null_mut());
            code as u16
        }
    }

    pub fn request(method: &str, url: &str, headers: &[(&str, &str)], body: Option<&[u8]>) -> Result<Resp, String> {
        let (_conn, req) = open(method, url, headers)?;
        unsafe {
            let (ptr, len) = match body {
                Some(b) => (Some(b.as_ptr() as *const core::ffi::c_void), b.len() as u32),
                None => (None, 0),
            };
            WinHttpSendRequest(req.0, None, ptr, len, len, 0).map_err(|e| e.to_string())?;
            WinHttpReceiveResponse(req.0, std::ptr::null_mut()).map_err(|e| e.to_string())?;
            let status = status(req.0);
            let mut out = Vec::new();
            let mut chunk = vec![0u8; 64 * 1024];
            loop {
                let mut n = 0u32;
                WinHttpReadData(req.0, chunk.as_mut_ptr() as *mut _, chunk.len() as u32, &mut n).map_err(|e| e.to_string())?;
                if n == 0 || out.len() > 32 * 1024 * 1024 {
                    break;
                }
                out.extend_from_slice(&chunk[..n as usize]);
            }
            Ok(Resp { status, body: out })
        }
    }

    pub struct Ws {
        h: Handle,
        _conn: Handle,
    }

    impl Ws {
        pub fn connect(url: &str, headers: &[(&str, &str)]) -> Result<Ws, String> {
            let (conn, req) = open("GET", url, headers)?;
            unsafe {
                WinHttpSetOption(Some(req.0 as *const _), WINHTTP_OPTION_UPGRADE_TO_WEB_SOCKET, None).map_err(|e| format!("upgrade: {e}"))?;
                WinHttpSendRequest(req.0, None, None, 0, 0, 0).map_err(|e| e.to_string())?;
                WinHttpReceiveResponse(req.0, std::ptr::null_mut()).map_err(|e| e.to_string())?;
                let code = status(req.0);
                if code != 101 {
                    return Err(format!("server menolak WebSocket (HTTP {code})"));
                }
                let ws = WinHttpWebSocketCompleteUpgrade(req.0, None);
                if ws.is_null() {
                    return Err("WebSocket gagal dibuka".into());
                }
                drop(req);
                Ok(Ws { h: Handle(ws), _conn: conn })
            }
        }

        pub fn send_text(&mut self, text: &str) -> Result<(), String> {
            let r = unsafe { WinHttpWebSocketSend(self.h.0, WINHTTP_WEB_SOCKET_UTF8_MESSAGE_BUFFER_TYPE, Some(text.as_bytes())) };
            if r == 0 {
                Ok(())
            } else {
                Err(format!("kirim WebSocket gagal ({r})"))
            }
        }

        /// Pesan utuh berikutnya; None = koneksi ditutup.
        pub fn recv(&mut self) -> Result<Option<WsMsg>, String> {
            let mut msg = Vec::new();
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                let mut n = 0u32;
                let mut kind = WINHTTP_WEB_SOCKET_BUFFER_TYPE(0);
                let r = unsafe { WinHttpWebSocketReceive(self.h.0, buf.as_mut_ptr() as *mut _, buf.len() as u32, &mut n, &mut kind) };
                if r != 0 {
                    return Err(format!("terima WebSocket gagal ({r})"));
                }
                msg.extend_from_slice(&buf[..n as usize]);
                match kind {
                    WINHTTP_WEB_SOCKET_UTF8_MESSAGE_BUFFER_TYPE => return Ok(Some(WsMsg::Text(String::from_utf8_lossy(&msg).to_string()))),
                    WINHTTP_WEB_SOCKET_BINARY_MESSAGE_BUFFER_TYPE => return Ok(Some(WsMsg::Binary(msg))),
                    WINHTTP_WEB_SOCKET_CLOSE_BUFFER_TYPE => return Ok(None),
                    _ => {} // potongan: lanjut membaca
                }
            }
        }
    }

    impl Drop for Ws {
        fn drop(&mut self) {
            unsafe {
                let _ = WinHttpWebSocketClose(self.h.0, 1000, None, 0);
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    use std::io::{Read, Write};

    pub fn request(method: &str, url: &str, headers: &[(&str, &str)], body: Option<&[u8]>) -> Result<Resp, String> {
        let (secure, host, port, object) = split_url(url)?;
        if secure {
            return Err("hanya http di Linux".into());
        }
        let mut s = std::net::TcpStream::connect((host.as_str(), port)).map_err(|e| e.to_string())?;
        let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(10)));
        let mut head = format!("{method} {object} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
        for (k, v) in headers {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        let b = body.unwrap_or(&[]);
        head.push_str(&format!("Content-Length: {}\r\n\r\n", b.len()));
        s.write_all(head.as_bytes()).map_err(|e| e.to_string())?;
        s.write_all(b).map_err(|e| e.to_string())?;
        let mut all = Vec::new();
        let _ = s.read_to_end(&mut all);
        let split = all.windows(4).position(|w| w == b"\r\n\r\n").ok_or("jawaban tidak lengkap")?;
        let headtxt = String::from_utf8_lossy(&all[..split]).to_string();
        let status = headtxt.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
        Ok(Resp { status, body: all[split + 4..].to_vec() })
    }

    pub struct Ws;
    impl Ws {
        pub fn connect(_url: &str, _headers: &[(&str, &str)]) -> Result<Ws, String> {
            Err("WebSocket hanya di Windows".into())
        }
        pub fn send_text(&mut self, _text: &str) -> Result<(), String> {
            Err("WebSocket hanya di Windows".into())
        }
        pub fn recv(&mut self) -> Result<Option<WsMsg>, String> {
            Ok(None)
        }
    }
}

pub use imp::{request, Ws};

pub fn get(url: &str, headers: &[(&str, &str)]) -> Result<Resp, String> {
    request("GET", url, headers, None)
}

pub fn post(url: &str, headers: &[(&str, &str)], body: &[u8]) -> Result<Resp, String> {
    request("POST", url, headers, Some(body))
}

/// Bagian alamat untuk query string.
pub fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}
