//! Pemblokir iklan dan pelacak memakai `adblock-rust`, mesin yang sama dengan Brave Shields.
//!
//! Daftar filter (EasyList, EasyPrivacy, ABPindo, uBlock) diunduh ke `<data>/filters` saat
//! pertama dipakai dan diperbarui seminggu sekali di latar. Mesin yang sudah disusun disimpan
//! sebagai cache, jadi membuka Ovner berikutnya hanya butuh beberapa milidetik.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use adblock::lists::{FilterFormat, FilterSet, ParseOptions};
use adblock::resources::PermissionMask;
use adblock::request::Request;
use adblock::Engine;
use url::Url;

/// Daftar utama yang selalu diunduh: (nama file, URL).
pub const FILTER_LISTS: &[(&str, &str)] = &[
    ("easylist.txt", "https://easylist.to/easylist/easylist.txt"),
    ("easyprivacy.txt", "https://easylist.to/easylist/easyprivacy.txt"),
    ("abpindo.txt", "https://raw.githubusercontent.com/ABPindo/indonesianadblockrules/master/subscriptions/abpindo.txt"),
    ("ublock-filters.txt", "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/filters.txt"),
    ("ublock-privacy.txt", "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/privacy.txt"),
];

/// Daftar yang selalu dipakai juga, mengikuti Brave dan uBlock Origin: perbaikan cepat
/// (YouTube, anti-adblock), daftar "unbreak" agar situs tidak rusak, dan situs berbahaya.
pub const EXTRA_LISTS: &[(&str, &str)] = &[
    ("ublock-quick-fixes.txt", "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/quick-fixes.txt"),
    ("ublock-unbreak.txt", "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/unbreak.txt"),
    ("ublock-badware.txt", "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/badware.txt"),
    ("brave-unbreak.txt", "https://raw.githubusercontent.com/brave/adblock-lists/master/brave-unbreak.txt"),
    ("brave-specific.txt", "https://raw.githubusercontent.com/brave/adblock-lists/master/brave-lists/brave-specific.txt"),
    // 0.7.1: sisa "Brave Default Adblock Filters" yang belum dipakai Ovner.
    ("ublock-filters-general.txt", "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/filters-general.txt"),
    ("ublock-filters-2020.txt", "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/filters-2020.txt"),
    ("ublock-filters-2021.txt", "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/filters-2021.txt"),
    ("ublock-filters-2022.txt", "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/filters-2022.txt"),
    ("ublock-filters-2023.txt", "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/filters-2023.txt"),
    ("ublock-filters-2024.txt", "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/filters-2024.txt"),
    ("ublock-filters-2025.txt", "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/filters-2025.txt"),
    ("ublock-filters-2026.txt", "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/filters-2026.txt"),
    ("ublock-resource-abuse.txt", "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/resource-abuse.txt"),
    ("ublock-link-shorteners.txt", "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/ubo-link-shorteners.txt"),
    ("urlhaus-malware.txt", "https://malware-filter.gitlab.io/malware-filter/urlhaus-filter-agh-online.txt"),
    ("brave-social.txt", "https://raw.githubusercontent.com/brave/adblock-lists/master/brave-lists/brave-social.txt"),
    ("brave-sugarcoat.txt", "https://raw.githubusercontent.com/brave/adblock-lists/master/brave-lists/brave-sugarcoat.txt"),
    ("brave-firstparty.txt", "https://raw.githubusercontent.com/brave/adblock-lists/master/brave-lists/brave-firstparty.txt"),
    ("brave-firstparty-regional.txt", "https://raw.githubusercontent.com/brave/adblock-lists/master/brave-lists/brave-firstparty-regional.txt"),
];

/// Parameter pelacak di alamat ($removeparam), seperti yang ditambahkan Mullvad Browser ke uBO.
pub const URL_TRACKING_LIST: (&str, &str) = ("adguard-url-tracking.txt", "https://filters.adtidy.org/extension/ublock/filters/17.txt");

