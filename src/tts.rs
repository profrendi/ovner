//! Baca keras: suara Natural online seperti Read aloud Microsoft Edge (jalur tidak resmi yang juga
//! dipakai edge-tts; bisa berubah sewaktu-waktu) dan Azure Speech berbayar (kunci sendiri).
//! Suara lokal Windows tidak lewat sini: diputar langsung oleh toolbar (speechSynthesis).

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::net;

const TOKEN: &str = "6A5AA1D4EAFF4E9FB37E23D68491D6F4";
const CHROMIUM: &str = "143.0.3650.75";
const BASE: &str = "speech.platform.bing.com/consumer/speech/synthesize/readaloud";

/// Hasil sintesis: MP3 + batas kata [(mulai ms, lama ms, kata)].
pub struct Speech {
    pub mp3: Vec<u8>,
    pub marks: Vec<(u64, u64, String)>,
}

fn now_secs() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// Token Sec-MS-GEC (berganti tiap 5 menit).
pub fn sec_ms_gec(unix: f64) -> String {
    let mut ticks = (unix + 11_644_473_600.0).floor() as u64;
    ticks -= ticks % 300;
    let s = format!("{}{TOKEN}", ticks as u128 * 10_000_000);
    let h = Sha256::digest(s.as_bytes());
    h.iter().map(|b| format!("{b:02X}")).collect()
}

fn rand_hex() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let seed = format!("{}-{}-{}", now_secs(), std::process::id(), N.fetch_add(1, Ordering::Relaxed));
    Sha256::digest(seed.as_bytes())[..16].iter().map(|b| format!("{b:02x}")).collect()
}

/// "Thu Nov 19 2026 10:46:00 GMT+0000 (Coordinated Universal Time)"
pub fn js_date(unix: u64) -> String {
    let days = (unix / 86_400) as i64;
    let secs = unix % 86_400;
    let wd = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][(days.rem_euclid(7)) as usize];
    // Hari sipil dari jumlah hari sejak 1970 (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    let mon = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][(m - 1) as usize];
    format!("{wd} {mon} {d:02} {y} {:02}:{:02}:{:02} GMT+0000 (Coordinated Universal Time)", secs / 3600, secs / 60 % 60, secs % 60)
}

fn xml_esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

fn locale_of(voice: &str) -> String {
    voice.splitn(3, '-').take(2).collect::<Vec<_>>().join("-")
}

pub fn ssml(text: &str, voice: &str, rate: u32) -> String {
    let r = rate as i64 - 100;
    format!(
        "<speak version='1.0' xmlns='http://www.w3.org/2001/10/synthesis' xml:lang='{}'><voice name='{}'><prosody pitch='+0Hz' rate='{}{}%' volume='+0%'>{}</prosody></voice></speak>",
        xml_esc(&locale_of(voice)),
        xml_esc(voice),
        if r >= 0 { "+" } else { "" },
        r,
        xml_esc(text)
    )
}

fn edge_headers(muid: &str) -> Vec<(String, String)> {
    let major = CHROMIUM.split('.').next().unwrap_or("143");
    vec![
        ("Pragma".into(), "no-cache".into()),
        ("Cache-Control".into(), "no-cache".into()),
        ("Origin".into(), "chrome-extension://jdiccldimpdaibmpdkjnbmckianbfold".into()),
        ("User-Agent".into(), format!("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{major}.0.0.0 Safari/537.36 Edg/{major}.0.0.0")),
        ("Accept-Language".into(), "en-US,en;q=0.9".into()),
        ("Cookie".into(), format!("muid={};", muid.to_ascii_uppercase())),
    ]
}

