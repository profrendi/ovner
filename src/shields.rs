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
    let farble_off: Vec<&String> = s.farbling_off.iter().chain(s.exceptions.iter()).collect();
    let cfg = json!({
        "spellcheck": s.spellcheck,
        "noBeforeUnload": s.disable_beforeunload,
        "deAmp": s.de_amp,
        "userCss": s.user_css,
        "farbling": s.farbling,
        "farbleOff": farble_off,
        // Tab privat memakai kunci berbeda supaya tidak bisa dicocokkan dengan tab biasa.
        "key": format!("{:016x}", session ^ if private { 0x9e37_79b9_7f4a_7c15 } else { 0 }),
    });
    format!("(function(){{try{{Object.defineProperty(window,'__ovnerCfg',{{value:{cfg},configurable:false}});}}catch(e){{}}}})();\n{CONTENT_JS}\n{FARBLE_JS}")
}

#[cfg(test)]
mod tests {
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
    }
}
