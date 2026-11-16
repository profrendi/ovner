//! Pemblokir iklan dan pelacak memakai `adblock-rust`, mesin yang sama dengan Brave Shields.
//!
//! Daftar filter (EasyList, EasyPrivacy, ABPindo, uBlock) diunduh ke `<data>/filters` saat
//! pertama dipakai dan diperbarui seminggu sekali di latar. Mesin yang sudah disusun disimpan
//! sebagai cache, jadi membuka Ovner berikutnya hanya butuh beberapa milidetik.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use adblock::lists::{FilterFormat, FilterSet, ParseOptions};
use adblock::request::Request;
use adblock::Engine;
use url::Url;

/// Daftar yang diunduh: (nama file, URL).
pub const FILTER_LISTS: &[(&str, &str)] = &[
    ("easylist.txt", "https://easylist.to/easylist/easylist.txt"),
    ("easyprivacy.txt", "https://easylist.to/easylist/easyprivacy.txt"),
    ("abpindo.txt", "https://raw.githubusercontent.com/ABPindo/indonesianadblockrules/master/subscriptions/abpindo.txt"),
    ("ublock-filters.txt", "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/filters.txt"),
    ("ublock-privacy.txt", "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/privacy.txt"),
];

/// Daftar diperbarui bila sudah lebih tua dari ini.
pub const REFRESH_AFTER: Duration = Duration::from_secs(7 * 24 * 3600);

/// Daftar kecil bawaan: tetap memblokir pelacak besar sebelum daftar lengkap selesai diunduh.
const BUILTIN_HOSTS: &[&str] = &[
    "doubleclick.net", "googlesyndication.com", "googleadservices.com", "google-analytics.com",
    "googletagmanager.com", "googletagservices.com", "adservice.google.com", "2mdn.net",
    "connect.facebook.net", "analytics.twitter.com", "ads-twitter.com", "ads.linkedin.com",
    "bat.bing.com", "clarity.ms", "scorecardresearch.com", "quantserve.com", "hotjar.com",
    "criteo.com", "criteo.net", "taboola.com", "outbrain.com", "adnxs.com", "rubiconproject.com",
    "pubmatic.com", "openx.net", "moatads.com", "adsrvr.org", "amazon-adsystem.com",
    "smartadserver.com", "teads.tv", "media.net", "popads.net", "mgid.com", "yandex.ru/ads",
];

const TRACKING_PARAMS: &[&str] = &[
    "fbclid", "gclid", "gclsrc", "dclid", "gbraid", "wbraid", "msclkid", "yclid", "twclid", "ttclid",
    "igshid", "mc_eid", "mc_cid", "_hsenc", "_hsmi", "mkt_tok", "vero_id", "oly_enc_id", "oly_anon_id",
    "s_cid",
];

const CACHE_FILE: &str = ".mesin-filter.bin";
/// Format serialisasi mesin bisa berubah antar versi adblock-rust; naikkan bila berubah.
const CACHE_TAG: &str = "ovner-filter-v1;adblock-0.13";

pub fn filters_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("filters")
}

fn list_files(data_dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(filters_dir(data_dir))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("txt")))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn mtime_secs(p: &Path) -> u64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cache_key(files: &[PathBuf]) -> String {
    let mut key = CACHE_TAG.to_string();
    for f in files {
        let len = std::fs::metadata(f).map(|m| m.len()).unwrap_or(0);
        key.push_str(&format!(";{}:{len}:{}", f.file_name().unwrap_or_default().to_string_lossy(), mtime_secs(f)));
    }
    key
}

fn builtin_rules() -> String {
    BUILTIN_HOSTS.iter().map(|h| format!("||{h}^\n")).collect()
}

/// Mesin dengan daftar bawaan saja.
pub fn builtin_engine() -> Engine {
    let mut set = FilterSet::new(false);
    set.add_filter_list(builtin_rules(), ParseOptions::default());
    Engine::new_with_filter_set(set)
}

/// Muat mesin dari cache bila cocok dengan daftar di disk.
pub fn load_cached(data_dir: &Path) -> Option<Engine> {
    let files = list_files(data_dir);
    if files.is_empty() {
        return None;
    }
    let bytes = std::fs::read(filters_dir(data_dir).join(CACHE_FILE)).ok()?;
    let body = bytes.strip_prefix(cache_key(&files).as_bytes())?.strip_prefix(b"\n")?;
    let mut engine = Engine::default();
    engine.deserialize(body).ok()?;
    Some(engine)
}