/// Daftar suara Natural online (seperti di Edge). Disimpan sementara di memori.
pub fn edge_voices() -> Result<Value, String> {
    static CACHE: std::sync::Mutex<Option<Value>> = std::sync::Mutex::new(None);
    if let Some(v) = CACHE.lock().unwrap().clone() {
        return Ok(v);
    }
    let url = format!("https://{BASE}/voices/list?trustedclienttoken={TOKEN}&Sec-MS-GEC={}&Sec-MS-GEC-Version=1-{CHROMIUM}", sec_ms_gec(now_secs()));
    let h = edge_headers(&rand_hex());
    let hv: Vec<(&str, &str)> = h.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let r = net::get(&url, &hv)?;
    if r.status != 200 {
        return Err(format!("daftar suara ditolak (HTTP {})", r.status));
    }
    let v = simplify_voices(&r.text())?;
    *CACHE.lock().unwrap() = Some(v.clone());
    Ok(v)
}

/// [{name, label, locale, gender}] dari daftar suara Microsoft (Edge/Azure, formatnya sama).
pub fn simplify_voices(text: &str) -> Result<Value, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("daftar suara tidak terbaca: {e}"))?;
    let arr = v.as_array().ok_or("daftar suara kosong")?;
    Ok(Value::Array(
        arr.iter()
            .filter_map(|x| {
                let name = x["ShortName"].as_str()?;
                let label = x["FriendlyName"].as_str().or(x["DisplayName"].as_str()).unwrap_or(name);
                Some(json!({"name": name, "label": label, "locale": x["Locale"].as_str().unwrap_or(""), "gender": x["Gender"].as_str().unwrap_or("")}))
            })
            .collect(),
    ))
}

/// Pesan teks WebSocket: (header, isi).
pub fn split_text_msg(m: &str) -> (&str, &str) {
    m.split_once("\r\n\r\n").unwrap_or((m, ""))
}

/// Pesan biner WebSocket: (header, audio).
pub fn split_binary_msg(b: &[u8]) -> Option<(String, &[u8])> {
    if b.len() < 2 {
        return None;
    }
    let n = u16::from_be_bytes([b[0], b[1]]) as usize;
    if b.len() < 2 + n {
        return None;
    }
    Some((String::from_utf8_lossy(&b[2..2 + n]).to_string(), &b[2 + n..]))
}

