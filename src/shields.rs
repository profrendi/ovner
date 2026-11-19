//! Fitur privasi berbasis alamat dan skrip halaman:
//! - !bangs (seperti Helium/DuckDuckGo),
//! - debounce: lewati link pengalih pelacak (seperti Brave),
//! - de-AMP: buka halaman asli, bukan versi AMP Google (seperti Brave),
//! - skrip yang disuntikkan ke halaman: farbling (acak sidik jari), matikan spellcheck,
//!   abaikan beforeunload, CSS sendiri, pemilih elemen, dan AMP canonical.

use serde_json::json;
use url::Url;

use crate::settings::Settings;

const CONTENT_JS: &str = include_str!("ui/content.js");
const FARBLE_JS: &str = include_str!("ui/farble.js");
const PRIVACY_JS: &str = include_str!("ui/privacy.js");
/// Dark Reader (MIT, darkreader.org), diperkecil dan dibungkus supaya selalu jadi global.
pub const DARKREADER_JS: &str = include_str!("ui/darkreader.js");
const DARKFILTER_JS: &str = include_str!("ui/darkfilter.js");
const ALLOWCOPY_JS: &str = include_str!("ui/allowcopy.js");

/// Tema Windows sedang gelap? (untuk mode gelap "ikut Windows"; diperbarui thread UI)
pub static SYSTEM_DARK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn system_dark() -> bool {
    SYSTEM_DARK.load(std::sync::atomic::Ordering::Relaxed)
}

/// Setelan Dark Reader dari Ovner (persen → angka yang dipakai Dark Reader).
fn dark_theme_json(s: &Settings) -> String {
    json!({
        "brightness": s.dark_brightness, "contrast": s.dark_contrast,
        "sepia": s.dark_sepia, "grayscale": s.dark_grayscale,
    })
    .to_string()
}

/// Nyalakan Dark Reader begitu `<head>` ada (skrip awal dokumen jalan sebelum ada DOM).
fn dark_enable_js(s: &Settings) -> String {
    format!(
        "\n(function(){{var go=function(){{try{{DarkReader.setFetchMethod(window.fetch.bind(window));DarkReader.enable({});}}catch(e){{}}}};\
if(document.head){{go();return;}}\
var mo=new MutationObserver(function(){{if(document.head){{mo.disconnect();go();}}}});\
mo.observe(document,{{childList:true,subtree:true}});}})();\n",
        dark_theme_json(s)
    )
}

/// Skrip awal dokumen untuk fitur per situs (mode gelap, izinkan salin), atau kosong.
/// Mode gelap Chromium tidak memakai skrip (lihat [`dark_cdp`]).
pub fn site_feature_script(s: &Settings, host: &str) -> String {
    let mut out = String::new();
    match s.dark_for(host, system_dark()) {
        Some("dynamic") => {
            out.push_str(DARKREADER_JS);
            out.push_str(&dark_enable_js(s));
        }
        Some("filter") => out.push_str(&DARKFILTER_JS.replace("__OVNER_DARK__", &dark_theme_json(s))),
        _ => {}
    }
    if s.allow_copy_for(host) {
        out.push('\n');
        out.push_str(ALLOWCOPY_JS);
    }
    out
}

/// Skrip untuk menyalakan/mematikan mode gelap di halaman yang sudah terbuka (tanpa muat ulang).
pub fn dark_live_script(s: &Settings, engine: Option<&str>) -> String {
    let off = "try{window.DarkReader&&DarkReader.disable()}catch(e){};window.__ovnerDarkFilter&&__ovnerDarkFilter(false);";
    match engine {
        Some("dynamic") => format!("{off}if(!window.DarkReader){{{DARKREADER_JS}\n}}{}", dark_enable_js(s)),
        Some("filter") => format!("{off}{}", DARKFILTER_JS.replace("__OVNER_DARK__", &dark_theme_json(s))),
        _ => off.to_string(),
    }
}