/// Daftar gangguan: notifikasi, widget sosial, popup newsletter, banner aplikasi.
pub const ANNOYANCE_LISTS: &[(&str, &str)] = &[
    ("ublock-annoyances.txt", "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/annoyances-others.txt"),
    ("fanboy-annoyance.txt", "https://secure.fanboy.co.nz/fanboy-annoyance.txt"),
];

/// Scriptlet (+js) dan resource pengganti ($redirect) dari uBlock Origin dan Brave, disusun
/// oleh CI Ovner setiap hari (tools/build-resources.mjs).
pub const RESOURCES: (&str, &str) = ("resources.json", "https://github.com/profrendi/ovner/releases/download/ovner-resources/resources.json");

/// Daftar kecil yang cepat berubah: diperbarui tiap `filter_update_hours`.
fn is_fast(name: &str) -> bool {
    matches!(name, "ublock-quick-fixes.txt" | "ublock-unbreak.txt" | "brave-unbreak.txt" | "resources.json")
}

/// Daftar tepercaya (uBO dan Brave) boleh memakai scriptlet "trusted-*".
fn is_trusted(name: &str) -> bool {
    name.starts_with("ublock-") || name.starts_with("brave-")
}

/// Daftar popup persetujuan cookie (seperti "cookie consent blocking" Brave).
pub const COOKIE_LISTS: &[(&str, &str)] = &[
    ("ublock-cookies.txt", "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/annoyances-cookies.txt"),
    ("easylist-cookie.txt", "https://secure.fanboy.co.nz/fanboy-cookiemonster.txt"),
    ("brave-cookie-specific.txt", "https://raw.githubusercontent.com/brave/adblock-lists/master/brave-lists/brave-cookie-specific.txt"),
];

/// Berkas aturan sendiri pengguna (dari Pengaturan dan pemilih elemen).
pub const CUSTOM_RULES_FILE: &str = "zz-aturan-saya.txt";

/// Semua daftar yang diinginkan menurut setelan: (nama file, URL).
pub fn wanted_lists(s: &crate::settings::Settings) -> Vec<(String, String)> {
    let pair = |(n, u): &(&str, &str)| (n.to_string(), u.to_string());
    let mut v: Vec<(String, String)> = FILTER_LISTS.iter().chain(EXTRA_LISTS).map(pair).collect();
    if s.block_cookie_notices {
        v.extend(COOKIE_LISTS.iter().map(pair));
    }
    if s.strip_tracking_params {
        v.push(pair(&URL_TRACKING_LIST));
    }
    if s.block_annoyances {
        v.extend(ANNOYANCE_LISTS.iter().map(pair));
    }
    // Daftar opsional dari katalog (seperti brave://settings/shields → Filter lists).
    for id in &s.filter_lists {
        if let Some(c) = crate::catalog::find(id).filter(|c| c.setting.is_none()) {
            for (k, u) in c.urls.iter().enumerate() {
                v.push((crate::catalog::file_name(c.id, k), u.to_string()));
            }
        }
    }
    v.push(pair(&RESOURCES));
    for u in &s.custom_lists {
        if u.starts_with("https://") || u.starts_with("http://") {
            v.push((format!("kustom-{:08x}.txt", fnv(u)), u.clone()));
        }
    }
    v
}