/// Batas kata dari "audio.metadata".
pub fn parse_marks(body: &str) -> Vec<(u64, u64, String)> {
    let Ok(v) = serde_json::from_str::<Value>(body) else { return Vec::new() };
    v["Metadata"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|m| m["Type"] == "WordBoundary")
                .filter_map(|m| {
                    let d = &m["Data"];
                    Some((d["Offset"].as_u64()? / 10_000, d["Duration"].as_u64().unwrap_or(0) / 10_000, d["text"]["Text"].as_str()?.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn edge_speak(text: &str, voice: &str, rate: u32) -> Result<Speech, String> {
    let conn = rand_hex();
    let url = format!("wss://{BASE}/edge/v1?TrustedClientToken={TOKEN}&ConnectionId={conn}&Sec-MS-GEC={}&Sec-MS-GEC-Version=1-{CHROMIUM}", sec_ms_gec(now_secs()));
    let h = edge_headers(&rand_hex());
    let hv: Vec<(&str, &str)> = h.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let mut ws = net::Ws::connect(&url, &hv)?;
    let date = js_date(now_secs() as u64);
    ws.send_text(&format!(
        "X-Timestamp:{date}\r\nContent-Type:application/json; charset=utf-8\r\nPath:speech.config\r\n\r\n{{\"context\":{{\"synthesis\":{{\"audio\":{{\"metadataoptions\":{{\"sentenceBoundaryEnabled\":\"false\",\"wordBoundaryEnabled\":\"true\"}},\"outputFormat\":\"audio-24khz-48kbitrate-mono-mp3\"}}}}}}}}\r\n"
    ))?;
    ws.send_text(&format!("X-RequestId:{}\r\nContent-Type:application/ssml+xml\r\nX-Timestamp:{date}Z\r\nPath:ssml\r\n\r\n{}", rand_hex(), ssml(text, voice, rate)))?;
    let mut sp = Speech { mp3: Vec::new(), marks: Vec::new() };
    let started = std::time::Instant::now();
    loop {
        if started.elapsed().as_secs() > 60 {
            return Err("suara online tidak menjawab".into());
        }
        match ws.recv()? {
            None => break,
            Some(net::WsMsg::Text(t)) => {
                let (head, body) = split_text_msg(&t);
                if head.contains("Path:turn.end") {
                    break;
                }
                if head.contains("Path:audio.metadata") {
                    sp.marks.extend(parse_marks(body));
                }
            }
            Some(net::WsMsg::Binary(b)) => {
                if let Some((head, audio)) = split_binary_msg(&b) {
                    if head.contains("Path:audio") {
                        sp.mp3.extend_from_slice(audio);
                    }
                }
            }
        }
    }
    if sp.mp3.is_empty() {
        return Err("suara online tidak mengirim audio".into());
    }
    Ok(sp)
}

// ---- Azure Speech (berbayar, kunci pengguna) ----

pub fn azure_voices(key: &str, region: &str) -> Result<Value, String> {
    let r = net::get(&format!("https://{region}.tts.speech.microsoft.com/cognitiveservices/voices/list"), &[("Ocp-Apim-Subscription-Key", key)])?;
    if r.status != 200 {
        return Err(format!("Azure menolak kunci/region (HTTP {})", r.status));
    }
    simplify_voices(&r.text())
}

pub fn azure_speak(key: &str, region: &str, text: &str, voice: &str, rate: u32) -> Result<Speech, String> {
    let body = ssml(text, voice, rate);
    let r = net::post(
        &format!("https://{region}.tts.speech.microsoft.com/cognitiveservices/v1"),
        &[
            ("Ocp-Apim-Subscription-Key", key),
            ("Content-Type", "application/ssml+xml"),
            ("X-Microsoft-OutputFormat", "audio-24khz-48kbitrate-mono-mp3"),
            ("User-Agent", "Ovner"),
        ],
        body.as_bytes(),
    )?;
    if r.status != 200 {
        return Err(format!("Azure Speech menolak (HTTP {})", r.status));
    }
    Ok(Speech { mp3: r.body, marks: Vec::new() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gec_and_date() {
        let g = sec_ms_gec(1_795_090_000.0);
        assert_eq!(g.len(), 64);
        assert_eq!(g, sec_ms_gec(1_795_090_000.0 + 10.0), "sama dalam jendela 5 menit");
        assert!(g.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_lowercase()));
        assert_eq!(js_date(0), "Thu Jan 01 1970 00:00:00 GMT+0000 (Coordinated Universal Time)");
        assert_eq!(js_date(1_795_084_000), "Thu Nov 19 2026 10:26:40 GMT+0000 (Coordinated Universal Time)");
    }

    #[test]
    fn ssml_and_frames() {
        let s = ssml("A & <b>", "id-ID-GadisNeural", 125);
        assert!(s.contains("xml:lang='id-ID'") && s.contains("rate='+25%'") && s.contains("A &amp; &lt;b&gt;"));
        assert!(ssml("x", "en-US-AriaNeural", 80).contains("rate='-20%'"));
        let mut b = vec![0u8, 11];
        b.extend_from_slice(b"Path:audio\n");
        b.extend_from_slice(&[1, 2, 3]);
        let (h, a) = split_binary_msg(&b).unwrap();
        assert!(h.contains("Path:audio"));
        assert_eq!(a, &[1, 2, 3]);
        let m = parse_marks(r#"{"Metadata":[{"Type":"WordBoundary","Data":{"Offset":1000000,"Duration":2500000,"text":{"Text":"Halo","Length":4}}},{"Type":"SessionEnd","Data":{}}]}"#);
        assert_eq!(m, vec![(100, 250, "Halo".to_string())]);
        let v = simplify_voices(r#"[{"ShortName":"id-ID-GadisNeural","FriendlyName":"Microsoft Gadis Online (Natural) - Indonesian (Indonesia)","Locale":"id-ID","Gender":"Female"}]"#).unwrap();
        assert_eq!(v[0]["name"], "id-ID-GadisNeural");
    }
}