/// Perintah DevTools mode gelap otomatis Chromium untuk situs ini.
pub fn dark_cdp(s: &Settings, host: &str) -> (&'static str, &'static str) {
    if s.dark_for(host, system_dark()) == Some("chromium") {
        ("Emulation.setAutoDarkModeOverride", r#"{"enabled":true}"#)
    } else {
        ("Emulation.setAutoDarkModeOverride", "{}")
    }
}

/// Skrip izinkan salin untuk halaman yang sudah terbuka.
pub fn allow_copy_script() -> &'static str {
    ALLOWCOPY_JS
}

/// Bang bawaan: (kode, URL dengan %s).
pub const BANGS: &[(&str, &str)] = &[
    ("g", "https://www.google.com/search?q=%s"),
    ("img", "https://www.google.com/search?tbm=isch&q=%s"),
    ("maps", "https://www.google.com/maps/search/%s"),
    ("tr", "https://translate.google.com/?sl=auto&tl=id&text=%s"),
    ("ddg", "https://duckduckgo.com/?q=%s"),
    ("b", "https://www.bing.com/search?q=%s"),
    ("bs", "https://search.brave.com/search?q=%s"),
    ("w", "https://en.wikipedia.org/w/index.php?search=%s"),
    ("wid", "https://id.wikipedia.org/w/index.php?search=%s"),
    ("kbbi", "https://kbbi.kemdikbud.go.id/entri/%s"),
    ("yt", "https://www.youtube.com/results?search_query=%s"),
    ("gh", "https://github.com/search?q=%s"),
    ("so", "https://stackoverflow.com/search?q=%s"),
    ("r", "https://www.reddit.com/search/?q=%s"),
    ("x", "https://x.com/search?q=%s"),
    ("tok", "https://www.tokopedia.com/search?q=%s"),
    ("shp", "https://shopee.co.id/search?keyword=%s"),
    ("a", "https://www.amazon.com/s?k=%s"),
    ("imdb", "https://www.imdb.com/find/?q=%s"),
    ("mdn", "https://developer.mozilla.org/search?q=%s"),
    ("rs", "https://docs.rs/releases/search?query=%s"),
    ("crates", "https://crates.io/search?q=%s"),
    ("npm", "https://www.npmjs.com/search?q=%s"),
    ("pp", "https://www.perplexity.ai/search?q=%s"),
    ("claude", "https://claude.ai/new?q=%s"),
    ("gpt", "https://chatgpt.com/?q=%s"),
];

/// Bang sendiri dari setelan: baris `!kode https://alamat?q=%s`.
fn custom_bang(settings: &Settings, code: &str) -> Option<String> {
    settings.custom_bangs.iter().find_map(|line| {
        let mut it = line.split_whitespace();
        let c = it.next()?.trim_start_matches('!');
        let u = it.next()?;
        (c.eq_ignore_ascii_case(code) && u.contains("%s") && u.starts_with("http")).then(|| u.to_string())
    })
}

