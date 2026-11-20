//! Server HTTP kecil untuk `--self-test` (uji Ovner sungguhan dengan WebView2 di GitHub):
//! halaman tiruan untuk PDF (dengan/tanpa ukuran, seperti Infor dan SIPP), unduhan dengan Range,
//! iklan, formulir, dan pemutar YouTube tiruan. Hanya mendengarkan 127.0.0.1.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};

pub const BIG_FILE: usize = 20 * 1_048_576;

/// PDF sah berukuran ±`size` byte (isi tambahan setelah %%EOF, yang diabaikan penampil PDF).
pub fn pdf(size: usize) -> Vec<u8> {
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 144] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
        "<< /Length 44 >>\nstream\nBT /F1 24 Tf 40 70 Td (Uji Ovner) Tj ET\nendstream",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offs = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offs.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offs {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    while out.len() < size {
        out.extend_from_slice(b"% isi tambahan untuk uji ukuran ...........................................\n");
    }
    out
}

/// Isi berkas unduhan uji (pola tetap, supaya bisa diperiksa).
pub fn big_byte(i: usize) -> u8 {
    (i % 251) as u8
}

/// Mulai server di port acak. Ok = port.
pub fn start() -> std::io::Result<u16> {
    let l = TcpListener::bind("127.0.0.1:0")?;
    let port = l.local_addr()?.port();
    std::thread::Builder::new().name("ovner-uji-server".into()).spawn(move || {
        for s in l.incoming().flatten() {
            std::thread::spawn(move || {
                let _ = handle(s);
            });
        }
    })?;
    Ok(port)
}

fn html(title: &str, body: &str) -> Vec<u8> {
    format!("<!doctype html><meta charset=utf-8><title>{title}</title><body style=\"font:16px sans-serif\"><h1>{title}</h1>{body}</body>").into_bytes()
}

const YT_MOCK: &str = r#"<div id="movie_player"></div><video></video><script>
var p=document.getElementById("movie_player");
p.getPlayerResponse=function(){return {videoDetails:{videoId:"wtp3OolekmA"},captions:{playerCaptionsTracklistRenderer:{captionTracks:[{languageCode:"en",kind:"asr",baseUrl:"/api/timedtext?v=wtp3OolekmA&lang=en&kind=asr"}]}}};};
p.mute=function(){};p.pauseVideo=function(){};p.getOption=function(){return {};};p.loadModule=function(){};p.unloadModule=function(){};
p.setOption=function(a,b,c){if(c&&c.languageCode){setTimeout(function(){var x=new XMLHttpRequest();x.open("GET","/api/timedtext?v=wtp3OolekmA&lang=en&kind=asr&fmt=json3&pot=TOKEN");x.send();},100);}};
</script>"#;

