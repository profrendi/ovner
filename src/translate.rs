//! Penerjemah halaman (seperti Edge/Chrome). Teks halaman dikirim ke layanan yang dipilih HANYA
//! saat pengguna menekan Terjemahkan:
//! - "microsoft": jalur yang dipakai Microsoft Edge (token dari edge.microsoft.com/translate/auth,
//!   lalu Microsoft Translator v3),
//! - "google": jalur elemen terjemahan Google (translateHtml), seperti Chrome.
//! Keduanya bukan API resmi untuk pihak ketiga; bila berubah, Ovner diperbaiki lewat update.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::net;

/// Hasil satu kelompok teks: terjemahan (urutan sama) + bahasa sumber yang terdeteksi.
pub struct Out {
    pub texts: Vec<String>,
    pub detected: Option<String>,
}

/// Uji --self-test: terjemahan tiruan tanpa internet ("[id] HURUF BESAR").
pub static MOCK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn translate(engine: &str, texts: &[String], from: &str, to: &str) -> Result<Out, String> {
    if texts.is_empty() {
        return Ok(Out { texts: Vec::new(), detected: None });
    }
    if MOCK.load(std::sync::atomic::Ordering::Relaxed) {
        return Ok(Out { texts: texts.iter().map(|t| format!("[{to}] {}", t.to_uppercase())).collect(), detected: Some("en".into()) });
    }
    match engine {
        "google" => google(texts, from, to).or_else(|e| {
            crate::log(&format!("[terjemah] Google gagal ({e}), dicoba Microsoft"));
            microsoft(texts, from, to)
        }),
        _ => microsoft(texts, from, to).or_else(|e| {
            crate::log(&format!("[terjemah] Microsoft gagal ({e}), dicoba Google"));
            google(texts, from, to)
        }),
    }
}

// ---- Microsoft (seperti Edge) ----

static TOKEN: Mutex<Option<(String, Instant)>> = Mutex::new(None);

fn ms_token(fresh: bool) -> Result<String, String> {
    let mut t = TOKEN.lock().unwrap();
    if !fresh {
        if let Some((tok, at)) = t.as_ref() {
            if at.elapsed() < Duration::from_secs(8 * 60) {
                return Ok(tok.clone());
            }
        }
    }
    let r = net::get("https://edge.microsoft.com/translate/auth", &[])?;
    let tok = r.text().trim().to_string();
    if r.status != 200 || tok.len() < 20 || tok.contains(char::is_whitespace) {
        return Err(format!("token Microsoft ditolak (HTTP {})", r.status));
    }
    *t = Some((tok.clone(), Instant::now()));
    Ok(tok)
}

fn microsoft(texts: &[String], from: &str, to: &str) -> Result<Out, String> {
    let body = Value::Array(texts.iter().map(|t| json!({ "Text": t })).collect()).to_string();
    let mut url = format!("https://api-edge.cognitivetranslator.com/translate?api-version=3.0&to={}", net::enc(to));
    if !from.is_empty() && from != "auto" {
        url.push_str(&format!("&from={}", net::enc(from)));
    }
    for attempt in 0..2 {
        let tok = ms_token(attempt > 0)?;
        let auth = format!("Bearer {tok}");
        let r = net::post(&url, &[("Authorization", auth.as_str()), ("Content-Type", "application/json")], body.as_bytes())?;
        if r.status == 401 && attempt == 0 {
            continue;
        }
        if r.status != 200 {
            return Err(format!("HTTP {}: {}", r.status, r.text().chars().take(160).collect::<String>()));
        }
        return parse_microsoft(&r.text(), texts.len());
    }
    Err("token Microsoft kedaluwarsa".into())
}

pub fn parse_microsoft(text: &str, n: usize) -> Result<Out, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("jawaban Microsoft tidak terbaca: {e}"))?;
    let arr = v.as_array().ok_or("jawaban Microsoft bukan daftar")?;
    if arr.len() != n {
        return Err(format!("jumlah terjemahan {} ≠ {n}", arr.len()));
    }
    let mut detected = None;
    let mut out = Vec::with_capacity(n);
    for item in arr {
        if detected.is_none() {
            detected = item["detectedLanguage"]["language"].as_str().map(str::to_string);
        }
        out.push(item["translations"][0]["text"].as_str().ok_or("terjemahan kosong")?.to_string());
    }
    Ok(Out { texts: out, detected })
}