/// `!kode kata kunci` atau `kata kunci !kode` → URL tujuan. Bang tanpa kata kunci membuka situsnya.
pub fn expand_bang(text: &str, settings: &Settings) -> Option<String> {
    if !settings.bangs {
        return None;
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    let (pos, code) = words.iter().enumerate().find_map(|(i, w)| {
        let c = w.strip_prefix('!')?;
        (!c.is_empty() && (i == 0 || i == words.len() - 1)).then(|| (i, c.to_ascii_lowercase()))
    })?;
    let template = custom_bang(settings, &code)
        .or_else(|| BANGS.iter().find(|(c, _)| *c == code).map(|(_, u)| u.to_string()))?;
    let query: Vec<&str> = words.iter().enumerate().filter(|(i, _)| *i != pos).map(|(_, w)| *w).collect();
    let query = query.join(" ");
    if query.is_empty() {
        let u = Url::parse(&template.replace("%s", "")).ok()?;
        return Some(format!("{}://{}/", u.scheme(), u.host_str()?));
    }
    let q: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
    Some(template.replacen("%s", &q, 1))
}

/// Daftar pengalih: (akhiran host, awalan path, nama parameter tujuan).
const REDIRECTORS: &[(&str, &str, &str)] = &[
    ("l.facebook.com", "/l.php", "u"),
    ("lm.facebook.com", "/l.php", "u"),
    ("l.messenger.com", "/l.php", "u"),
    ("l.instagram.com", "/", "u"),
    ("l.threads.net", "/", "u"),
    ("out.reddit.com", "/", "url"),
    ("www.youtube.com", "/redirect", "q"),
    ("youtube.com", "/redirect", "q"),
    ("t.umblr.com", "/redirect", "z"),
    ("away.vk.com", "/away.php", "to"),
    ("steamcommunity.com", "/linkfilter/", "url"),
    ("www.linkedin.com", "/safety/go", "url"),
    ("slack-redir.net", "/link", "url"),
    ("exit.sc", "/", "url"),
    ("click.linksynergy.com", "/", "murl"),
    ("go.redirectingat.com", "/", "url"),
];

fn is_google_host(host: &str) -> bool {
    let h = host.trim_start_matches("www.");
    h.starts_with("google.") && h.split('.').count() <= 3
}

/// Tujuan asli bila `raw` adalah link pengalih pelacak (debounce).
pub fn debounce(raw: &str) -> Option<String> {
    let u = Url::parse(raw).ok()?;
    let host = u.host_str()?.to_ascii_lowercase();
    let param = if is_google_host(&host) && u.path() == "/url" {
        ["q", "url"].into_iter().find(|p| u.query_pairs().any(|(k, _)| k == *p))?
    } else {
        REDIRECTORS.iter().find(|(h, p, _)| host == *h && u.path().starts_with(p)).map(|(_, _, q)| *q)?
    };
    let target = u.query_pairs().find(|(k, _)| k == param).map(|(_, v)| v.into_owned())?;
    let t = Url::parse(&target).ok()?;
    matches!(t.scheme(), "http" | "https").then(|| t.to_string())
}

/// Halaman asli untuk alamat AMP Google (`google.com/amp/s/...`, `*.cdn.ampproject.org/c/s/...`).
pub fn de_amp(raw: &str) -> Option<String> {
    let u = Url::parse(raw).ok()?;
    let host = u.host_str()?.to_ascii_lowercase();
    let path = u.path();
    let rest = if is_google_host(&host) {
        path.strip_prefix("/amp/s/").map(|r| format!("https://{r}")).or_else(|| {
            let r = path.strip_prefix("/amp/")?;
            Some(if r.starts_with("http") { r.replacen(":/", "://", 1).replace(":///", "://") } else { format!("http://{r}") })
        })?
    } else if host.ends_with(".cdn.ampproject.org") {
        let r = path.strip_prefix("/c/s/").map(|r| format!("https://{r}")).or_else(|| path.strip_prefix("/c/").map(|r| format!("http://{r}")))?;
        r
    } else {
        return None;
    };
    let mut t = Url::parse(&rest).ok()?;
    if t.host_str().is_none() {
        return None;
    }
    if let Some(q) = u.query() {
        if t.query().is_none() {
            t.set_query(Some(q));
        }
    }
    Some(t.to_string())
}

/// Ubahan alamat sebelum halaman dibuka: debounce lalu de-AMP. None bila tidak ada.
pub fn rewrite_navigation(url: &str, s: &Settings) -> Option<String> {
    let mut cur = url.to_string();
    let mut changed = false;
    for _ in 0..3 {
        let next = s.debounce.then(|| debounce(&cur)).flatten().or_else(|| s.de_amp.then(|| de_amp(&cur)).flatten());
        match next {
            Some(n) if n != cur => {
                cur = n;
                changed = true;
            }
            _ => break,
        }
    }
    changed.then_some(cur)
}

/// Kunci sesi acak untuk farbling (berganti setiap Ovner dibuka).
pub fn session_key() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    h.write_u32(std::process::id());
    h.finish()
}