/// Susun mesin dari semua daftar di disk, simpan cache, dan kembalikan hasil serialisasinya
/// (supaya bisa dikirim antar-thread; `Engine` sendiri tidak boleh berpindah thread).
pub fn build_and_cache(data_dir: &Path) -> Option<Vec<u8>> {
    let files = list_files(data_dir);
    if files.is_empty() {
        return None;
    }
    let mut set = FilterSet::new(false);
    set.add_filter_list(builtin_rules(), ParseOptions::default());
    for f in &files {
        if let Ok(text) = std::fs::read_to_string(f) {
            set.add_filter_list(text, ParseOptions { format: FilterFormat::Standard, ..ParseOptions::default() });
        }
    }
    let engine = Engine::new_with_filter_set(set);
    let body = engine.serialize();
    let mut out = cache_key(&files).into_bytes();
    out.push(b'\n');
    out.extend_from_slice(&body);
    let _ = std::fs::write(filters_dir(data_dir).join(CACHE_FILE), &out);
    Some(body)
}

pub fn engine_from_bytes(body: &[u8]) -> Option<Engine> {
    let mut engine = Engine::default();
    engine.deserialize(body).ok()?;
    Some(engine)
}

/// Apakah ada daftar yang belum ada atau sudah lebih tua dari `REFRESH_AFTER`.
pub fn lists_need_refresh(data_dir: &Path) -> bool {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    FILTER_LISTS.iter().any(|(name, _)| {
        let p = filters_dir(data_dir).join(name);
        !p.is_file() || now.saturating_sub(mtime_secs(&p)) > REFRESH_AFTER.as_secs()
    })
}

/// Unduh semua daftar (yang gagal tetap memakai berkas lama). Mengembalikan jumlah yang berhasil.
pub fn download_lists(data_dir: &Path) -> usize {
    let dir = filters_dir(data_dir);
    let _ = std::fs::create_dir_all(&dir);
    let mut ok = 0;
    for (name, url) in FILTER_LISTS {
        let tmp = dir.join(format!("{name}.unduh"));
        let _ = std::fs::remove_file(&tmp);
        match crate::win::download(url, &tmp) {
            // Berkas HTML (halaman galat proxy kantor, misalnya) bukan daftar filter.
            Ok(()) if std::fs::read_to_string(&tmp).is_ok_and(|t| t.len() > 1000 && !t.trim_start().starts_with('<')) => {
                if std::fs::rename(&tmp, dir.join(name)).is_ok() {
                    ok += 1;
                }
            }
            _ => {
                let _ = std::fs::remove_file(&tmp);
            }
        }
    }
    ok
}

/// Jenis permintaan EasyList dari konteks sumber daya WebView2.
pub fn request_type_from_webview2(context: i32, is_main_document: bool) -> &'static str {
    // Nilai COREWEBVIEW2_WEB_RESOURCE_CONTEXT_*.
    match context {
        1 if is_main_document => "document",
        1 => "sub_frame",
        2 => "stylesheet",
        3 => "image",
        4 => "media",
        5 => "font",
        6 => "script",
        7 | 8 => "xmlhttprequest", // XHR, Fetch
        9 => "other",              // TextTrack
        10 => "other",             // EventSource
        11 => "websocket",
        12 => "other", // Manifest
        13 => "other", // SignedExchange
        14 => "ping",
        15 => "other", // CspViolationReport
        _ => "other",
    }
}

pub struct Blocker {
    engine: Engine,
    /// Mesin sudah memakai daftar lengkap (bukan hanya daftar bawaan).
    pub full: bool,
}

impl Blocker {
    pub fn new(engine: Engine, full: bool) -> Blocker {
        Blocker { engine, full }
    }

    /// `true` bila permintaan ini harus dibatalkan. `source` adalah alamat halaman (tab).
    pub fn should_block(&self, url: &str, source: &str, request_type: &str) -> bool {
        if request_type == "document" || !(url.starts_with("http://") || url.starts_with("https://")) {
            return false;
        }
        match Request::new(url, source, request_type, "GET") {
            Ok(req) => self.engine.check_network_request(&req).should_block(),
            Err(_) => false,
        }
    }

    /// CSS penyembunyi kotak iklan khusus situs ini (aturan `situs##selektor`).
    /// Mengembalikan (css, pengecualian, generichide).
    pub fn page_cosmetics(&self, page_url: &str) -> (String, HashSet<String>, bool) {
        let res = self.engine.url_cosmetic_resources(page_url);
        (css_for(res.hide_selectors.iter()), res.exceptions, res.generichide)
    }

    /// CSS untuk aturan umum (`##.kelas`, `###id`) yang cocok dengan kelas/id di halaman.
    pub fn generic_cosmetics(&self, classes: &[String], ids: &[String], exceptions: &HashSet<String>) -> String {
        css_for(self.engine.hidden_class_id_selectors(classes, ids, exceptions).iter())
    }
}