fn handle(mut s: TcpStream) -> std::io::Result<()> {
    let mut r = BufReader::new(s.try_clone()?);
    let mut first = String::new();
    r.read_line(&mut first)?;
    let path = first.split_whitespace().nth(1).unwrap_or("/").to_string();
    let mut range: Option<usize> = None;
    let mut seen_headers = String::new();
    loop {
        let mut h = String::new();
        if r.read_line(&mut h)? == 0 || h == "\r\n" || h == "\n" {
            break;
        }
        let low = h.to_ascii_lowercase();
        seen_headers.push_str(&low);
        if let Some(v) = low.strip_prefix("range: bytes=") {
            range = v.trim().split('-').next().and_then(|a| a.parse().ok());
        }
    }
    let route = path.split('?').next().unwrap_or("/");
    // (status, tipe, isi, sebut ukuran?, header tambahan)
    let (status, ctype, body, with_len, extra): (&str, &str, Vec<u8>, bool, String) = match route {
        "/" => ("200 OK", "text/html; charset=utf-8", html("Uji Ovner", "beranda"), true, String::new()),
        p if p.starts_with("/page/") => ("200 OK", "text/html; charset=utf-8", html(&format!("Uji {}", &p[6..]), "<p>halaman biasa</p>"), true, String::new()),
        "/form" => ("200 OK", "text/html; charset=utf-8", html("Uji formulir", "<input id=nama><textarea id=isi></textarea>"), true, String::new()),
        "/ads" => (
            "200 OK",
            "text/html; charset=utf-8",
            html("Uji iklan", r#"<script src="http://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js"></script><div class="adsbygoogle">iklan</div>"#),
            true,
            String::new(),
        ),
        "/pagead/js/adsbygoogle.js" => ("200 OK", "application/javascript", b"document.title='IKLAN LOLOS';".to_vec(), true, String::new()),
        "/infor" => ("200 OK", "text/html; charset=utf-8", html("Uji Infor", r#"<iframe id=f src="/webui/gwtui/download?id=1" style="width:600px;height:400px"></iframe>"#), true, String::new()),
        "/webui/gwtui/download" | "/pdf-small-nolen" => ("200 OK", "application/pdf", pdf(40 * 1024), false, "Content-Disposition: inline; filename=\"permintaan.pdf\"\r\n".into()),
        "/pdf-big-nolen" => ("200 OK", "application/pdf", pdf(7 * 1_048_576), false, String::new()),
        "/pdf-big-len" => ("200 OK", "application/pdf", pdf(6 * 1_048_576), true, String::new()),
        "/yt-mock" => ("200 OK", "text/html; charset=utf-8", html("Uji YouTube", YT_MOCK), true, String::new()),
        // Seperti YouTube: Trusted Types diwajibkan (innerHTML dengan teks biasa ditolak).
        "/tt-video" => (
            "200 OK",
            "text/html; charset=utf-8",
            html("Uji video Trusted Types", r#"<video id=v width=480 height=270 style="background:#000;display:block"></video>"#),
            true,
            "Content-Security-Policy: require-trusted-types-for 'script'\r\n".into(),
        ),
        "/iframe-video" => ("200 OK", "text/html; charset=utf-8", html("Uji video dalam iframe", r#"<iframe id=f src="/tt-video" style="width:640px;height:420px;border:0"></iframe>"#), true, String::new()),
        // Pemutar HLS tiruan: halaman memuat daftar putar .m3u8 (seperti video.js di detik).
        "/hls-page" => (
            "200 OK",
            "text/html; charset=utf-8",
            html("Uji HLS", r#"<video id=v width=480 height=270 style="background:#000"></video><script>fetch("/media/master.m3u8").then(function(r){return r.text()}).then(function(t){document.title="HLS dimuat"});</script>"#),
            true,
            String::new(),
        ),
        "/media/master.m3u8" => ("200 OK", "application/vnd.apple.mpegurl", b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360\n360/index.m3u8\n".to_vec(), true, String::new()),
        "/echo" => ("200 OK", "text/plain; charset=utf-8", seen_headers.clone().into_bytes(), true, String::new()),
        // Ekstensi Chrome buatan (CRX3) untuk uji pasang ekstensi.
        "/ext.crx" => ("200 OK", "application/x-chrome-extension", crate::extensions::test_crx().1, true, String::new()),
        // Halaman dengan tautan .crx (seperti tombol Web Store): unduhannya harus dicegat Ovner.
        "/crx-page" => {
            let id = crate::extensions::test_crx().0;
            ("200 OK", "text/html; charset=utf-8", format!("<!doctype html><title>crx</title><a id=a href=\"/crx/{id}_1.crx\">Tambahkan</a>").into_bytes(), true, String::new())
        }
        p if p.starts_with("/crx/") => ("200 OK", "application/x-chrome-extension", crate::extensions::test_crx().1, true, String::new()),
        "/tr" => ("200 OK", "text/html; charset=utf-8", html("Translate test", r#"<p id=a>Hello world</p><p id=b>Good morning, how are you?</p><code id=c>let x = 1;</code>"#), true, String::new()),
        "/read" => (
            "200 OK",
            "text/html; charset=utf-8",
            html("Uji baca keras", "<article><p>Paragraf pertama untuk dibaca keras.</p><p>Paragraf kedua berisi beberapa kata lagi.</p><p>Paragraf ketiga adalah yang terakhir.</p></article>"),
            true,
            String::new(),
        ),
        "/api/timedtext" => {
            let ok = path.contains("pot=");
            let body = if ok { r#"{"events":[{"tStartMs":0,"dDurationMs":1500,"segs":[{"utf8":"halo uji"}]}]}"# } else { "" };
            ("200 OK", "application/json", body.as_bytes().to_vec(), true, String::new())
        }
        "/file-20mb" => {
            let start = range.unwrap_or(0).min(BIG_FILE);
            let body: Vec<u8> = (start..BIG_FILE).map(big_byte).collect();
            let extra = format!(
                "Accept-Ranges: bytes\r\nContent-Disposition: attachment; filename=\"uji-20mb.bin\"\r\n{}",
                if range.is_some() { format!("Content-Range: bytes {start}-{}/{BIG_FILE}\r\n", BIG_FILE - 1) } else { String::new() }
            );
            (if range.is_some() { "206 Partial Content" } else { "200 OK" }, "application/octet-stream", body, true, extra)
        }
        _ => ("404 Not Found", "text/plain", b"tidak ada".to_vec(), true, String::new()),
    };
    let mut head = format!("HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nCache-Control: no-store\r\nConnection: close\r\n{extra}");
    if with_len {
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes())?;
    // Tanpa Content-Length: isi dikirim bertahap lalu koneksi ditutup (seperti PDF yang dibuat
    // server saat itu juga).
    for chunk in body.chunks(64 * 1024) {
        s.write_all(chunk)?;
    }
    s.flush()?;
    let _ = s.shutdown(std::net::Shutdown::Both);
    let _ = r.read(&mut [0u8; 1]);
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn serves_pdf_without_length() {
        let port = super::start().unwrap();
        use std::io::{Read, Write};
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(b"GET /pdf-small-nolen HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut v = Vec::new();
        s.read_to_end(&mut v).unwrap();
        let t = String::from_utf8_lossy(&v);
        assert!(t.starts_with("HTTP/1.1 200 OK"));
        assert!(!t.to_ascii_lowercase().contains("content-length"));
        assert!(t.contains("%PDF-1.4") && t.contains("%%EOF"));
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(b"GET /file-20mb HTTP/1.1\r\nRange: bytes=1000-\r\n\r\n").unwrap();
        let mut v = Vec::new();
        s.read_to_end(&mut v).unwrap();
        let i = v.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
        assert!(String::from_utf8_lossy(&v[..i]).contains("206"));
        assert_eq!(v.len() - i, super::BIG_FILE - 1000);
        assert_eq!(v[i], super::big_byte(1000));
    }
}