/// Skrip yang disuntikkan ke setiap halaman tab ini: konfigurasi + skrip konten (bingkai utama)
/// + farbling (semua bingkai). Dibuat ulang setiap tab dibangun, jadi perubahan setelan per
/// situs berlaku setelah tab dimuat ulang.
pub fn page_scripts(s: &Settings, session: u64, private: bool) -> String {
    let compat: Vec<String> = COMPAT_HOSTS.iter().map(|h| h.to_string()).collect();
    let farble_off: Vec<&String> = s.farbling_off.iter().chain(s.exceptions.iter()).chain(compat.iter()).collect();
    let cfg = json!({
        "spellcheck": s.spellcheck,
        "noBeforeUnload": s.disable_beforeunload,
        "deAmp": s.de_amp,
        "userCss": s.user_css,
        // Font yang dipaksakan ke halaman web ("" = font asli situs).
        "pageFont": if s.page_font == "off" { String::new() } else { crate::fonts::page_css(&s.page_font) },
        "farbling": s.farbling,
        "farbleOff": farble_off,
        "uniform": s.uniform_fingerprint,
        "hideWv": s.hide_webview2 || s.uniform_fingerprint,
        "referrer": s.referrer_policy,
        // Codec YouTube (seperti enhanced-h264ify) + deteksi dekoder GPU sekali per sesi.
        "yt": yt_codec_cfg(s),
        // Tombol unduh di atas video (tab privat juga), kecuali di situs yang dikecualikan.
        "videoBtn": s.video_button,
        "videoNever": s.video_button_never,
        "lang": crate::i18n::resolve(&s.language).code(),
        // Tab privat memakai kunci berbeda supaya tidak bisa dicocokkan dengan tab biasa.
        "key": format!("{:016x}", session ^ if private { 0x9e37_79b9_7f4a_7c15 } else { 0 }),
    });
    format!("(function(){{try{{Object.defineProperty(window,'__ovnerCfg',{{value:{cfg},configurable:false}});}}catch(e){{}}}})();\n{CONTENT_JS}\n{PRIVACY_JS}\n{FARBLE_JS}")
}

/// Dekoder GPU sudah dilaporkan halaman pada sesi ini (halaman berikutnya tidak memeriksa lagi).
pub static CODEC_PROBED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Potongan teks codec di MIME/codec string ("video/webm; codecs=\"vp09.00.40.08\"") per codec,
/// sama seperti enhanced-h264ify (av99 = AV1 eksperimen YouTube).
pub fn codec_tokens(codec: &str) -> &'static [&'static str] {
    match codec {
        "av1" => &["av01", "av99"],
        "vp9" => &["vp9", "vp09"],
        "vp8" => &["vp8", "vp08"],
        "h264" => &["avc1", "avc3"],
        "opus" => &["opus"],
        "aac" => &["mp4a"],
        _ => &[],
    }
}

fn yt_codec_cfg(s: &Settings) -> serde_json::Value {
    let (blocked, no60) = s.yt_codec_blocks();
    let tokens: Vec<&str> = blocked.iter().flat_map(|c| codec_tokens(c).iter().copied()).collect();
    json!({
        "block": tokens,
        "no60": no60,
        "noLN": s.yt_no_loudness,
        "probe": !CODEC_PROBED.load(std::sync::atomic::Ordering::Relaxed),
    })
}

/// Situs yang menolak browser dengan identitas atau sidik jari yang diubah (menganggapnya bot
/// atau "user-agent switcher"). Di sini Ovner memakai identitas asli WebView2 tanpa farbling;
/// iklan dan pelacak tetap diblokir.
pub const COMPAT_HOSTS: &[&str] = &["startpage.com", "startpage.nl", "inforcloudsuite.com", "infor.com"];