/// Satu aturan per selektor: selektor yang tidak dikenal browser tidak menggugurkan yang lain.
fn css_for<'a>(selectors: impl Iterator<Item = &'a String>) -> String {
    let mut css = String::new();
    for s in selectors {
        if s.contains('{') || s.contains('}') || s.contains("</") {
            continue;
        }
        css.push_str(s);
        css.push_str("{display:none!important}\n");
    }
    css
}

fn is_tracking_param(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with("utm_") || TRACKING_PARAMS.contains(&name.as_str())
}

/// URL tanpa parameter pelacak klik (fbclid, utm_*, dll.), atau None bila tidak ada yang dibuang.
pub fn strip_tracking_params(raw: &str) -> Option<String> {
    let mut url = Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.query().is_none() {
        return None;
    }
    let all: Vec<(String, String)> = url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
    let kept: Vec<&(String, String)> = all.iter().filter(|(k, _)| !is_tracking_param(k)).collect();
    if kept.len() == all.len() {
        return None;
    }
    if kept.is_empty() {
        url.set_query(None);
    } else {
        url.query_pairs_mut().clear().extend_pairs(kept.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    }
    Some(url.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ovner-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(filters_dir(&d)).unwrap();
        d
    }

    const PAGE: &str = "https://berita.contoh.id/artikel";

    #[test]
    fn builtin_blocks_big_trackers_only() {
        let b = Blocker::new(builtin_engine(), false);
        assert!(b.should_block("https://www.googletagmanager.com/gtag/js?id=1", PAGE, "script"));
        assert!(b.should_block("https://stats.g.doubleclick.net/x", PAGE, "image"));
        assert!(!b.should_block("https://cdn.contoh.id/app.js", PAGE, "script"));
        // Navigasi halaman utama tidak pernah diblokir.
        assert!(!b.should_block("https://doubleclick.net/", PAGE, "document"));
    }

    #[test]
    fn full_lists_cache_and_cosmetics() {
        let d = tmp("full");
        std::fs::write(
            filters_dir(&d).join("uji.txt"),
            "/iklan/banner.\n@@||contoh.id/iklan/banner.js\n||pelacak.example^$third-party\n\
             contoh.id##.kotak-iklan\n##.adsbygoogle\n",
        )
        .unwrap();
        assert!(load_cached(&d).is_none());
        let bytes = build_and_cache(&d).unwrap();
        let b = Blocker::new(engine_from_bytes(&bytes).unwrap(), true);
        assert!(b.should_block("https://lain.id/iklan/banner.js", PAGE, "script"));
        assert!(!b.should_block("https://contoh.id/iklan/banner.js", PAGE, "script"));
        assert!(b.should_block("https://pelacak.example/p.gif", PAGE, "image"));
        assert!(!b.should_block("https://pelacak.example/p.gif", "https://pelacak.example/", "image"));

        // Cache dipakai ulang selama daftar tidak berubah.
        let cached = Blocker::new(load_cached(&d).unwrap(), true);
        assert!(cached.should_block("https://lain.id/iklan/banner.js", PAGE, "script"));

        let (css, exc, generichide) = cached.page_cosmetics("https://www.contoh.id/berita");
        assert!(css.contains(".kotak-iklan{display:none!important}"));
        assert!(!generichide);
        let generic = cached.generic_cosmetics(&["adsbygoogle".into(), "artikel".into()], &[], &exc);
        assert!(generic.contains(".adsbygoogle{display:none!important}"));
        assert!(!generic.contains("artikel"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn refresh_needed_when_lists_missing() {
        let d = tmp("refresh");
        assert!(lists_need_refresh(&d));
        for (name, _) in FILTER_LISTS {
            std::fs::write(filters_dir(&d).join(name), "! daftar\n").unwrap();
        }
        assert!(!lists_need_refresh(&d));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn request_types() {
        assert_eq!(request_type_from_webview2(1, true), "document");
        assert_eq!(request_type_from_webview2(1, false), "sub_frame");
        assert_eq!(request_type_from_webview2(6, false), "script");
        assert_eq!(request_type_from_webview2(3, false), "image");
        assert_eq!(request_type_from_webview2(8, false), "xmlhttprequest");
    }

    #[test]
    fn tracking_params() {
        assert_eq!(
            strip_tracking_params("https://a.id/p?id=7&utm_source=x&fbclid=1").as_deref(),
            Some("https://a.id/p?id=7")
        );
        assert_eq!(strip_tracking_params("https://a.id/?gclid=1").as_deref(), Some("https://a.id/"));
        assert_eq!(strip_tracking_params("https://a.id/?id=7"), None);
    }

    #[test]
    fn css_skips_unsafe_selectors() {
        let sels = vec![".a".to_string(), "x{}".to_string()];
        assert_eq!(css_for(sels.iter()), ".a{display:none!important}\n");
    }
}