fn fnv(s: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// Hapus berkas daftar yang tidak lagi diinginkan (mis. daftar cookie dimatikan).
/// Mengembalikan true bila ada yang dihapus.
pub fn prune_lists(data_dir: &Path, wanted: &[(String, String)]) -> bool {
    let mut removed = false;
    for f in list_files(data_dir) {
        let name = f.file_name().unwrap_or_default().to_string_lossy().to_string();
        if name != CUSTOM_RULES_FILE && !wanted.iter().any(|(n, _)| *n == name) {
            removed |= std::fs::remove_file(&f).is_ok();
        }
    }
    removed
}

/// Tulis aturan sendiri ke berkasnya. Mengembalikan true bila isinya berubah.
pub fn write_custom_rules(data_dir: &Path, rules: &str) -> bool {
    let dir = filters_dir(data_dir);
    let _ = std::fs::create_dir_all(&dir);
    let p = dir.join(CUSTOM_RULES_FILE);
    let text = format!("! Aturan sendiri Ovner\n{}\n", rules.trim());
    if std::fs::read_to_string(&p).ok().as_deref() == Some(text.as_str()) {
        return false;
    }
    std::fs::write(&p, text).is_ok()
}

/// Daftar besar diperbarui bila sudah lebih tua dari ini (daftar kecil: `filter_update_hours`).
pub const REFRESH_AFTER: Duration = Duration::from_secs(3 * 24 * 3600);

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
const CACHE_TAG: &str = "ovner-filter-v3;adblock-0.13";

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

/// Sidik isi berkas (FNV-1a 64-bit, dibaca per 64 KB supaya tidak memakai RAM besar).
/// Kunci cache memakai isi, bukan waktu ubah: menyegarkan waktu berkas yang isinya sama (supaya
/// tidak diunduh lagi) tidak boleh membuat cache dianggap usang, dan isi yang berubah tidak
/// boleh lolos hanya karena ukurannya sama.
fn content_hash(p: &Path) -> u64 {
    use std::io::Read;
    let mut h: u64 = 0xcbf29ce484222325;
    let Ok(mut f) = std::fs::File::open(p) else { return 0 };
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match f.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                for b in &buf[..n] {
                    h ^= *b as u64;
                    h = h.wrapping_mul(0x100000001b3);
                }
            }
            Err(_) => return 0,
        }
    }
    h
}