/// Situs ini perlu identitas asli (kompatibilitas bawaan, atau dikecualikan pengguna)?
#[allow(dead_code)]
pub fn needs_real_identity(s: &Settings, host: &str) -> bool {
    let h = host.trim_start_matches("www.").to_ascii_lowercase();
    COMPAT_HOSTS.iter().any(|c| h == *c || h.ends_with(&format!(".{c}")))
        || crate::settings::host_in(&s.exceptions, &h)
        || crate::settings::host_in(&s.farbling_off, &h)
}

/// Perintah protokol DevTools untuk identitas tab: menyamarkan merek "WebView2" agar terlihat
/// seperti Microsoft Edge biasa, dan (mode seragam) identitas generik seperti Mullvad.
pub fn identity_overrides(s: &Settings, ua: &str) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    if !(s.hide_webview2 || s.uniform_fingerprint) || ua.is_empty() {
        return out;
    }
    let version = |key: &str| ua.split(key).nth(1).and_then(|r| r.split([' ', ';', ')']).next()).unwrap_or("").to_string();
    let edg = version("Edg/");
    let chrome = version("Chrome/");
    let full = if edg.is_empty() { chrome.clone() } else { edg.clone() };
    let major = full.split('.').next().unwrap_or("").to_string();
    if major.is_empty() {
        return out;
    }
    // Versi lengkap juga disamarkan menjadi X.0.0.0 dalam mode seragam.
    let full = if s.uniform_fingerprint { format!("{major}.0.0.0") } else { full };
    let ua_text = if s.uniform_fingerprint {
        format!("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{major}.0.0.0 Safari/537.36 Edg/{major}.0.0.0")
    } else {
        ua.to_string()
    };
    let brands = json!([
        {"brand": "Chromium", "version": major},
        {"brand": "Microsoft Edge", "version": major},
        {"brand": "Not_A Brand", "version": "24"},
    ]);
    let full_list = json!([
        {"brand": "Chromium", "version": full},
        {"brand": "Microsoft Edge", "version": full},
        {"brand": "Not_A Brand", "version": "24.0.0.0"},
    ]);
    let meta = json!({
        "brands": brands, "fullVersionList": full_list, "fullVersion": full,
        "platform": "Windows", "platformVersion": "10.0.0", "architecture": "x86", "bitness": "64",
        "model": "", "mobile": false, "wow64": false,
    });
    out.push(("Emulation.setUserAgentOverride", json!({"userAgent": ua_text, "userAgentMetadata": meta}).to_string()));
    if s.uniform_fingerprint {
        out.push(("Emulation.setHardwareConcurrencyOverride", json!({"hardwareConcurrency": 8}).to_string()));
        if s.uniform_utc {
            out.push(("Emulation.setTimezoneOverride", json!({"timezoneId": "UTC"}).to_string()));
        }
    }
    out
}

/// Ukuran area halaman dengan letterboxing: dibulatkan ke bawah ke kelipatan 200×100
/// (seperti Tor/Mullvad), sisanya jadi margin di tengah. Mengembalikan (x, y, lebar, tinggi).
pub fn letterbox(w: f64, h: f64) -> (f64, f64, f64, f64) {
    let lw = if w >= 400.0 { (w / 200.0).floor() * 200.0 } else { w };
    let lh = if h >= 200.0 { (h / 100.0).floor() * 100.0 } else { h };
    (((w - lw) / 2.0).floor(), ((h - lh) / 2.0).floor(), lw, lh)
}

/// Host di jaringan lokal atau komputer ini (0.0.0.0, localhost, 127.x, 10.x, 172.16-31.x,
/// 192.168.x, 169.254.x, IPv6 lokal). Situs publik yang memanggilnya biasanya memindai port
/// untuk sidik jari (Waterfox dan Chromium baru juga memblokirnya).
pub fn is_local_target(host: &str) -> bool {
    let h = host.trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
    if h == "localhost" || h.ends_with(".localhost") || h.ends_with(".local") {
        return true;
    }
    match h.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => ip.is_unspecified() || ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        Ok(std::net::IpAddr::V6(ip)) => {
            ip.is_unspecified() || ip.is_loopback() || (ip.segments()[0] & 0xfe00) == 0xfc00 || (ip.segments()[0] & 0xffc0) == 0xfe80
        }
        Err(_) => false,
    }
}

