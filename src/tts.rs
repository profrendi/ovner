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

/// Token Sec-MS-GEC v2 (berganti tiap 5 menit): "<CS64 16 hex>-<SHA-256 termodifikasi 64 hex>".
/// Sejak akhir 2026 Microsoft menolak token v1 (SHA-256 biasa) dengan HTTP 403. Algoritmanya
/// disalin dari edge-tts 7.3.1 (drm.py) dan diuji dengan nilai keluaran edge-tts.
pub fn sec_ms_gec(unix: f64) -> String {
    let mut ticks = (unix + 11_644_473_600.0).floor() as u64;
    ticks -= ticks % 300;
    let s = format!("{}{TOKEN}", ticks as u128 * 10_000_000);
    gec::token(s.as_bytes())
}

mod gec {
    const INITIAL: [u32; 8] = [0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A, 0x510E527F, 0x9B05688C, 0x1F83D9AB, 0x5BE0CD19];
    const STATE: [u32; 8] = [0xFD9FBEA2, 0x91243CE7, 0xBE499094, 0xCD2E1E0A, 0x6C006F49, 0x04BD35AB, 0xFEB0A7E4, 0x03EFFBD2];
    const SCHEDULE: [u32; 14] = [
        0xEB24BF9E, 0x1558691F, 0xA0C09EE2, 0x0CFA32BA, 0xA622EE8C, 0xD1DD9491, 0xC0DD25A0, 0xF9E70AB6, 0xF0C31573, 0x7058CE4E, 0x7188E606, 0x00E5AB33, 0xD258B060, 0x72766564,
    ];
    const K: [u32; 64] = [
        0x428A2F98, 0x71374491, 0xB5C0FBCF, 0xE9B5DBA5, 0x3956C25B, 0x59F111F1, 0x923F82A4, 0xAB1C5ED5, 0xD807AA98, 0x12835B01, 0x243185BE, 0x550C7DC3, 0x72BE5D74, 0x80DEB1FE,
        0x9BDC06A7, 0xC19BF174, 0xE49B69C1, 0xEFBE4786, 0x0FC19DC6, 0x240CA1CC, 0x2DE92C6F, 0x4A7484AA, 0x5CB0A9DC, 0x76F988DA, 0x983E5152, 0xA831C66D, 0xB00327C8, 0xBF597FC7,
        0xC6E00BF3, 0xD5A79147, 0x06CA6351, 0x14292967, 0x27B70A85, 0x2E1B2138, 0x4D2C6DFC, 0x53380D13, 0x650A7354, 0x766A0ABB, 0x81C2C92E, 0x92722C85, 0xA2BFE8A1, 0xA81A664B,
        0xC24B8B70, 0xC76C51A3, 0xD192E819, 0xD6990624, 0xF40E3585, 0x106AA070, 0x19A4C116, 0x1E376C08, 0x2748774C, 0x34B0BCB5, 0x391C0CB3, 0x4ED8AA4A, 0x5B9CCA4F, 0x682E6FF3,
        0x748F82EE, 0x78A5636F, 0x84C87814, 0x8CC70208, 0x90BEFFFA, 0xA4506CEB, 0xBEF9A3F7, 0xC67178F2,
    ];
    const ADJ: u32 = 0xC9DC06A7;
    const SIGMA0_ADJ: u32 = 0x059C0B80;
    const SIGMA1_ADJ: u32 = 0x000B92C0;
    const CS64: [(u32, u32); 14] = [
        (0xE82E91B1, 0xB32700D0),
        (0x44DE24C6, 0x88DFF51D),
        (0x3BEB65BC, 0x29514038),
        (0xC89C5933, 0xB328CBDF),
        (0x8D1EC211, 0xD405B4D5),
        (0x314EF5D5, 0x26E55F3C),
        (0xE92491B1, 0x607801C6),
        (0xF0A5B9D5, 0x82B2C1FB),
        (0x06064CF3, 0xFA51A3DA),
        (0xDA79FA60, 0x207CC464),
        (0xBED20560, 0x3C3C74F3),
        (0x89F9C9F8, 0xD2D14B61),
        (0x35FA38C6, 0xBB04A2C1),
        (0xCB49E3C6, 0xC5626167),
    ];
    const CS64_FACTOR: u32 = 0x82893933;
    const CS64_ADJ0: u32 = 0xF2F90B1A;
    const CS64_ADJ1: u32 = 0x0B1A0000;