fn cache_key(files: &[PathBuf]) -> String {
    let mut key = CACHE_TAG.to_string();
    for f in files {
        let len = std::fs::metadata(f).map(|m| m.len()).unwrap_or(0);
        key.push_str(&format!(";{}:{len}:{:016x}", f.file_name().unwrap_or_default().to_string_lossy(), content_hash(f)));
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

/// Cache mesin ada **dan** cocok dengan daftar di disk. Hanya membaca kepala berkas cache
/// (kuncinya), tanpa memuat mesinnya: memuat = ±15 MB alokasi sementara.
/// Sengaja bukan sekadar "berkasnya ada": cache usang yang dianggap sah membuat pemblokir
/// berjalan dengan daftar bawaan saja (bug 0.8.28).
pub fn cache_valid(data_dir: &Path) -> bool {
    use std::io::Read;
    let files = list_files(data_dir);
    if files.is_empty() {
        return false;
    }
    let want = format!("{}\n", cache_key(&files));
    let Ok(mut f) = std::fs::File::open(filters_dir(data_dir).join(CACHE_FILE)) else { return false };
    let mut head = vec![0u8; want.len()];
    f.read_exact(&mut head).is_ok() && head == want.as_bytes()
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
            let name = f.file_name().unwrap_or_default().to_string_lossy();
            let permissions = if is_trusted(&name) { PermissionMask::from_bits(1) } else { PermissionMask::default() };
            set.add_filter_list(text, ParseOptions { format: FilterFormat::Standard, permissions, ..ParseOptions::default() });
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

/// Ukur mesin pemblokir: muat salinan dari cache (persis cara Ovner saat mulai) dan hitung heap
/// Rust yang dipakainya. (byte di memori, ms, byte cache di disk). Salinan dibuang lagi.
pub fn measure_engine(data_dir: &Path) -> Option<(u64, u128, u64)> {
    let before = crate::heap::now();
    let t = std::time::Instant::now();
    let engine = load_cached(data_dir)?;
    let b = Blocker::new(engine, true, data_dir);
    let used = crate::heap::now().saturating_sub(before);
    let ms = t.elapsed().as_millis();
    drop(b);
    let cache = std::fs::metadata(filters_dir(data_dir).join(CACHE_FILE)).map(|m| m.len()).unwrap_or(0);
    Some((used, ms, cache))
}

pub fn engine_from_bytes(body: &[u8]) -> Option<Engine> {
    let mut engine = Engine::default();
    engine.deserialize(body).ok()?;
    Some(engine)
}

/// Daftar yang perlu diunduh: belum ada, atau lebih tua dari batasnya (daftar kecil yang
/// cepat berubah: `fast_hours`; daftar besar: `REFRESH_AFTER`).
pub fn stale_lists(data_dir: &Path, wanted: &[(String, String)], fast_hours: u32) -> Vec<(String, String)> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    wanted
        .iter()
        .filter(|(name, _)| {
            let p = filters_dir(data_dir).join(name);
            let max = if is_fast(name) { fast_hours as u64 * 3600 } else { REFRESH_AFTER.as_secs() };
            !p.is_file() || now.saturating_sub(mtime_secs(&p)) > max
        })
        .cloned()
        .collect()
}

/// Scriptlet dan resource pengganti yang sudah diunduh.
pub fn load_resources(data_dir: &Path) -> Vec<adblock::resources::Resource> {
    std::fs::read_to_string(filters_dir(data_dir).join(RESOURCES.0))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Isi unduhan tampak seperti daftar filter: bukan halaman HTML (galat proxy kantor, login,
/// blokir jaringan) dan berisi aturan atau tajuk daftar. Daftar kecil yang sah (mis. perbaikan
/// cepat 300 byte) ikut diterima; sebelumnya semua yang di bawah 1000 byte ditolak.
fn looks_like_list(t: &str) -> bool {
    let t = t.trim_start_matches('\u{feff}').trim_start();
    if t.is_empty() || t.starts_with('<') {
        return false;
    }
    t.len() > 1000
        || t.lines().any(|l| {
            let l = l.trim_start();
            l.starts_with('!') || l.starts_with("[Adblock") || l.starts_with("||") || l.starts_with("@@") || l.contains("##") || l.starts_with('/') || l.starts_with('$')
        })
}

/// Unduh daftar (yang gagal tetap memakai berkas lama). Mengembalikan jumlah yang berubah dan
/// daftar yang gagal beserta alasannya. `only_missing`: hanya unduh yang belum ada.
pub fn download_lists(data_dir: &Path, wanted: &[(String, String)], only_missing: bool) -> (usize, Vec<String>) {
    let dir = filters_dir(data_dir);
    let _ = std::fs::create_dir_all(&dir);
    let mut ok = 0;
    let mut failed = Vec::new();
    for (name, url) in wanted {
        if only_missing && dir.join(name).is_file() {
            continue;
        }
        let tmp = dir.join(format!("{name}.unduh"));
        let _ = std::fs::remove_file(&tmp);
        let res = crate::win::download(url, &tmp);
        let host = url::Url::parse(url).ok().and_then(|u| u.host_str().map(String::from)).unwrap_or_default();
        match res {
            // Berkas HTML (halaman galat proxy kantor, misalnya) bukan daftar filter.
            Ok(()) if std::fs::read_to_string(&tmp).is_ok_and(|t| looks_like_list(&t)) => {
                let new = std::fs::read(&tmp).unwrap_or_default();
                if std::fs::read(dir.join(name)).ok().as_deref() == Some(new.as_slice()) {
                    // Isi sama: cukup segarkan waktunya supaya tidak diunduh lagi, tanpa
                    // menyusun ulang mesin.
                    let _ = std::fs::rename(&tmp, dir.join(name));
                    let _ = std::fs::File::options().write(true).open(dir.join(name)).and_then(|f| f.set_modified(SystemTime::now()));
                } else if std::fs::rename(&tmp, dir.join(name)).is_ok() {
                    ok += 1;
                }
            }
            Ok(()) => {
                let head: String = std::fs::read_to_string(&tmp).unwrap_or_default().chars().take(60).collect();
                failed.push(format!("{name} dari {host}: isinya bukan daftar filter (mis. halaman login/blokir jaringan: {:?})", head.trim()));
                let _ = std::fs::remove_file(&tmp);
            }
            Err(e) => {
                failed.push(format!("{name} dari {host}: {e}"));
                let _ = std::fs::remove_file(&tmp);
            }
        }
    }
    (ok, failed)
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

/// Keputusan untuk satu permintaan jaringan.
#[derive(Default, Debug, PartialEq)]
pub struct Verdict {
    pub block: bool,
    /// Ganti dengan resource kosong (data URL), bukan diblokir mentah-mentah.
    pub redirect: Option<String>,
    /// Alamat tanpa parameter pelacak ($removeparam).
    pub rewritten: Option<String>,
}

/// Bahan kosmetik satu halaman.
pub struct PageResources {
    pub css: String,
    pub exceptions: HashSet<String>,
    pub generichide: bool,
    /// Aturan procedural (JSON), dijalankan skrip halaman.
    pub procedural: Vec<String>,
}

impl Blocker {
    /// Mesin + scriptlet/resource pengganti dari folder data.
    pub fn new(mut engine: Engine, full: bool, data_dir: &Path) -> Blocker {
        let res = load_resources(data_dir);
        if !res.is_empty() {
            engine.use_resources(res);
        }
        Blocker { engine, full }
    }

    /// Pemeriksaan lengkap: blokir, ganti dengan resource kosong, atau tulis ulang alamat.
    pub fn check(&self, url: &str, source: &str, request_type: &str) -> Verdict {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Verdict::default();
        }
        let Ok(req) = Request::new(url, source, request_type, "GET") else { return Verdict::default() };
        let r = self.engine.check_network_request(&req);
        let block = request_type != "document" && r.should_block();
        Verdict {
            block,
            redirect: if block { r.redirect } else { None },
            rewritten: if block { None } else { r.rewritten_url },
        }
    }

    /// Skrip scriptlet (+js) untuk halaman/iframe di alamat ini (kosong bila tidak ada).
    pub fn scriptlets(&self, url: &str) -> String {
        self.engine.url_cosmetic_resources(url).injected_script
    }

    pub fn page_resources(&self, page_url: &str) -> PageResources {
        let res = self.engine.url_cosmetic_resources(page_url);
        PageResources {
            css: css_for(res.hide_selectors.iter()),
            procedural: res.procedural_actions.into_iter().collect(),
            exceptions: res.exceptions,
            generichide: res.generichide,
        }
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

    /// Pemeriksaan CNAME uncloaking: blokir bila nama asli (CNAME) host ini cocok dengan
    /// daftar, walau host-nya sendiri subdomain situs (pelacak yang menyamar).
    pub fn should_block_cname(&self, url: &str, cname: &str, source: &str, request_type: &str) -> bool {
        let Ok(mut u) = Url::parse(url) else { return false };
        if u.set_host(Some(cname.trim_end_matches('.'))).is_err() {
            return false;
        }
        self.should_block(u.as_str(), source, request_type)
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
        let b = Blocker::new(builtin_engine(), false, Path::new("/tidak-ada"));
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
             contoh.id##.kotak-iklan\n##.adsbygoogle\ncontoh.id##div.kartu:has-text(Sponsor)\n\
             contoh.id##+js(set-constant, iklanAktif, false)\n||lain.id/ads/ima3.js$script,redirect=google-ima.js\n\
             $removeparam=utm_campaign\n",
        )
        .unwrap();
        // Resource uji dalam format tools/build-resources.mjs.
        let b64 = |s: &str| {
            use base64::Engine as _;
            base64::prelude::BASE64_STANDARD.encode(s)
        };
        let res = serde_json::json!([
            {"name": "set-constant.js", "aliases": [], "kind": {"mime": "application/javascript"},
             "content": b64("function setConstant(a, b) { window[a] = b === 'false' ? false : b; }")},
            {"name": "google-ima.js", "aliases": ["google-ima3"], "kind": {"mime": "application/javascript"}, "content": b64("/* kosong */")},
        ]);
        std::fs::write(filters_dir(&d).join(RESOURCES.0), res.to_string()).unwrap();
        assert!(load_cached(&d).is_none());
        let bytes = build_and_cache(&d).unwrap();
        let b = Blocker::new(engine_from_bytes(&bytes).unwrap(), true, &d);
        assert!(b.should_block("https://lain.id/iklan/banner.js", PAGE, "script"));
        assert!(!b.should_block("https://contoh.id/iklan/banner.js", PAGE, "script"));
        assert!(b.should_block("https://pelacak.example/p.gif", PAGE, "image"));
        assert!(!b.should_block("https://pelacak.example/p.gif", "https://pelacak.example/", "image"));

        // Cache dipakai ulang selama daftar tidak berubah.
        let cached = Blocker::new(load_cached(&d).unwrap(), true, &d);
        assert!(cached.should_block("https://lain.id/iklan/banner.js", PAGE, "script"));

        // CNAME: subdomain situs yang aslinya pelacak.
        assert!(cached.should_block_cname("https://metrik.contoh.id/t.js", "pelacak.example", PAGE, "script"));
        assert!(!cached.should_block_cname("https://cdn.contoh.id/a.js", "cdn.cloudflare.net", PAGE, "script"));
        let pr = cached.page_resources("https://www.contoh.id/berita");
        let (css, exc) = (pr.css, pr.exceptions);
        assert!(css.contains(".kotak-iklan{display:none!important}"));
        assert!(!pr.generichide);
        assert_eq!(pr.procedural.len(), 1);
        assert!(pr.procedural[0].contains("has-text") && pr.procedural[0].contains("Sponsor"));
        // Scriptlet dari resources.json, dan resource pengganti untuk skrip iklan.
        let s = cached.scriptlets("https://www.contoh.id/");
        assert!(s.contains("function setConstant") && s.contains("setConstant(\"iklanAktif\", \"false\")"), "{s}");
        let v = cached.check("https://lain.id/ads/ima3.js", PAGE, "script");
        assert!(v.block && v.redirect.as_deref().is_some_and(|r| r.starts_with("data:application/javascript;base64,")), "{v:?}");
        let v = cached.check("https://toko.id/p?id=7&utm_campaign=x", PAGE, "xmlhttprequest");
        assert_eq!(v.rewritten.as_deref(), Some("https://toko.id/p?id=7"));
        let generic = cached.generic_cosmetics(&["adsbygoogle".into(), "artikel".into()], &[], &exc);
        assert!(generic.contains(".adsbygoogle{display:none!important}"));
        assert!(!generic.contains("artikel"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn refresh_needed_when_lists_missing() {
        let d = tmp("refresh");
        let s = crate::settings::Settings::default();
        let wanted = wanted_lists(&s);
        // Utama + tambahan + cookie + parameter pelacak + resources.json + promo aplikasi (bawaan Brave).
        assert_eq!(wanted.len(), FILTER_LISTS.len() + EXTRA_LISTS.len() + COOKIE_LISTS.len() + 3);
        assert!(wanted.iter().any(|(n, u)| n == "opt-2f3dce16-0.txt" && u.contains("fanboy-mobile")));
        assert_eq!(stale_lists(&d, &wanted, 6).len(), wanted.len());
        for (name, _) in &wanted {
            std::fs::write(filters_dir(&d).join(name), "! daftar\n").unwrap();
        }
        assert!(stale_lists(&d, &wanted, 6).is_empty());
        let mut s_ann = s.clone();
        s_ann.block_annoyances = true;
        assert_eq!(stale_lists(&d, &wanted_lists(&s_ann), 6).len(), ANNOYANCE_LISTS.len());
        // Daftar cookie dimatikan: berkasnya dibuang, aturan sendiri tetap.
        assert!(write_custom_rules(&d, "contoh.id##.iklan"));
        assert!(!write_custom_rules(&d, "contoh.id##.iklan"));
        let s2 = crate::settings::Settings { block_cookie_notices: false, custom_lists: vec!["https://x.id/l.txt".into()], ..s };
        let w2 = wanted_lists(&s2);
        assert!(w2.iter().any(|(n, _)| n.starts_with("kustom-")));
        assert!(prune_lists(&d, &w2));
        assert!(!filters_dir(&d).join("ublock-cookies.txt").exists());
        assert!(filters_dir(&d).join(CUSTOM_RULES_FILE).exists());
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

    #[test]
    fn cache_tidak_usang_karena_waktu_berkas() {
        let d = tmp("cache-waktu");
        let f = filters_dir(&d).join("uji.txt");
        std::fs::write(&f, "||pelacak.example^$third-party\n").unwrap();
        // Belum ada cache: tidak sah.
        assert!(!cache_valid(&d));
        build_and_cache(&d).unwrap();
        assert!(cache_valid(&d) && load_cached(&d).is_some());
        // Menyegarkan waktu berkas yang isinya sama (jalur "isi sama" di download_lists) tidak
        // boleh membuang cache.
        std::fs::File::options().write(true).open(&f).unwrap().set_modified(SystemTime::now() + Duration::from_secs(3600)).unwrap();
        assert!(cache_valid(&d), "waktu berubah, isi sama: cache harus tetap sah");
        assert!(load_cached(&d).is_some());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn cache_usang_terdeteksi_dan_tidak_dianggap_sah() {
        let d = tmp("cache-usang");
        let f = filters_dir(&d).join("uji.txt");
        std::fs::write(&f, "||pelacak.example^$third-party\n").unwrap();
        build_and_cache(&d).unwrap();
        // Isi berubah dengan ukuran SAMA: harus terdeteksi (kunci memakai isi).
        std::fs::write(&f, "||pelacak.example^$third-partz\n").unwrap();
        assert!(!cache_valid(&d), "isi berubah: cache usang tidak boleh sah");
        assert!(load_cached(&d).is_none());
        // Daftar bertambah: usang juga.
        build_and_cache(&d).unwrap();
        assert!(cache_valid(&d));
        std::fs::write(filters_dir(&d).join("baru.txt"), "||lain.example^\n").unwrap();
        assert!(!cache_valid(&d));
        // Berkas cache rusak / terpotong: tidak sah dan tidak membuat panik.
        build_and_cache(&d).unwrap();
        let cf = filters_dir(&d).join(CACHE_FILE);
        let bytes = std::fs::read(&cf).unwrap();
        std::fs::write(&cf, &bytes[..10]).unwrap();
        assert!(!cache_valid(&d) && load_cached(&d).is_none());
        std::fs::write(&cf, b"").unwrap();
        assert!(!cache_valid(&d));
        // Setelah susun ulang dengan isi yang sah, pemblokir penuh benar-benar memblokir.
        std::fs::write(&f, "||pelacak.example^$third-party\n").unwrap();
        assert!(!cache_valid(&d));
        let bytes = build_and_cache(&d).unwrap();
        assert!(cache_valid(&d));
        let b = Blocker::new(engine_from_bytes(&bytes).unwrap(), true, &d);
        assert!(b.should_block("https://pelacak.example/p.gif", PAGE, "image"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn daftar_kecil_sah_diterima_html_ditolak() {
        assert!(looks_like_list("! Title: perbaikan\n||iklan.example^\n"));
        assert!(looks_like_list("[Adblock Plus 2.0]\n! x\n"));
        assert!(looks_like_list("\u{feff}||iklan.example^$third-party\n"));
        assert!(!looks_like_list(""));
        assert!(!looks_like_list("<!doctype html><html>Login</html>"));
        assert!(!looks_like_list("  \n<html>"));
        assert!(!looks_like_list("Access denied"));
        assert!(looks_like_list(&"||a.example^\n".repeat(100)));
    }
}