/// Isi dan jenis dari data URL `data:<mime>;base64,<isi>` (resource pengganti adblock-rust).
pub fn decode_data_url(u: &str) -> Option<(String, Vec<u8>)> {
    use base64::Engine as _;
    let rest = u.strip_prefix("data:")?;
    let (head, body) = rest.split_once(',')?;
    let mime = head.trim_end_matches(";base64").to_string();
    let bytes = if head.ends_with(";base64") { base64::prelude::BASE64_STANDARD.decode(body).ok()? } else { body.as_bytes().to_vec() };
    Some((mime, bytes))
}

#[cfg(test)]
mod tests {
    #[test]
    fn site_features() {
        let mut s = Settings::default();
        assert!(site_feature_script(&s, "contoh.id").is_empty());
        s.dark_on_sites.push("contoh.id".into());
        let js = site_feature_script(&s, "contoh.id");
        assert!(js.contains("DarkReader.enable(") && js.contains("\"brightness\":100"));
        if let Ok(p) = std::env::var("OVNER_DUMP_JS") {
            let _ = std::fs::write(p, &js);
        }
        s.dark_engine = "filter".into();
        let js = site_feature_script(&s, "contoh.id");
        assert!(js.contains("__ovnerDarkFilter") && !js.contains("__OVNER_DARK__") && !js.contains("DarkReader.enable"));
        s.dark_engine = "chromium".into();
        assert!(site_feature_script(&s, "contoh.id").is_empty());
        assert_eq!(dark_cdp(&s, "contoh.id").1, r#"{"enabled":true}"#);
        assert_eq!(dark_cdp(&s, "lain.id").1, "{}");
        s.allow_copy_sites.push("lain.id".into());
        assert!(site_feature_script(&s, "lain.id").contains("__ovnerAllowCopy"));
        assert!(dark_live_script(&s, None).contains("DarkReader.disable"));
        assert!(dark_live_script(&s, Some("dynamic")).contains("if(!window.DarkReader)"));
    }

    use super::*;

    #[test]
    fn bangs_front_back_and_custom() {
        let s = Settings::default();
        assert_eq!(expand_bang("!w rust lang", &s).as_deref(), Some("https://en.wikipedia.org/w/index.php?search=rust+lang"));
        assert_eq!(expand_bang("harga emas !g", &s).as_deref(), Some("https://www.google.com/search?q=harga+emas"));
        assert_eq!(expand_bang("!yt", &s).as_deref(), Some("https://www.youtube.com/"));
        assert_eq!(expand_bang("a !tidakada b", &s), None);
        assert_eq!(expand_bang("tanpa bang", &s), None);
        let s = Settings { custom_bangs: vec!["!hr https://hr.kantor.id/cari?q=%s".into()], ..Settings::default() };
        assert_eq!(expand_bang("!hr cuti", &s).as_deref(), Some("https://hr.kantor.id/cari?q=cuti"));
        let off = Settings { bangs: false, ..Settings::default() };
        assert_eq!(expand_bang("!w x", &off), None);
    }

    #[test]
    fn debounce_known_redirectors_only() {
        assert_eq!(
            debounce("https://l.facebook.com/l.php?u=https%3A%2F%2Fkompas.com%2Fa&h=AT0").as_deref(),
            Some("https://kompas.com/a")
        );
        assert_eq!(debounce("https://www.google.com/url?q=https://detik.com/&sa=D").as_deref(), Some("https://detik.com/"));
        assert_eq!(debounce("https://www.google.co.id/url?url=https://a.id/x").as_deref(), Some("https://a.id/x"));
        assert_eq!(debounce("https://www.youtube.com/redirect?q=https%3A%2F%2Fx.id"), Some("https://x.id/".into()));
        assert_eq!(debounce("https://l.facebook.com/l.php?u=javascript:alert(1)"), None);
        assert_eq!(debounce("https://www.google.com/search?q=https://a.id"), None);
        assert_eq!(debounce("https://kompas.com/l.php?u=https://a.id"), None);
    }

    #[test]
    fn de_amp_google_and_cdn() {
        assert_eq!(de_amp("https://www.google.com/amp/s/www.kompas.com/amp/read/1").as_deref(), Some("https://www.kompas.com/amp/read/1"));
        assert_eq!(de_amp("https://www-detik-com.cdn.ampproject.org/c/s/www.detik.com/berita/x").as_deref(), Some("https://www.detik.com/berita/x"));
        assert_eq!(de_amp("https://www.google.com/search?q=amp"), None);
        assert_eq!(de_amp("https://kompas.com/amp/s/x"), None);
    }

    #[test]
    fn rewrite_chains() {
        let s = Settings::default();
        let u = "https://www.google.com/url?q=https://www.google.com/amp/s/a.id/b";
        assert_eq!(rewrite_navigation(u, &s).as_deref(), Some("https://a.id/b"));
        let off = Settings { debounce: false, de_amp: false, ..Settings::default() };
        assert_eq!(rewrite_navigation(u, &off), None);
    }

    #[test]
    fn page_scripts_embed_config() {
        let s = Settings { farbling_off: vec!["bank.co.id".into()], ..Settings::default() };
        let js = page_scripts(&s, 42, false);
        assert!(js.contains("__ovnerCfg") && js.contains("bank.co.id") && js.contains("\"farbling\":\"standard\""));
        assert_ne!(page_scripts(&s, 42, true), js);
        // Codec YouTube: bawaan Otomatis tanpa hasil deteksi = AV1 (+VP8) diblokir.
        assert!(js.contains(r#""block":["vp8","vp08","av01","av99"]"#), "{js:.400}");
        let m = Settings { yt_codec: "manual".into(), yt_no_loudness: true, ..Settings::default() };
        let js = page_scripts(&m, 42, false);
        assert!(js.contains(r#""block":["av01","av99","vp9","vp09","vp8","vp08"]"#) && js.contains(r#""noLN":true"#));
    }
    #[test]
    fn identity_and_letterbox() {
        let ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36 Edg/142.0.3595.65";
        // Bawaan: identitas asli WebView2 (tanpa samaran yang tidak berlaku di worker).
        assert!(identity_overrides(&Settings::default(), ua).is_empty());
        let s = Settings { hide_webview2: true, ..Settings::default() };
        let o = identity_overrides(&s, ua);
        assert_eq!(o.len(), 1);
        assert!(o[0].1.contains("\"Microsoft Edge\"") && o[0].1.contains("142.0.3595.65") && !o[0].1.contains("WebView2"));
        let u = Settings { uniform_fingerprint: true, uniform_utc: true, ..Settings::default() };
        let o = identity_overrides(&u, ua);
        assert_eq!(o.len(), 3);
        assert!(o[0].1.contains("Edg/142.0.0.0") && o[2].1.contains("UTC"));
        assert!(identity_overrides(&Settings { hide_webview2: false, ..Settings::default() }, ua).is_empty());
        assert_eq!(letterbox(1366.0, 657.0), (83.0, 28.0, 1200.0, 600.0));
        assert_eq!(letterbox(300.0, 150.0), (0.0, 0.0, 300.0, 150.0));
    }

    #[test]
    fn local_targets_and_data_urls() {
        for h in ["0.0.0.0", "localhost", "127.0.0.1", "192.168.1.1", "10.0.0.5", "172.20.1.1", "[::1]", "printer.local"] {
            assert!(is_local_target(h), "{h}");
        }
        for h in ["8.8.8.8", "kompas.com", "172.32.0.1"] {
            assert!(!is_local_target(h), "{h}");
        }
        assert_eq!(decode_data_url("data:application/javascript;base64,Ly8="), Some(("application/javascript".into(), b"//".to_vec())));
    }

}