    fn s0(x: u32) -> u32 {
        x.rotate_right(7) ^ x.rotate_right(18) ^ (x >> 3)
    }
    fn s1(x: u32) -> u32 {
        x.rotate_right(17) ^ x.rotate_right(19) ^ (x >> 10)
    }
    fn rol16(x: u32) -> u32 {
        x.rotate_left(16)
    }
    fn expand(w: &mut Vec<u32>, first: usize) {
        for i in first..64 {
            let v = s1(w[i - 2]).wrapping_add(w[i - 7]).wrapping_add(s0(w[i - 15])).wrapping_add(w[i - 16]);
            w.push(v);
        }
    }
    fn rounds(state: [u32; 8], w: &[u32], first: usize) -> [u32; 8] {
        let mut k = K;
        if first == 14 {
            k[14] = ADJ;
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for i in first..64 {
            let t1 = h
                .wrapping_add(e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25))
                .wrapping_add(g ^ (e & (f ^ g)))
                .wrapping_add(k[i])
                .wrapping_add(w[i]);
            let t2 = (a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22)).wrapping_add((a & b) | (c & (a | b)));
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        [a, b, c, d, e, f, g, h]
    }
    fn add(x: [u32; 8], y: [u32; 8]) -> [u32; 8] {
        let mut o = [0u32; 8];
        for i in 0..8 {
            o[i] = x[i].wrapping_add(y[i]);
        }
        o
    }
    fn init(d: &[u8]) -> [u32; 8] {
        let mut w = vec![0u32; 14];
        w.push(u32::from_be_bytes([0, d[0], d[1], d[2]]));
        w.push(u32::from_be_bytes([d[3], d[4], d[5], d[6]]));
        for i in 16..30 {
            let mut v = SCHEDULE[i - 16];
            v = v.wrapping_add(if i == 16 { s1(w[i - 2]) ^ SIGMA1_ADJ } else { s1(w[i - 2]) });
            if i >= 21 {
                v = v.wrapping_add(w[i - 7]);
            }
            if i >= 29 {
                v = v.wrapping_add(if i == 29 { s0(w[i - 15]) ^ SIGMA0_ADJ } else { s0(w[i - 15]) });
            }
            w.push(v);
        }
        let v = s1(w[28]).wrapping_add(w[23]).wrapping_add(s0(w[15])).wrapping_add(w[14]).wrapping_add(ADJ).wrapping_sub(K[14]);
        w.push(v);
        expand(&mut w, 31);
        add(INITIAL, rounds(STATE, &w, 14))
    }
    pub fn sha(d: &[u8]) -> [u8; 32] {
        let length = 57 + d.len();
        let mut p = d[7..].to_vec();
        p.push(0x80);
        let pad = (55i64 - length as i64).rem_euclid(64) as usize;
        p.extend(std::iter::repeat_n(0u8, pad));
        p.extend_from_slice(&((length as u64) * 8).to_be_bytes());
        let mut st = init(&d[..7]);
        for blk in p.chunks(64) {
            let mut w: Vec<u32> = blk.chunks(4).map(|c| u32::from_be_bytes([c[0], c[1], c[2], c[3]])).collect();
            expand(&mut w, 16);
            st = add(st, rounds(st, &w, 0));
        }
        let mut out = [0u8; 32];
        for (i, x) in st.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&x.to_be_bytes());
        }
        out
    }
    fn cs64(d: &[u8], digest: &[u8; 32]) -> (u32, u32) {
        let ka = u32::from_le_bytes([digest[0], digest[1], digest[2], digest[3]]) | 1;
        let kb = u32::from_le_bytes([digest[4], digest[5], digest[6], digest[7]]) | 1;
        let fac = [ka.wrapping_mul(CS64_FACTOR), kb.wrapping_mul(CS64_FACTOR)];
        let mut terms: Vec<u32> = CS64.iter().enumerate().map(|(i, (c, _))| c.wrapping_mul(fac[i % 2])).collect();
        let mut offs: Vec<u32> = CS64.iter().map(|(_, o)| *o).collect();
        let mut data = vec![0u8];
        data.extend_from_slice(d);
        while data.len() % 4 != 0 {
            data.push(0);
        }
        let mut words: Vec<u32> = data.chunks(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        if words.len() % 2 == 1 {
            words.push(0);
        }
        for (i, &wd) in words.iter().enumerate() {
            let even = i % 2 == 0;
            let mut term = wd.wrapping_mul(if even { ka } else { kb });
            let mut off = rol16(wd).wrapping_mul(if even { 0xEF0569FB } else { 0xC31713DB });
            if i == 0 {
                term = term.wrapping_add(CS64_ADJ0.wrapping_mul(fac[0]));
                off = off.wrapping_add(CS64_ADJ1);
            }
            terms.push(term);
            offs.push(off);
        }
        let (mut swap_acc, mut swap_last, mut state, mut acc) = (0u32, 0u32, 0u32, 0u32);
        for i in (0..terms.len()).step_by(2) {
            let (x, y) = (terms[i], terms[i + 1]);
            let t = rol16(offs[i].wrapping_add(x));
            let q = t.wrapping_mul(0x76F50F34);
            swap_acc = swap_acc.wrapping_add(rol16(q).wrapping_mul(0xC3EFEA97).wrapping_add(q));
            let t = rol16(offs[i + 1].wrapping_add(y));
            let q = t.wrapping_mul(0x1881CE3C);
            swap_last = rol16(q).wrapping_mul(0x35BD1EC9).wrapping_add(q);
            swap_acc = swap_acc.wrapping_add(swap_last);
            state = state.wrapping_mul(ka).wrapping_add(x);
            state = rol16(state).wrapping_mul(0xCF98B111);
            state = rol16(state).wrapping_mul(0x87085B9F);
            state = rol16(state).wrapping_mul(0x12CEB96D);
            state = rol16(state).wrapping_mul(0x257E1D83);
            acc = acc.wrapping_add(state);
            let mut t = state.wrapping_mul(kb).wrapping_add(y);
            t = rol16(t).wrapping_mul(0xA27416F5);
            t = rol16(t).wrapping_mul(0xD38396FF);
            state = rol16(rol16(t).wrapping_mul(0x7C932B89)).wrapping_mul(0xBFA49F69);
            acc = acc.wrapping_add(state);
        }
        (swap_last ^ state, swap_acc ^ acc)
    }
    pub fn token(d: &[u8]) -> String {
        let digest = sha(d);
        let (a, b) = cs64(d, &digest);
        let mut cs = a.to_le_bytes().to_vec();
        cs.extend_from_slice(&b.to_le_bytes());
        let hex = |v: &[u8]| v.iter().map(|x| format!("{x:02X}")).collect::<String>();
        format!("{}-{}", hex(&cs), hex(&digest))
    }
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
    let url = format!("https://{BASE}/voices/list?trustedclienttoken={TOKEN}&Sec-MS-GEC={}&Sec-MS-GEC-Version=2-{CHROMIUM}", sec_ms_gec(now_secs()));
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
    let url = format!("wss://{BASE}/edge/v1?TrustedClientToken={TOKEN}&ConnectionId={conn}&Sec-MS-GEC={}&Sec-MS-GEC-Version=2-{CHROMIUM}", sec_ms_gec(now_secs()));
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
        // Nilai pembanding dari edge-tts 7.3.1 (generate_sec_ms_gec_v2).
        assert_eq!(sec_ms_gec(1_795_090_000.0), "3EEC149CE1B512F6-73479C4BC61DE45A1A022C75C42FA52F6279B827F18EB7BB893A8CA20B7A6BF4");
        assert_eq!(sec_ms_gec(1_700_000_000.5), "2CD23A2CA0B2595A-F3C846C370E05F9CC83ACC5F995C5F5D87A4D0F14CE8936A0F902E64099E38A2");
        assert_eq!(sec_ms_gec(1_795_084_863.9), "A267D259332B2600-5C588148BF4B8D8D2EAA72F169E2FBB41BCE58BE6B56B320E10C8F3890188AE9");
        assert_eq!(sec_ms_gec(1_795_090_000.0), sec_ms_gec(1_795_090_000.0 + 10.0), "sama dalam jendela 5 menit");
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