// ---- Google (seperti Chrome) ----

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub fn unesc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let end = tail.find(';').filter(|&e| e <= 10);
        let ent = end.map(|e| &tail[1..e]);
        let rep = match ent {
            Some("amp") => Some('&'),
            Some("lt") => Some('<'),
            Some("gt") => Some('>'),
            Some("quot") => Some('"'),
            Some("apos") => Some('\''),
            Some("nbsp") => Some('\u{a0}'),
            Some(e) if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
            Some(e) if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match (rep, end) {
            (Some(c), Some(e)) => {
                out.push(c);
                rest = &tail[e + 1..];
            }
            _ => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn google(texts: &[String], from: &str, to: &str) -> Result<Out, String> {
    let src = if from.is_empty() { "auto" } else { google_code(from) };
    let to = google_code(to);
    let list: Vec<String> = texts.iter().map(|t| esc(t)).collect();
    let body = json!([[list, src, to], "te_lib"]).to_string();
    let r = net::post(
        "https://translate-pa.googleapis.com/v1/translateHtml",
        &[("Content-Type", "application/json+protobuf"), ("X-Goog-API-Key", "AIzaSyATBXajvzQLTDHEQbcpq0Ihe0vWDHmO520")],
        body.as_bytes(),
    )?;
    if r.status != 200 {
        return Err(format!("HTTP {}", r.status));
    }
    parse_google(&r.text(), texts.len())
}

pub fn parse_google(text: &str, n: usize) -> Result<Out, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("jawaban Google tidak terbaca: {e}"))?;
    let arr = v[0].as_array().ok_or("jawaban Google tanpa terjemahan")?;
    if arr.len() != n {
        return Err(format!("jumlah terjemahan {} ≠ {n}", arr.len()));
    }
    let texts = arr.iter().map(|t| unesc(t.as_str().unwrap_or_default())).collect();
    let detected = v[1][0].as_str().map(str::to_string);
    Ok(Out { texts, detected })
}

/// Bahasa tujuan bawaan: halaman berbahasa Indonesia → Inggris, selain itu → Indonesia.
pub fn auto_target(page_lang: &str) -> &'static str {
    let l = page_lang.to_ascii_lowercase();
    if l == "id" || l.starts_with("id-") || l == "in" || l.starts_with("in-") {
        "en"
    } else {
        "id"
    }
}

/// Daftar bahasa di panel terjemahan: (kode, nama Indonesia, nama Inggris).
pub const LANGS: &[(&str, &str, &str)] = &[
    ("id", "Indonesia", "Indonesian"),
    ("en", "Inggris", "English"),
    ("ms", "Melayu", "Malay"),
    ("jv", "Jawa", "Javanese"),
    ("su", "Sunda", "Sundanese"),
    ("ar", "Arab", "Arabic"),
    ("zh-Hans", "Tionghoa (Sederhana)", "Chinese (Simplified)"),
    ("zh-Hant", "Tionghoa (Tradisional)", "Chinese (Traditional)"),
    ("ja", "Jepang", "Japanese"),
    ("ko", "Korea", "Korean"),
    ("th", "Thai", "Thai"),
    ("vi", "Vietnam", "Vietnamese"),
    ("tl", "Filipina", "Filipino"),
    ("hi", "Hindi", "Hindi"),
    ("de", "Jerman", "German"),
    ("fr", "Prancis", "French"),
    ("es", "Spanyol", "Spanish"),
    ("pt", "Portugis", "Portuguese"),
    ("it", "Italia", "Italian"),
    ("nl", "Belanda", "Dutch"),
    ("ru", "Rusia", "Russian"),
    ("tr", "Turki", "Turkish"),
];

/// Kode bahasa Google untuk kode Microsoft.
pub fn google_code(code: &str) -> &str {
    match code {
        "zh-Hans" => "zh-CN",
        "zh-Hant" => "zh-TW",
        "tl" => "fil",
        c => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_microsoft() {
        let t = r#"[{"detectedLanguage":{"language":"en","score":1.0},"translations":[{"text":"Halo dunia","to":"id"}]},{"translations":[{"text":"Apa kabar?","to":"id"}]}]"#;
        let o = parse_microsoft(t, 2).unwrap();
        assert_eq!(o.texts, vec!["Halo dunia", "Apa kabar?"]);
        assert_eq!(o.detected.as_deref(), Some("en"));
        assert!(parse_microsoft(t, 3).is_err());
    }

    #[test]
    fn parses_google() {
        let t = r#"[["Halo &amp; selamat &lt;datang&gt;","Ya &#39;benar&#39;"],["en","en"]]"#;
        let o = parse_google(t, 2).unwrap();
        assert_eq!(o.texts, vec!["Halo & selamat <datang>", "Ya 'benar'"]);
        assert_eq!(o.detected.as_deref(), Some("en"));
        assert_eq!(unesc("a &x b &#x41;"), "a &x b A");
    }

    #[test]
    fn default_target() {
        assert_eq!(auto_target("id-ID"), "en");
        assert_eq!(auto_target("en"), "id");
        assert_eq!(auto_target(""), "id");
        assert_eq!(google_code("zh-Hans"), "zh-CN");
    }
}
