//! Ovner: browser ringan untuk Windows di atas WebView2 (mesin Edge yang sudah ada di Windows)
//! dengan pemblokir iklan Brave (`adblock-rust`), tab yang ditidurkan otomatis, tray yang
//! menekan RAM, dan instalasi tanpa hak admin.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
// Sebagian kode hanya dipakai di Windows; di Linux (untuk pengujian) jangan dianggap mati.
#![cfg_attr(not(windows), allow(dead_code))]
#![recursion_limit = "256"]

mod blocker;
mod i18n;
mod icon;
mod import;
mod install;
#[cfg(windows)]
mod netfilter;
mod antiblock;
mod anubis;
mod catalog;
mod dlengine;
mod video;
mod usage;
mod settings;
mod setup_ui;
mod vault;
mod shields;
mod store;
mod update;
mod versions;
mod win;

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tao::dpi::LogicalSize;
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy, EventLoopWindowTarget};
use tao::window::{Icon, Window, WindowBuilder};
use tray_icon::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{TrayIcon, TrayIconBuilder, TrayIconEvent};
use wry::dpi::{LogicalPosition as WLogicalPosition, LogicalSize as WLogicalSize};
use wry::{NewWindowResponse, PageLoadEvent, PermissionKind, PermissionResponse, Rect, WebContext, WebView, WebViewBuilder};

use blocker::Blocker;
use i18n::{t, tf, Lang};
use settings::{host_in, toggle_host, Settings};
use store::Store;

const TOOLBAR_HTML: &str = include_str!("ui/toolbar.html");
const SIDEBAR_HTML: &str = include_str!("ui/sidebar.html");
const SIDEBAR_W: f64 = 230.0;
const NEWTAB_HTML: &str = include_str!("ui/newtab.html");
const SETTINGS_HTML: &str = include_str!("ui/settings.html");
const HISTORY_HTML: &str = include_str!("ui/history.html");
const BOOKMARKS_HTML: &str = include_str!("ui/bookmarks.html");
const TASKS_HTML: &str = include_str!("ui/tasks.html");
const SETUP_HTML: &str = include_str!("ui/setup.html");
const HTTPS_HTML: &str = include_str!("ui/https.html");
const DIAGNOSTICS_HTML: &str = include_str!("ui/diagnostics.html");
const SESSIONS_HTML: &str = include_str!("ui/sessions.html");
const PASSWORDS_HTML: &str = include_str!("ui/passwords.html");
const DOWNLOADS_HTML: &str = include_str!("ui/downloads.html");

/// Satu unduhan di riwayat unduhan.
#[derive(Serialize, Deserialize, Clone, Debug)]
struct Download {
    #[serde(skip)]
    id: u64,
    url: String,
    path: String,
    name: String,
    total: i64,
    received: i64,
    /// "progress", "paused", "done", atau "failed".
    state: String,
    time: u64,
    /// Diunduh mesin multi-koneksi Ovner (bisa dijeda/dilanjutkan).
    #[serde(default)]
    engine: bool,
    #[serde(skip)]
    speed: u64,
    #[serde(skip)]
    conns: usize,
    /// Video yang diunduh lewat yt-dlp.
    #[serde(default)]
    video: bool,
    #[serde(skip)]
    tab: u64,
    /// Video yang ditunda ("Nanti" di dialog unduh video): dimulai saat dilanjutkan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    vspec: Option<video::Spec>,
}

/// Kabar jalur koneksi Ovner untuk PDF (nomor jalur, isi).
#[derive(Debug)]
enum RaceMsg {
    Progress(usize, u64),
    Attempt(usize, u32),
    Failed(usize, String),
    Done(usize, Result<u64, String>),
}

/// Jumlah jalur koneksi Ovner (WinHTTP) untuk PDF yang dibuat server tanpa Range. Server tidak
/// bisa dibagi per bagian, jadi tiap jalur mengunduh seluruh berkas; yang pertama selesai utuh
/// dipakai. Dua jalur: bila satu diputus server di tengah (log 0.8.10: putus di 99,8%), jalur
/// lain sudah jauh berjalan, tidak mengulang dari nol.
const RACE_LANES: usize = 2;

#[derive(Default, Clone)]
struct Lane {
    received: u64,
    /// Saat jalur ini mulai menerima data (percobaan sekarang).
    t0: Option<Instant>,
    done: bool,
}

/// Koneksi Ovner untuk PDF: diunduh ulang lewat WinHTTP dengan cookie situs, berlomba dengan
/// pembacaan respons halaman. Server SIPP mengirim pelan lewat jalur halaman (±33 KB/dtk) tetapi
/// lebih cepat lewat WinHTTP (±150 KB/dtk), tanpa Range, dan kadang memutus koneksi. Mana yang
/// selesai utuh lebih dulu, itulah yang disimpan.
struct PdfRace {
    req: dlengine::Request,
    /// Awalan berkas sementara tiap jalur (`{base}.ovnerraceN`).
    base: String,
    /// Mulai setelah ini (bila pembacaan respons belum selesai).
    due: Instant,
    started: bool,
    cancel: Arc<std::sync::atomic::AtomicBool>,
    lanes: Vec<Lane>,
    /// Byte terjauh dari semua jalur.
    received: u64,
    finished: bool,
    /// Posisi pembacaan respons saat jalur pertama mulai menerima data.
    t0: Option<(Instant, i64)>,
    /// Sudah diputuskan mana yang dipertahankan (yang lebih lambat dihentikan).
    decided: bool,
    /// Sudah dijalankan ulang sekali setelah respons halaman putus.
    restarted: bool,
}

impl PdfRace {
    fn lanes_running(&self) -> usize {
        if !self.started || self.finished {
            return 0;
        }
        self.lanes.iter().filter(|l| !l.done).count()
    }
}

/// Info tambahan unduhan untuk kotak unduhan (seperti IDM/Mister Download Manager).
#[derive(Default)]
struct DlMeta {
    started: Option<Instant>,
    /// (waktu, byte) sampel terakhir untuk menghitung kecepatan unduhan WebView2.
    last: Option<(Instant, i64)>,
    /// Server: host, IP, negara, kota, penyedia, jarak (km).
    server: Value,
    eta: u64,
    /// Sedang menguji server sebelum mulai (ditampilkan langsung supaya terasa instan).
    connecting: bool,
    /// Bisa dilanjutkan (server mendukung Range): Some(true/false) bila sudah diketahui.
    resumable: Option<bool>,
    /// Alamat untuk mencari lokasi server (mirror hasil pengalihan), kosong = alamat unduhan.
    lookup_url: String,
    /// PDF yang dibaca Ovner langsung dari respons halaman (bukan unduhan WebView2).
    pdf: bool,
    /// Byte yang sudah dibaca dari respons halaman (PDF).
    stream_received: i64,
    /// Pembacaan respons PDF sudah berhenti (gagal); koneksi kedua mungkin masih berjalan.
    stream_dead: bool,
    /// PDF sudah selesai lewat koneksi kedua.
    race_won: bool,
    /// Dibatalkan pengguna.
    user_cancel: bool,
}
const PAGE_CSS: &str = include_str!("ui/page.css");
/// Halaman internal disajikan lewat protokol `ovner://`. Di Windows, wry memetakannya ke
/// `http://ovner.<nama>/` (WebView2 belum mendukung skema sendiri secara penuh).
#[cfg(windows)]
const INTERNAL_PREFIX: &str = "http://ovner.";
#[cfg(not(windows))]
const INTERNAL_PREFIX: &str = "ovner://";
const INSTANCE_PORT: u16 = 47893;
const MAGIC: &str = "OVNER-1";
const TICK: Duration = Duration::from_secs(1);
const SLEEP_CHECK: Duration = Duration::from_secs(30);
const DIAG_EVERY: Duration = Duration::from_secs(5 * 60);
const ZOOM_STEPS: &[u32] = &[25, 33, 50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300, 400, 500];

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

fn data_dir() -> &'static Path {
    DATA_DIR.get().map(PathBuf::as_path).unwrap_or(Path::new("."))
}

/// Catatan sederhana di `<data>/ovner.log` untuk menelusuri masalah.
pub fn log(msg: &str) {
    let p = data_dir().join("ovner.log");
    if std::fs::metadata(&p).map(|m| m.len() > 256 * 1024).unwrap_or(false) {
        let _ = std::fs::rename(&p, data_dir().join("ovner.old.log"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let _ = writeln!(f, "{} {msg}", store::now());
    }
    if cfg!(debug_assertions) {
        eprintln!("[ovner] {msg}");
    }
}

// ---------------------------------------------------------------------------
// Alamat

/// Lokasi kira-kira komputer ini (lintang, bujur) dari alamat IP publik, sekali per sesi.
fn my_location() -> Option<(f64, f64)> {
    static LOC: std::sync::Mutex<Option<Option<(f64, f64)>>> = std::sync::Mutex::new(None);
    if let Some(v) = *LOC.lock().unwrap() {
        return v;
    }
    let v = dlengine::get_text("https://ipwho.is/")
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| Some((v["latitude"].as_f64()?, v["longitude"].as_f64()?)));
    *LOC.lock().unwrap() = Some(v);
    v
}

/// Hapus berkas unduhan beserta sisa-sisanya (.ovnerpart mesin Ovner, .crdownload, dan
/// .part/.ytdl/potongan format yt-dlp). Dicoba ulang sebentar di latar karena pegangan berkas
/// bisa baru saja dilepas.
/// Catat laporan crash WebView2 terbaru (Crashpad) ke log, sekali saja per jalannya Ovner.
/// Berkas .dmp itu bisa dikirim untuk mencari penyebab mesin WebView2 mati.
fn log_crash_report() {
    static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if DONE.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let dir = data_dir().join("WebView2").join("EBWebView").join("Crashpad").join("reports");
    let newest = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path(), e.metadata().ok()?.len())))
        .max_by_key(|(t, _, _)| *t);
    match newest {
        Some((t, p, len)) if t.elapsed().map_or(false, |e| e.as_secs() < 300) => log(&format!("laporan crash WebView2: {} ({} KB)", p.display(), len / 1024)),
        _ => log(&format!("laporan crash WebView2: tidak ada yang baru di {}", dir.display())),
    }
}

/// Kunci alamat PDF yang dipelajari: host + path (tanpa query), huruf kecil.
fn pdf_key(url: &str) -> String {
    let u = url.split(['?', '#']).next().unwrap_or("").to_ascii_lowercase();
    u.split_once("://").map(|(_, r)| r.to_string()).unwrap_or(u)
}

/// Path unduhan yang belum dipakai: berkasnya maupun sisa `.ovnerpart`/`.crdownload`-nya
/// belum ada (dua unduhan tidak menulis ke berkas yang sama).
fn free_download_path(path: &Path) -> PathBuf {
    let used = |p: &Path| {
        let s = p.display().to_string();
        p.exists() || ["ovnerpart", "crdownload", "ovnerrace0", "ovnerrace1"].iter().any(|e| Path::new(&format!("{s}.{e}")).exists())
    };
    if !used(path) {
        return path.to_path_buf();
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    for n in 1..1000 {
        let p = path.with_file_name(format!("{stem} ({n}){ext}"));
        if !used(&p) {
            return p;
        }
    }
    path.to_path_buf()
}

fn delete_download_files(path: &str) {
    let p = PathBuf::from(path);
    if path.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        let (Some(dir), Some(name)) = (p.parent().map(Path::to_path_buf), p.file_name().map(|n| n.to_string_lossy().to_string())) else { return };
        let stem = p.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        for _ in 0..10 {
            let mut left = false;
            for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                let ours = n == name
                    || n == format!("{name}.ovnerpart")
                    || n == format!("{name}.crdownload")
                    || n.starts_with(&format!("{name}.ovnerrace"))
                    || (!stem.is_empty() && n.starts_with(&format!("{stem}.")) && (n.ends_with(".part") || n.ends_with(".ytdl") || n.contains(".part-Frag") || n.contains(".temp.") || {
                        // Potongan format yt-dlp: "judul [id].f137.mp4".
                        let rest = &n[stem.len() + 1..];
                        rest.starts_with('f') && rest[1..].split('.').next().is_some_and(|x| !x.is_empty() && x.chars().all(|c| c.is_ascii_digit() || c == '-'))
                    }));
                if ours && e.path().is_file() && std::fs::remove_file(e.path()).is_err() {
                    left = true;
                }
            }
            if !left {
                return;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
}

/// Kirim `{"<kind>": data}` ke halaman internal Ovner lewat pesan WebView2 (lihat
/// win::post_text); bila tidak bisa (Linux), panggil `fallback` sebagai skrip.
fn post_page(wv: &WebView, kind: &str, data: &str, fallback: &str) {
    if !win::post_text(wv, &format!("{{\"{kind}\":{data}}}")) {
        let _ = wv.evaluate_script(&format!("{fallback}({data})"));
    }
}

/// Jarak dua titik di bumi (km).
fn haversine_km(a1: f64, o1: f64, a2: f64, o2: f64) -> f64 {
    let (p1, p2) = (a1.to_radians(), a2.to_radians());
    let (dp, dl) = ((a2 - a1).to_radians(), (o2 - o1).to_radians());
    let h = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    6371.0 * 2.0 * h.sqrt().asin()
}

fn internal_url(name: &str) -> String {
    format!("{INTERNAL_PREFIX}{name}/")
}

/// Nama halaman internal ("newtab", "settings", ...) bila alamat ini halaman Ovner.
fn internal_page(url: &str) -> Option<&str> {
    url.strip_prefix(INTERNAL_PREFIX).and_then(|rest| rest.split('/').next()).filter(|n| !n.is_empty())
}

fn is_newtab(url: &str) -> bool {
    url.is_empty() || internal_page(url) == Some("newtab") || url == "about:blank"
}

fn is_local_host(host: &str) -> bool {
    host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.parse::<std::net::Ipv4Addr>().is_ok_and(|ip| ip.is_private() || ip.is_loopback() || ip.is_link_local())
}

/// Teks dari kolom alamat menjadi URL: !bang, alamat apa adanya, domain diberi https://,
/// alamat lokal diberi http://, selain itu dicari dengan mesin pencari. None bila teks bukan
/// alamat dan pencarian dimatikan ("No Search").
fn to_url(text: &str, settings: &Settings) -> Option<String> {
    let t = text.trim();
    let lower = t.to_ascii_lowercase();
    if lower.starts_with("ovner://") {
        return Some(internal_url(lower.trim_start_matches("ovner://").trim_end_matches('/')));
    }
    if ["http://", "https://", "file://", "about:", "edge://", "view-source:"].iter().any(|p| lower.starts_with(p)) {
        return Some(t.to_string());
    }
    if let Some(u) = shields::expand_bang(t, settings) {
        return Some(u);
    }
    let search = |q: &str| (!settings.no_search).then(|| settings.search_url(q));
    if t.contains(char::is_whitespace) || t.is_empty() {
        return search(t);
    }
    let host = lower.split(['/', '?', '#']).next().unwrap_or("");
    let host_no_port = host.rsplit_once(':').map(|(h, p)| if p.chars().all(|c| c.is_ascii_digit()) { h } else { host }).unwrap_or(host);
    if is_local_host(host_no_port) {
        return Some(format!("http://{t}"));
    }
    let looks_like_domain = host_no_port.contains('.')
        && !host_no_port.starts_with('.')
        && !host_no_port.ends_with('.')
        && host_no_port.rsplit('.').next().is_some_and(|tld| tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic()));
    if looks_like_domain || host_no_port.parse::<std::net::Ipv4Addr>().is_ok() {
        Some(format!("https://{t}"))
    } else if settings.no_search && !host_no_port.is_empty() && !host_no_port.contains('/') {
        // No Search: kata tunggal dianggap nama situs.
        Some(format!("https://{t}.com"))
    } else {
        search(t)
    }
}

/// Alamat yang ditampilkan di kolom alamat.
fn display_url(url: &str) -> String {
    if is_newtab(url) {
        String::new()
    } else if let Some(name) = internal_page(url) {
        format!("ovner://{name}")
    } else {
        // Nama domain internasional ditampilkan sebagai punycode (perlindungan dari alamat
        // tiruan yang hurufnya mirip, seperti ungoogled-chromium).
        url::Url::parse(url).map(|u| u.to_string()).unwrap_or_else(|_| url.to_string())
    }
}

fn host_of(url: &str) -> Option<String> {
    url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string))
}

/// https:// untuk alamat http:// publik (pengaturan "Naikkan ke HTTPS").
fn upgrade_https(url: &str) -> Option<String> {
    let rest = url.strip_prefix("http://")?;
    let host = rest.split(['/', ':', '?', '#']).next().unwrap_or("");
    if host.is_empty() || is_local_host(host) || url.starts_with(INTERNAL_PREFIX) {
        return None;
    }
    Some(format!("https://{rest}"))
}

// ---------------------------------------------------------------------------
// Kejadian

enum Remote {
    Show,
    /// `ovner --new-window`: buka jendela Ovner baru.
    NewWindow,
    Open(String),
    Exit,
}

enum UserEvent {
    /// Pesan dari toolbar/daftar tab jendela nomor `.0`.
    Toolbar(u32, String),
    Content(u64, String),
    Title(u64, String),
    PageLoad(u64, PageLoadEvent, String),
    /// Jendela baru diminta halaman tab `opener`.
    NewTab(u64, String),
    Navigate(u64, String),
    /// Navigasi ulang dengan JavaScript dinyalakan/dimatikan (shields per situs).
    ScriptNav(u64, String, bool),
    /// Proses WebView2 tab ini crash/macet.
    ProcFailed(u64, win::ProcFail, String),
    /// Hasil CDP Performance.getMetrics untuk satu tab (JSON).
    TabMetrics(u64, String),
    /// Sampel pemakaian semua proses dari thread pemantau (tiap detik).
    Usage(Vec<usage::ProcStat>),
    /// Proses halaman WebView2 dan bingkai utama yang dilayaninya: (pid, [frame id]).
    RendererFrames(Vec<(u32, Vec<u32>)>),
    /// Jawaban tantangan Anubis (anti-bot Startpage dan situs lain) dari pemecah native.
    Anubis(u64, String),
    /// Kejadian DevTools dari tab yang sedang direkam: (tab, nama, JSON).
    Problem(u64, String, String),
    /// Daftar cookie situs (Network.getCookies) yang harus dihapus: (domain dasar, JSON).
    DeleteCookies(String, String),
    /// Pop-up window.open sudah dibuat sebagai WebView: jadikan tab.
    PopupReady,
    /// Halaman meminta jendelanya ditutup (window.close).
    ScriptClose(u64),
    /// Hasil uji diagnostik di latar.
    DiagResult(String, bool, String),
    /// Ikon situs hasil unduhan untuk bookmark (host, isi berkas).
    IconFetched(String, Vec<u8>),
    /// Navigasi halaman utama selesai (berhasil?, kode galat WebView2).
    NavDone(u64, bool, i32),
    /// Alamat http:// di tab ini baru saja dinaikkan ke https.
    Upgraded(u64, String),
    EngineReady(Vec<u8>),
    UpdateReady(String),
    UpdateInfo(String),
    Menu(String),
    /// Tab terakhir jendela nomor `.0` ditutup: tutup jendela itu seperti tombol X.
    CloseWindow(u32),
    /// Ikon situs (PNG) tab ini berubah.
    Favicon(u64, Vec<u8>),
    Download(u64, win::DlEvent),
    /// Hasil uji server untuk unduhan yang ditahan: (id unduhan, tab, permintaan, ukuran/galat).
    DlProbe(u64, u64, dlengine::Request, String, Result<u64, String>),
    /// Kabar dari mesin unduh Ovner.
    DlUpdate(u64, dlengine::Update),
    /// Koneksi kedua untuk PDF (WinHTTP): kemajuan / hasil.
    PdfRace(u64, RaceMsg),
    /// Info server unduhan (host, IP, negara, kota, jarak).
    DlServer(u64, Value),
    /// Kabar unduhan video (yt-dlp).
    Video(u64, video::Event),
    VideoJob(u64, video::Job),
    /// Hasil penelusuran format video (nomor penelusuran, data dialog).
    VideoInfo(u64, Result<Value, String>),
    TrayClick,
    Remote(Remote),
}

// ---------------------------------------------------------------------------
// Tab

struct Tab {
    id: u64,
    webview: Option<WebView>,
    url: String,
    title: String,
    loading: bool,
    private: bool,
    zoom: u32,
    suspended: bool,
    /// Kapan tab ini terakhir aktif (dipakai untuk menidurkan tab).
    last_active: Instant,
    blocked: Rc<Cell<u32>>,
    page: Rc<RefCell<String>>,
    cos_exceptions: HashSet<String>,
    generichide: bool,
    /// Alamat http:// yang sengaja diketik pengguna: tidak dinaikkan ke https.
    allow_http: Rc<RefCell<Option<String>>>,
    /// Alamat http:// asli yang terakhir dinaikkan ke https (untuk kembali bila gagal).
    upgraded_from: Option<String>,
    /// Kata kunci pencarian di halaman riwayat.
    page_query: String,
    /// JavaScript menyala untuk navigasi berikutnya di tab ini.
    scripts_on: Rc<Cell<bool>>,
    /// Generasi skrip halaman saat tab ini dibangun (dibangun ulang bila setelan berubah).
    script_gen: u64,
    /// Pemakaian tab dari protokol DevTools: memori JavaScript (byte) dan persen CPU.
    js_heap: u64,
    tab_cpu: f64,
    perf_prev: Option<(f64, Instant)>,
    perf_on: bool,
    /// Sejak kapan tab ini ditinggal sambil memutar media (setelan "berhenti setelah N menit").
    media_hidden_at: Option<Instant>,
    /// ID bingkai utama WebView2 (untuk mencocokkan proses halaman seperti Task Manager Windows).
    frame_id: Option<u32>,
    /// Proses halaman tab ini: pid, RAM (private working set), CPU, dan jumlah tab yang berbagi.
    proc_pid: u32,
    proc_ram: u64,
    proc_cpu: f64,
    proc_shared: usize,
    /// Formulir di halaman ini sedang diisi (jangan ditidurkan).
    form_dirty: bool,
    /// Beberapa alamat terakhir tab ini (untuk mencocokkan login yang dikirim sebelum pindah halaman).
    recent_urls: Vec<String>,
    /// Dibuka situs lewat window.open (boleh ditutup lewat window.close).
    popup: bool,
    /// Tab pop-up yang hanya memicu unduhan (halamannya kosong): ditutup saat unduhannya
    /// selesai, seperti Chrome.
    close_after_dl: bool,
    /// Perekam masalah halaman menyala untuk tab ini.
    recording: bool,
    rec_hooked: bool,
    /// Unduhan WebView2 yang sedang berjalan dari tab ini: tab tidak boleh ditidurkan atau
    /// ditangguhkan (WebView2 membatalkan unduhannya, lalu mulai ulang).
    downloads_live: u32,
}

/// Statistik penghemat tab (seperti Lifetime Stats TabWars), disimpan di tab-stats.json.
#[derive(Serialize, Deserialize, Default, Clone)]
struct TabStats {
    slept: u64,
    ram_freed: u64,
    duplicates: u64,
    sessions_saved: u64,
    restored: u64,
}

/// Sesi tersimpan (seperti "fleet" TabWars): sekumpulan tab bernama.
#[derive(Serialize, Deserialize, Clone)]
struct SavedSession {
    name: String,
    /// Detik sejak 1970.
    saved: u64,
    tabs: Vec<SessionTab>,
}

#[derive(Serialize, Deserialize, Default)]
struct Session {
    tabs: Vec<SessionTab>,
    active: usize,
    /// Jendela Ovner lainnya (sejak 0.8.13).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    windows: Vec<SessionWindow>,
}

#[derive(Serialize, Deserialize, Default)]
struct SessionWindow {
    tabs: Vec<SessionTab>,
    active: usize,
}

#[derive(Serialize, Deserialize, Clone)]
struct SessionTab {
    url: String,
    title: String,
}

struct Menus {
    main: Menu,
    /// Submenu Bookmark di ☰: 4 item tetap, lalu daftar bookmark (diisi ulang tiap dibuka).
    bookmarks: Submenu,
    tab: Menu,
    tray: Menu,
}

struct App {
    window: Rc<Window>,
    toolbar: WebView,
    /// Pengukur cadangan untuk log diagnostik (pemantau utama berjalan di thread sendiri).
    sampler: usage::Sampler,
    /// Proses halaman → frame id utama (dari GetProcessExtendedInfos).
    renderer_frames: Vec<(u32, Vec<u32>)>,
    frames_pending: bool,
    frames_at: Instant,
    /// Pemetaan proses per tab boleh dipakai (mati otomatis bila pernah membuat Ovner tertutup).
    frames_ok: bool,
    tab_stats: TabStats,
    /// Catatan masalah halaman yang direkam (ringkas, paling banyak 400 baris).
    problems: Vec<String>,
    sessions: Vec<SavedSession>,
    stats: Vec<usage::ProcStat>,
    stats_at: Instant,
    /// Port proxy anti-blokir (bila menyala).
    antiblock_port: Option<u16>,
    /// Galat pembuatan environment WebView2 sendiri (diagnostik).
    env_error: Option<String>,
    /// Kapan ikon bookmark yang belum ada diunduh berikutnya.
    icons_due: Option<Instant>,
    icons_tried: HashSet<String>,
    /// Ovner harus keluar (bukan buka ulang), mis. setelah dihapus dari Pengaturan.
    quit_requested: bool,
    /// Hasil uji diagnostik terakhir: nama → (berhasil, keterangan).
    diag: Vec<(String, bool, String)>,
    /// Daftar tab vertikal di kiri (bila dinyalakan).
    sidebar: Option<WebView>,
    toolbar_ready: bool,
    toolbar_h: f64,
    tabs: Vec<Tab>,
    active: usize,
    next_id: Rc<Cell<u64>>,
    /// Pop-up (window.open) yang sudah dibuat dan menunggu dijadikan tab.
    popups: Rc<RefCell<Vec<PopupTab>>>,
    closed: Vec<String>,
    settings: Rc<RefCell<Settings>>,
    blocker: Rc<RefCell<Blocker>>,
    store: Store,
    /// Brankas sandi (pengelola sandi Ovner).
    vault: vault::Vault,
    /// Unduhan (riwayat seperti brave://downloads) dan gelembung unduhan di kanan atas.
    downloads: Vec<Download>,
    dl_bubble: bool,
    /// Unduhan yang dikerjakan mesin unduh Ovner: id → (pekerjaan, tab asal).
    dl_jobs: HashMap<u64, (dlengine::Job, u64)>,
    /// Alamat yang harus diunduh WebView2 (mesin Ovner gagal sebelumnya).
    dl_skip: Rc<RefCell<HashSet<String>>>,
    dl_render_at: Instant,
    dl_page_at: Instant,
    dl_meta: HashMap<u64, DlMeta>,
    /// Sedang membuka ulang Ovner (setelah mesin WebView2 mati).
    restarting: bool,
    /// Buka ulang untuk memasang update (instalasi Program Files: minta izin admin).
    apply_update: bool,
    /// Unduhan yang dibatalkan saat masih menguji server.
    dl_cancelled: HashSet<u64>,
    /// Unduhan WebView2 yang dibatalkan paksa (WebView2 tidak menanggapi Cancel).
    dl_forced: HashSet<u64>,
    /// Halaman mirip unduhan yang menunggu server: id → (tab, alamat, sejak).
    dl_waiting: HashMap<u64, (u64, String, Instant)>,
    /// Unduhan mesin Ovner yang dijeda sementara supaya PDF (satu koneksi) dapat seluruh
    /// bandwidth; dilanjutkan otomatis setelah PDF selesai.
    dl_yield: HashSet<u64>,
    /// Dilanjutkan sendiri oleh pengguna saat PDF berjalan: jangan dijeda lagi.
    dl_no_yield: HashSet<u64>,
    /// Mode efisiensi Windows (EcoQoS, prioritas rendah) sedang diterapkan ke proses Ovner.
    eco_on: bool,
    pdf_race: HashMap<u64, PdfRace>,
    /// Alamat (host/path) yang pernah menghasilkan PDF besar: jalur Ovner dimulai lebih awal.
    pdf_learned: HashSet<String>,
    /// Kotak "Bookmark ditambahkan" yang terbuka: (alamat, baru ditambahkan).
    bm_edit: Option<(String, bool)>,
    /// Folder bookmark yang sedang diedit (alamat yang sama bisa ada di beberapa folder).
    bm_edit_from: Option<String>,
    /// Folder terakhir yang dipilih di kotak bookmark (bookmark baru masuk ke sana).
    last_bm_folder: String,
    record_at: Instant,
    video_jobs: HashMap<u64, video::Job>,
    /// Tinggi tambahan toolbar saat dropdown/gelembung terbuka (menutupi halaman).
    toolbar_over: f64,
    /// Tawaran simpan/perbarui sandi yang sedang ditampilkan: (alamat, pengguna, sandi, perbarui?).
    pw_offer: Option<(String, String, String, bool)>,
    web_context: WebContext,
    #[cfg(windows)]
    env: Option<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment>,
    proxy: EventLoopProxy<UserEvent>,
    browser_args: String,
    update_ready: Option<String>,
    /// Pengguna meminta periksa update sendiri: begitu siap, langsung tawarkan pasang.
    update_prompt: bool,
    last_sleep_check: Instant,
    last_diag: Instant,
    last_render_key: String,
    /// Ikon terakhir yang dikirim ke toolbar (dikirim ulang hanya bila berubah).
    last_icons_key: String,
    /// Frame utama toolbar dan PID proses rendernya (untuk task manager).
    ui_frame: Option<u32>,
    ui_pids: Vec<u32>,
    menus: Menus,
    menu_tab: Option<u64>,
    _tray: Option<TrayIcon>,
    lang: Lang,
    /// Kunci sesi untuk farbling.
    session_key: u64,
    /// Naik setiap setelan yang memengaruhi skrip halaman berubah.
    script_gen: u64,
    /// Tab yang tampil di sisi kanan (split view).
    split: Option<u64>,
    /// Cache CNAME bersama untuk semua tab.
    cname: Arc<Mutex<HashMap<String, Option<String>>>>,

    last_unresponsive: HashMap<u64, Instant>,
    extensions_loaded: bool,
    /// Tersembunyi di tray.
    in_tray: bool,
    /// Sedang dalam keadaan hemat (tray atau diminimalkan).
    background: bool,
    next_trim: Option<Instant>,
    find_open: bool,
    /// Nomor jendela yang sedang dipegang App. Jendela lain disimpan di `others` dan ditukar
    /// masuk (lihat `switch_to`) saat ada kejadian untuknya, sehingga semua fungsi tab tetap
    /// bekerja pada "jendela saat ini" tanpa diubah.
    win_no: u32,
    others: Vec<WinState>,
    next_win: u32,
    /// Jendela yang terakhir difokuskan pengguna (tujuan perintah dari tray/peluncuran ulang).
    focused_win: u32,
    /// Jendela yang menampilkan menu terakhir (pilihan menu berlaku untuk jendela itu).
    menu_win: Cell<u32>,
    /// Jendela baru yang menunggu dibuat (butuh target event loop).
    want_windows: Vec<WinRequest>,
    /// Sejak kapan jendela saat ini diminimalkan/di tray, dan apakah tabnya sudah ditangguhkan.
    bg_since: Option<Instant>,
    bg_suspended: bool,
    /// Urutan fokus jendela (paling baru di depan), untuk memilih jendela tujuan tab yang diseret.
    z_order: Vec<u32>,
    /// Proses halaman yang sedang dalam mode efisiensi karena semua tabnya tersembunyi.
    eco_pids: HashSet<u32>,
    eco_at: Instant,
    /// Dialog unduh video: nomor penelusuran terakhir dan jendelanya.
    video_req: u64,
    video_win: u32,
}

/// Bagian App yang berbeda untuk tiap jendela Ovner. Urutan field penting: tab, toolbar, dan
/// daftar tab dibuang lebih dulu daripada jendelanya.
struct WinState {
    tabs: Vec<Tab>,
    toolbar: WebView,
    sidebar: Option<WebView>,
    window: Rc<Window>,
    no: u32,
    toolbar_ready: bool,
    toolbar_h: f64,
    toolbar_over: f64,
    active: usize,
    split: Option<u64>,
    find_open: bool,
    last_render_key: String,
    last_icons_key: String,
    background: bool,
    bg_since: Option<Instant>,
    bg_suspended: bool,
}

/// Permintaan membuka jendela baru.
struct WinRequest {
    /// Tab yang dipindahkan ke jendela baru (kosong = satu tab baru).
    tabs: Vec<Tab>,
    active: usize,
    /// Tab baru pertama jendela ini privat.
    private: bool,
    /// Posisi layar (logis) jendela; None = bertingkat dari jendela saat ini.
    pos: Option<(f64, f64)>,
    maximized: bool,
    /// Jendela asal (tab dikembalikan ke sana bila jendela baru gagal dibuat).
    from: u32,
    /// Jadikan jendela aktif setelah dibuat (false untuk jendela yang dipulihkan saat mulai).
    focus: bool,
}

/// Bahan bersama pembuat WebView tab.
#[derive(Clone)]
struct TabEnv {
    /// Jendela induk saat WebView dibuat (lemah: tab bisa pindah jendela, jendela bisa ditutup).
    window: std::rc::Weak<Window>,
    settings: Rc<RefCell<Settings>>,
    blocker: Rc<RefCell<Blocker>>,
    proxy: EventLoopProxy<UserEvent>,
    session_key: u64,
    browser_args: String,
    #[cfg(windows)]
    env: Option<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment>,
    cname: Arc<Mutex<HashMap<String, Option<String>>>>,
    dl_skip: Rc<RefCell<HashSet<String>>>,
    next_id: Rc<Cell<u64>>,
    popups: Rc<RefCell<Vec<PopupTab>>>,
}

/// Sel bersama milik satu tab (dipakai handler WebView-nya).
struct TabCells {
    id: u64,
    private: bool,
    blocked: Rc<Cell<u32>>,
    page: Rc<RefCell<String>>,
    allow_http: Rc<RefCell<Option<String>>>,
    scripts_on: Rc<Cell<bool>>,
}

impl TabCells {
    fn new(id: u64, private: bool) -> TabCells {
        TabCells {
            id,
            private,
            blocked: Rc::new(Cell::new(0)),
            page: Rc::new(RefCell::new(String::new())),
            allow_http: Rc::new(RefCell::new(None)),
            scripts_on: Rc::new(Cell::new(true)),
        }
    }
}

/// Pop-up yang dibuka situs lewat window.open dan sudah dibuat sebagai WebView (opener
/// tetap tersambung), menunggu dijadikan tab.
struct PopupTab {
    opener: u64,
    url: String,
    cells: TabCells,
    webview: WebView,
}

/// Buat WebView sebuah tab beserta semua handler-nya. `url` = None untuk pop-up yang akan
/// dinavigasi oleh WebView2 sendiri (window.open).
#[allow(unused_variables, unused_mut)]
fn build_tab_view(
    tenv: &TabEnv,
    cells: &TabCells,
    url: Option<&str>,
    rect: Rect,
    visible: bool,
    focused: bool,
    web_context: Option<&mut WebContext>,
) -> wry::Result<WebView> {
    let s = tenv.settings.borrow().clone();
    let (id, private) = (cells.id, cells.private);
    let (blocked, page) = (cells.blocked.clone(), cells.page.clone());
    if let Some(u) = url {
        *page.borrow_mut() = u.to_string();
    }
    let allow_http = cells.allow_http.clone();
    let scripts_on = cells.scripts_on.clone();
    scripts_on.set(true);
    #[allow(unused_mut)]
    let mut init = shields::page_scripts(&s, tenv.session_key, private);
    // Windows: `window.ipc` hanya untuk halaman internal Ovner (bisa dihapus/ditimpa), bukan untuk
    // situs dan iframe-nya.
    #[cfg(windows)]
    {
        init = format!(
            "if(location.href.startsWith({p:?})&&window.chrome&&window.chrome.webview){{Object.defineProperty(window,'ipc',{{value:Object.freeze({{postMessage:s=>window.chrome.webview.postMessage(s)}}),configurable:true,writable:true}});window.chrome.webview.addEventListener('message',e=>{{let m;try{{m=JSON.parse(e.data)}}catch(_){{return}}if(m&&m.page&&window.ovnerPage)window.ovnerPage(m.page)}})}}\n{init}",
            p = INTERNAL_PREFIX
        );
    }
    let p_ipc = tenv.proxy.clone();
    let p_title = tenv.proxy.clone();
    let p_load = tenv.proxy.clone();
    let p_nav = tenv.proxy.clone();
    let p_new = tenv.proxy.clone();
    let (nav_blocked, nav_page) = (blocked.clone(), page.clone());
    let nav_settings = tenv.settings.clone();
    let nav_blocker = tenv.blocker.clone();
    let (deny_perm, autoplay, popups_to_tabs) = (s.deny_sensitive_permissions, s.autoplay, s.popups_to_tabs);
    let popup_env = tenv.clone();

    let mut builder = match (private, web_context) {
        (false, Some(ctx)) => WebViewBuilder::new_with_web_context(ctx),
        (true, _) => WebViewBuilder::new().with_incognito(true),
        _ => WebViewBuilder::new(),
    };
    // Identitas (user agent + merek Sec-CH-UA) dipasang lewat DevTools sesudah WebView dibuat.
    // Bila alamat langsung dimuat saat dibuat, permintaan pertama masih memakai identitas asli
    // dan permintaan berikutnya identitas samaran: Startpage/Anubis menganggapnya "user-agent
    // switcher" (Access Temporarily Suspended, verifikasi gagal). Jadi alamat baru dimuat
    // setelah identitasnya terpasang.
    let ident_first = !shields::identity_overrides(&s, "Mozilla/5.0 Chrome/120.0.0.0 Edg/120.0.0.0").is_empty();
    if let Some(u) = url.filter(|_| !ident_first) {
        builder = builder.with_url(u);
    }
    builder = builder
        .with_bounds(rect)
        .with_visible(visible)
        .with_focused(focused)
        .with_hotkeys_zoom(true)
        .with_autoplay(autoplay)
        .with_devtools(s.devtools)
        .with_initialization_script(&init)
        .with_custom_protocol("ovner".into(), |_, req| serve_internal(req))
        .with_document_title_changed_handler(move |t| send(&p_title, UserEvent::Title(id, t)))
        .with_on_page_load_handler(move |ev, url| send(&p_load, UserEvent::PageLoad(id, ev, url)))
        .with_navigation_handler(move |url| {
            let s = nav_settings.borrow();
            // 1. Debounce dan de-AMP, 2. buang parameter pelacak, 3. naikkan ke HTTPS.
            let mut target = shields::rewrite_navigation(&url, &s);
            if s.strip_tracking_params {
                if let Some(t) = blocker::strip_tracking_params(target.as_deref().unwrap_or(&url)) {
                    target = Some(t);
                }
                // Aturan $removeparam dari daftar filter (AdGuard URL Tracking, uBO).
                if s.block_ads {
                    let cur = target.clone().unwrap_or_else(|| url.clone());
                    if let Some(t) = nav_blocker.borrow().check(&cur, &cur, "document").rewritten {
                        target = Some(t);
                    }
                }
            }
            if s.https_only && allow_http.borrow().as_deref() != Some(url.as_str()) {
                if let Some(up) = upgrade_https(target.as_deref().unwrap_or(&url)) {
                    send(&p_nav, UserEvent::Upgraded(id, target.clone().unwrap_or_else(|| url.clone())));
                    target = Some(up);
                }
            }
            if let Some(t) = target {
                // Untuk Rekam masalah: navigasi yang dibatalkan Ovner lalu diulang ke alamat lain.
                send(&p_nav, UserEvent::Problem(id, "ovner".into(), format!("OVNER-REWRITE {url} -> {t}")));
                send(&p_nav, UserEvent::Navigate(id, t));
                return false;
            }
            // 4. Blokir JavaScript per situs: setelan berlaku untuk navigasi berikutnya,
            //    jadi navigasi ini dibatalkan lalu diulang dengan setelan yang benar.
            let want = internal_page(&url).is_some() || s.scripts_allowed(&url);
            if want != scripts_on.get() {
                send(&p_nav, UserEvent::Problem(id, "ovner".into(), format!("OVNER-SCRIPTNAV js={want} {url}")));
                send(&p_nav, UserEvent::ScriptNav(id, url, want));
                return false;
            }
            send(&p_nav, UserEvent::Problem(id, "ovner".into(), format!("NAV {url}")));
            *nav_page.borrow_mut() = url;
            nav_blocked.set(0);
            true
        })
        .with_new_window_req_handler(move |url, features| {
            // Jendela kecil berukuran tertentu (login Google/Microsoft, pembayaran) dibiarkan
            // sebagai jendela sendiri, kecuali "pop-up jadi tab" dinyalakan.
            if features.size.is_some() && !popups_to_tabs {
                return NewWindowResponse::Allow;
            }
            // Pop-up dibuka sebagai tab Ovner yang tetap tersambung ke pembukanya
            // (window.opener / nilai kembali window.open), seperti Brave. Situs seperti Infor
            // Ming.le, OAuth, dan pembayaran bergantung pada sambungan ini; dulu tab baru dibuat
            // terpisah sehingga situs menunggu selamanya.
            #[cfg(windows)]
            if let Some(core) = make_popup(&popup_env, id, private, &url) {
                return NewWindowResponse::Create { webview: core };
            }
            send(&p_new, UserEvent::NewTab(id, url));
            NewWindowResponse::Deny
        })
        .with_permission_handler(move |kind| {
            if deny_perm && matches!(kind, PermissionKind::Camera | PermissionKind::Microphone | PermissionKind::Geolocation) {
                PermissionResponse::Deny
            } else {
                PermissionResponse::Default
            }
        });

    #[cfg(windows)]
    let builder = {
        use wry::WebViewBuilderExtWindows;
        let mut b = builder
            .with_additional_browser_args(tenv.browser_args.clone())
            .with_browser_accelerator_keys(true)
            .with_default_context_menus(true)
            .with_theme(wry_theme(&s.theme));
        if let Some(env) = &tenv.env {
            b = b.with_environment(env.clone());
        }
        b
    };

    // Linux: saluran IPC wry (window.ipc). Windows: lihat win::on_message di bawah.
    #[cfg(not(windows))]
    let builder = {
        let p_ipc = p_ipc.clone();
        builder.with_ipc_handler(move |req| send(&p_ipc, UserEvent::Content(id, req.body().clone())))
    };
    let parent = tenv.window.upgrade().ok_or(wry::Error::MessageSender)?;
    let wv = builder.build_as_child(&*parent)?;
    #[cfg(windows)]
    win::on_message(&wv, move |body| send(&p_ipc, UserEvent::Content(id, body)));
    win::tune_webview(&wv, &s);
    for (method, params) in shields::identity_overrides(&s, &win::user_agent(&wv)) {
        win::cdp(&wv, method, &params);
    }
    if let Some(u) = url.filter(|_| ident_first) {
        let _ = wv.load_url(u);
    }
    #[cfg(windows)]
    {
        let ctx = netfilter::FilterCtx {
            blocker: tenv.blocker.clone(),
            settings: tenv.settings.clone(),
            blocked: blocked.clone(),
            page: page.clone(),
            cname: tenv.cname.clone(),
        };
        netfilter::attach_scriptlets(&wv, ctx.clone());
        if let Err(e) = netfilter::attach(&wv, ctx) {
            log(&format!("pemblokir tidak terpasang: {e}"));
        }
        let p_done = tenv.proxy.clone();
        netfilter::on_navigation_completed(&wv, move |ok, status| send(&p_done, UserEvent::NavDone(id, ok, status)));
    }
    let p_fail = tenv.proxy.clone();
    win::on_process_failed(&wv, move |k, d| send(&p_fail, UserEvent::ProcFailed(id, k, d)));
    let p_close = tenv.proxy.clone();
    win::on_window_close(&wv, move || send(&p_close, UserEvent::ScriptClose(id)));
    let p_dl = tenv.proxy.clone();
    let (dl_settings, dl_skip) = (tenv.settings.clone(), tenv.dl_skip.clone());
    // PDF besar langsung diunduh (setelan "pdf_download"): dibaca Ovner dari respons yang sudah
    // ada, bukan lewat unduhan WebView2 (lihat win::pdf_downloads).
    if tenv.settings.borrow().pdf_download && !private {
        let st = tenv.settings.clone();
        let p_pdf = tenv.proxy.clone();
        win::pdf_downloads(
            &wv,
            move |url, len, name| {
                let s = st.borrow();
                let min = s.pdf_download_mb as u64 * 1_048_576;
                // Ukuran tidak diketahui (PDF dibuat di server): unduh bila alamatnya jelas unduhan.
                let yes = s.pdf_download && len.map_or(url.to_ascii_lowercase().contains("download"), |l| l >= min);
                if !yes {
                    return None;
                }
                let dir = if s.download_dir.trim().is_empty() {
                    std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Downloads")).unwrap_or_else(|| data_dir().to_path_buf())
                } else {
                    PathBuf::from(s.download_dir.trim())
                };
                let _ = std::fs::create_dir_all(&dir);
                let path = dir.join(name);
                let path = if s.dl_box_v2 { dlengine::categorized(&path) } else { path };
                Some(free_download_path(&path).display().to_string())
            },
            move |ev| send(&p_pdf, UserEvent::Download(id, ev)),
        );
    }
    win::on_download(
        &wv,
        move |ev| send(&p_dl, UserEvent::Download(id, ev)),
        move |url, total| {
            // Mesin unduh Ovner hanya untuk berkas besar dari http(s) biasa.
            let s = dl_settings.borrow();
            s.dl_engine
                && total >= s.dl_min_mb as i64 * 1_048_576
                && total > 0
                && (url.starts_with("https://") || url.starts_with("http://"))
                && !dl_skip.borrow().contains(url)
        },
        {
            let st = tenv.settings.clone();
            move |path| {
                if st.borrow().dl_box_v2 {
                    dlengine::categorized(Path::new(path)).display().to_string()
                } else {
                    path.to_string()
                }
            }
        },
    );
    if !private {
        let p_icon = tenv.proxy.clone();
        win::on_favicon(&wv, move |png| send(&p_icon, UserEvent::Favicon(id, png)));
    }
    Ok(wv)
}

/// Buat WebView pop-up (tanpa alamat: WebView2 menavigasikannya sendiri) untuk window.open.
#[cfg(windows)]
fn make_popup(tenv: &TabEnv, opener: u64, private: bool, url: &str) -> Option<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2> {
    use wry::WebViewExtWindows;
    // Harus memakai environment yang sama dengan pembukanya.
    tenv.env.as_ref()?;
    let id = tenv.next_id.get() + 1;
    tenv.next_id.set(id);
    let cells = TabCells::new(id, private);
    *cells.page.borrow_mut() = url.to_string();
    let parent = tenv.window.upgrade()?;
    let size: LogicalSize<f64> = parent.inner_size().to_logical(parent.scale_factor());
    let rect = Rect { position: WLogicalPosition::new(0.0, 0.0).into(), size: WLogicalSize::new(size.width.max(1.0), size.height.max(1.0)).into() };
    match build_tab_view(tenv, &cells, None, rect, false, false, None) {
        Ok(wv) => {
            let core = wv.webview();
            tenv.popups.borrow_mut().push(PopupTab { opener, url: url.to_string(), cells, webview: wv });
            send(&tenv.proxy, UserEvent::PopupReady);
            Some(core)
        }
        Err(e) => {
            log(&format!("pop-up gagal dibuat sebagai tab: {e}"));
            None
        }
    }
}

/// Domain dasar (kompas.com, bpjsketenagakerjaan.go.id) untuk mengelompokkan subdomain.
fn netfilter_base_domain(host: &str) -> String {
    let parts: Vec<&str> = host.trim_end_matches('.').split('.').collect();
    if parts.len() <= 2 {
        return host.to_string();
    }
    let n = parts.len();
    let take = if parts[n - 2].len() <= 3 && parts[n - 1].len() == 2 { 3 } else { 2 };
    parts[n.saturating_sub(take)..].join(".")
}

fn send(proxy: &EventLoopProxy<UserEvent>, e: UserEvent) {
    let _ = proxy.send_event(e);
}

fn html_response(html: &'static str) -> wry::http::Response<Cow<'static, [u8]>> {
    wry::http::Response::builder().header("Content-Type", "text/html; charset=utf-8").body(Cow::Borrowed(html.as_bytes())).unwrap()
}

fn page_response(html: &str) -> wry::http::Response<Cow<'static, [u8]>> {
    let full = html.replace("<link rel=\"stylesheet\" href=\"ovner://page/page.css\">", &format!("<style>{PAGE_CSS}</style>"));
    wry::http::Response::builder().header("Content-Type", "text/html; charset=utf-8").body(Cow::Owned(full.into_bytes())).unwrap()
}

/// Penyaji halaman internal `ovner://<nama>/`.
fn serve_internal(req: wry::http::Request<Vec<u8>>) -> wry::http::Response<Cow<'static, [u8]>> {
    let uri = req.uri().to_string();
    let name = uri.strip_prefix("ovner://").and_then(|r| r.split('/').next()).unwrap_or("");
    match name {
        "newtab" => html_response(NEWTAB_HTML),
        "settings" => page_response(SETTINGS_HTML),
        "history" => page_response(HISTORY_HTML),
        "bookmarks" => page_response(BOOKMARKS_HTML),
        "tasks" => page_response(TASKS_HTML),
        "setup" => page_response(SETUP_HTML),
        "https" => page_response(HTTPS_HTML),
        "passwords" => page_response(PASSWORDS_HTML),
        "diagnostics" => page_response(DIAGNOSTICS_HTML),
        "sessions" => page_response(SESSIONS_HTML),
        "downloads" => page_response(DOWNLOADS_HTML),
        _ => wry::http::Response::builder().status(404).body(Cow::Borrowed(&b"Tidak ada"[..])).unwrap(),
    }
}

/// Tab sedang memutar suara (video/musik).
fn playing_audio(wv: &WebView) -> bool {
    #[cfg(windows)]
    {
        netfilter::is_playing_audio(wv)
    }
    #[cfg(not(windows))]
    {
        let _ = wv;
        false
    }
}

/// Jeda semua video/musik di halaman (termasuk iframe yang bisa dijangkau).
fn pause_media(wv: &WebView) {
    let _ = wv.evaluate_script(
        "(function(){const p=(d)=>{try{d.querySelectorAll('video,audio').forEach(m=>{try{m.pause()}catch(e){}});\
         d.querySelectorAll('iframe').forEach(f=>{try{p(f.contentDocument)}catch(e){}})}catch(e){}};p(document)})()",
    );
}

fn set_memory_low(wv: &WebView, low: bool) {
    #[cfg(windows)]
    {
        use wry::{MemoryUsageLevel, WebViewExtWindows};
        let _ = wv.set_memory_usage_level(if low { MemoryUsageLevel::Low } else { MemoryUsageLevel::Normal });
    }
    #[cfg(not(windows))]
    let _ = (wv, low);
}

/// Teks aman untuk menu Windows: "&" berarti huruf pintas, jadi digandakan; judul panjang dipotong.
fn menu_label(text: &str) -> String {
    let mut t: String = text.trim().chars().take(70).collect();
    if text.trim().chars().count() > 70 {
        t.push('…');
    }
    if t.is_empty() {
        t.push('—');
    }
    if cfg!(windows) {
        t.replace('&', "&&")
    } else {
        t
    }
}

/// Buat toolbar (WebView atas) untuk jendela nomor `no`.
fn build_toolbar(
    window: &Window,
    web_context: &mut WebContext,
    proxy: &EventLoopProxy<UserEvent>,
    no: u32,
    args: &str,
    theme: &str,
    #[cfg(windows)] env: Option<&webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment>,
) -> wry::Result<WebView> {
    let p = proxy.clone();
    let size: LogicalSize<f64> = window.inner_size().to_logical(window.scale_factor());
    let b = WebViewBuilder::new_with_web_context(web_context)
        .with_html(TOOLBAR_HTML)
        .with_bounds(Rect { position: WLogicalPosition::new(0.0, 0.0).into(), size: WLogicalSize::new(size.width.max(1.0), 77.0).into() })
        .with_hotkeys_zoom(false)
        .with_devtools(cfg!(debug_assertions))
        .with_ipc_handler(move |req| send(&p, UserEvent::Toolbar(no, req.body().clone())));
    #[cfg(windows)]
    let b = {
        use wry::WebViewBuilderExtWindows;
        let mut b = b
            .with_additional_browser_args(args.to_string())
            .with_browser_accelerator_keys(false)
            .with_default_context_menus(false)
            .with_theme(wry_theme(theme));
        if let Some(env) = env {
            b = b.with_environment(env.clone());
        }
        b
    };
    #[cfg(not(windows))]
    let _ = (args, theme);
    b.build_as_child(window)
}

/// Buat jendela utama Ovner (tanpa isi).
fn build_window(target: &EventLoopWindowTarget<UserEvent>, theme: &str, visible: bool, maximized: bool, size: (f64, f64), pos: Option<(f64, f64)>) -> Result<Window, String> {
    let mut b = WindowBuilder::new()
        .with_title("Ovner")
        .with_inner_size(LogicalSize::new(size.0, size.1))
        .with_min_inner_size(LogicalSize::new(480.0, 320.0))
        .with_maximized(maximized)
        .with_visible(visible)
        .with_theme(match theme {
            "light" => Some(tao::window::Theme::Light),
            "dark" => Some(tao::window::Theme::Dark),
            _ => None,
        })
        .with_window_icon(Icon::from_rgba(icon::rgba(64), 64, 64).ok());
    if let Some((x, y)) = pos {
        b = b.with_position(tao::dpi::LogicalPosition::new(x, y));
    }
    b.build(target).map_err(|e| e.to_string())
}

fn build_menus(lang: Lang) -> Menus {
    let item = |id: &str, key: &str| MenuItem::with_id(id, t(lang, key), true, None);
    let main = Menu::new();
    let bookmarks = Submenu::new(t(lang, "m.bookmarks_menu"), true);
    let _ = bookmarks.append_items(&[
        &item("bookmark", "m.bookmark"),
        &item("bookmark-bar", "m.bookmark_bar"),
        &item("bookmarks", "m.bookmarks"),
        &item("import", "m.import"),
        &PredefinedMenuItem::separator(),
    ]);
    let zoom = Submenu::new(t(lang, "m.zoom"), true);
    let _ = zoom.append_items(&[&item("zoom-out", "m.zoom_out"), &item("zoom-reset", "m.zoom_reset"), &item("zoom-in", "m.zoom_in")]);
    let tools = Submenu::new(t(lang, "m.tools"), true);
    let _ = tools.append_items(&[
        &item("tasks", "m.tasks"),
        &item("sessions", "m.sessions"),
        &item("sleep-all", "m.sleep_all"),
        &item("close-duplicates", "m.close_duplicates"),
        &item("trim", "m.trim"),
        &PredefinedMenuItem::separator(),
        &item("pick", "m.pick"),
        &item("filters", "m.filters"),
        &item("clear", "m.clear"),
        &item("data", "m.data"),
        &item("devtools", "m.devtools"),
        &PredefinedMenuItem::separator(),
        &item("diagnostics", "m.diagnostics"),
    ]);
    let _ = main.append_items(&[
        &item("newtab", "m.newtab"),
        &item("private", "m.private"),
        &item("new-window", "m.new_window"),
        &item("new-private-window", "m.new_private_window"),
        &item("merge-windows", "m.merge_windows"),
        &item("reopen", "m.reopen"),
        &item("new-identity", "m.new_identity"),
        &PredefinedMenuItem::separator(),
        &bookmarks,
        &item("history", "m.history"),
        &item("passwords", "m.passwords"),
        &item("downloads", "m.downloads"),
        &PredefinedMenuItem::separator(),
        &zoom,
        &item("print", "m.print"),
        &item("findbar", "m.find"),
        &item("split", "m.split"),
        &tools,
        &PredefinedMenuItem::separator(),
        &item("settings", "m.settings"),
        &item("check-update", "m.check_update"),
        &item("about", "m.about"),
        &item("hide", "m.hide"),
        &item("quit", "m.quit"),
    ]);
    let tab = Menu::new();
    let _ = tab.append_items(&[
        &item("tab-reload", "tm.reload"),
        &item("tab-duplicate", "tm.duplicate"),
        &item("tab-sleep", "tm.sleep"),
        &item("tab-split", "tm.split"),
        &item("tab-new-window", "tm.new_window"),
        &PredefinedMenuItem::separator(),
        &item("copy-urls", "tm.copy_urls"),
        &item("close-duplicates", "m.close_duplicates"),
        &item("tab-close", "tm.close"),
        &item("tab-close-others", "tm.close_others"),
    ]);
    let tray = Menu::new();
    let _ = tray.append_items(&[&item("show", "tray.show"), &item("newtab", "tray.newtab"), &item("new-window", "tray.new_window"), &PredefinedMenuItem::separator(), &item("quit", "m.quit")]);
    Menus { main, bookmarks, tab, tray }
}

impl App {
    fn toolbar_rect(&self) -> Rect {
        let size: LogicalSize<f64> = self.window.inner_size().to_logical(self.window.scale_factor());
        // Saat dropdown alamat/gelembung terbuka, toolbar memanjang menutupi bagian atas
        // halaman (overlay), tanpa menggeser halaman.
        let h = (self.toolbar_h + self.toolbar_over).min(size.height.max(1.0));
        Rect { position: WLogicalPosition::new(0.0, 0.0).into(), size: WLogicalSize::new(size.width.max(1.0), h).into() }
    }

    fn side_w(&self) -> f64 {
        if self.sidebar.is_some() {
            SIDEBAR_W
        } else {
            0.0
        }
    }

    fn content_rect(&self) -> Rect {
        let size: LogicalSize<f64> = self.window.inner_size().to_logical(self.window.scale_factor());
        let x = self.side_w();
        Rect {
            position: WLogicalPosition::new(x, self.toolbar_h).into(),
            size: WLogicalSize::new((size.width - x).max(1.0), (size.height - self.toolbar_h).max(1.0)).into(),
        }
    }

    fn sidebar_rect(&self) -> Rect {
        let size: LogicalSize<f64> = self.window.inner_size().to_logical(self.window.scale_factor());
        Rect {
            position: WLogicalPosition::new(0.0, self.toolbar_h).into(),
            size: WLogicalSize::new(SIDEBAR_W, (size.height - self.toolbar_h).max(1.0)).into(),
        }
    }

    /// Buat/buang daftar tab vertikal sesuai setelan.
    fn ensure_sidebar(&mut self) {
        let want = self.settings.borrow().vertical_tabs;
        if !want {
            self.sidebar = None;
            return;
        }
        if self.sidebar.is_some() {
            return;
        }
        let p = self.proxy.clone();
        let no = self.win_no;
        let theme = self.settings.borrow().theme.clone();
        let rect = self.sidebar_rect();
        let b = WebViewBuilder::new_with_web_context(&mut self.web_context)
            .with_html(SIDEBAR_HTML)
            .with_bounds(rect)
            .with_hotkeys_zoom(false)
            .with_devtools(cfg!(debug_assertions))
            .with_ipc_handler(move |req| send(&p, UserEvent::Toolbar(no, req.body().clone())));
        #[cfg(windows)]
        let b = {
            use wry::WebViewBuilderExtWindows;
            let mut b = b
                .with_additional_browser_args(self.browser_args.clone())
                .with_browser_accelerator_keys(false)
                .with_default_context_menus(false)
                .with_theme(wry_theme(&theme));
            if let Some(env) = &self.env {
                b = b.with_environment(env.clone());
            }
            b
        };
        #[cfg(not(windows))]
        let _ = theme;
        match b.build_as_child(&*self.window) {
            Ok(w) => self.sidebar = Some(w),
            Err(e) => log(&format!("tab vertikal gagal dibuat: {e}")),
        }
        self.last_render_key.clear();
    }

    /// Terapkan ulang semua setelan (setelah preset dipilih).
    fn apply_all_settings(&mut self) {
        let s = self.settings.borrow().clone();
        start_filter_update(self.proxy.clone(), s);
        self.script_gen += 1;
        self.each_window(|a| {
            a.rebuild_shown();
            a.ensure_sidebar();
            a.layout();
            a.last_render_key.clear();
            a.render();
        });
        self.refresh_pages("settings");
        self.last_render_key.clear();
        self.render();
    }

    fn tab_index(&self, id: u64) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    fn new_tab_struct(&mut self, url: &str, title: &str, private: bool) -> Tab {
        let id = self.next_id.get() + 1;
        self.next_id.set(id);
        Tab {
            id,
            webview: None,
            url: url.to_string(),
            title: title.to_string(),
            loading: false,
            private,
            zoom: self.settings.borrow().default_zoom,
            suspended: false,
            last_active: Instant::now(),
            blocked: Rc::new(Cell::new(0)),
            page: Rc::new(RefCell::new(String::new())),
            cos_exceptions: HashSet::new(),
            generichide: false,
            allow_http: Rc::new(RefCell::new(None)),
            upgraded_from: None,
            page_query: String::new(),
            scripts_on: Rc::new(Cell::new(true)),
            script_gen: 0,
            js_heap: 0,
            tab_cpu: 0.0,
            perf_prev: None,
            perf_on: false,
            media_hidden_at: None,
            frame_id: None,
            proc_pid: 0,
            proc_ram: 0,
            proc_cpu: 0.0,
            proc_shared: 0,
            form_dirty: false,
            recent_urls: Vec::new(),
            popup: false,
            close_after_dl: false,
            recording: false,
            rec_hooked: false,
            downloads_live: 0,
        }
    }

    fn newtab_url(&self) -> String {
        if self.settings.borrow().newtab == "blank" {
            "about:blank".into()
        } else {
            internal_url("newtab")
        }
    }

    /// Kotak untuk tab `i`: penuh, atau separuh kiri/kanan saat split view.
    fn rect_for(&self, i: usize) -> Rect {
        let r = self.rect_for_raw(i);
        if !self.settings.borrow().letterboxing {
            return r;
        }
        let (pos, size): (WLogicalPosition<f64>, WLogicalSize<f64>) = (r.position.to_logical(1.0), r.size.to_logical(1.0));
        let (dx, dy, w, h) = shields::letterbox(size.width, size.height);
        Rect { position: WLogicalPosition::new(pos.x + dx, pos.y + dy).into(), size: WLogicalSize::new(w, h).into() }
    }

    fn rect_for_raw(&self, i: usize) -> Rect {
        let full = self.content_rect();
        let Some(partner) = self.split.and_then(|id| self.tab_index(id)) else { return full };
        if i != self.active && i != partner {
            return full;
        }
        let size: LogicalSize<f64> = self.window.inner_size().to_logical(self.window.scale_factor());
        let half = (size.width / 2.0).floor();
        let h = (size.height - self.toolbar_h).max(1.0);
        if i == partner {
            Rect { position: WLogicalPosition::new(half + 1.0, self.toolbar_h).into(), size: WLogicalSize::new((size.width - half - 1.0).max(1.0), h).into() }
        } else {
            Rect { position: WLogicalPosition::new(0.0, self.toolbar_h).into(), size: WLogicalSize::new(half.max(1.0), h).into() }
        }
    }

    /// Tab `i` sedang tampil (aktif atau pasangan split).
    fn is_shown(&self, i: usize) -> bool {
        i == self.active || self.split.is_some_and(|id| self.tabs.get(i).is_some_and(|t| t.id == id))
    }

    /// Buat WebView untuk tab `i` (tab baru atau tab yang dibangunkan).
    /// Bahan bersama untuk membuat WebView tab (juga dipakai pembuat jendela pop-up).
    fn tab_env(&self) -> TabEnv {
        TabEnv {
            window: Rc::downgrade(&self.window),
            settings: self.settings.clone(),
            blocker: self.blocker.clone(),
            proxy: self.proxy.clone(),
            session_key: self.session_key,
            browser_args: self.browser_args.clone(),
            #[cfg(windows)]
            env: self.env.clone(),
            cname: self.cname.clone(),
            dl_skip: self.dl_skip.clone(),
            next_id: self.next_id.clone(),
            popups: self.popups.clone(),
        }
    }

    fn wake(&mut self, i: usize) {
        if self.tabs[i].webview.is_some() {
            return;
        }
        let rect = self.rect_for(i);
        let s = self.settings.borrow().clone();
        let url = if self.tabs[i].url.is_empty() { self.newtab_url() } else { self.tabs[i].url.clone() };
        let visible = self.is_shown(i) && !self.in_tray;
        let tab = &self.tabs[i];
        let cells = TabCells {
            id: tab.id,
            private: tab.private,
            blocked: tab.blocked.clone(),
            page: tab.page.clone(),
            allow_http: tab.allow_http.clone(),
            scripts_on: tab.scripts_on.clone(),
        };
        let zoom = tab.zoom;
        let tenv = self.tab_env();
        match build_tab_view(&tenv, &cells, Some(&url), rect, visible, visible && i == self.active, Some(&mut self.web_context)) {
            Ok(wv) => {
                if s.extensions_enabled && !self.extensions_loaded && !cells.private {
                    self.extensions_loaded = true;
                    win::load_extensions(&wv, &s.extension_dirs, log);
                }
                if zoom != 100 {
                    let _ = wv.zoom(zoom as f64 / 100.0);
                }
                self.tabs[i].webview = Some(wv);
                self.tabs[i].suspended = false;
                self.tabs[i].script_gen = self.script_gen;
            }
            Err(e) => log(&format!("gagal membuat tab: {e}")),
        }
    }

    /// Jadikan pop-up yang sudah dibuat (window.open dengan opener) sebagai tab baru di
    /// sebelah tab pembukanya, lalu tampilkan.
    fn adopt_popups(&mut self) {
        let list: Vec<PopupTab> = self.popups.borrow_mut().drain(..).collect();
        for p in list {
            // Pop-up masuk ke jendela pembukanya (WebView-nya mungkin dibuat di jendela lain).
            if let Some(no) = self.win_of_tab(p.opener) {
                self.switch_to(no);
            }
            self.attach_webview(&p.webview);
            let mut t = self.new_tab_struct(&p.url, "", p.cells.private);
            t.id = p.cells.id;
            t.blocked = p.cells.blocked;
            t.page = p.cells.page;
            t.allow_http = p.cells.allow_http;
            t.scripts_on = p.cells.scripts_on;
            t.webview = Some(p.webview);
            t.script_gen = self.script_gen;
            t.popup = true;
            let at = self.tab_index(p.opener).map(|k| k + 1).unwrap_or(self.tabs.len()).min(self.tabs.len());
            self.tabs.insert(at, t);
            if at <= self.active && self.active + 1 < self.tabs.len() {
                self.active += 1;
            }
            log(&format!("pop-up dibuka sebagai tab (opener tetap tersambung): {}", display_url(&p.url)));
            self.activate(at);
        }
    }

    /// Bangun ulang tab yang tampil (bukan halaman internal) di jendela saat ini.
    fn rebuild_shown(&mut self) {
        let shown: Vec<usize> = (0..self.tabs.len()).filter(|&j| self.is_shown(j) && internal_page(&self.tabs[j].url).is_none()).collect();
        for j in shown {
            self.rebuild_tab(j);
        }
    }

    /// Bangun ulang tab `i` (setelan skrip halaman berubah). Tab tersembunyi cukup ditidurkan.
    fn rebuild_tab(&mut self, i: usize) {
        let Some(t) = self.tabs.get_mut(i) else { return };
        let Some(wv) = t.webview.take() else { return };
        if let Ok(u) = wv.url() {
            if !u.is_empty() {
                t.url = u;
            }
        }
        drop(wv);
        t.loading = false;
        if self.is_shown(i) {
            self.wake(i);
        }
    }

    fn open_tab(&mut self, url: &str, activate: bool, private: bool) {
        let private = private || self.settings.borrow().always_private;
        let url = if url.is_empty() { self.newtab_url() } else { url.to_string() };
        let title = if is_newtab(&url) { "Tab baru" } else { "" };
        let tab = self.new_tab_struct(&url, title, private);
        let at = if self.tabs.is_empty() { 0 } else { self.active + 1 };
        self.tabs.insert(at, tab);
        if activate {
            self.activate(at);
            if is_newtab(&url) {
                self.focus_address();
            }
        } else {
            self.wake(at);
            if let Some(wv) = &self.tabs[at].webview {
                let _ = wv.set_visible(false);
            }
        }
        self.save_session();
        self.render();
    }

    fn activate(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        let now = Instant::now();
        // Mengklik tab pasangan split: tukar sisi.
        if self.split == Some(self.tabs[i].id) {
            self.split = self.tabs.get(self.active).map(|t| t.id);
        }
        if let Some(old) = self.tabs.get_mut(self.active) {
            old.last_active = now;
        }
        self.active = i;
        // Setelan skrip halaman berubah sejak tab ini dibangun: bangun ulang.
        if self.tabs[i].webview.is_some() && self.tabs[i].script_gen != self.script_gen {
            self.rebuild_tab(i);
        }
        self.wake(i);
        let partner = self.split.and_then(|id| self.tab_index(id)).filter(|&p| p != i);
        if self.split.is_some() && partner.is_none() {
            self.split = None;
        }
        if let Some(p) = partner {
            if self.tabs[p].webview.is_some() && self.tabs[p].script_gen != self.script_gen {
                self.rebuild_tab(p);
            }
            self.wake(p);
        }
        let suspend = self.settings.borrow().suspend_hidden_tabs;
        let low = self.settings.borrow().low_memory_when_hidden;
        let media_policy = self.settings.borrow().background_media.clone();
        let rects: Vec<Rect> = (0..self.tabs.len()).map(|j| self.rect_for(j)).collect();
        for (j, t) in self.tabs.iter_mut().enumerate() {
            if let Some(wv) = &t.webview {
                let on = j == i || Some(j) == partner;
                if on {
                    t.media_hidden_at = None;
                    let _ = wv.set_bounds(rects[j]);
                    win::resume(wv);
                    t.suspended = false;
                    set_memory_low(wv, false);
                    let _ = wv.set_visible(!self.in_tray);
                } else {
                    let _ = wv.set_visible(false);
                    if low {
                        set_memory_low(wv, true);
                    }
                    // Media yang sedang diputar mengikuti setelan "Media di tab latar".
                    let playing = playing_audio(wv);
                    if playing && media_policy != "pause" {
                        if media_policy == "timer" && t.media_hidden_at.is_none() {
                            t.media_hidden_at = Some(now);
                        }
                    } else if suspend && !t.loading && t.downloads_live == 0 {
                        if playing {
                            pause_media(wv);
                        }
                        win::try_suspend(wv);
                        t.suspended = true;
                    }
                }
            }
        }
        // Proses tab yang kini tampil keluar dari mode efisiensi saat itu juga (tidak menunggu tick).
        let shown_pids: Vec<u32> = [Some(i), partner].into_iter().flatten().map(|j| self.tabs[j].proc_pid).filter(|p| *p != 0 && self.eco_pids.contains(p)).collect();
        if !shown_pids.is_empty() {
            win::set_pids_eco(&shown_pids, false);
            for p in &shown_pids {
                self.eco_pids.remove(p);
            }
        }
        let t = &mut self.tabs[i];
        t.last_active = now;
        if let Some(wv) = &t.webview {
            let _ = wv.focus();
        }
        self.find_open = false;
        self.save_session();
        self.render();
    }

    fn close_tab(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        let t = self.tabs.remove(i);
        if self.split == Some(t.id) {
            self.split = None;
        }
        if !is_newtab(&t.url) && !t.private {
            self.closed.push(t.url.clone());
            if self.closed.len() > 25 {
                self.closed.remove(0);
            }
        }
        drop(t);
        if self.tabs.is_empty() {
            // Seperti Brave: menutup tab terakhir menutup jendela (ke tray atau keluar).
            // Tab baru disiapkan supaya jendela tidak kosong saat dibuka lagi.
            self.active = 0;
            self.open_tab("", true, false);
            send(&self.proxy, UserEvent::CloseWindow(self.win_no));
            return;
        }
        if i < self.active || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1).min(self.tabs.len() - 1);
        }
        let a = self.active;
        self.activate(a);
    }

    /// Tidurkan tab `i`: WebView dibuang, dimuat ulang saat diklik.
    fn sleep_tab(&mut self, i: usize) {
        if self.is_shown(i) {
            return;
        }
        let Some(t) = self.tabs.get_mut(i) else { return };
        if t.webview.is_none() {
            return;
        }
        if let Some(wv) = &t.webview {
            if let Ok(u) = wv.url() {
                if !u.is_empty() {
                    t.url = u;
                }
            }
        }
        t.webview = None;
        t.loading = false;
        t.suspended = false;
        t.form_dirty = false;
        // Perkiraan RAM yang dibebaskan (statistik seperti TabWars).
        let freed = if t.proc_pid != 0 { t.proc_ram / t.proc_shared.max(1) as u64 } else { t.js_heap };
        t.proc_pid = 0;
        t.frame_id = None;
        log(&format!("tab ditidurkan: {}", display_url(&t.url)));
        self.tab_stats.slept += 1;
        self.tab_stats.ram_freed += freed;
        self.save_tab_stats();
    }

    fn save_sessions(&self) {
        if let Ok(t) = serde_json::to_string(&self.sessions) {
            let _ = std::fs::write(data_dir().join("sessions.json"), t);
        }
    }

    fn save_tab_stats(&self) {
        if let Ok(t) = serde_json::to_string(&self.tab_stats) {
            let _ = std::fs::write(data_dir().join("tab-stats.json"), t);
        }
    }

    fn active_webview(&self) -> Option<&WebView> {
        self.tabs.get(self.active).and_then(|t| t.webview.as_ref())
    }

    fn navigate_active(&mut self, url: &str, typed: bool) {
        if self.tabs.is_empty() {
            self.open_tab(url, true, false);
            return;
        }
        let i = self.active;
        self.tabs[i].url = url.to_string();
        *self.tabs[i].allow_http.borrow_mut() = if typed && url.starts_with("http://") { Some(url.to_string()) } else { None };
        if let Some(wv) = &self.tabs[i].webview {
            let _ = wv.load_url(url);
            let _ = wv.focus();
        } else {
            self.wake(i);
        }
        self.render();
    }

    fn open_internal(&mut self, name: &str) {
        let url = internal_url(name);
        // Halaman internal yang sudah terbuka dipakai lagi.
        if let Some(i) = self.tabs.iter().position(|t| t.url == url) {
            self.activate(i);
            return;
        }
        if self.tabs.get(self.active).is_some_and(|t| is_newtab(&t.url)) {
            self.navigate_active(&url, false);
        } else {
            self.open_tab(&url, true, false);
        }
    }

    fn focus_address(&self) {
        let _ = self.toolbar.focus();
        let _ = self.toolbar.evaluate_script("window.ovner && ovner.focusAddress()");
    }

    // ---- hemat RAM --------------------------------------------------------

    /// Masuk mode hemat (ke tray atau diminimalkan).
    fn enter_background(&mut self) {
        if self.background {
            return;
        }
        self.background = true;
        self.bg_since = Some(Instant::now());
        self.bg_suspended = false;
        let s = self.settings.borrow().clone();
        for t in &mut self.tabs {
            if let Some(wv) = &t.webview {
                if s.low_memory_when_hidden {
                    set_memory_low(wv, true);
                }
                if s.suspend_hidden_tabs && self.in_tray {
                    let _ = wv.set_visible(false);
                    // Musik tetap berputar di tray bila setelan media tidak "langsung berhenti".
                    if playing_audio(wv) && s.background_media != "pause" {
                        if s.background_media == "timer" && t.media_hidden_at.is_none() {
                            t.media_hidden_at = Some(Instant::now());
                        }
                        continue;
                    }
                    if t.downloads_live > 0 {
                        continue;
                    }
                    win::try_suspend(wv);
                    t.suspended = true;
                }
            }
        }
        self.refresh_eco();
        if s.trim_when_hidden {
            // Beri waktu WebView2 membuang cache dulu, baru working set dikosongkan.
            self.next_trim = Some(Instant::now() + Duration::from_secs(3));
        }
        log(&format!("mode hemat: {}", if self.in_tray { "tray" } else { "diminimalkan" }));
    }

    /// Jendela yang diminimalkan lebih dari 30 detik: tabnya disembunyikan dan ditangguhkan
    /// (seperti di tray), kecuali yang memutar media atau mengunduh. Dibangunkan lagi saat jendela
    /// dibuka (leave_background → activate).
    fn suspend_minimized(&mut self) {
        if !self.background || self.in_tray || self.bg_suspended || self.bg_since.is_none_or(|t| t.elapsed() < Duration::from_secs(30)) {
            return;
        }
        self.bg_suspended = true;
        let s = self.settings.borrow().clone();
        if !s.suspend_hidden_tabs {
            return;
        }
        let mut n = 0;
        for t in &mut self.tabs {
            let Some(wv) = &t.webview else { continue };
            if t.suspended || t.downloads_live > 0 || t.loading {
                continue;
            }
            if playing_audio(wv) && s.background_media != "pause" {
                continue;
            }
            let _ = wv.set_visible(false);
            win::try_suspend(wv);
            t.suspended = true;
            n += 1;
        }
        if n > 0 {
            log(&format!("jendela {} diminimalkan: {n} tab ditangguhkan", self.win_no));
        }
    }

    /// Mode efisiensi per jendela: proses halaman yang semua tabnya tersembunyi (jendela
    /// diminimalkan, atau tab latar) diberi EcoQoS dan prioritas rendah; proses yang menampilkan
    /// tab di jendela terlihat, memutar media, memuat, atau mengunduh tetap normal.
    fn update_renderer_eco(&mut self) {
        let on = self.settings.borrow().efficiency_mode && !self.eco_on && !self.downloading() && self.frames_ok;
        if !on {
            if !self.eco_pids.is_empty() && !self.eco_on {
                let pids: Vec<u32> = self.eco_pids.drain().collect();
                win::set_pids_eco(&pids, false);
            }
            self.eco_pids.clear();
            return;
        }
        // Pemetaan proses → tab diperlukan; diminta paling sering tiap 10 detik.
        self.request_renderer_frames(true);
        let mut awake: HashSet<u32> = self.ui_pids.iter().copied().collect();
        let mut idle: HashSet<u32> = HashSet::new();
        let mut look = |tabs: &[Tab], active: usize, split: Option<u64>, background: bool| {
            for (j, t) in tabs.iter().enumerate() {
                let Some(wv) = &t.webview else { continue };
                if t.proc_pid == 0 {
                    continue;
                }
                let shown = (j == active || split == Some(t.id)) && !background;
                if shown || t.loading || t.downloads_live > 0 || t.media_hidden_at.is_some() || playing_audio(wv) {
                    awake.insert(t.proc_pid);
                } else {
                    idle.insert(t.proc_pid);
                }
            }
        };
        look(&self.tabs, self.active, self.split, self.background || self.in_tray);
        for w in &self.others {
            look(&w.tabs, w.active, w.split, w.background || self.in_tray);
        }
        let want: HashSet<u32> = idle.difference(&awake).copied().collect();
        let start: Vec<u32> = want.difference(&self.eco_pids).copied().collect();
        let stop: Vec<u32> = self.eco_pids.difference(&want).copied().collect();
        if !start.is_empty() {
            win::set_pids_eco(&start, true);
        }
        if !stop.is_empty() {
            win::set_pids_eco(&stop, false);
        }
        if !start.is_empty() || !stop.is_empty() {
            log(&format!("efisiensi per proses: {} proses hemat (+{} -{})", want.len(), start.len(), stop.len()));
        }
        self.eco_pids = want;
    }

    fn leave_background(&mut self) {
        if !self.background {
            return;
        }
        self.background = false;
        self.bg_since = None;
        self.bg_suspended = false;
        self.next_trim = None;
        self.refresh_eco();
        // activate() membangunkan tab aktif (dan pasangan split) dan menampilkannya lagi.
        let a = self.active;
        self.activate(a);
    }

    fn hide_to_tray(&mut self) {
        self.in_tray = true;
        self.each_window(|a| {
            a.window.set_visible(false);
            a.enter_background();
        });
        self.push_usage();
    }

    fn show_from_tray(&mut self) {
        self.in_tray = false;
        let focus = self.focused_win;
        self.each_window(|a| {
            a.window.set_visible(true);
            a.window.set_minimized(false);
            a.leave_background();
            let x = a.active;
            a.activate(x);
        });
        self.switch_to(focus);
        self.window.set_focus();
    }

    /// Unduhan selesai/dibatalkan: kembalikan RAM yang terpakai selama mengunduh (buffer jaringan
    /// dan disk) bila tidak ada unduhan lain yang masih berjalan.
    fn trim_after_download(&mut self) {
        let busy = self.downloads.iter().filter(|d| d.state == "progress" || d.state == "paused").count();
        if busy <= 1 {
            self.next_trim = Some(Instant::now() + Duration::from_secs(3));
        }
    }

    fn trim_now(&self) -> u32 {
        let n = win::trim_process_tree();
        log(&format!("RAM dirapikan ({n} proses)"));
        n
    }

    /// Tidurkan tab yang lama tidak dibuka.
    fn sleep_idle_tabs(&mut self) {
        let mins = self.settings.borrow().sleep_tabs_minutes;
        if mins == 0 {
            return;
        }
        let mut limit = Duration::from_secs(mins * 60);
        // Jendela diminimalkan/di tray: tab latarnya ditidurkan 3x lebih cepat (minimal 2 menit).
        if self.background {
            limit = (limit / 3).max(Duration::from_secs(120));
        }
        let mut changed = false;
        for j in 0..self.tabs.len() {
            let t = &self.tabs[j];
            if self.is_shown(j) || t.webview.is_none() || t.last_active.elapsed() < limit || t.downloads_live > 0 {
                continue;
            }
            if t.webview.as_ref().is_some_and(playing_audio) && self.settings.borrow().background_media != "pause" {
                continue;
            }
            {
                // Situs yang tidak boleh ditidurkan, dan formulir yang sedang diisi.
                let s = self.settings.borrow();
                let host = host_of(&t.url).map(|h| settings::norm_host(&h)).unwrap_or_default();
                if host_in(&s.sleep_never_sites, &host) || (s.sleep_skip_forms && t.form_dirty) {
                    continue;
                }
            }
            self.sleep_tab(j);
            changed = true;
        }
        if changed {
            self.render();
        }
    }

    // ---- tampilan ---------------------------------------------------------

    fn render(&mut self) {
        if !self.toolbar_ready {
            return;
        }
        let s = self.settings.borrow();
        let tabs: Vec<Value> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(j, t)| {
                json!({
                    "id": t.id,
                    "title": if t.title.is_empty() { display_url(&t.url) } else { t.title.clone() },
                    "url": t.url,
                    "active": j == self.active,
                    "sleeping": t.webview.is_none() && j != self.active,
                    "loading": t.loading,
                    "private": t.private,
                    "split": self.split == Some(t.id),
                })
            })
            .collect();
        let tab = self.tabs.get(self.active);
        let url = tab.map(|t| display_url(&t.url)).unwrap_or_default();
        let host = tab.and_then(|t| host_of(&t.url));
        let shield_on = s.block_ads && !host.as_deref().is_some_and(|h| s.is_exception(h));
        let site = host.as_deref().filter(|_| tab.is_some_and(|t| internal_page(&t.url).is_none())).unwrap_or("");
        let (can_back, can_forward) = tab.and_then(|t| t.webview.as_ref()).map(nav_state).unwrap_or((false, false));
        let bookmarks: Vec<Value> = self.store.bookmarks.iter().map(|b| json!({"title": b.title, "url": b.url, "folder": b.folder})).collect();
        let dark = {
            let h = settings::norm_host(site);
            let site_state = if host_in(&s.dark_on_sites, &h) {
                "on"
            } else if host_in(&s.dark_off_sites, &h) {
                "off"
            } else {
                "default"
            };
            json!({
                "mode": s.dark_mode, "engine": s.dark_engine,
                "b": s.dark_brightness, "c": s.dark_contrast, "se": s.dark_sepia, "g": s.dark_grayscale,
                "active": !site.is_empty() && s.dark_for(&h, shields::system_dark()).is_some(),
                "site": site_state,
            })
        };
        let state = json!({
            "tabs": tabs,
            "url": url,
            "loading": tab.is_some_and(|t| t.loading),
            "blocked": tab.map(|t| t.blocked.get()).unwrap_or(0),
            "shieldOn": shield_on,
            "blockAds": s.block_ads,
            "listsReady": self.blocker.borrow().full,
            "update": self.update_ready,
            "theme": s.theme,
            "homeButton": s.home_button,
            "bookmarkBar": s.bookmark_bar,
            "bookmarks": bookmarks,
            "bookmarked": tab.is_some_and(|t| self.store.is_bookmarked(&t.url)),
            "canBack": can_back,
            "canForward": can_forward,
            "find": self.find_open,
            "lang": self.lang.code(),
            "split": self.split.is_some(),
            "site": settings::norm_host(site),
            "siteScriptsBlocked": !site.is_empty() && host_in(&s.script_blocked, site),
            "siteFarbling": if site.is_empty() { "off" } else { s.farbling_for(site) },
            "farbling": s.farbling,
            "suggest": s.address_suggestions,
            "noSearch": s.no_search,
            "vertical": s.vertical_tabs,
            "downloads": self.downloads.iter().rev().take(6).map(|d| self.download_view(d)).collect::<Vec<_>>(),
            "dlBubble": self.dl_bubble,
            "dlV2": self.settings.borrow().dl_box_v2,
            "bmEdit": self.bm_edit.as_ref().and_then(|(u, fresh)| {
                let from = self.bm_edit_from.as_deref();
                let b = self.store.bookmarks.iter().find(|b| &b.url == u && from.is_none_or(|f| b.folder == f))?;
                Some(json!({"url": b.url, "title": b.title, "folder": b.folder, "fresh": fresh, "folders": self.store.all_folders()}))
            }),
            "pwOffer": self.pw_offer.as_ref().map(|(u, user, _, update)| json!({"site": vault::site_of(u), "user": user, "update": update})),
            "toolbarCss": s.toolbar_css,
            "dark": dark,
            "video": tab.is_some_and(|t| video::is_video_page(&t.url)),
            "pageTitle": tab.map(|t| t.title.clone()).unwrap_or_default(),
            "videoTools": [video::ytdlp(data_dir()).exists(), video::ffmpeg(data_dir()).exists()],
        });
        drop(s);
        let text = state.to_string();
        if text == self.last_render_key {
            return;
        }
        // Ikon (data URL) hanya dikirim saat berubah, terpisah dari keadaan yang berubah tiap detik.
        let icons = self.store.icons_for(self.store.bookmarks.iter().map(|b| b.url.as_str()).chain(self.tabs.iter().map(|t| t.url.as_str())));
        let icons_text = Value::Object(icons).to_string();
        if icons_text != self.last_icons_key {
            post_page(&self.toolbar, "icons", &icons_text, "window.ovner && ovner.icons");
            if let Some(sb) = &self.sidebar {
                post_page(sb, "icons", &icons_text, "window.ovner && ovner.icons");
            }
            self.last_icons_key = icons_text;
        }
        post_page(&self.toolbar, "render", &text, "window.ovner && ovner.render");
        if let Some(sb) = &self.sidebar {
            post_page(sb, "render", &text, "window.ovner && ovner.render");
        }
        if let Some(t) = tab {
            let title = if t.title.is_empty() { "Ovner".to_string() } else { format!("{} - Ovner", t.title) };
            self.window.set_title(&title);
        }
        self.last_render_key = text;
    }

    /// Potong toolbar ke baris toolbar + kotak overlay yang terbuka (logis → fisik).
    fn clip_toolbar(&self, rects: &[(f64, f64, f64, f64)]) {
        if rects.is_empty() || self.toolbar_over <= 0.0 {
            win::clip_webview(&self.toolbar, &[]);
            return;
        }
        let sf = self.window.scale_factor();
        let w = self.toolbar_rect().size.to_logical::<f64>(sf).width;
        let px = |v: f64| (v * sf).round() as i32;
        let mut out = vec![(0, 0, px(w), px(self.toolbar_h))];
        for &(x, y, rw, rh) in rects {
            out.push((px(x) - 2, px(y) - 2, px(rw) + 4, px(rh) + 4));
        }
        win::clip_webview(&self.toolbar, &out);
    }

    fn set_toolbar_height(&mut self, h: f64, over: f64) {
        let h = h.clamp(60.0, 640.0);
        let over = over.clamp(0.0, 900.0);
        if (h - self.toolbar_h).abs() > 0.5 {
            self.toolbar_h = h;
            self.toolbar_over = over;
            self.layout();
        } else if (over - self.toolbar_over).abs() > 0.5 {
            self.toolbar_over = over;
            let _ = self.toolbar.set_bounds(self.toolbar_rect());
        }
        if over > 0.0 {
            win::raise(&self.toolbar);
        }
    }

    fn download_view(&self, d: &Download) -> Value {
        let meta = self.dl_meta.get(&d.id);
        let left = (d.total - d.received).max(0) as u64;
        let eta = if d.speed > 0 && d.total > 0 { left / d.speed.max(1) } else { meta.map(|m| m.eta).unwrap_or(0) };
        let segs: Vec<Value> = if d.state == "progress" {
            self.dl_jobs.get(&d.id).map(|(j, _)| j.segments().into_iter().map(|(a, p, e, busy)| json!([a, p, e, busy])).collect()).unwrap_or_default()
        } else {
            Vec::new()
        };
        json!({"id": d.id, "name": d.name, "url": d.url, "path": d.path, "total": d.total, "received": d.received, "state": d.state, "time": d.time,
            "engine": d.engine, "speed": d.speed, "video": d.video, "later": d.vspec.is_some(),
            "conns": if meta.is_some_and(|m| m.pdf) {
                // PDF: respons halaman + koneksi kedua Ovner yang masih berjalan.
                let lanes = self.pdf_race.get(&d.id).map_or(0, |r| r.lanes_running());
                (usize::from(!meta.is_some_and(|m| m.stream_dead && !m.race_won)) + lanes).max(1)
            } else {
                d.conns
            },
            "connecting": meta.is_some_and(|m| m.connecting),
            "pdf": meta.is_some_and(|m| m.pdf),
            "resumable": meta.and_then(|m| m.resumable),
            "eta": eta, "elapsed": meta.and_then(|m| m.started).map(|t| t.elapsed().as_secs()).unwrap_or(0),
            "server": meta.map(|m| m.server.clone()).unwrap_or(Value::Null), "segs": segs,
            "folder": Path::new(&d.path).parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            "exists": d.state == "done" && Path::new(&d.path).exists()})
    }

    fn save_downloads(&self) {
        let keep: Vec<&Download> = self.downloads.iter().rev().take(300).collect::<Vec<_>>().into_iter().rev().collect();
        if let Ok(t) = serde_json::to_string(&keep) {
            let _ = std::fs::write(data_dir().join("downloads.json"), t);
        }
    }

    fn on_download(&mut self, tab: u64, ev: win::DlEvent) {
        match ev {
            win::DlEvent::Log(text) => {
                log(&text);
                return;
            }
            win::DlEvent::Waiting { id, url } => {
                // Ditampilkan bila server belum menjawab setelah sebentar (lihat on_tick), supaya
                // halaman biasa yang alamatnya berisi "download" tidak berkedip di daftar.
                self.dl_waiting.insert(id, (tab, url, Instant::now()));
                return;
            }
            win::DlEvent::WaitEnd { id } => {
                self.dl_waiting.remove(&id);
                self.drop_waiting_entry(id);
                // Ternyata bukan PDF: hentikan jalur yang dimulai lebih awal.
                if let Some(r) = self.pdf_race.remove(&id) {
                    r.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                    log("PDF: jalur awal dihentikan (server tidak mengirim PDF)");
                }
                return;
            }
            win::DlEvent::Race { id, url, cookie, referer, user_agent, total } => {
                let early = total < 0;
                if early && !self.pdf_learned.contains(&pdf_key(&url)) {
                    // Belum pernah menghasilkan PDF besar: tunggu jawaban server dulu.
                    return;
                }
                if let Some(r) = self.pdf_race.get(&id) {
                    // Sudah dimulai lebih awal (tahap permintaan): cukup.
                    if r.started || early {
                        return;
                    }
                }
                let ua = if user_agent.is_empty() {
                    self.tab_index(tab).and_then(|i| self.tabs[i].webview.as_ref().map(win::user_agent)).unwrap_or_default()
                } else {
                    user_agent
                };
                let mut headers = Vec::new();
                if !cookie.is_empty() {
                    headers.push(("Cookie".to_string(), cookie));
                }
                if referer.starts_with("http") {
                    headers.push(("Referer".to_string(), referer));
                }
                headers.push(("Accept".to_string(), "application/pdf,*/*".to_string()));
                // Kecil (selesai beberapa detik) tidak perlu koneksi kedua: tunggu sebentar dulu.
                // Besar: langsung, karena server butuh waktu lagi untuk membuat berkasnya.
                let wait = if total > 0 && total < 2 * 1_048_576 { 15 } else { 0 };
                if early {
                    log(&format!("PDF: alamat ini dulu menghasilkan PDF besar, jalur Ovner dimulai bersamaan dengan halaman: {url}"));
                }
                let base = self.downloads.iter().find(|d| d.id == id && !d.path.is_empty()).map(|d| d.path.clone()).unwrap_or_else(|| {
                    let mut dir = self.download_dir();
                    if self.settings.borrow().dl_box_v2 {
                        dir = dir.join("Documents");
                    }
                    let _ = std::fs::create_dir_all(&dir);
                    dir.join(format!(".ovner-pdf-{id}")).display().to_string()
                });
                self.pdf_race.insert(
                    id,
                    PdfRace {
                        base,
                        req: dlengine::Request { url, user_agent: ua, headers, if_range: None, wait_secs: 180 },
                        due: Instant::now() + Duration::from_secs(wait),
                        started: false,
                        cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                        lanes: vec![Lane::default(); RACE_LANES],
                        received: 0,
                        finished: false,
                        t0: None,
                        decided: false,
                        restarted: false,
                    },
                );
                return;
            }
            win::DlEvent::Offer { id, url, path, total, cookie } => {
                self.popup_started_download(tab, &url);
                // Uji server di latar: Range didukung → mesin unduh Ovner, selain itu WebView2.
                let (ua, referer) = self
                    .tab_index(tab)
                    .map(|i| {
                        let t = &self.tabs[i];
                        (t.webview.as_ref().map(win::user_agent).unwrap_or_default(), t.page.borrow().clone())
                    })
                    .unwrap_or_default();
                let mut headers = Vec::new();
                if !cookie.is_empty() {
                    headers.push(("Cookie".to_string(), cookie));
                }
                if referer.starts_with("http") {
                    headers.push(("Referer".to_string(), referer));
                }
                // Langsung tampil di gelembung (Menghubungkan…) supaya terasa instan, sambil
                // server diuji di latar.
                let name = Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| url.clone());
                self.downloads.push(Download {
                    id,
                    url: url.clone(),
                    path: path.clone(),
                    name,
                    total,
                    received: 0,
                    state: "progress".into(),
                    time: store::now(),
                    engine: false,
                    speed: 0,
                    conns: 0,
                    video: false,
                    vspec: None,
                    tab: 0,
                });
                self.dl_meta.entry(id).or_default().connecting = true;
                self.dl_bubble = true;
                self.download_started(id);
                self.last_render_key.clear();
                self.render();
                let req = dlengine::Request { url, user_agent: ua, headers, if_range: None, wait_secs: 0 };
                let p = self.proxy.clone();
                std::thread::spawn(move || {
                    let mut req = req;
                    let r = dlengine::probe(&req).map(|pr| {
                        // Semua koneksi ke mirror yang sama (yang menjawab uji ini). Server seperti
                        // mirror Parrot/SourceForge mengalihkan tiap permintaan ke mirror berbeda.
                        if let Some(u) = pr.url {
                            req.url = u;
                        }
                        req.if_range = pr.validator;
                        pr.total
                    });
                    send(&p, UserEvent::DlProbe(id, tab, req, path, r));
                });
                return;
            }
            win::DlEvent::Started { id, url, path, total } => {
                self.dl_waiting.remove(&id);
                if let Some(i) = self.tab_index(tab) {
                    self.tabs[i].downloads_live += 1;
                }
                self.popup_started_download(tab, &url);
                if let Some(d) = self.downloads.iter_mut().rev().find(|d| d.id == id) {
                    d.tab = tab;
                }
                let name = Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| url.clone());
                {
                    let m = self.dl_meta.entry(id).or_default();
                    m.connecting = false;
                    if win::is_pdf_stream(id) {
                        m.pdf = true;
                        m.resumable = Some(false);
                    }
                }
                if self.dl_meta.get(&id).is_some_and(|m| m.pdf) && total >= 2 * 1_048_576 && self.pdf_learned.insert(pdf_key(&url)) {
                    if let Ok(t) = serde_json::to_string(&self.pdf_learned) {
                        let _ = std::fs::write(data_dir().join("pdf_endpoints.json"), t);
                    }
                }
                self.connecting_done(id);
                if let Some(d) = self.downloads.iter_mut().rev().find(|d| d.id == id) {
                    // Sudah ditampilkan saat menguji server: lanjut dengan WebView2.
                    d.path = path;
                    d.name = name;
                    d.total = total;
                    self.download_started(id);
                    self.refresh_pages("downloads");
                    self.last_render_key.clear();
                    self.render();
                    return;
                }
                self.downloads.push(Download {
                    id,
                    url,
                    path,
                    name,
                    total,
                    received: 0,
                    state: "progress".into(),
                    time: store::now(),
                    engine: false,
                    speed: 0,
                    conns: 0,
                    video: false,
                    vspec: None,
                    tab: 0,
                });
                self.dl_bubble = true;
                self.download_started(id);
            }
            win::DlEvent::Progress { id, received, total } => {
                // PDF: tampilkan yang paling jauh dari pembacaan respons dan koneksi kedua.
                let received = if let Some(m) = self.dl_meta.get_mut(&id).filter(|m| m.pdf) {
                    m.stream_received = received;
                    received.max(self.pdf_race.get(&id).map_or(0, |r| r.received as i64))
                } else {
                    received
                };
                let meta = self.dl_meta.entry(id).or_default();
                let now = Instant::now();
                let mut speed = None;
                if let Some((t0, b0)) = meta.last {
                    let dt = now.duration_since(t0).as_secs_f64();
                    if dt > 0.2 {
                        speed = Some(((received - b0).max(0) as f64 / dt) as u64);
                    }
                }
                meta.last = Some((now, received));
                if let Some(d) = self.downloads.iter_mut().rev().find(|d| d.id == id) {
                    d.received = received;
                    d.total = total;
                    if let Some(sp) = speed {
                        d.speed = if d.speed == 0 { sp } else { (d.speed * 2 + sp) / 3 };
                    }
                }
            }
            win::DlEvent::Done { id, ok, path, received } => {
                if self.pdf_stream_done(id, tab, ok) {
                    return;
                }
                self.trim_after_download();
                if self.dl_forced.remove(&id) {
                    // Sudah ditandai batal paksa (jumlah unduhan tab sudah dikurangi).
                    self.refresh_pages("downloads");
                    self.last_render_key.clear();
                    self.render();
                    return;
                }
                if let Some(i) = self.tab_index(tab) {
                    self.tabs[i].downloads_live = self.tabs[i].downloads_live.saturating_sub(1);
                }
                self.close_finished_popup(tab);
                if let Some(d) = self.downloads.iter_mut().rev().find(|d| d.id == id) {
                    d.state = if ok { "done".into() } else { "failed".into() };
                    d.received = received;
                    if !path.is_empty() {
                        d.name = Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| d.name.clone());
                        d.path = path;
                    }
                }
                self.save_downloads();
                self.update_priority();
            }
        }
        self.refresh_pages("downloads");
        self.last_render_key.clear();
        self.render();
    }

    /// Mulai jalur koneksi Ovner untuk PDF yang sudah waktunya (dipanggil tiap 2 detik).
    fn start_due_races(&mut self) {
        let due: Vec<u64> = self.pdf_race.iter().filter(|(_, r)| !r.started && !r.finished && Instant::now() >= r.due).map(|(id, _)| *id).collect();
        for id in due {
            let entry = self.downloads.iter().find(|d| d.id == id);
            if entry.is_some_and(|d| d.state != "progress") {
                self.pdf_race.remove(&id);
                continue;
            }
            let Some(r) = self.pdf_race.get_mut(&id) else { continue };
            let base = r.base.clone();
            r.started = true;
            log(&format!("PDF: {RACE_LANES} jalur koneksi Ovner (WinHTTP) dimulai untuk {}", r.req.url));
            for lane in 0..RACE_LANES {
                let race_path = PathBuf::from(format!("{base}.ovnerrace{lane}"));
                let (req, cancel, p) = (r.req.clone(), r.cancel.clone(), self.proxy.clone());
                std::thread::spawn(move || {
                    // Jalur berikutnya sedikit belakangan: tidak diputus server di saat yang sama.
                    std::thread::sleep(Duration::from_secs(lane as u64 * 4));
                    let mut last = String::new();
                    for attempt in 1..=3u32 {
                        if cancel.load(std::sync::atomic::Ordering::SeqCst) {
                            break;
                        }
                        send(&p, UserEvent::PdfRace(id, RaceMsg::Attempt(lane, attempt)));
                        let p2 = p.clone();
                        match dlengine::fetch_whole(&req, &race_path, true, &cancel, move |g| send(&p2, UserEvent::PdfRace(id, RaceMsg::Progress(lane, g)))) {
                            Ok(n) => {
                                send(&p, UserEvent::PdfRace(id, RaceMsg::Done(lane, Ok(n))));
                                return;
                            }
                            Err(e) => {
                                last = e;
                                if cancel.load(std::sync::atomic::Ordering::SeqCst) {
                                    break;
                                }
                                send(&p, UserEvent::PdfRace(id, RaceMsg::Failed(lane, format!("percobaan {attempt}: {last}"))));
                                std::thread::sleep(Duration::from_secs(2));
                            }
                        }
                    }
                    let _ = std::fs::remove_file(&race_path);
                    send(&p, UserEvent::PdfRace(id, RaceMsg::Done(lane, Err(last))));
                });
            }
        }
    }

    /// Setelah jalur Ovner menerima data ±10 detik: bila jelas lebih cepat dari respons halaman,
    /// respons halaman dihentikan (dan sebaliknya), supaya batas koneksi server dipakai yang
    /// lebih cepat selesai.
    fn decide_races(&mut self) {
        let mut stop_stream = Vec::new();
        for (id, r) in self.pdf_race.iter_mut() {
            if r.decided || r.finished || !r.started {
                continue;
            }
            let Some((t0, s0)) = r.t0 else { continue };
            let dt = t0.elapsed().as_secs_f64();
            if dt < 10.0 {
                continue;
            }
            let Some(d) = self.downloads.iter().find(|d| d.id == *id && d.state == "progress") else { continue };
            let Some(m) = self.dl_meta.get(id) else { continue };
            r.decided = true;
            if d.total <= 0 || m.stream_dead {
                continue;
            }
            // Jalur Ovner tercepat.
            let (best_rate, best_got) = r
                .lanes
                .iter()
                .filter(|l| !l.done)
                .filter_map(|l| l.t0.map(|t| (l.received as f64 / t.elapsed().as_secs_f64().max(1.0), l.received)))
                .fold((0.0f64, 0u64), |a, b| if b.0 > a.0 { b } else { a });
            let stream_rate = (m.stream_received - s0).max(0) as f64 / dt;
            let eta_race = (d.total as f64 - best_got as f64).max(0.0) / best_rate.max(1.0);
            let eta_stream = (d.total - m.stream_received).max(0) as f64 / stream_rate.max(1.0);
            log(&format!(
                "PDF: respons halaman {:.0} KB/dtk (sisa {:.0} dtk), jalur Ovner tercepat {:.0} KB/dtk (sisa {:.0} dtk)",
                stream_rate / 1024.0,
                eta_stream,
                best_rate / 1024.0,
                eta_race
            ));
            if eta_race < eta_stream * 0.8 {
                stop_stream.push(*id);
            } else if eta_stream < eta_race * 0.8 {
                log("PDF: jalur Ovner dihentikan (respons halaman lebih cepat selesai)");
                r.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                r.finished = true;
                r.received = 0;
            }
        }
        for id in stop_stream {
            log("PDF: respons halaman dihentikan (jalur Ovner lebih cepat selesai)");
            win::cancel_download(id);
        }
    }

    /// Kabar jalur koneksi Ovner untuk PDF.
    fn on_pdf_race(&mut self, id: u64, m: RaceMsg) {
        match m {
            RaceMsg::Attempt(lane, n) => {
                if n > 1 {
                    log(&format!("PDF: jalur {} diulang (percobaan {n})", lane + 1));
                }
                if let Some(l) = self.pdf_race.get_mut(&id).and_then(|r| r.lanes.get_mut(lane)) {
                    l.received = 0;
                    l.t0 = None;
                }
                return;
            }
            RaceMsg::Failed(lane, e) => {
                log(&format!("PDF: jalur {} gagal, {e}", lane + 1));
                if let Some(r) = self.pdf_race.get_mut(&id) {
                    if let Some(l) = r.lanes.get_mut(lane) {
                        l.received = 0;
                        l.t0 = None;
                    }
                    r.received = r.lanes.iter().map(|l| l.received).max().unwrap_or(0);
                }
                return;
            }
            RaceMsg::Progress(lane, g) => {
                let stream = self.dl_meta.get(&id).map_or(0, |m| m.stream_received);
                if let Some(r) = self.pdf_race.get_mut(&id) {
                    if r.finished {
                        return;
                    }
                    if let Some(l) = r.lanes.get_mut(lane) {
                        l.received = g;
                        if g > 0 && l.t0.is_none() {
                            l.t0 = Some(Instant::now());
                        }
                    }
                    if g > 0 && r.t0.is_none() {
                        r.t0 = Some((Instant::now(), stream));
                    }
                    r.received = r.lanes.iter().map(|l| l.received).max().unwrap_or(0);
                }
                let best = self.pdf_race.get(&id).map_or(0, |r| r.received) as i64;
                let shown = if self.dl_meta.get(&id).is_some_and(|m| m.stream_dead) { best } else { stream.max(best) };
                let now = Instant::now();
                let meta = self.dl_meta.entry(id).or_default();
                let mut speed = None;
                if let Some((t0, b0)) = meta.last {
                    let dt = now.duration_since(t0).as_secs_f64();
                    if dt > 0.5 {
                        speed = Some(((shown - b0).max(0) as f64 / dt) as u64);
                        meta.last = Some((now, shown));
                    }
                } else {
                    meta.last = Some((now, shown));
                }
                if let Some(d) = self.downloads.iter_mut().rev().find(|d| d.id == id && d.state == "progress") {
                    d.received = shown;
                    if let Some(sp) = speed {
                        d.speed = if d.speed == 0 { sp } else { (d.speed * 2 + sp) / 3 };
                    }
                }
                return;
            }
            RaceMsg::Done(lane, res) => {
                let base = self.pdf_race.get(&id).map(|r| r.base.clone());
                let race_path = base.as_ref().map(|b| PathBuf::from(format!("{b}.ovnerrace{lane}")));
                if let Some(l) = self.pdf_race.get_mut(&id).and_then(|r| r.lanes.get_mut(lane)) {
                    l.done = true;
                }
                let active = self.downloads.iter().any(|d| d.id == id && d.state == "progress");
                match res {
                    Ok(n) if active => {
                        let Some(pos) = self.downloads.iter().position(|d| d.id == id) else { return };
                        if self.downloads[pos].path.is_empty() {
                            // Jalur awal selesai sebelum halaman menjawab: nama dari alamat.
                            let url = self.downloads[pos].url.clone();
                            let mut dir = self.download_dir();
                            if self.settings.borrow().dl_box_v2 {
                                dir = dir.join("Documents");
                            }
                            let p = free_download_path(&dir.join(dlengine::pdf_name("", &url)));
                            self.downloads[pos].name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                            self.downloads[pos].path = p.display().to_string();
                        }
                        let final_path = self.downloads[pos].path.clone();
                        let _ = std::fs::remove_file(&final_path);
                        let moved = race_path.as_ref().is_some_and(|rp| std::fs::rename(rp, &final_path).is_ok());
                        if !moved {
                            log(&format!("PDF: hasil jalur {} tidak bisa dipindah ke {final_path}", lane + 1));
                            return;
                        }
                        let secs = self.dl_meta.get(&id).and_then(|m| m.started).map_or(0.0, |t| t.elapsed().as_secs_f64());
                        log(&format!("PDF tersimpan lewat jalur {}: {final_path} ({} KB, {:.0} dtk sejak dibuka)", lane + 1, n / 1024, secs));
                        if let Some(m) = self.dl_meta.get_mut(&id) {
                            m.race_won = true;
                        }
                        // Jalur lain berhenti (berkasnya dihapus oleh jalur itu sendiri).
                        if let Some(r) = self.pdf_race.get_mut(&id) {
                            r.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                            r.finished = true;
                        }
                        let d = &mut self.downloads[pos];
                        d.state = "done".into();
                        d.received = n as i64;
                        d.total = n as i64;
                        d.speed = 0;
                        // Hentikan pembacaan respons (halaman pop-up lalu ditutup).
                        win::cancel_download(id);
                        self.save_downloads();
                        self.trim_after_download();
                        self.update_priority();
                    }
                    Ok(_) => {
                        // Sudah selesai lewat jalur lain / respons halaman.
                        if let Some(rp) = race_path {
                            let _ = std::fs::remove_file(rp);
                        }
                    }
                    Err(e) => {
                        let all_done = self.pdf_race.get(&id).is_none_or(|r| r.lanes.iter().all(|l| l.done));
                        if !e.is_empty() && e != "dibatalkan" {
                            log(&format!("PDF: jalur {} berhenti ({e})", lane + 1));
                        }
                        if all_done {
                            if let Some(r) = self.pdf_race.get_mut(&id) {
                                r.finished = true;
                            }
                            let dead = self.dl_meta.get(&id).is_some_and(|m| m.stream_dead);
                            if dead && active {
                                if let Some(d) = self.downloads.iter_mut().find(|d| d.id == id) {
                                    d.state = "failed".into();
                                    d.speed = 0;
                                }
                                log("PDF gagal: semua jalur berhenti");
                                self.save_downloads();
                                self.update_priority();
                            }
                        }
                    }
                }
            }
        }
        self.refresh_pages("downloads");
        self.last_render_key.clear();
        self.render();
    }

    /// Pembacaan respons PDF berakhir. True bila sudah ditangani di sini (jangan ubah status):
    /// koneksi kedua sudah menang, atau masih berjalan setelah pembacaan respons gagal.
    fn pdf_stream_done(&mut self, id: u64, tab: u64, ok: bool) -> bool {
        let Some(m) = self.dl_meta.get(&id).filter(|m| m.pdf) else { return false };
        let (won, user) = (m.race_won, m.user_cancel);
        let mut race_running = self.pdf_race.get(&id).is_some_and(|r| !r.finished);
        // Jalur Ovner sempat dihentikan karena respons halaman lebih cepat, lalu respons halaman
        // putus: jalankan jalur Ovner lagi (sekali) daripada gagal.
        if !ok && !won && !user && !race_running {
            if let Some(r) = self.pdf_race.get_mut(&id).filter(|r| r.started && !r.restarted) {
                r.restarted = true;
                r.started = false;
                r.finished = false;
                r.decided = true;
                r.t0 = None;
                r.received = 0;
                r.lanes = vec![Lane::default(); RACE_LANES];
                r.cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
                r.due = Instant::now();
                race_running = true;
                log("PDF: respons halaman putus, jalur Ovner dijalankan lagi");
            }
        }
        if ok {
            // Pembacaan respons menang: hentikan koneksi kedua.
            if let Some(r) = self.pdf_race.get_mut(&id) {
                r.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                r.finished = true;
            }
            return false;
        }
        if !(won || (race_running && !user)) {
            return false;
        }
        // Tab pop-up boleh ditutup; unduhan diteruskan koneksi kedua (atau sudah selesai).
        if let Some(i) = self.tab_index(tab) {
            self.tabs[i].downloads_live = self.tabs[i].downloads_live.saturating_sub(1);
        }
        self.close_finished_popup(tab);
        if !won {
            if let Some(m) = self.dl_meta.get_mut(&id) {
                m.stream_dead = true;
            }
            log("PDF: pembacaan respons berhenti, diteruskan koneksi kedua");
            // Belum dimulai: mulai sekarang juga.
            if let Some(r) = self.pdf_race.get_mut(&id) {
                r.due = Instant::now();
            }
            self.start_due_races();
        }
        self.refresh_pages("downloads");
        self.last_render_key.clear();
        self.render();
        true
    }

    /// Folder unduhan (setelan, atau Downloads pengguna).
    fn download_dir(&self) -> PathBuf {
        let d = self.settings.borrow().download_dir.trim().to_string();
        if d.is_empty() {
            std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Downloads")).unwrap_or_else(|| data_dir().to_path_buf())
        } else {
            PathBuf::from(d)
        }
    }

    /// Hapus tampilan "menunggu server" yang ternyata bukan unduhan.
    fn drop_waiting_entry(&mut self, id: u64) {
        let connecting = self.dl_meta.get(&id).is_some_and(|m| m.connecting);
        if let Some(pos) = self.downloads.iter().position(|d| d.id == id && d.state == "progress") {
            if connecting {
                self.downloads.remove(pos);
                self.dl_meta.remove(&id);
                self.save_downloads();
                self.refresh_pages("downloads");
                self.last_render_key.clear();
                self.render();
            }
        }
    }

    /// Tampilkan halaman yang sudah menunggu server lebih dari 1,5 detik sebagai unduhan
    /// ("Menghubungkan…" dengan waktu berjalan), sebelum byte pertama datang.
    fn promote_waiting(&mut self) {
        // Tab-nya sudah ditutup: lupakan.
        let gone: Vec<u64> = self.dl_waiting.iter().filter(|(_, w)| self.tab_index(w.0).is_none()).map(|(k, _)| *k).collect();
        for k in gone {
            self.dl_waiting.remove(&k);
            self.drop_waiting_entry(k);
        }
        let due: Vec<(u64, u64, String)> = self
            .dl_waiting
            .iter()
            .filter(|(id, (_, _, t))| t.elapsed() >= Duration::from_millis(1500) && !self.downloads.iter().any(|d| d.id == **id))
            .map(|(id, (tab, url, _))| (*id, *tab, url.clone()))
            .collect();
        for (id, _tab, url) in due {
            let name = url.split(['?', '#']).next().unwrap_or("").rsplit('/').find(|s| !s.is_empty()).unwrap_or("unduhan").to_string();
            self.downloads.push(Download {
                id,
                url: url.clone(),
                path: String::new(),
                name,
                total: 0,
                received: 0,
                state: "progress".into(),
                time: store::now(),
                engine: false,
                speed: 0,
                conns: 0,
                video: false,
                vspec: None,
                tab: 0,
            });
            let started = self.dl_waiting.get(&id).map(|w| w.2);
            let m = self.dl_meta.entry(id).or_default();
            m.connecting = true;
            m.started = started;
            self.dl_bubble = true;
            self.download_started(id);
            log(&format!("menunggu server membuat berkas: {url}"));
            self.last_render_key.clear();
            self.render();
        }
    }

    /// Server mulai mengirim: tampilan "menunggu" menjadi unduhan berjalan.
    fn connecting_done(&mut self, id: u64) {
        if let Some(m) = self.dl_meta.get_mut(&id) {
            m.connecting = false;
        }
        self.update_priority();
    }

    /// PDF (satu koneksi, dari respons halaman) didahulukan: unduhan mesin Ovner yang lain
    /// dijeda sementara supaya tidak berebut bandwidth (dulu PDF kartu SIPP hanya ±35 KB/dtk
    /// karena ISO 8 koneksi berjalan bersamaan, lalu server memutus sebelum selesai).
    fn update_priority(&mut self) {
        let pdf = self.downloads.iter().any(|d| d.state == "progress" && self.dl_meta.get(&d.id).is_some_and(|m| m.pdf));
        if pdf {
            for (id, (job, _)) in &self.dl_jobs {
                if !job.paused() && !self.dl_yield.contains(id) && !self.dl_no_yield.contains(id) {
                    job.pause();
                    self.dl_yield.insert(*id);
                    log(&format!("unduhan {id} dijeda sementara: PDF didahulukan"));
                }
            }
        } else if !self.dl_yield.is_empty() {
            for id in std::mem::take(&mut self.dl_yield) {
                if let Some((job, _)) = self.dl_jobs.get(&id) {
                    job.resume();
                    log(&format!("unduhan {id} dilanjutkan"));
                }
            }
            self.dl_no_yield.clear();
        }
    }

    /// Ada unduhan berjalan (atau halaman yang menunggu server membuat berkas).
    fn downloading(&self) -> bool {
        !self.dl_waiting.is_empty() || self.downloads.iter().any(|d| d.state == "progress")
    }

    /// Mode efisiensi Windows hanya saat Ovner di latar DAN tidak sedang mengunduh: EcoQoS dan
    /// prioritas rendah juga memperlambat proses jaringan WebView2 dan pembacaan PDF.
    fn refresh_eco(&mut self) {
        let want = self.all_background() && self.settings.borrow().efficiency_mode && !self.downloading();
        if want != self.eco_on {
            win::set_background_mode(want);
            self.eco_on = want;
            // Semua proses baru saja diatur ulang: mode efisiensi per proses dihitung lagi.
            self.eco_pids.clear();
            self.eco_at = Instant::now() - Duration::from_secs(60);
        }
    }

    /// Unduhan dimulai dari tab pop-up yang tidak menampilkan halaman (mis. tombol "Download
    /// Kartu" SIPP yang membuka tab baru): kembali ke tab sebelumnya, dan tutup tab kosong itu
    /// setelah unduhannya selesai (menutupnya sekarang membatalkan unduhan WebView2).
    fn popup_started_download(&mut self, tab: u64, url: &str) {
        let Some(i) = self.tab_index(tab) else { return };
        let t = &self.tabs[i];
        let blank = t.url.is_empty() || t.url == "about:blank" || t.url == url || t.title.is_empty();
        if !t.popup || !blank || t.close_after_dl {
            return;
        }
        self.tabs[i].close_after_dl = true;
        if self.active == i {
            let prev = (0..self.tabs.len()).filter(|&j| j != i).max_by_key(|&j| self.tabs[j].last_active);
            if let Some(j) = prev {
                self.activate(j);
            }
        }
    }

    /// Tutup tab pop-up kosong yang unduhannya sudah tidak berjalan lagi.
    fn close_finished_popup(&mut self, tab: u64) {
        if let Some(i) = self.tab_index(tab) {
            if self.tabs[i].close_after_dl && self.tabs[i].downloads_live == 0 && self.tabs.len() > 1 {
                self.close_tab(i);
            }
        }
    }

    /// Unduhan baru dimulai: catat waktu dan cari server (IP, negara, kota, jarak) di latar.
    fn download_started(&mut self, id: u64) {
        // Catat sekarang juga, supaya tidak hilang bila Ovner mati sebelum unduhan selesai.
        self.save_downloads();
        let meta = self.dl_meta.entry(id).or_default();
        if meta.started.is_none() {
            meta.started = Some(Instant::now());
        }
        if !meta.server.is_null() {
            return;
        }
        meta.server = json!({});
        let lookup_url = meta.lookup_url.clone();
        let lookup = self.settings.borrow().dl_server_info;
        let Some(d) = self.downloads.iter().rev().find(|d| d.id == id) else { return };
        let Some(host) = host_of(if lookup_url.is_empty() { &d.url } else { &lookup_url }) else { return };
        let p = self.proxy.clone();
        std::thread::spawn(move || {
            use std::net::ToSocketAddrs;
            let addr = (host.as_str(), 443).to_socket_addrs().ok().and_then(|mut a| a.next());
            let ip = addr.map(|a| a.ip().to_string()).unwrap_or_default();
            // Ping = waktu membuka koneksi TCP ke server (seperti kolom "Server" di IDM/MDM).
            let ping = addr.and_then(|a| {
                let t = Instant::now();
                std::net::TcpStream::connect_timeout(&a, Duration::from_secs(3)).ok().map(|_| t.elapsed().as_millis() as u64)
            });
            send(&p, UserEvent::DlServer(id, json!({"host": host, "ip": ip, "ping": ping})));
            if !lookup || ip.is_empty() {
                return;
            }
            // Lokasi server dan jarak dari lokasi kita (keduanya dari ipwho.is, hanya alamat IP).
            let Some(v) = dlengine::get_text(&format!("https://ipwho.is/{ip}")).and_then(|t| serde_json::from_str::<Value>(&t).ok()) else { return };
            let mut info = json!({
                "host": host, "ip": ip, "ping": ping,
                "country": v["country"], "code": v["country_code"], "city": v["city"],
                "org": v["connection"]["org"].as_str().or(v["connection"]["isp"].as_str()).unwrap_or(""),
            });
            if let (Some(la), Some(lo), Some((ma, mo))) = (v["latitude"].as_f64(), v["longitude"].as_f64(), my_location()) {
                info["km"] = json!(haversine_km(ma, mo, la, lo).round());
            }
            send(&p, UserEvent::DlServer(id, info));
        });
    }

    /// Unduh video halaman aktif dengan yt-dlp (kualitas: "best", "720", "audio").
    /// Folder simpan bawaan untuk video/audio (Video/Music bila Download box v2).
    fn video_dir(&self, audio: bool) -> PathBuf {
        let out = self.download_dir();
        if self.settings.borrow().dl_box_v2 {
            out.join(if audio { "Music" } else { "Video" })
        } else {
            out
        }
    }

    /// Buka dialog "Unduh video dari halaman" (seperti Mister Download Manager) untuk `url`
    /// (kosong = alamat tab aktif), lalu telusuri formatnya dengan yt-dlp.
    fn open_video_dialog(&mut self, url: &str) {
        let url = if url.is_empty() { self.tabs.get(self.active).map(|t| t.url.clone()).unwrap_or_default() } else { url.to_string() };
        if !url.starts_with("http") {
            return;
        }
        self.video_req += 1;
        self.video_win = self.win_no;
        let v = json!({
            "open": true, "loading": true, "url": url,
            "title": self.tabs.get(self.active).map(|t| t.title.clone()).unwrap_or_default(),
            "videoDir": self.video_dir(false).display().to_string(),
            "audioDir": self.video_dir(true).display().to_string(),
        });
        post_page(&self.toolbar, "videoDlg", &v.to_string(), "window.ovner && ovner.videoDlg");
        self.lookup_video(&url);
    }

    fn lookup_video(&mut self, url: &str) {
        let (p, data, req, url) = (self.proxy.clone(), data_dir().to_path_buf(), self.video_req, url.to_string());
        let lang = self.lang;
        log(&format!("telusuri video: {url}"));
        std::thread::spawn(move || {
            let note = |what: &str| send(&p, UserEvent::VideoInfo(req, Ok(json!({"note": tf(lang, "d.video_tools", &[("t", what)])}))));
            let r = video::ensure_tools(&data, false, &note).and_then(|_| video::probe(&data, &url));
            send(&p, UserEvent::VideoInfo(req, r));
        });
    }

    fn on_video_info(&mut self, req: u64, r: Result<Value, String>) {
        if req != self.video_req || !self.switch_to(self.video_win) {
            return;
        }
        let v = match r {
            Ok(mut v) if v.get("note").is_some() => {
                v["loading"] = json!(true);
                v
            }
            Ok(mut v) => {
                v["loading"] = json!(false);
                v["tools"] = json!([video::ytdlp(data_dir()).exists(), video::ffmpeg(data_dir()).exists()]);
                v
            }
            Err(e) => {
                log(&format!("telusuri video gagal: {e}"));
                json!({"loading": false, "error": e})
            }
        };
        post_page(&self.toolbar, "videoDlg", &v.to_string(), "window.ovner && ovner.videoDlg");
    }

    /// Mulai (atau tunda, `later`) unduhan video dari dialog.
    fn start_video_spec(&mut self, spec: video::Spec, later: bool) {
        let id = win::next_download_id();
        let name = format!("{}.{}", video::strip_ext(&spec.name), video::ext_for(&spec.key));
        self.downloads.push(Download {
            id,
            url: spec.page.clone(),
            path: Path::new(&spec.dir).join(&name).display().to_string(),
            name,
            total: 0,
            received: 0,
            state: if later { "paused".into() } else { "progress".into() },
            time: store::now(),
            engine: false,
            speed: 0,
            conns: 0,
            video: true,
            tab: 0,
            vspec: later.then(|| spec.clone()),
        });
        self.dl_bubble = true;
        if later {
            log(&format!("unduh video ditunda ({}): {}", spec.key, spec.page));
            self.save_downloads();
        } else {
            self.download_started(id);
            self.run_video(id, spec);
        }
        self.refresh_pages("downloads");
        self.last_render_key.clear();
        self.render();
    }

    /// Jalankan yt-dlp untuk unduhan video `id`.
    fn run_video(&mut self, id: u64, spec: video::Spec) {
        let _ = std::fs::create_dir_all(&spec.dir);
        let (p, data) = (self.proxy.clone(), data_dir().to_path_buf());
        let lang = self.lang;
        log(&format!("unduh video ({}): {}", spec.key, spec.page));
        std::thread::spawn(move || {
            let note = |what: &str| send(&p, UserEvent::Video(id, video::Event::Title(tf(lang, "d.video_tools", &[("t", what)]))));
            if let Err(e) = video::ensure_tools(&data, spec.needs_ffmpeg(), &note) {
                send(&p, UserEvent::Video(id, video::Event::Done { ok: false, path: String::new(), error: e }));
                return;
            }
            let p2 = p.clone();
            match video::start(&data, &spec, move |ev| send(&p2, UserEvent::Video(id, ev))) {
                Ok(job) => send(&p, UserEvent::VideoJob(id, job)),
                Err(e) => send(&p, UserEvent::Video(id, video::Event::Done { ok: false, path: String::new(), error: e })),
            }
        });
    }

    fn on_video(&mut self, id: u64, ev: video::Event) {
        let Some(d) = self.downloads.iter_mut().rev().find(|d| d.id == id) else { return };
        match ev {
            video::Event::Title(t) => d.name = t,
            video::Event::Path(p) => d.path = p,
            video::Event::Progress { received, total, speed, eta } => {
                d.received = received as i64;
                d.total = total as i64;
                d.speed = speed;
                self.dl_meta.entry(id).or_default().eta = eta;
            }
            video::Event::Done { ok, path, error } => {
                d.state = if ok { "done".into() } else { "failed".into() };
                d.speed = 0;
                if ok {
                    if let Ok(m) = std::fs::metadata(&path) {
                        d.received = m.len() as i64;
                        d.total = m.len() as i64;
                    }
                    d.name = Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| d.name.clone());
                    d.path = path;
                } else {
                    // Sisa .part/.ytdl dan potongan format (nama.f137.mp4) dihapus.
                    if !d.path.is_empty() {
                        delete_download_files(&d.path);
                    }
                    log(&format!("unduh video gagal: {error}"));
                    d.name = format!("{} ({error})", d.name);
                }
                self.video_jobs.remove(&id);
                self.trim_after_download();
                self.save_downloads();
                self.last_render_key.clear();
                self.render();
            }
        }
        self.refresh_pages("downloads");
        if self.dl_render_at.elapsed() >= Duration::from_millis(900) {
            self.dl_render_at = Instant::now();
            self.last_render_key.clear();
            self.render();
        }
    }

    /// Jawaban uji server: ambil alih dengan mesin unduh Ovner, atau biarkan WebView2.
    fn on_dl_probe(&mut self, id: u64, tab: u64, req: dlengine::Request, path: String, res: Result<u64, String>) {
        if self.dl_cancelled.remove(&id) {
            return;
        }
        let total = match res {
            Ok(t) => {
                let m = self.dl_meta.entry(id).or_default();
                m.resumable = Some(true);
                // Mirror berbeda dari alamat asal: cari lokasi server mirror itu.
                if host_of(&req.url) != self.downloads.iter().rev().find(|d| d.id == id).and_then(|d| host_of(&d.url)) {
                    m.lookup_url = req.url.clone();
                    m.server = Value::Null;
                }
                t
            }
            Err(e) => {
                if e.contains("tidak mendukung") {
                    self.dl_meta.entry(id).or_default().resumable = Some(false);
                }
                log(&format!("unduhan biasa (WebView2): {e}"));
                win::resolve_download(id, false);
                return;
            }
        };
        let conns = self.settings.borrow().dl_connections as usize;
        let target = dlengine::unique_path(Path::new(&path));
        let p = self.proxy.clone();
        match dlengine::start(req.clone(), &target, total, conns, move |u| send(&p, UserEvent::DlUpdate(id, u))) {
            Ok(job) => {
                win::resolve_download(id, true);
                self.close_finished_popup(tab);
                log(&format!("mesin unduh Ovner: {} ({} MB, {conns} koneksi)", req.url, total / 1_048_576));
                let name = target.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| req.url.clone());
                self.dl_meta.entry(id).or_default().connecting = false;
                match self.downloads.iter_mut().rev().find(|d| d.id == id) {
                    Some(d) => {
                        d.path = target.display().to_string();
                        d.name = name;
                        d.total = total as i64;
                        d.engine = true;
                    }
                    None => self.downloads.push(Download {
                        id,
                        url: req.url.clone(),
                        path: target.display().to_string(),
                        name,
                        total: total as i64,
                        received: 0,
                        state: "progress".into(),
                        time: store::now(),
                        engine: true,
                        speed: 0,
                        conns: 0,
                        video: false,
                        vspec: None,
                        tab: 0,
                    }),
                }
                self.dl_jobs.insert(id, (job, tab));
                self.download_started(id);
                self.dl_bubble = true;
                self.refresh_pages("downloads");
                self.last_render_key.clear();
                self.render();
            }
            Err(e) => {
                log(&format!("mesin unduh Ovner tidak bisa mulai ({e}); unduhan biasa"));
                win::resolve_download(id, false);
            }
        }
    }

    fn on_dl_update(&mut self, id: u64, u: dlengine::Update) {
        let u_kind = if matches!(u, dlengine::Update::Progress { .. }) { "progress" } else { "done" };
        let Some(d) = self.downloads.iter_mut().rev().find(|d| d.id == id) else { return };
        match u {
            dlengine::Update::Progress { received, total, speed, conns, paused } => {
                d.received = received as i64;
                d.total = total as i64;
                d.speed = speed;
                d.conns = conns;
                d.state = if paused { "paused".into() } else { "progress".into() };
            }
            dlengine::Update::Done { ok, path, received, error } => {
                d.state = if ok { "done".into() } else { "failed".into() };
                d.received = received as i64;
                d.speed = 0;
                d.conns = 0;
                if ok {
                    d.name = Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| d.name.clone());
                    d.path = path;
                }
                let (url, err) = (d.url.clone(), error.unwrap_or_default());
                let job = self.dl_jobs.remove(&id);
                if err == "dibatalkan" {
                    // Pegangan berkas dilepas bersama job; pastikan sisa .ovnerpart hilang.
                    drop(job);
                    delete_download_files(&d.path);
                    self.trim_after_download();
                    self.save_downloads();
                    self.last_render_key.clear();
                    self.render();
                    self.refresh_pages("downloads");
                    return;
                }
                if !ok && err != "dibatalkan" && received > 1_048_576 {
                    // Sudah berjalan jauh: jangan mulai ulang dari nol lewat WebView2.
                    log(&format!("mesin unduh Ovner gagal setelah {} MB: {err}", received / 1_048_576));
                } else if !ok && err != "dibatalkan" {
                    // Gagal sejak awal (mis. tautan sekali pakai): ulangi lewat WebView2.
                    log(&format!("mesin unduh Ovner gagal: {err}; diulang dengan WebView2"));
                    self.dl_skip.borrow_mut().insert(url.clone());
                    let tab = job.map(|j| j.1).and_then(|t| self.tab_index(t)).filter(|&i| self.tabs[i].webview.is_some()).unwrap_or(self.active);
                    if let Some(wv) = self.tabs.get(tab).and_then(|t| t.webview.as_ref()) {
                        let _ = wv.load_url(&url);
                    }
                }
                self.trim_after_download();
                self.save_downloads();
                self.last_render_key.clear();
                self.render();
            }
        }
        // Halaman Unduhan paling sering tiap detik (dulu 4× per detik).
        if self.dl_page_at.elapsed() >= Duration::from_millis(950) || !matches!(u_kind, "progress") {
            self.dl_page_at = Instant::now();
            self.refresh_pages("downloads");
        }
        // Gelembung toolbar diperbarui paling sering tiap detik.
        if self.dl_render_at.elapsed() >= Duration::from_millis(900) {
            self.dl_render_at = Instant::now();
            self.last_render_key.clear();
            self.render();
        }
    }

    fn download_action(&mut self, kind: &str, id: u64) {
        let Some(pos) = self.downloads.iter().position(|d| d.id == id) else { return };
        let path = self.downloads[pos].path.clone();
        // Video yang ditunda ("Nanti"): lanjutkan = mulai sekarang, batalkan = hapus dari antrean.
        if self.downloads[pos].vspec.is_some() && matches!(kind, "dl-resume" | "dl-cancel") {
            let spec = self.downloads[pos].vspec.take();
            if kind == "dl-resume" {
                self.downloads[pos].state = "progress".into();
                self.download_started(id);
                if let Some(spec) = spec {
                    self.run_video(id, spec);
                }
            } else {
                self.downloads[pos].state = "failed".into();
            }
            self.save_downloads();
            self.refresh_pages("downloads");
            self.last_render_key.clear();
            self.render();
            return;
        }
        match kind {
            "dl-open" if Path::new(&path).exists() => win::open_path(&path, false),
            "dl-show" if Path::new(&path).exists() => win::open_path(&path, true),
            "dl-cancel" => match (self.dl_jobs.get(&id), self.video_jobs.get(&id)) {
                (Some((job, _)), _) => job.cancel(),
                (_, Some(v)) => v.cancel(),
                _ if self.dl_meta.get(&id).is_some_and(|m| m.pdf) && self.downloads[pos].state == "progress" => {
                    // PDF: hentikan pembacaan respons dan koneksi kedua.
                    if let Some(m) = self.dl_meta.get_mut(&id) {
                        m.user_cancel = true;
                    }
                    if let Some(r) = self.pdf_race.get_mut(&id) {
                        r.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                        r.finished = true;
                    }
                    if !win::cancel_download(id) {
                        // Pembacaan respons sudah berhenti: tandai batal di sini.
                        let d = &mut self.downloads[pos];
                        d.state = "failed".into();
                        d.speed = 0;
                        self.save_downloads();
                        self.refresh_pages("downloads");
                        self.update_priority();
                    }
                }
                _ if self.dl_meta.get(&id).is_some_and(|m| m.connecting) => {
                    // Masih menguji server: batalkan unduhan WebView2 yang ditahan dan jangan
                    // mulai mesin unduh saat hasil uji datang (dulu tetap lanjut sampai selesai).
                    self.dl_cancelled.insert(id);
                    win::resolve_download(id, true);
                    // Masih menunggu server (halaman mirip unduhan): batalkan permintaannya.
                    win::cancel_download(id);
                    self.dl_waiting.remove(&id);
                    if let Some(m) = self.dl_meta.get_mut(&id) {
                        m.connecting = false;
                    }
                    let d = &mut self.downloads[pos];
                    d.state = "failed".into();
                    d.speed = 0;
                    self.save_downloads();
                    self.refresh_pages("downloads");
                }
                _ => {
                    let stream = win::is_pdf_stream(id);
                    win::cancel_download(id);
                    // Unduhan WebView2 yang macet (mis. 0 KB) kadang tidak menanggapi Cancel:
                    // tetap tandai batal di Ovner, supaya tidak "calculating…" selamanya dan bisa
                    // dihapus dari daftar.
                    if !stream && self.downloads[pos].state == "progress" {
                        let d = &mut self.downloads[pos];
                        d.state = "failed".into();
                        d.speed = 0;
                        let (tab, path) = (d.tab, d.path.clone());
                        self.dl_forced.insert(id);
                        if let Some(i) = self.tab_index(tab) {
                            self.tabs[i].downloads_live = self.tabs[i].downloads_live.saturating_sub(1);
                        }
                        delete_download_files(&path);
                        self.save_downloads();
                        self.refresh_pages("downloads");
                        self.close_finished_popup(tab);
                    }
                }
            },
            "dl-pause" => {
                if let Some((job, _)) = self.dl_jobs.get(&id) {
                    job.pause();
                }
            }
            "dl-resume" => {
                if let Some((job, _)) = self.dl_jobs.get(&id) {
                    job.resume();
                }
                if self.dl_yield.remove(&id) {
                    self.dl_no_yield.insert(id);
                }
            }
            "dl-remove" | "dl-delete" => {
                let d = self.downloads.remove(pos);
                if kind == "dl-delete" {
                    delete_download_files(&d.path);
                }
                self.save_downloads();
                self.refresh_pages("downloads");
            }
            _ => {}
        }
        self.last_render_key.clear();
        self.render();
    }

    fn save_session(&self) {
        let keep = |t: &Tab| !is_newtab(&t.url) && !t.private && !matches!(internal_page(&t.url), Some("tasks" | "setup"));
        let one = |tabs: &[Tab], active: usize| SessionWindow {
            tabs: tabs.iter().filter(|t| keep(t)).map(|t| SessionTab { url: t.url.clone(), title: t.title.clone() }).collect(),
            active: tabs.iter().take(active).filter(|t| keep(t)).count(),
        };
        // Jendela saat ini menjadi jendela utama saat Ovner dibuka lagi; jendela lain ikut dipulihkan.
        let main = one(&self.tabs, self.active);
        let s = Session {
            tabs: main.tabs,
            active: main.active,
            windows: self.others.iter().map(|w| one(&w.tabs, w.active)).filter(|w| !w.tabs.is_empty()).collect(),
        };
        if let Ok(text) = serde_json::to_string(&s) {
            let _ = std::fs::write(data_dir().join("session.json"), text);
        }
    }

    fn layout(&mut self) {
        let _ = self.toolbar.set_bounds(self.toolbar_rect());
        if let Some(sb) = &self.sidebar {
            let _ = sb.set_bounds(self.sidebar_rect());
        }
        for i in 0..self.tabs.len() {
            let r = self.rect_for(i);
            if let Some(wv) = &self.tabs[i].webview {
                let _ = wv.set_bounds(r);
            }
        }
    }

    /// Mulai split view: tab aktif di kiri, `partner` di kanan.
    fn start_split(&mut self, partner: usize) {
        if partner == self.active || partner >= self.tabs.len() {
            return;
        }
        self.split = Some(self.tabs[partner].id);
        let a = self.active;
        self.activate(a);
    }

    fn stop_split(&mut self) {
        if let Some(p) = self.split.take().and_then(|id| self.tab_index(id)) {
            if let Some(wv) = &self.tabs[p].webview {
                let _ = wv.set_visible(false);
            }
        }
        let a = self.active;
        self.activate(a);
    }

    fn inject_css(&self, i: usize, css: &str) {
        if css.is_empty() {
            return;
        }
        if let Some(wv) = &self.tabs[i].webview {
            let js = format!(
                "(function(c){{var s=document.getElementById('__ovner_sembunyi');if(!s){{s=document.createElement('style');\
                 s.id='__ovner_sembunyi';(document.head||document.documentElement).appendChild(s);}}s.textContent+=c;}})({});",
                serde_json::to_string(css).unwrap_or_default()
            );
            let _ = wv.evaluate_script(&js);
        }
    }

    fn blocking_active_for(&self, url: &str) -> bool {
        let s = self.settings.borrow();
        s.block_ads && s.cosmetic_filtering && !host_of(url).is_some_and(|h| s.is_exception(&h))
    }

    /// Kirim data ke halaman internal di tab `i`.
    fn push_page_state(&mut self, i: usize) {
        let Some(t) = self.tabs.get(i) else { return };
        let Some(name) = internal_page(&t.url).map(str::to_string) else { return };
        let (theme, lang) = (self.settings.borrow().theme.clone(), self.lang.code());
        let state = match name.as_str() {
            "settings" | "setup" => {
                let browsers: Vec<Value> = if name == "setup" || name == "settings" {
                    import::detect().into_iter().map(|(n, p)| json!({"name": n, "path": p.display().to_string()})).collect()
                } else {
                    Vec::new()
                };
                let catalog_view: Vec<Value> = {
                    let st = self.settings.borrow();
                    let sv = serde_json::to_value(&*st).unwrap_or_default();
                    catalog::CATALOG
                        .iter()
                        .map(|c| {
                            let on = match c.setting {
                                Some(k) => sv[k].as_bool().unwrap_or(false),
                                None => st.filter_lists.iter().any(|x| x.eq_ignore_ascii_case(c.id)),
                            };
                            json!({"id": c.id, "title": [c.title.0, c.title.1], "desc": c.desc, "on": on, "setting": c.setting, "regional": c.regional})
                        })
                        .collect()
                };
                json!({
                    "settings": &*self.settings.borrow(),
                    "lang": lang,
                    "version": update::display_version(),
                    "engine": wry::webview_version().unwrap_or_else(|_| "?".into()),
                    "lists": i18n::t(self.lang, if self.blocker.borrow().full { "d.lists_full" } else { "d.lists_builtin" }),
                    "data": data_dir().display().to_string(),
                    "browsers": browsers,
                    "catalog": catalog_view,
                })
            }
            "newtab" => json!({ "theme": theme, "lang": lang }),
            "downloads" => {
                let list: Vec<Value> = self.downloads.iter().rev().map(|d| self.download_view(d)).collect();
                json!({ "theme": theme, "lang": lang, "downloads": list })
            }
            "passwords" => {
                let s = self.settings.borrow();
                let logins: Vec<Value> = self
                    .vault
                    .logins()
                    .iter()
                    .map(|l| json!({"id": l.id, "site": l.site, "url": l.url, "user": l.username, "note": l.note, "used": l.used}))
                    .collect();
                let icons = self.store.icons_for(self.vault.logins().iter().map(|l| l.url.as_str()));
                json!({
                    "theme": theme, "lang": lang, "logins": logins, "icons": icons,
                    "settings": {"password_manager": s.password_manager, "password_autosignin": s.password_autosignin, "autofill": s.autofill},
                })
            }
            "history" => json!({ "theme": theme, "lang": lang, "history": self.store.search_history(&t.page_query, 500) }),
            "bookmarks" => {
                let icons = self.store.icons_for(self.store.bookmarks.iter().map(|b| b.url.as_str()));
                let browsers: Vec<Value> = import::detect().into_iter().map(|(n, p)| json!({"name": n, "path": p.display().to_string()})).collect();
                json!({ "theme": theme, "lang": lang, "bookmarks": self.store.bookmarks, "folders": self.store.all_folders(), "icons": icons, "browsers": browsers })
            }
            "tasks" => {
                let tabs: Vec<Value> = self
                    .tabs_with_shown()
                    .into_iter()
                    .map(|(t, shown)| {
                        json!({
                            "id": t.id, "title": t.title, "url": display_url(&t.url), "active": shown,
                            "sleeping": t.webview.is_none(), "suspended": t.suspended, "blocked": t.blocked.get(),
                            "zoom": t.zoom, "private": t.private, "js": t.js_heap, "cpu": t.tab_cpu,
                            "pid": t.proc_pid, "ram": t.proc_ram, "pcpu": t.proc_cpu, "shared": t.proc_shared,
                            "media": t.media_hidden_at.is_some() || t.webview.as_ref().is_some_and(playing_audio),
                        })
                    })
                    .collect();
                let procs: Vec<Value> = self
                    .stats
                    .iter()
                    .map(|p| {
                        // Seperti "WebView2: <judul>" di Task Manager Windows.
                        let mut titles: Vec<String> = self
                            .all_tabs()
                            .filter(|t| t.proc_pid == p.pid && p.pid != 0)
                            .map(|t| if t.title.is_empty() { display_url(&t.url) } else { t.title.clone() })
                            .collect();
                        // Proses toolbar/sidebar Ovner sendiri (di Task Manager Windows tampil
                        // sebagai "WebView2: about:blank").
                        if p.pid != 0 && self.ui_pids.contains(&p.pid) {
                            titles.insert(0, "Ovner (toolbar)".into());
                        }
                        json!({"pid": p.pid, "kind": p.kind, "working": p.working, "commit": p.commit, "cpu": p.cpu, "gpu": p.gpu, "gpuMem": p.gpu_mem, "tabs": titles})
                    })
                    .collect();
                let mapped = !self.renderer_frames.is_empty();
                json!({ "theme": theme, "lang": lang, "tabs": tabs, "procs": procs, "mapped": mapped, "mapOff": !self.frames_ok,
                        "stats": self.tab_stats })
            }
            "sessions" => {
                let open: Vec<Value> = self
                    .all_tabs()
                    .filter(|t| internal_page(&t.url).is_none() && !is_newtab(&t.url))
                    .map(|t| json!({"title": t.title, "url": t.url, "sleeping": t.webview.is_none()}))
                    .collect();
                let mut seen = HashSet::new();
                let dups = self.tabs.iter().filter(|t| !is_newtab(&t.url) && !seen.insert(t.url.trim_end_matches('/').to_string())).count();
                let urls = self.sessions.iter().flat_map(|x| x.tabs.iter().map(|t| t.url.as_str())).chain(self.tabs.iter().map(|t| t.url.as_str()));
                json!({ "theme": theme, "lang": lang, "sessions": self.sessions, "open": open, "dups": dups,
                        "closed": self.closed.iter().rev().take(15).collect::<Vec<_>>(), "stats": self.tab_stats,
                        "icons": self.store.icons_for(urls) })
            }
            "diagnostics" => {
                let v = self.diagnostics_report();
                json!({ "theme": theme, "lang": lang, "report": v })
            }
            _ => return,
        };
        if let Some(wv) = &self.tabs[i].webview {
            post_page(wv, "page", &state.to_string(), "window.ovnerPage && ovnerPage");
        }
    }

    /// Pengukuran berkala (dipanggil tiap tick): atur pemantau latar, minta metrik JavaScript
    /// tab yang tampil lewat DevTools, dan (Windows) peta proses halaman → tab.
    fn sample_usage(&mut self) {
        let tasks = self.tasks_open();
        let (app_on, tab_on) = {
            let s = self.settings.borrow();
            (s.usage_overlay_app, s.usage_overlay_tab)
        };
        // Semua jendela tersembunyi/diminimalkan: indikator tidak terlihat, pemantau proses diam.
        let hidden = self.all_background() && !tasks;
        let mode = if hidden { 0 } else if tasks || app_on { 2 } else if tab_on { 1 } else { 0 };
        usage::set_monitor(mode);
        if mode == 0 {
            return;
        }
        // Pengguna diam lebih dari 1 menit (tanpa mouse/keyboard): ukur tiap 3 detik saja.
        let idle = win::idle_secs() >= 60 && !tasks;
        usage::set_monitor_every(if idle { 3000 } else { 1000 });
        if self.stats_at.elapsed() < Duration::from_millis(if idle { 2900 } else { 900 }) {
            return;
        }
        self.stats_at = Instant::now();
        // Jendela yang diminimalkan/di tray tidak diukur kecuali Task Manager terbuka.
        self.each_window(|a| {
            if a.background && !tasks {
                return;
            }
            for j in 0..a.tabs.len() {
                let shown = a.is_shown(j);
                let t = &mut a.tabs[j];
                let Some(wv) = &t.webview else { continue };
                if t.suspended || (!shown && !tasks) {
                    continue;
                }
                if !t.perf_on {
                    win::cdp(wv, "Performance.enable", "{}");
                    t.perf_on = true;
                }
                let (p, id) = (a.proxy.clone(), t.id);
                win::cdp_call(wv, "Performance.getMetrics", "{}", move |r| send(&p, UserEvent::TabMetrics(id, r)));
            }
        });
        self.request_renderer_frames(tasks || tab_on);
    }

    /// Minta daftar proses halaman WebView2 beserta tabnya (paling sering tiap 3 detik).
    #[allow(unused_variables)]
    fn request_renderer_frames(&mut self, wanted: bool) {
        #[cfg(windows)]
        {
            if !wanted || !self.frames_ok || self.frames_pending || self.frames_at.elapsed() < Duration::from_secs(3) {
                if self.frames_pending && self.frames_at.elapsed() > Duration::from_secs(20) {
                    self.frames_pending = false;
                }
                return;
            }
            let Some(env) = self.env.clone() else { return };
            for t in self.tabs.iter_mut().chain(self.others.iter_mut().flat_map(|w| w.tabs.iter_mut())) {
                if t.frame_id.is_none() {
                    if let Some(wv) = &t.webview {
                        t.frame_id = win::main_frame_id(wv);
                    }
                }
            }
            let p = self.proxy.clone();
            self.frames_at = Instant::now();
            self.frames_pending =
                win::request_renderer_frames(&env, &data_dir().join(".proses-tab"), move |v| send(&p, UserEvent::RendererFrames(v)));
        }
    }

    /// Pasangkan sampel proses dengan tab: RAM dan CPU proses halaman tiap tab, seperti baris
    /// "WebView2: <judul>" di Task Manager Windows.
    fn assign_tab_procs(&mut self) {
        let by_pid: HashMap<u32, (u64, f64)> = self.stats.iter().map(|p| (p.pid, (p.working, p.cpu))).collect();
        let mut owner: HashMap<u32, u32> = HashMap::new();
        for (pid, frames) in &self.renderer_frames {
            for f in frames {
                owner.insert(*f, *pid);
            }
        }
        // Proses toolbar dan sidebar Ovner.
        if self.ui_frame.is_none() {
            self.ui_frame = win::main_frame_id(&self.toolbar);
        }
        self.ui_pids = self.ui_frame.and_then(|f| owner.get(&f).copied()).into_iter().collect();
        let mut shared: HashMap<u32, usize> = HashMap::new();
        for t in self.all_tabs() {
            if let Some(pid) = t.frame_id.and_then(|f| owner.get(&f)).filter(|_| t.webview.is_some()) {
                *shared.entry(*pid).or_default() += 1;
            }
        }
        for t in self.tabs.iter_mut().chain(self.others.iter_mut().flat_map(|w| w.tabs.iter_mut())) {
            let pid = t.frame_id.and_then(|f| owner.get(&f).copied()).filter(|_| t.webview.is_some());
            match pid {
                Some(pid) => {
                    let (ram, cpu) = by_pid.get(&pid).copied().unwrap_or((0, 0.0));
                    t.proc_pid = pid;
                    t.proc_ram = ram;
                    t.proc_cpu = cpu;
                    t.proc_shared = shared.get(&pid).copied().unwrap_or(1);
                }
                None => {
                    t.proc_pid = 0;
                    t.proc_ram = 0;
                    t.proc_cpu = 0.0;
                    t.proc_shared = 0;
                }
            }
        }
    }

    /// Kirim angka indikator pemakaian ke toolbar (terpisah dari render supaya ringan).
    fn push_usage(&self) {
        let v = {
            let s = self.settings.borrow();
            self.usage_view(&s)
        };
        self.update_caption(&v);
        let vt = v.to_string();
        post_page(&self.toolbar, "usage", &vt, "window.ovner && ovner.usage && ovner.usage");
        if let Some(sb) = &self.sidebar {
            post_page(sb, "usage", &vt, "window.ovner && ovner.usage && ovner.usage");
        }
    }

    /// Pekerjaan berkala (tiap 2 detik): tidurkan tab, rapikan RAM, ukur pemakaian, dsb.
    fn on_tick(&mut self) {
        let sleep_due = self.last_sleep_check.elapsed() >= SLEEP_CHECK;
        if sleep_due {
            self.last_sleep_check = Instant::now();
        }
        self.promote_waiting();
        self.start_due_races();
        self.decide_races();
        self.refresh_eco();
        if !self.dl_yield.is_empty() || self.downloads.iter().any(|d| d.state == "progress" && self.dl_meta.get(&d.id).is_some_and(|m| m.pdf)) {
            self.update_priority();
        }
        if let Some(due) = self.next_trim.filter(|_| self.downloading()) {
            // Jangan kosongkan working set proses jaringan selagi mengunduh: tunda.
            if Instant::now() >= due {
                self.next_trim = Some(Instant::now() + Duration::from_secs(30));
            }
        }
        if let Some(due) = self.next_trim.filter(|_| !self.downloading()) {
            if Instant::now() >= due {
                self.trim_now();
                let every = self.settings.borrow().trim_every_minutes;
                self.next_trim = if self.all_background() && every > 0 { Some(Instant::now() + Duration::from_secs(every as u64 * 60)) } else { None };
            }
        }
        self.sample_usage();
        if self.icons_due.is_some_and(|d| Instant::now() >= d) {
            self.icons_due = None;
            self.fetch_missing_icons();
        }
        if self.settings.borrow().diagnostics && self.last_diag.elapsed() >= DIAG_EVERY {
            self.last_diag = Instant::now();
            if self.stats.is_empty() || self.stats_at.elapsed() > Duration::from_secs(10) {
                self.stats = self.sampler.sample(false);
            }
            let t = usage::total(&self.stats);
            // Jenis proses beserta RAM-nya (mis. "ovner 40MB"), supaya terlihat siapa yang membengkak.
            let kinds: Vec<String> = self.stats.iter().map(|p| format!("{} {}MB", p.kind, p.working / 1_048_576)).collect();
            log(&format!(
                "RAM: {} MB, CPU {:.1}%, {} proses [{}], {} tab ({} hidup)",
                t.working / 1_048_576,
                t.cpu,
                t.procs,
                kinds.join(","),
                self.all_tabs().count(),
                self.all_tabs().filter(|t| t.webview.is_some()).count()
            ));
        }
        self.store.flush();
        // Pekerjaan per jendela.
        self.each_window(|a| {
            if sleep_due {
                a.sleep_idle_tabs();
            }
            a.suspend_minimized();
            a.check_background_media();
            a.render();
        });
        if self.eco_at.elapsed() >= Duration::from_secs(10) {
            self.eco_at = Instant::now();
            self.update_renderer_eco();
        }
    }

    /// Jarak tick: 1 detik saat dipakai; 5 detik saat semua jendela tersembunyi dan tidak ada
    /// unduhan, supaya Ovner hampir tidak memakai CPU di tray/diminimalkan.
    fn tick_every(&self) -> Duration {
        let busy = self.downloading() || !self.pdf_race.is_empty() || !self.dl_waiting.is_empty() || !self.dl_jobs.is_empty() || !self.video_jobs.is_empty();
        if self.all_background() && !busy {
            Duration::from_secs(5)
        } else {
            TICK
        }
    }

    fn tasks_open(&self) -> bool {
        self.all_tabs().any(|t| internal_page(&t.url) == Some("tasks") && t.webview.is_some())
    }

    /// Hentikan media di tab yang ditinggal lebih lama dari setelan "berhenti setelah N menit".
    fn check_background_media(&mut self) {
        let (policy, mins) = {
            let s = self.settings.borrow();
            (s.background_media.clone(), s.background_media_minutes)
        };
        let limit = Duration::from_secs(mins as u64 * 60);
        let suspend = self.settings.borrow().suspend_hidden_tabs;
        for j in 0..self.tabs.len() {
            if self.is_shown(j) && !self.in_tray {
                self.tabs[j].media_hidden_at = None;
                continue;
            }
            let t = &mut self.tabs[j];
            let Some(since) = t.media_hidden_at else { continue };
            if policy == "timer" && since.elapsed() < limit {
                continue;
            }
            if policy == "keep" {
                continue;
            }
            t.media_hidden_at = None;
            if let Some(wv) = &t.webview {
                pause_media(wv);
                if suspend && t.downloads_live == 0 {
                    win::try_suspend(wv);
                    t.suspended = true;
                }
            }
        }
    }

    /// Indikator pemakaian di bilah judul Windows, di kiri tombol minimize (Windows saja).
    fn update_caption(&self, v: &Value) {
        let mut parts: Vec<(String, bool)> = Vec::new();
        if !self.in_tray && !self.window.is_minimized() && self.window.fullscreen().is_none() {
            let mb = |b: u64| format!("{} MB", b / 1_048_576);
            let pct = |c: f64| if c >= 10.0 { format!("{c:.0}%") } else { format!("{c:.1}%") };
            let n = |x: &Value| x.as_f64().unwrap_or(0.0);
            if let Some(a) = v["app"].as_object() {
                parts.push(("Ovner ".into(), false));
                parts.push((mb(n(&a["ram"]) as u64), true));
                parts.push(("  ·  CPU ".into(), false));
                parts.push((pct(n(&a["cpu"])), true));
                parts.push(("  ·  GPU ".into(), false));
                parts.push((pct(n(&a["gpu"])), true));
            }
            if let Some(t) = v["tab"].as_object() {
                let tab = if self.lang.code() == "en" { "Tab" } else { "Tab" };
                parts.push((if parts.is_empty() { format!("{tab} ") } else { format!("     |     {tab} ") }, false));
                let ram = n(&t["ram"]) as u64;
                parts.push((if ram > 0 { mb(ram) } else { "—".into() }, true));
                if t["proc"] != json!(true) {
                    parts.push((" JS".into(), false));
                }
                parts.push(("  ·  CPU ".into(), false));
                parts.push((pct(n(&t["cpu"])), true));
            }
        }
        let theme = self.settings.borrow().theme.clone();
        let dark = match theme.as_str() {
            "dark" => true,
            "light" => false,
            _ => win::system_dark(),
        };
        #[cfg(windows)]
        {
            use tao::platform::windows::WindowExtWindows;
            win::caption_text(self.window.hwnd() as isize, &parts, dark, self.window.scale_factor());
        }
        #[cfg(not(windows))]
        let _ = (parts, dark);
    }

    /// Angka untuk indikator pemakaian di toolbar.
    fn usage_view(&self, s: &Settings) -> Value {
        if !(s.usage_overlay_app || s.usage_overlay_tab) || self.stats.is_empty() {
            return Value::Null;
        }
        let tot = usage::total(&self.stats);
        let app = s.usage_overlay_app.then(|| json!({"ram": tot.working, "cpu": tot.cpu, "gpu": tot.gpu, "procs": tot.procs}));
        let tab = s
            .usage_overlay_tab
            .then(|| {
                self.tabs.get(self.active).filter(|t| t.webview.is_some()).map(|t| {
                    // RAM proses halaman (seperti Task Manager) bila diketahui; selain itu RAM JS.
                    let (ram, cpu, proc_) = if t.proc_pid != 0 { (t.proc_ram, t.proc_cpu, true) } else { (t.js_heap, t.tab_cpu, false) };
                    json!({"ram": ram, "cpu": cpu, "proc": proc_, "shared": t.proc_shared, "js": t.js_heap})
                })
            })
            .flatten();
        // Windows: angka tampil di bilah judul (dekat tombol minimize), bukan di baris tab.
        json!({"app": app, "tab": tab, "title": cfg!(windows)})
    }

    /// Laporan lengkap untuk halaman diagnostik dan "Salin laporan".
    fn diagnostics_report(&self) -> Value {
        let s = self.settings.borrow();
        let dir = data_dir();
        let lists: Vec<Value> = std::fs::read_dir(dir.join("filters"))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let m = e.metadata().ok()?;
                let age = m.modified().ok().and_then(|t| t.elapsed().ok()).map(|d| d.as_secs() / 3600).unwrap_or(0);
                Some(json!({"name": e.file_name().to_string_lossy(), "kb": m.len() / 1024, "ageHours": age}))
            })
            .collect();
        let resources = blocker::load_resources(dir).len();
        let blocked = self.blocker.borrow().check("https://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js", "https://contoh.id/", "script").block;
        let vault_ok = vault::self_test();
        let log_tail: Vec<String> = std::fs::read_to_string(dir.join("ovner.log"))
            .unwrap_or_default()
            .lines()
            .rev()
            .take(150)
            .map(str::to_string)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let crashes: Vec<&String> = log_tail.iter().filter(|l| l.contains("CRASH") || l.contains("PANIC") || l.contains("gagal")).collect();
        let total = usage::total(&self.stats);
        let exe = std::env::current_exe().unwrap_or_default();
        json!({
            "version": update::display_version(),
            "channel": s.update_channel,
            "engine": wry::webview_version().unwrap_or_else(|_| "?".into()),
            "engineLatest": versions::cached_latest(&dir).unwrap_or_default(),
            "components": versions::COMPONENTS,
            "os": win::os_version(),
            "exe": exe.display().to_string(),
            "data": dir.display().to_string(),
            "portable": install::is_portable(&exe, false),
            "envCustom": self.env_error.is_none(),
            "envError": self.env_error,
            "args": self.browser_args,
            "preset": s.preset, "gpuMode": s.gpu_mode, "dns": s.secure_dns, "security": s.security_level,
            "antiblock": s.antiblock, "antiblockPort": self.antiblock_port,
            "tabProcMap": self.frames_ok, "tabProcMapped": self.renderer_frames.len(),
            "dark": format!("{}/{}", s.dark_mode, s.dark_engine), "dlEngine": s.dl_engine, "dlJobs": self.dl_jobs.len(),
            "antiblockConns": antiblock::CONNECTIONS.load(std::sync::atomic::Ordering::Relaxed),
            "dohOk": antiblock::DOH_OK.load(std::sync::atomic::Ordering::Relaxed),
            "dohFail": antiblock::DOH_FAIL.load(std::sync::atomic::Ordering::Relaxed),
            "blockerFull": self.blocker.borrow().full, "blockerTest": blocked, "lists": lists, "resources": resources,
            "vault": vault_ok, "bookmarks": self.store.bookmarks.len(), "icons": self.store.icon_count(),
            "tabs": self.all_tabs().count(), "tabsLive": self.all_tabs().filter(|t| t.webview.is_some()).count(),
            "windows": self.window_count(),
            "procs": total.procs, "ram": total.working, "cpu": total.cpu, "gpu": total.gpu, "gpuMem": total.gpu_mem,
            "kinds": self.stats.iter().map(|p| format!("{}:{}", p.kind, p.pid)).collect::<Vec<_>>(),
            "tests": self.diag.iter().map(|(n, ok, m)| json!({"name": n, "ok": ok, "msg": m})).collect::<Vec<_>>(),
            "crashes": crashes,
            "log": log_tail,
        })
    }

    /// Tombol di Pengaturan → Sistem dan halaman Diagnostik. Mengembalikan true bila Ovner keluar.
    fn page_action(&mut self, i: usize, name: &str) -> bool {
        let exe = std::env::current_exe().unwrap_or_default();
        match name {
            "open-app-folder" => win::open_folder(exe.parent().unwrap_or(Path::new("."))),
            "open-data-folder" => win::open_folder(data_dir()),
            "open-log" => {
                let p = data_dir().join("ovner.log");
                win::open_path(&p.display().to_string(), false);
            }
            "diagnostics" => self.open_internal("diagnostics"),
            "update-filters" => {
                start_filter_update_forced(self.proxy.clone(), self.settings.borrow().clone());
                win::info(t(self.lang, "d.filters"));
            }
            "run-tests" => {
                self.run_diag_tests();
                self.push_page_state(i);
            }
            "copy-report" => {
                let report = serde_json::to_string_pretty(&self.diagnostics_report()).unwrap_or_default();
                win::set_clipboard(&report);
                win::info(t(self.lang, "d.report_copied"));
            }
            "uninstall" => {
                if !win::confirm(t(self.lang, "d.uninstall")) {
                    return false;
                }
                let portable = install::is_portable(&exe, false);
                let _ = win::set_autostart(false, &exe);
                install::remove_integration();
                if install::is_installed_copy(&exe) {
                    win::delete_dirs_later(&[data_dir(), &install::install_dir()]);
                    win::info(t(self.lang, "d.uninstalled"));
                } else {
                    let _ = portable;
                    win::delete_dirs_later(&[data_dir()]);
                    win::info(t(self.lang, "d.uninstalled_portable"));
                }
                log("dihapus dari pengaturan");
                return true;
            }
            _ => {}
        }
        false
    }

    /// Unduh ikon situs untuk bookmark yang belum punya ikon (sekali per sesi per situs).
    fn fetch_missing_icons(&mut self) {
        let hosts: Vec<String> = self.store.hosts_without_icon().into_iter().filter(|h| !self.icons_tried.contains(h)).take(150).collect();
        if hosts.is_empty() {
            return;
        }
        self.icons_tried.extend(hosts.iter().cloned());
        let p = self.proxy.clone();
        std::thread::spawn(move || {
            let tmp = std::env::temp_dir().join(format!("ovner-ikon-{}.bin", std::process::id()));
            for host in hosts {
                for url in [format!("https://{host}/favicon.ico"), format!("https://icons.duckduckgo.com/ip3/{host}.ico")] {
                    let _ = std::fs::remove_file(&tmp);
                    if win::download(&url, &tmp).is_ok() {
                        let bytes = std::fs::read(&tmp).unwrap_or_default();
                        if store::icon_mime(&bytes).is_some() && bytes.len() <= 64 * 1024 {
                            send(&p, UserEvent::IconFetched(host.clone(), bytes));
                            break;
                        }
                    }
                }
            }
            let _ = std::fs::remove_file(&tmp);
        });
    }

    /// Uji yang butuh jaringan dijalankan di latar.
    fn run_diag_tests(&mut self) {
        self.diag.clear();
        let p = self.proxy.clone();
        let lang_id = self.lang == Lang::Id;
        std::thread::spawn(move || {
            let (ok, msg) = antiblock::self_test();
            send(&p, UserEvent::DiagResult("doh".into(), ok, msg));
            let tmp = std::env::temp_dir().join("ovner-uji-unduh.txt");
            let r = win::download("https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/quick-fixes.txt", &tmp);
            let size = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
            let _ = std::fs::remove_file(&tmp);
            send(&p, UserEvent::DiagResult("download".into(), r.is_ok() && size > 1000, format!("{r:?}, {size} byte")));
            let up = update::latest_url("stable");
            let tmp2 = std::env::temp_dir().join("ovner-uji-update.json");
            let r2 = win::download(&up, &tmp2);
            let body = std::fs::read_to_string(&tmp2).unwrap_or_default();
            let _ = std::fs::remove_file(&tmp2);
            send(&p, UserEvent::DiagResult("update".into(), r2.is_ok() && body.contains("version"), body.chars().take(120).collect()));
            // WebView2 terbaru dari API resmi Microsoft dibanding yang terpasang.
            let installed = wry::webview_version().unwrap_or_default();
            let (ok, msg) = match versions::refresh_latest(&data_dir(), None, win::download) {
                Ok(latest) => versions::status(&installed, &latest, lang_id),
                Err(e) => (false, format!("{e} ({})", versions::EDGE_API)),
            };
            send(&p, UserEvent::DiagResult("webview2".into(), ok, msg));
        });
    }

    /// Segarkan semua tab yang menampilkan halaman internal `name`.
    fn refresh_pages(&mut self, name: &str) {
        if !self.all_tabs().any(|t| internal_page(&t.url) == Some(name)) {
            return;
        }
        self.each_window(|a| {
            for i in 0..a.tabs.len() {
                if internal_page(&a.tabs[i].url) == Some(name) {
                    a.push_page_state(i);
                }
            }
        });
    }

    fn zoom_active(&mut self, dir: i32) {
        let i = self.active;
        let Some(t) = self.tabs.get_mut(i) else { return };
        let cur = t.zoom;
        let z = if dir == 0 {
            100
        } else {
            let pos = ZOOM_STEPS.iter().position(|&z| z >= cur).unwrap_or(7);
            let next = (pos as i32 + dir).clamp(0, ZOOM_STEPS.len() as i32 - 1) as usize;
            ZOOM_STEPS[next]
        };
        t.zoom = z;
        if let Some(wv) = &t.webview {
            let _ = wv.zoom(z as f64 / 100.0);
        }
    }

    fn apply_setting_side_effects(&mut self, key: &str) {
        let s = self.settings.borrow().clone();
        match key {
            "autostart" => {
                let exe = if install::is_installed_copy(&std::env::current_exe().unwrap_or_default()) {
                    install::installed_exe()
                } else {
                    std::env::current_exe().unwrap_or_default()
                };
                if let Err(e) = win::set_autostart(s.autostart, &exe) {
                    log(&format!("autostart: {e}"));
                }
            }
            "theme" => {
                #[cfg(windows)]
                {
                    use wry::WebViewExtWindows;
                    let th = wry_theme(&s.theme);
                    self.each_window(|a| {
                        let _ = a.toolbar.set_theme(th);
                        if let Some(sb) = &a.sidebar {
                            let _ = sb.set_theme(th);
                        }
                    });
                    for t in self.all_tabs() {
                        if let Some(wv) = &t.webview {
                            let _ = wv.set_theme(th);
                        }
                    }
                }
                let theme = s.theme.clone();
                self.each_window(|a| {
                    a.window.set_theme(match theme.as_str() {
                        "light" => Some(tao::window::Theme::Light),
                        "dark" => Some(tao::window::Theme::Dark),
                        _ => None,
                    })
                });
            }
            "dark_mode" | "dark_engine" | "dark_brightness" | "dark_contrast" | "dark_sepia" | "dark_grayscale" | "dark_on_sites" | "dark_off_sites" => {
                self.apply_dark_live();
            }
            "block_ads" | "exceptions" | "cosmetic_filtering" => {
                if let Some(wv) = self.active_webview() {
                    let _ = wv.reload();
                }
            }
            "language" => {
                self.lang = i18n::resolve(&s.language);
                self.menus = build_menus(self.lang);
                if let Some(tray) = &self._tray {
                    let _ = tray.set_menu(Some(Box::new(self.menus.tray.clone())));
                }
                for name in ["settings", "history", "bookmarks", "tasks", "setup"] {
                    self.refresh_pages(name);
                }
                self.each_window(|a| {
                    a.last_render_key.clear();
                    a.render();
                });
            }
            "custom_rules" => {
                if blocker::write_custom_rules(data_dir(), &s.custom_rules) {
                    start_filter_update(self.proxy.clone(), s.clone());
                }
            }
            "block_cookie_notices" | "custom_lists" | "filter_lists" => start_filter_update(self.proxy.clone(), s.clone()),
            // Skrip halaman dibuat saat tab dibangun: tandai semua tab untuk dibangun ulang.
            "autofill" | "password_manager" | "password_autosignin" => {
                for t in self.all_tabs() {
                    if let Some(wv) = &t.webview {
                        win::tune_webview(wv, &s);
                    }
                }
                self.refresh_pages("passwords");
            }
            "usage_overlay_app" | "usage_overlay_tab" => {
                self.stats_at = Instant::now() - Duration::from_secs(5);
                self.sample_usage();
                self.push_usage();
            }
            "letterboxing" | "vertical_tabs" => {
                self.each_window(|a| {
                    a.ensure_sidebar();
                    a.layout();
                    a.last_render_key.clear();
                    a.render();
                });
            }
            "block_annoyances" | "strip_tracking_params" | "filter_update_hours" => start_filter_update(self.proxy.clone(), s.clone()),
            "security_level" | "script_allowed" => {
                if let Some(wv) = self.active_webview() {
                    let _ = wv.reload();
                }
            }
            "farbling" | "farbling_off" | "spellcheck" | "disable_beforeunload" | "de_amp" | "user_css" | "hide_webview2" | "uniform_fingerprint"
            | "uniform_utc" | "youtube_h264" | "referrer_policy" | "gpu_mode" => {
                self.script_gen += 1;
                self.each_window(|a| a.rebuild_shown());
            }
            _ => {}
        }
        self.last_render_key.clear();
        self.render();
    }

    /// Tambah aturan "sembunyikan elemen" dari pemilih elemen untuk situs tab `i`.
    fn add_hide_rule(&mut self, i: usize, selector: &str) {
        let Some(host) = host_of(&self.tabs[i].url) else { return };
        let host = settings::norm_host(&host);
        let sel = selector.trim();
        if sel.is_empty() || sel.contains('\n') || sel.len() > 500 {
            return;
        }
        let rule = format!("{host}##{sel}");
        let rules = {
            let mut st = self.settings.borrow_mut();
            if !st.custom_rules.lines().any(|l| l.trim() == rule) {
                if !st.custom_rules.is_empty() && !st.custom_rules.ends_with('\n') {
                    st.custom_rules.push('\n');
                }
                st.custom_rules.push_str(&rule);
            }
            st.save(data_dir());
            st.custom_rules.clone()
        };
        log(&format!("aturan sendiri: {rule}"));
        if blocker::write_custom_rules(data_dir(), &rules) {
            start_filter_update(self.proxy.clone(), self.settings.borrow().clone());
        }
        self.refresh_pages("settings");
    }

    /// Host (tanpa www.) tab aktif bila halaman web biasa.
    fn active_site(&self) -> Option<String> {
        self.tabs.get(self.active).filter(|t| internal_page(&t.url).is_none()).and_then(|t| host_of(&t.url)).map(|h| settings::norm_host(&h))
    }

    /// Mode gelap situs: "on", "off", atau "default" (ikut setelan umum). Langsung berlaku.
    fn set_dark_site(&mut self, host: &str, value: &str) {
        {
            let mut st = self.settings.borrow_mut();
            st.dark_on_sites.retain(|h| h != host);
            st.dark_off_sites.retain(|h| h != host);
            let default_on = match st.dark_mode.as_str() {
                "on" => true,
                "system" => shields::system_dark(),
                _ => false,
            };
            match value {
                "on" if !default_on => st.dark_on_sites.push(host.to_string()),
                "off" if default_on => st.dark_off_sites.push(host.to_string()),
                _ => {}
            }
            st.save(data_dir());
        }
        self.apply_dark_live();
        self.refresh_pages("settings");
        self.last_render_key.clear();
        self.render();
    }

    /// Terapkan mode gelap ke semua tab yang hidup tanpa memuat ulang.
    fn apply_dark_live(&mut self) {
        let s = self.settings.borrow().clone();
        for t in self.all_tabs() {
            let Some(wv) = &t.webview else { continue };
            if internal_page(&t.url).is_some() || t.suspended {
                continue;
            }
            let host = host_of(&t.url).map(|h| settings::norm_host(&h)).unwrap_or_default();
            let engine = s.dark_for(&host, shields::system_dark());
            let (m, p) = shields::dark_cdp(&s, &host);
            win::cdp(wv, m, p);
            let _ = wv.evaluate_script(&shields::dark_live_script(&s, engine.filter(|e| *e != "chromium")));
        }
    }

    /// Hapus cookie, sesi, cache, dan penyimpanan situs ini (semua subdomainnya), lalu muat
    /// ulang tab yang membukanya. Untuk aplikasi yang macet karena sesi lama rusak.
    fn clear_site_data(&mut self, host: &str) {
        let base = netfilter_base_domain(host);
        let mut origins: Vec<String> = Vec::new();
        for t in self.all_tabs() {
            if let Some(h) = host_of(&t.url) {
                if netfilter_base_domain(&h) == base {
                    for scheme in ["https", "http"] {
                        let o = format!("{scheme}://{h}");
                        if !origins.contains(&o) {
                            origins.push(o);
                        }
                    }
                }
            }
        }
        for o in [format!("https://{base}"), format!("https://www.{base}"), format!("https://{host}")] {
            if !origins.contains(&o) {
                origins.push(o);
            }
        }
        let Some(wv) = self.all_tabs().find_map(|t| t.webview.as_ref()) else { return };
        for o in &origins {
            let p = json!({"origin": o, "storageTypes": "all"}).to_string();
            win::cdp(wv, "Storage.clearDataForOrigin", &p);
        }
        // Cookie domain induk (.bpjsketenagakerjaan.go.id) ikut terhapus lewat daftar cookie.
        let p = json!({"urls": origins}).to_string();
        let proxy = self.proxy.clone();
        let b2 = base.clone();
        win::cdp_call(wv, "Network.getCookies", &p, move |r| send(&proxy, UserEvent::DeleteCookies(b2, r)));
        log(&format!("data situs dihapus: {base} ({} origin)", origins.len()));
        #[cfg(not(windows))]
        self.reload_site(&base);
    }

    fn reload_site(&self, base: &str) {
        for t in self.all_tabs() {
            if let (Some(w), Some(h)) = (&t.webview, host_of(&t.url)) {
                if netfilter_base_domain(&h) == base {
                    let _ = w.reload();
                }
            }
        }
    }

    /// Simpan satu kejadian DevTools yang menarik untuk laporan masalah halaman.
    fn on_problem(&mut self, id: u64, name: &str, j: &str) {
        let Some(i) = self.tab_index(id) else { return };
        if !self.tabs[i].recording || self.problems.len() >= 3000 {
            return;
        }
        if name == "ovner" {
            let secs = self.record_at.elapsed().as_secs_f32();
            self.problems.push(format!("+{secs:.1}s {}", j.chars().take(600).collect::<String>()));
            return;
        }
        let v: Value = serde_json::from_str(j).unwrap_or_default();
        // Isu DevTools yang hanya saran (label formulir, cookie lambat, quirks, API usang) tidak
        // membantu mencari penyebab halaman rusak dan membanjiri laporan.
        if name == "Audits.issueAdded" {
            let d = v["issue"].to_string();
            if ["FormLabel", "FormEmptyIdAndName", "FormAutocomplete", "DocumentCookie", "QuirksModeIssue", "DeprecationIssue", "NavigationEntryMarkedSkippable", "FormInputWithNoLabel", "FormDuplicateId"].iter().any(|k| d.contains(k)) {
                return;
            }
        }
        let cut = |s: &str, n: usize| s.chars().take(n).collect::<String>();
        let line = match name {
            "Runtime.exceptionThrown" => {
                let d = &v["exceptionDetails"];
                let msg = d["exception"]["description"].as_str().or(d["text"].as_str()).unwrap_or("");
                format!("JS-ERROR {} @ {}:{}", cut(msg, 400), d["url"].as_str().unwrap_or(""), d["lineNumber"])
            }
            "Runtime.consoleAPICalled" => {
                let kind = v["type"].as_str().unwrap_or("");
                if kind != "error" && kind != "warning" {
                    return;
                }
                let args: Vec<String> = v["args"].as_array().into_iter().flatten().map(|a| a["value"].as_str().map(str::to_string).unwrap_or_else(|| a["description"].as_str().unwrap_or("").to_string())).collect();
                format!("CONSOLE-{} {}", kind.to_uppercase(), cut(&args.join(" "), 400))
            }
            "Log.entryAdded" => {
                let e = &v["entry"];
                if e["level"] == "verbose" || e["level"] == "info" {
                    return;
                }
                format!("LOG-{} [{}] {} {}", e["level"].as_str().unwrap_or(""), e["source"].as_str().unwrap_or(""), cut(e["text"].as_str().unwrap_or(""), 400), e["url"].as_str().unwrap_or(""))
            }
            "Network.loadingFailed" => format!(
                "NET-FAIL {} {} blocked={} cors={} canceled={}",
                v["type"].as_str().unwrap_or(""),
                v["errorText"].as_str().unwrap_or(""),
                v["blockedReason"],
                v["corsErrorStatus"]["corsError"],
                v["canceled"]
            ),
            "Network.responseReceived" => {
                let r = &v["response"];
                let status = r["status"].as_i64().unwrap_or(0);
                let doc = v["type"] == "Document";
                if status < 400 && !doc {
                    return;
                }
                format!("HTTP-{status} {} {}", v["type"].as_str().unwrap_or(""), cut(r["url"].as_str().unwrap_or(""), 300))
            }
            // Halaman (dokumen): metode, alamat, dan pengalihan (302 → Location).
            "Network.requestWillBeSent" => {
                if v["type"] != "Document" {
                    return;
                }
                let req = &v["request"];
                let redir = &v["redirectResponse"];
                let from = if redir.is_object() { format!(" (dialihkan dari HTTP-{} {})", redir["status"], cut(redir["url"].as_str().unwrap_or(""), 200)) } else { String::new() };
                format!("DOC-REQ {} {}{from}", req["method"].as_str().unwrap_or(""), cut(req["url"].as_str().unwrap_or(""), 300))
            }
            // Cookie yang ditolak mesin (penyebab umum login/logout macet).
            "Network.responseReceivedExtraInfo" => {
                let blocked: Vec<String> = v["blockedCookies"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|c| format!("{}={:?}", c["cookie"]["name"].as_str().or(c["cookieLine"].as_str().map(|l| l.split('=').next().unwrap_or(""))).unwrap_or("?"), c["blockedReasons"]))
                    .collect();
                if blocked.is_empty() {
                    return;
                }
                format!("COOKIE-BLOCKED {}", cut(&blocked.join("; "), 500))
            }
            "Audits.issueAdded" => format!("ISSUE {}", cut(&v["issue"].to_string(), 600)),
            _ => return,
        };
        // Permintaan gagal tidak membawa alamat: cari dari requestId di baris lain tidak perlu,
        // cukup sertakan requestId untuk dicocokkan.
        let line = if name == "Network.loadingFailed" { format!("{line} req={}", v["requestId"].as_str().unwrap_or("")) } else { line };
        // Waktu sejak mulai merekam, supaya urutan kejadian jelas.
        let secs = self.record_at.elapsed().as_secs_f32();
        self.problems.push(format!("+{secs:.1}s {line}"));
    }

    /// Tutup tab yang alamatnya sama (seperti Duplicate Cleaner TabWars): yang aktif tetap.
    fn close_duplicates(&mut self) {
        let mut seen: HashSet<String> = HashSet::new();
        if let Some(t) = self.tabs.get(self.active) {
            seen.insert(t.url.trim_end_matches('/').to_string());
        }
        let mut closed = 0;
        let mut j = 0;
        while j < self.tabs.len() {
            let key = self.tabs[j].url.trim_end_matches('/').to_string();
            if j != self.active && !is_newtab(&key) && !seen.insert(key) {
                self.close_tab(j);
                closed += 1;
                continue;
            }
            j += 1;
        }
        if closed > 0 {
            self.tab_stats.duplicates += closed;
            self.save_tab_stats();
        }
        log(&format!("tab duplikat ditutup: {closed}"));
    }

    /// Ubah salah satu daftar per situs (skrip/sidik jari) untuk tab aktif, lalu muat ulang.
    fn toggle_site(&mut self, which: &str) {
        let Some(host) = self.tabs.get(self.active).filter(|t| internal_page(&t.url).is_none()).and_then(|t| host_of(&t.url)) else { return };
        {
            let mut st = self.settings.borrow_mut();
            let list = if which == "scripts" { &mut st.script_blocked } else { &mut st.farbling_off };
            toggle_host(list, &host);
            st.save(data_dir());
        }
        if which == "fp" {
            self.script_gen += 1;
            let a = self.active;
            self.rebuild_tab(a);
        } else if let Some(wv) = self.active_webview() {
            // Navigation handler mengatur JavaScript lalu memuat ulang dengan setelan baru.
            let _ = wv.reload();
        }
        self.refresh_pages("settings");
        self.last_render_key.clear();
        self.render();
    }

    /// Seret-dan-lepas bookmark/folder (bilah bookmark, menu folder, halaman Bookmark).
    fn move_bookmark(&mut self, msg: &Value) {
        let g = |k: &str| msg[k].as_str().filter(|v| !v.is_empty());
        self.store.move_item(g("url"), g("folder"), msg["dest"].as_str().unwrap_or(""), g("beforeUrl"), g("beforeFolder"));
        self.bookmarks_changed();
    }

    fn bookmarks_changed(&mut self) {
        self.refresh_pages("bookmarks");
        self.last_render_key.clear();
        self.render();
    }

    /// Impor bookmark dari berkas Bookmarks browser lain.
    fn import_bookmarks(&mut self, path: Option<&str>) -> String {
        let found = import::detect();
        let targets: Vec<(String, std::path::PathBuf)> = match path {
            Some(p) => found.into_iter().filter(|(_, f)| f.display().to_string() == p).collect(),
            None => found,
        };
        if targets.is_empty() {
            return t(self.lang, "d.import_none").to_string();
        }
        let mut msgs = Vec::new();
        for (name, file) in targets {
            let n = self.store.add_bookmarks(&import::read_bookmarks(&file));
            msgs.push(tf(self.lang, "d.imported", &[("n", &n.to_string()), ("b", &name)]));
        }
        // Ikon situs bookmark baru diunduh sebentar lagi.
        self.icons_due = Some(Instant::now() + Duration::from_secs(3));
        self.refresh_pages("bookmarks");
        self.last_render_key.clear();
        self.render();
        msgs.join("\n")
    }

    // ---- pesan dari halaman --------------------------------------------------

    fn on_content(&mut self, id: u64, body: &str) -> bool {
        let Some(i) = self.tab_index(id) else { return false };
        let Ok(msg) = serde_json::from_str::<Value>(body) else { return false };
        let current_url = self.tabs[i].webview.as_ref().and_then(|w| w.url().ok()).unwrap_or_default();
        let internal = internal_page(&current_url).map(str::to_string);
        let kind = msg["t"].as_str().unwrap_or("");
        if std::env::var_os("OVNER_DEBUG").is_some() {
            log(&format!("pesan tab {id}: {kind} ({} byte) url={current_url}", body.len()));
        }
        // Perintah halaman internal hanya diterima dari halaman internal.
        if internal.is_none() && !matches!(kind, "key" | "url" | "page" | "cos" | "pick" | "pw-query" | "pw-capture" | "pw-fill" | "anubis" | "paste" | "dirty" | "clean" | "video-dl" | "video-never" | "video-settings") {
            return false;
        }
        match kind {
            // Tombol "Unduh video" di atas video halaman.
            "video-dl" => {
                if i != self.active {
                    self.activate(i);
                }
                // Alamat halaman saat ini (YouTube berpindah video tanpa memuat ulang).
                let page = msg["page"].as_str().filter(|u| u.starts_with("http")).map(str::to_string).unwrap_or(current_url.clone());
                self.open_video_dialog(&page);
            }
            "video-never" => {
                if let Some(h) = host_of(&current_url).map(|h| settings::norm_host(&h)) {
                    {
                        let mut st = self.settings.borrow_mut();
                        if !host_in(&st.video_button_never, &h) {
                            st.video_button_never.push(h.clone());
                        }
                        st.normalize();
                        st.save(data_dir());
                    }
                    log(&format!("tombol unduh video tidak ditampilkan lagi di {h}"));
                    self.refresh_pages("settings");
                }
            }
            "video-settings" => {
                let url = format!("{}#s-downloads", internal_url("settings"));
                self.open_tab(&url, true, false);
            }
            "anubis" => {
                // Tantangan anti-bot Anubis: hitung di thread latar dengan semua inti CPU.
                let data = msg["data"].as_str().unwrap_or("").to_string();
                let diff = msg["difficulty"].as_u64().unwrap_or(0) as u32;
                let p = self.proxy.clone();
                log(&format!("Anubis: tantangan kesulitan {diff} di {}", host_of(&current_url).unwrap_or_default()));
                std::thread::spawn(move || {
                    let t0 = Instant::now();
                    if let Some((hash, nonce)) = anubis::solve(&data, diff) {
                        log(&format!("Anubis: terpecahkan dalam {} ms (nonce {nonce})", t0.elapsed().as_millis()));
                        send(&p, UserEvent::Anubis(id, format!("window.__ovnerAnubis && __ovnerAnubis({:?}, {nonce})", hash)));
                    }
                });
            }
            "dirty" | "clean" => self.tabs[i].form_dirty = kind == "dirty",
            "paste" => {
                // Paksa tempel (Alt+Shift+V): masukkan teks clipboard tanpa event paste,
                // sehingga situs yang melarang tempel tidak bisa menolaknya.
                if let (Some(text), Some(wv)) = (win::read_clipboard(), &self.tabs[i].webview) {
                    let t = serde_json::to_string(&text).unwrap_or_default();
                    let _ = wv.evaluate_script(&format!("window.__ovnerForcePaste && __ovnerForcePaste({t})"));
                }
            }
            "key" => {
                if i == self.active {
                    return self.on_key(msg["k"].as_str().unwrap_or(""));
                }
            }
            "url" => {
                if !current_url.is_empty() {
                    self.tabs[i].url = current_url;
                    self.render();
                }
            }
            "page" => {
                if current_url.is_empty() || !self.blocking_active_for(&current_url) || internal.is_some() {
                    return false;
                }
                let pr = self.blocker.borrow().page_resources(&current_url);
                self.tabs[i].cos_exceptions = pr.exceptions;
                self.tabs[i].generichide = pr.generichide;
                self.inject_css(i, &pr.css);
                if !pr.procedural.is_empty() {
                    // Aturan procedural (:has-text, :upward, :remove, ...) dijalankan content.js.
                    let list = format!("[{}]", pr.procedural.join(","));
                    if let Some(wv) = &self.tabs[i].webview {
                        let _ = wv.evaluate_script(&format!("window.__ovnerProc && __ovnerProc({list})"));
                    }
                }
            }
            "cos" => {
                if self.tabs[i].generichide || current_url.is_empty() || !self.blocking_active_for(&current_url) || internal.is_some() {
                    return false;
                }
                let strings = |v: &Value| -> Vec<String> {
                    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
                };
                let (classes, ids) = (strings(&msg["c"]), strings(&msg["i"]));
                let css = self.blocker.borrow().generic_cosmetics(&classes, &ids, &self.tabs[i].cos_exceptions);
                self.inject_css(i, &css);
            }
            "go" => {
                let url = msg["text"].as_str().and_then(|text| to_url(text, &self.settings.borrow()));
                if let Some(url) = url {
                    self.navigate_active(&url, true);
                }
            }
            "pick" => {
                if let Some(sel) = msg["sel"].as_str() {
                    self.add_hide_rule(i, sel);
                }
            }
            // ---- Pengelola sandi: halaman web ----
            // Alamat selalu diambil dari WebView (bukan dari pesan), jadi situs hanya bisa
            // meminta login untuk dirinya sendiri.
            "pw-query" => {
                if !self.settings.borrow().password_manager || self.tabs[i].private {
                    return false;
                }
                let list: Vec<Value> = self.vault.for_url(&current_url).iter().map(|l| json!({"id": l.id, "user": l.username})).collect();
                if !list.is_empty() {
                    let auto = self.settings.borrow().password_autosignin && list.len() == 1;
                    if let Some(wv) = &self.tabs[i].webview {
                        let _ = wv.evaluate_script(&format!("window.__ovnerPw && __ovnerPw.offer({}, {auto})", Value::Array(list)));
                    }
                }
            }
            "pw-fill" => {
                let id = msg["id"].as_u64().unwrap_or(0);
                let found = self.vault.for_url(&current_url).iter().find(|l| l.id == id).map(|l| (l.username.clone(), l.password.clone()));
                if let Some((u, p)) = found {
                    self.vault.touch(id);
                    if let Some(wv) = &self.tabs[i].webview {
                        let args = json!([u, p]);
                        let _ = wv.evaluate_script(&format!("window.__ovnerPw && __ovnerPw.fill({}, {})", args[0], args[1]));
                    }
                }
            }
            "pw-capture" => {
                // Login dikirim lalu halaman langsung pindah (mis. SSO Infor → portal): alamat saat
                // pesan tiba sudah alamat baru. Pakai alamat halaman login yang dikirim content.js,
                // asalkan memang salah satu halaman yang baru saja dibuka tab ini.
                let claimed = msg["url"].as_str().unwrap_or("");
                let site = vault::site_of(claimed);
                let current_url = if !site.is_empty()
                    && (site == vault::site_of(&current_url) || self.tabs[i].recent_urls.iter().any(|u| vault::site_of(u) == site))
                {
                    claimed.to_string()
                } else {
                    current_url.clone()
                };
                if !self.settings.borrow().password_manager || self.tabs[i].private || !current_url.starts_with("http") {
                    return false;
                }
                let user = msg["user"].as_str().unwrap_or("").trim().to_string();
                let pass = msg["pass"].as_str().unwrap_or("").to_string();
                if let Some(update) = self.vault.offer_for(&current_url, &user, &pass) {
                    self.pw_offer = Some((current_url.clone(), user, pass, update));
                    self.last_render_key.clear();
                    self.render();
                }
            }
            // ---- Pengelola sandi: halaman ovner://passwords ----
            "pw-reveal" => {
                let id = msg["id"].as_u64().unwrap_or(0);
                if let (Some(l), Some(wv)) = (self.vault.get(id), &self.tabs[i].webview) {
                    let _ = wv.evaluate_script(&format!("window.ovnerReveal && ovnerReveal({id}, {})", Value::String(l.password.clone())));
                }
            }
            "pw-add" => {
                let url = to_url(msg["url"].as_str().unwrap_or(""), &self.settings.borrow()).unwrap_or_default();
                let (u, p) = (msg["user"].as_str().unwrap_or(""), msg["pass"].as_str().unwrap_or(""));
                if !vault::site_of(&url).is_empty() && !p.is_empty() {
                    self.vault.upsert(&url, u, p);
                }
                self.push_page_state(i);
            }
            "pw-edit" => {
                let s = |k: &str| msg[k].as_str().unwrap_or("").to_string();
                self.vault.edit(msg["id"].as_u64().unwrap_or(0), &s("user"), &s("pass"), &s("note"));
                self.push_page_state(i);
            }
            "pw-delete" => {
                self.vault.remove(msg["id"].as_u64().unwrap_or(0));
                self.push_page_state(i);
            }
            "pw-import-csv" => {
                let n = self.vault.import_csv(msg["csv"].as_str().unwrap_or(""));
                win::info(&tf(self.lang, "d.pw_imported", &[("n", &n.to_string())]));
                self.push_page_state(i);
            }
            "pw-export" => {
                if win::confirm(t(self.lang, "d.pw_export")) {
                    let csv = Value::String(self.vault.export_csv());
                    if let Some(wv) = &self.tabs[i].webview {
                        let _ = wv.evaluate_script(&format!("window.ovnerSave && ovnerSave('Ovner Passwords.csv', 'text/csv', {csv})"));
                    }
                }
            }
            "dl-open" | "dl-show" | "dl-remove" | "dl-delete" | "dl-cancel" | "dl-pause" | "dl-resume" => self.download_action(kind, msg["id"].as_u64().unwrap_or(0)),
            "dl-clear" => {
                if msg["files"].as_bool() == Some(true) {
                    for d in self.downloads.iter().filter(|d| d.state != "progress" && d.state != "paused") {
                        delete_download_files(&d.path);
                    }
                }
                self.downloads.retain(|d| d.state == "progress" || d.state == "paused");
                self.save_downloads();
                self.push_page_state(i);
            }
            "pw-clear" => {
                if win::confirm(t(self.lang, "d.pw_clear")) {
                    self.vault.clear();
                    self.push_page_state(i);
                }
            }
            "action" => {
                let name = msg["name"].as_str().unwrap_or("");
                if self.page_action(i, name) {
                    self.quit_requested = true;
                }
            }
            "allow-http" => {
                if let Some(u) = msg["url"].as_str().filter(|u| u.starts_with("http://")) {
                    *self.tabs[i].allow_http.borrow_mut() = Some(u.to_string());
                    if let Some(wv) = &self.tabs[i].webview {
                        let _ = wv.load_url(u);
                    }
                }
            }
            "preset" => {
                let name = msg["name"].as_str().unwrap_or("");
                let ok = self.settings.borrow_mut().apply_preset(name);
                if ok {
                    self.settings.borrow().save(data_dir());
                    log(&format!("preset: {name}"));
                    self.apply_all_settings();
                }
            }
            "import" => {
                let msg_text = self.import_bookmarks(msg["path"].as_str());
                win::info(&msg_text);
                self.push_page_state(i);
            }
            "setup-done" => {
                self.settings.borrow_mut().setup_done = true;
                self.settings.borrow().save(data_dir());
                let nt = self.newtab_url();
                self.tabs[i].url = nt.clone();
                if let Some(wv) = &self.tabs[i].webview {
                    let _ = wv.load_url(&nt);
                }
            }
            "page-ready" => self.push_page_state(i),
            "set" => {
                let key = msg["key"].as_str().unwrap_or("");
                if self.settings.borrow_mut().set(key, &msg["value"]) {
                    self.settings.borrow().save(data_dir());
                    self.apply_setting_side_effects(key);
                } else {
                    log(&format!("pengaturan ditolak: {key}"));
                }
                self.push_page_state(i);
            }
            "restart" => return true,
            "check-update" => self.check_update_now(),
            "history-search" => {
                self.tabs[i].page_query = msg["q"].as_str().unwrap_or("").to_string();
                self.push_page_state(i);
            }
            "history-remove" => {
                self.store.remove_visits(msg["url"].as_str().unwrap_or(""));
                self.refresh_pages("history");
            }
            "history-clear" => {
                self.store.clear_history();
                self.store.flush();
                self.refresh_pages("history");
            }
            "bookmark-rename" => {
                self.store.rename_bookmark(msg["url"].as_str().unwrap_or(""), msg["title"].as_str().unwrap_or(""));
                self.refresh_pages("bookmarks");
                self.last_render_key.clear();
                self.render();
            }
            "bookmark-remove" => {
                self.store.remove_bookmark(msg["url"].as_str().unwrap_or(""));
                self.bookmarks_changed();
            }
            "bm-add" => {
                let s = |k: &str| msg[k].as_str().unwrap_or("").to_string();
                let url = to_url(&s("url"), &self.settings.borrow()).unwrap_or_default();
                self.store.add_bookmark(&s("title"), &url, &s("folder"));
                self.bookmarks_changed();
            }
            "bm-edit" => {
                let s = |k: &str| msg[k].as_str().unwrap_or("").to_string();
                let url = to_url(&s("newUrl"), &self.settings.borrow()).unwrap_or_default();
                self.store.edit_bookmark(&s("url"), &s("title"), &url, &s("folder"));
                self.bookmarks_changed();
            }
            "folder-add" => {
                self.store.add_folder(msg["path"].as_str().unwrap_or(""));
                self.bookmarks_changed();
            }
            "folder-rename" => {
                self.store.rename_folder(msg["path"].as_str().unwrap_or(""), msg["name"].as_str().unwrap_or(""));
                self.bookmarks_changed();
            }
            "folder-remove" => {
                self.store.remove_folder(msg["path"].as_str().unwrap_or(""));
                self.bookmarks_changed();
            }
            "bm-sort" => {
                self.store.sort_folder(msg["folder"].as_str().unwrap_or(""));
                self.bookmarks_changed();
            }
            "bm-move" => self.move_bookmark(&msg),
            "bm-import-html" => {
                let items = store::parse_bookmarks_html(msg["html"].as_str().unwrap_or(""));
                let n = self.store.add_bookmarks(&items);
                win::info(&tf(self.lang, "d.imported", &[("n", &n.to_string()), ("b", "HTML")]));
                self.bookmarks_changed();
            }
            "bm-export" => {
                let html = serde_json::to_string(&self.store.export_html()).unwrap_or_default();
                if let Some(wv) = &self.tabs[i].webview {
                    let _ = wv.evaluate_script(&format!("window.ovnerSave && ovnerSave('bookmarks.html', 'text/html', {html})"));
                }
            }
            "open" => {
                let url = msg["url"].as_str().unwrap_or("").to_string();
                if msg["newtab"].as_bool().unwrap_or(false) {
                    self.open_tab(&url, true, false);
                } else {
                    self.navigate_active(&url, false);
                }
            }
            "trim" => {
                self.trim_now();
                self.push_page_state(i);
            }
            "sleep-all" => {
                self.on_menu("sleep-all");
                self.push_page_state(i);
            }
            "session-save" => {
                let name = msg["name"].as_str().unwrap_or("").trim().chars().take(80).collect::<String>();
                let tabs: Vec<SessionTab> = self
                    .tabs
                    .iter()
                    .filter(|t| !t.private && !is_newtab(&t.url) && internal_page(&t.url).is_none())
                    .map(|t| SessionTab { url: t.url.clone(), title: t.title.clone() })
                    .collect();
                if !tabs.is_empty() {
                    let saved = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                    let name = if name.is_empty() { format!("{} {}", t(self.lang, "p.sessions"), self.sessions.len() + 1) } else { name };
                    self.sessions.insert(0, SavedSession { name, saved, tabs });
                    self.tab_stats.sessions_saved += 1;
                    self.save_sessions();
                    self.save_tab_stats();
                }
                self.push_page_state(i);
            }
            "session-open" | "session-replace" => {
                if let Some(sess) = msg["idx"].as_u64().and_then(|k| self.sessions.get(k as usize)).cloned() {
                    if kind == "session-replace" {
                        // Seperti "Clear the Deck": tab lain ditutup, lalu sesi dibuka.
                        let keep = self.tabs[i].id;
                        self.tabs.retain(|t| t.id == keep);
                        self.active = 0;
                        self.split = None;
                    }
                    // Tab sesi dibuka tidur (dimuat saat diklik) supaya hemat RAM.
                    for st in &sess.tabs {
                        let t = self.new_tab_struct(&st.url, &st.title, false);
                        self.tabs.push(t);
                    }
                    self.tab_stats.restored += sess.tabs.len() as u64;
                    self.save_tab_stats();
                    self.render();
                    self.save_session();
                }
                if let Some(k) = self.tabs.iter().position(|t| internal_page(&t.url) == Some("sessions")) {
                    self.push_page_state(k);
                }
            }
            "session-delete" => {
                if let Some(k) = msg["idx"].as_u64().map(|k| k as usize).filter(|k| *k < self.sessions.len()) {
                    self.sessions.remove(k);
                    self.save_sessions();
                }
                self.push_page_state(i);
            }
            "session-rename" => {
                let name = msg["name"].as_str().unwrap_or("").trim().chars().take(80).collect::<String>();
                if let Some(sess) = msg["idx"].as_u64().and_then(|k| self.sessions.get_mut(k as usize)) {
                    if !name.is_empty() {
                        sess.name = name;
                    }
                }
                self.save_sessions();
                self.push_page_state(i);
            }
            "close-duplicates" => {
                self.close_duplicates();
                if let Some(k) = self.tabs.iter().position(|t| internal_page(&t.url) == Some("sessions")) {
                    self.push_page_state(k);
                }
            }
            "reopen-closed" => {
                if let Some(u) = msg["url"].as_str().map(str::to_string) {
                    self.closed.retain(|c| c != &u);
                    self.open_tab(&u, false, false);
                    self.push_page_state(i);
                }
            }
            "tab-sleep" | "tab-close" => {
                // Tab bisa berada di jendela lain (Task Manager menampilkan semua jendela).
                let back = self.win_no;
                if let Some(no) = msg["id"].as_u64().and_then(|id| self.win_of_tab(id)) {
                    self.switch_to(no);
                }
                if let Some(j) = msg["id"].as_u64().and_then(|id| self.tab_index(id)) {
                    if kind == "tab-sleep" {
                        self.sleep_tab(j);
                    } else if j != self.active {
                        self.close_tab(j);
                    }
                    self.render();
                }
                self.switch_to(back);
                self.refresh_pages("tasks");
            }
            _ => {}
        }
        false
    }

    /// Mengembalikan true bila Ovner harus dibuka ulang (update).
    fn on_key(&mut self, k: &str) -> bool {
        let n = self.tabs.len();
        match k {
            "newtab" => self.open_tab("", true, false),
            "private" => self.open_tab("", true, true),
            "newwindow" => self.new_window("", false),
            "close" => self.close_tab(self.active),
            "reopen" => {
                if let Some(u) = self.closed.pop() {
                    self.open_tab(&u, true, false);
                }
            }
            "address" => self.focus_address(),
            "home" => {
                self.on_menu("home");
            }
            "nexttab" if n > 0 => self.activate((self.active + 1) % n),
            "prevtab" if n > 0 => self.activate((self.active + n - 1) % n),
            "reload" => {
                if let Some(wv) = self.active_webview() {
                    let _ = wv.reload();
                }
            }
            "findbar" => {
                self.find_open = true;
                self.render();
                let _ = self.toolbar.focus();
                let _ = self.toolbar.evaluate_script("window.ovner && ovner.focusFind()");
            }
            "find-close" => {
                self.find_open = false;
                self.render();
                if let Some(wv) = self.active_webview() {
                    let _ = wv.focus();
                }
            }
            "bookmark" | "history" | "downloads" | "print" | "tasks" => {
                let _ = self.on_menu(k);
            }
            k if k.starts_with("tab") => {
                if let Ok(d) = k[3..].parse::<usize>() {
                    // Ctrl+9 = tab terakhir, seperti Chrome.
                    let idx = if d == 9 { n.saturating_sub(1) } else { d - 1 };
                    if idx < n {
                        self.activate(idx);
                    }
                }
            }
            _ => {}
        }
        false
    }

    // ---- pesan dari toolbar --------------------------------------------------

    fn on_toolbar(&mut self, body: &str) -> bool {
        let Ok(msg) = serde_json::from_str::<Value>(body) else { return false };
        let id = msg["id"].as_u64().unwrap_or(0);
        let cmd = msg["cmd"].as_str().unwrap_or("").to_string();
        match cmd.as_str() {
            "ready" => {
                self.toolbar_ready = true;
                self.last_render_key.clear();
                self.render();
            }
            "height" => {
                if let Some(h) = msg["h"].as_f64() {
                    self.set_toolbar_height(h, msg["over"].as_f64().unwrap_or(0.0));
                    let rects: Vec<(f64, f64, f64, f64)> = msg["rects"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|r| {
                            let a = r.as_array()?;
                            Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?, a.get(3)?.as_f64()?))
                        })
                        .collect();
                    self.clip_toolbar(&rects);
                }
            }
            "dl-open" | "dl-show" | "dl-cancel" | "dl-remove" | "dl-delete" | "dl-pause" | "dl-resume" => self.download_action(&cmd, id),
            "dl-toggle" => {
                self.dl_bubble = !self.dl_bubble;
                self.last_render_key.clear();
                self.render();
            }
            "dl-all" => {
                self.dl_bubble = false;
                self.open_internal("downloads");
            }
            "go" => {
                let url = to_url(msg["text"].as_str().unwrap_or(""), &self.search_settings());
                if let Some(url) = url {
                    if msg["newtab"].as_bool().unwrap_or(false) {
                        self.open_tab(&url, true, false);
                    } else {
                        self.navigate_active(&url, true);
                    }
                }
            }
            "suggest" => {
                let text = msg["text"].as_str().unwrap_or("");
                let list: Vec<Value> = if self.settings.borrow().address_suggestions {
                    self.store.suggest(text, 6).into_iter().map(|(t, u, bm)| json!({"title": t, "url": u, "bm": bm})).collect()
                } else {
                    Vec::new()
                };
                let bang = shields::expand_bang(text, &self.settings.borrow());
                let st = json!({"text": text, "items": list, "bang": bang});
                let _ = self.toolbar.evaluate_script(&format!("window.ovner && ovner.suggestions({st})"));
            }
            "shield-scripts" => self.toggle_site("scripts"),
            "shield-fp" => self.toggle_site("fp"),
            "bm-save" => {
                if let Some((u, _)) = self.bm_edit.take() {
                    let title = msg["title"].as_str().unwrap_or("").to_string();
                    let folder = msg["folder"].as_str().unwrap_or("").to_string();
                    let from = self.bm_edit_from.take();
                    let cur = self.store.bookmarks.iter().find(|b| b.url == u && from.as_deref().is_none_or(|f| b.folder == f)).map(|b| b.folder.clone());
                    if let Some(cur) = cur {
                        if cur != folder {
                            self.store.relocate(Some(&u), Some(&cur), None, &folder, None, None, false);
                        }
                    }
                    self.store.rename_bookmark_in(&u, &folder, &title);
                    // Alamat bisa diubah juga (ditulis tanpa https:// pun diterima).
                    let typed = msg["url"].as_str().unwrap_or("").trim().to_string();
                    if !typed.is_empty() && typed != u {
                        if let Some(nu) = to_url(&typed, &self.settings.borrow()) {
                            self.store.set_url_in(&u, &folder, &nu);
                        }
                    }
                    self.last_bm_folder = folder;
                    self.bookmarks_changed();
                }
            }
            "bm-remove" => {
                if let Some((u, _)) = self.bm_edit.take() {
                    self.store.remove_bookmark(&u);
                    self.bookmarks_changed();
                }
            }
            "bm-close" => {
                if self.bm_edit.take().is_some() {
                    self.last_render_key.clear();
                    self.render();
                }
            }
            "bm-move" => {
                self.move_bookmark(&msg);
            }
            // Menu klik kanan bookmark di bilah/menu folder: Edit, Potong/Salin/Tempel, Hapus.
            "copy-text" => win::set_clipboard(msg["text"].as_str().unwrap_or("")),
            "bm-edit-open" => {
                let url = msg["url"].as_str().unwrap_or("").to_string();
                if !url.is_empty() {
                    self.bm_edit_from = msg["from"].as_str().map(str::to_string);
                    self.bm_edit = Some((url, false));
                    self.dl_bubble = false;
                    self.last_render_key.clear();
                    self.render();
                }
            }
            "bm-folder-rename" => {
                self.store.rename_folder(msg["path"].as_str().unwrap_or(""), msg["name"].as_str().unwrap_or(""));
                self.bookmarks_changed();
            }
            "bm-delete" => {
                let g = |k: &str| msg[k].as_str().unwrap_or("").to_string();
                if !g("url").is_empty() {
                    self.store.remove_bookmark_in(&g("url"), &g("from"));
                    self.bookmarks_changed();
                } else if !g("folder").is_empty() {
                    let f = g("folder");
                    let n = self.store.bookmarks.iter().filter(|b| b.folder == f || b.folder.starts_with(&format!("{f}/"))).count();
                    let name = f.rsplit('/').next().unwrap_or(&f).to_string();
                    if n == 0 || win::confirm(&tf(self.lang, "d.bm_folder_del", &[("f", &name), ("n", &n.to_string())])) {
                        self.store.remove_folder(&f);
                        self.bookmarks_changed();
                    }
                }
            }
            "bm-paste" => {
                let g = |k: &str| msg[k].as_str().filter(|v| !v.is_empty());
                let copy = msg["mode"] == "copy";
                self.store.relocate(g("url"), g("from"), g("folder"), msg["dest"].as_str().unwrap_or(""), g("beforeUrl"), g("beforeFolder"), copy);
                self.bookmarks_changed();
            }
            "video-open" => self.open_video_dialog(""),
            "video-lookup" => {
                let url = msg["url"].as_str().unwrap_or("").trim().to_string();
                if url.starts_with("http") {
                    self.video_req += 1;
                    self.video_win = self.win_no;
                    self.lookup_video(&url);
                }
            }
            "video-browse" => {
                let start = msg["dir"].as_str().unwrap_or("").to_string();
                #[cfg(windows)]
                let owner = {
                    use tao::platform::windows::WindowExtWindows;
                    Some(self.window.hwnd() as isize)
                };
                #[cfg(not(windows))]
                let owner = None;
                if let Some(dir) = win::pick_folder(owner, t(self.lang, "d.video_folder"), Path::new(&start)) {
                    post_page(&self.toolbar, "videoDlg", &json!({"dir": dir.display().to_string()}).to_string(), "window.ovner && ovner.videoDlg");
                }
            }
            "video-start" => {
                let spec = video::Spec {
                    page: msg["url"].as_str().unwrap_or("").trim().to_string(),
                    key: msg["key"].as_str().unwrap_or("v:best").to_string(),
                    dir: msg["dir"].as_str().unwrap_or("").trim().to_string(),
                    name: video::safe_name(msg["name"].as_str().unwrap_or("video")),
                    sub: msg["sub"].as_str().unwrap_or("").to_string(),
                    embed: msg["embed"].as_bool().unwrap_or(false),
                };
                if spec.page.starts_with("http") && !spec.dir.is_empty() {
                    self.start_video_spec(spec, msg["later"].as_bool().unwrap_or(false));
                }
            }
            "dark-site" => {
                if let Some(h) = self.active_site() {
                    let v = msg["v"].as_str().unwrap_or("default").to_string();
                    self.set_dark_site(&h, &v);
                }
            }
            "dark-set" => {
                let key = msg["key"].as_str().unwrap_or("");
                if ["dark_mode", "dark_engine", "dark_brightness", "dark_contrast", "dark_sepia", "dark_grayscale"].contains(&key) {
                    let ok = self.settings.borrow_mut().set(key, &msg["value"]);
                    if ok {
                        self.settings.borrow().save(data_dir());
                        self.apply_dark_live();
                        self.refresh_pages("settings");
                        self.last_render_key.clear();
                        self.render();
                    }
                }
            }
            "pick" => {
                let _ = self.on_menu("pick");
            }
            "split-swap" => {
                if let Some(p) = self.split.and_then(|id| self.tab_index(id)) {
                    self.activate(p);
                }
            }
            "split-close" => self.stop_split(),
            "pw-save" => {
                if let Some((url, u, p, _)) = self.pw_offer.take() {
                    self.vault.upsert(&url, &u, &p);
                    self.refresh_pages("passwords");
                }
                self.last_render_key.clear();
                self.render();
            }
            "pw-never" => {
                if let Some((url, ..)) = self.pw_offer.take() {
                    self.vault.add_never(&url);
                }
                self.last_render_key.clear();
                self.render();
            }
            "bubble-close" => {
                self.pw_offer = None;
                self.dl_bubble = false;
                self.last_render_key.clear();
                self.render();
            }
            "open" => {
                let url = msg["url"].as_str().unwrap_or("").to_string();
                if msg["newtab"].as_bool().unwrap_or(false) {
                    self.open_tab(&url, true, false);
                } else {
                    self.navigate_active(&url, false);
                }
            }
            "newtab" => self.open_tab("", true, false),
            "bm-folder" => {
                let f = msg["folder"].as_str().unwrap_or("").to_string();
                self.show_bookmark_folder(&f, msg["x"].as_f64().unwrap_or(0.0), msg["y"].as_f64().unwrap_or(self.toolbar_h));
            }
            "bm-more" => {
                self.show_bookmark_overflow(&msg["items"], msg["x"].as_f64().unwrap_or(0.0), msg["y"].as_f64().unwrap_or(self.toolbar_h));
            }
            "activate" => {
                if let Some(i) = self.tab_index(id) {
                    self.activate(i);
                }
            }
            "tab-detach" => {
                // Tab ditarik keluar dari daftar tab: dijatuhkan di baris tab jendela Ovner lain
                // → gabung ke sana; di tempat lain → jendela baru di titik itu.
                if let Some(i) = self.tab_index(id) {
                    let pos = msg["x"].as_f64().zip(msg["y"].as_f64());
                    match pos.and_then(|(x, y)| self.window_at(x, y)) {
                        Some(no) => self.move_tab_to_window(i, no, pos.unwrap_or_default()),
                        None => self.detach_tab(i, pos),
                    }
                }
            }
            "tab-move" => {
                // Tab diseret ke posisi lain di baris tab / bilah samping.
                if let Some(from) = self.tab_index(id) {
                    let to = (msg["to"].as_u64().unwrap_or(from as u64) as usize).min(self.tabs.len().saturating_sub(1));
                    if to != from {
                        let active_id = self.tabs.get(self.active).map(|t| t.id);
                        let t = self.tabs.remove(from);
                        self.tabs.insert(to, t);
                        if let Some(a) = active_id.and_then(|a| self.tab_index(a)) {
                            self.active = a;
                        }
                        self.save_session();
                    }
                    self.last_render_key.clear();
                    self.render();
                }
            }
            "close" => {
                if let Some(i) = self.tab_index(id) {
                    self.close_tab(i);
                }
            }
            "back" => {
                if let Some(wv) = self.active_webview() {
                    #[cfg(windows)]
                    netfilter::go_back(wv);
                    #[cfg(not(windows))]
                    let _ = wv.evaluate_script("history.back()");
                }
            }
            "forward" => {
                if let Some(wv) = self.active_webview() {
                    #[cfg(windows)]
                    netfilter::go_forward(wv);
                    #[cfg(not(windows))]
                    let _ = wv.evaluate_script("history.forward()");
                }
            }
            "reload" => {
                if let Some(wv) = self.active_webview() {
                    let _ = wv.reload();
                }
            }
            "stop" => {
                if let Some(wv) = self.active_webview() {
                    #[cfg(windows)]
                    netfilter::stop(wv);
                    #[cfg(not(windows))]
                    let _ = wv.evaluate_script("window.stop()");
                }
            }
            "home" => {
                self.on_menu("home");
            }
            "bookmark" => {
                self.on_menu("bookmark");
            }
            "focus-page" => {
                if let Some(wv) = self.active_webview() {
                    let _ = wv.focus();
                }
            }
            "shield" => {
                let x = msg["x"].as_f64().unwrap_or(0.0);
                let y = msg["y"].as_f64().unwrap_or(self.toolbar_h);
                self.show_shield_menu(x, y);
            }
            "find" => {
                let text = msg["text"].as_str().unwrap_or("");
                let back = msg["back"].as_bool().unwrap_or(false);
                let fresh = msg["fresh"].as_bool().unwrap_or(false);
                if let Some(wv) = self.active_webview() {
                    let js = format!(
                        "(function(q){{if(!q)return;{}window.find(q,false,{back},true,false,true,false);}})({})",
                        if fresh { "var s=getSelection();if(s.rangeCount){var r=s.getRangeAt(0);r.collapse(true);s.removeAllRanges();s.addRange(r);}" } else { "" },
                        serde_json::to_string(text).unwrap_or_default()
                    );
                    let _ = wv.evaluate_script(&js);
                }
            }
            "find-close" => return self.on_key("find-close"),
            "key" => return self.on_key(msg["k"].as_str().unwrap_or("")),
            "menu" => {
                let x = msg["x"].as_f64().unwrap_or(0.0);
                let y = msg["y"].as_f64().unwrap_or(self.toolbar_h);
                self.show_menu(false, x, y);
            }
            "tabmenu" => {
                self.menu_tab = Some(id);
                let dy = if msg["side"].as_bool().unwrap_or(false) { self.toolbar_h } else { 0.0 };
                self.show_menu(true, msg["x"].as_f64().unwrap_or(0.0), msg["y"].as_f64().unwrap_or(0.0) + dy);
            }
            "restart" => return true,
            _ => {}
        }
        false
    }

    /// Setelan untuk mencari dari kolom alamat: tab privat bisa memakai mesin pencari lain.
    fn search_settings(&self) -> Settings {
        let mut s = self.settings.borrow().clone();
        let private = self.tabs.get(self.active).is_some_and(|t| t.private);
        if private && !s.private_search_engine.trim().is_empty() {
            s.search_engine = s.private_search_engine.clone();
        }
        s
    }

    /// Identitas baru: semua tab ditutup, data penjelajahan dihapus, mulai dari tab kosong.
    fn new_identity(&mut self) {
        self.split = None;
        self.closed.clear();
        let tabs = std::mem::take(&mut self.tabs);
        drop(tabs);
        self.store.clear_history();
        self.store.flush();
        self.session_key = shields::session_key();
        self.script_gen += 1;
        self.active = 0;
        self.open_tab("", true, false);
        // Toolbar memakai profil yang sama dengan tab biasa.
        let _ = self.toolbar.clear_all_browsing_data();
        let _ = std::fs::remove_file(data_dir().join("session.json"));
        log("identitas baru");
        self.last_render_key.clear();
        self.render();
    }

    /// Panel Shields: menu kecil dengan sakelar untuk situs di tab aktif.
    fn show_shield_menu(&self, x: f64, y: f64) {
        let Some(host) = self.tabs.get(self.active).filter(|t| internal_page(&t.url).is_none()).and_then(|t| host_of(&t.url)) else { return };
        let s = self.settings.borrow();
        let site = settings::norm_host(&host);
        let lang = self.lang;
        let blocked = self.tabs[self.active].blocked.get();
        let m = Menu::new();
        let head = MenuItem::with_id("shield-none", tf(lang, "sh.title", &[("h", &site)]), false, None);
        let count = MenuItem::with_id("shield-none2", tf(lang, "sh.blocked", &[("n", &blocked.to_string())]), false, None);
        let ads = CheckMenuItem::with_id("shield-ads", t(lang, "sh.ads"), s.block_ads, s.block_ads && !s.is_exception(&host), None);
        let scripts = CheckMenuItem::with_id("shield-scripts", t(lang, "sh.scripts"), true, host_in(&s.script_blocked, &host), None);
        let fp = CheckMenuItem::with_id("shield-fp", t(lang, "sh.fp"), s.farbling != "off", s.farbling_for(&host) != "off", None);
        let pick = MenuItem::with_id("pick", t(lang, "sh.pick"), true, None);
        let level = s.security_level.clone();
        let allow_js = CheckMenuItem::with_id("shield-allow-js", t(lang, "sh.allow_js"), level != "standard", host_in(&s.script_allowed, &host), None);
        let more = MenuItem::with_id("shield-settings", t(lang, "sh.settings"), true, None);
        let dark = CheckMenuItem::with_id("shield-dark", t(lang, "sh.dark"), true, s.dark_for(&site, shields::system_dark()).is_some(), None);
        let copy = CheckMenuItem::with_id("shield-copy", t(lang, "sh.copy"), !s.allow_copy_all, s.allow_copy_for(&site), None);
        let nosleep = CheckMenuItem::with_id("shield-nosleep", t(lang, "sh.nosleep"), true, host_in(&s.sleep_never_sites, &site), None);
        drop(s);
        let _ = m.append_items(&[&head, &count, &PredefinedMenuItem::separator(), &ads, &scripts, &fp]);
        if level != "standard" {
            let _ = m.append(&allow_js);
        }
        let clear = MenuItem::with_id("shield-clear-site", t(lang, "sh.clear_site"), true, None);
        // Rekam: "Mulai" bila belum ada tab yang direkam, "Selesai" (dari tab mana pun) bila sedang merekam.
        let recording = self.tabs.iter().any(|t| t.recording);
        let rec = MenuItem::with_id(
            "shield-record",
            if recording { tf(lang, "sh.record_stop", &[("n", &self.problems.len().to_string())]) } else { t(lang, "sh.record").to_string() },
            true,
            None,
        );
        let _ = m.append_items(&[&PredefinedMenuItem::separator(), &dark, &copy, &nosleep, &clear, &rec]);
        let _ = m.append_items(&[&PredefinedMenuItem::separator(), &pick, &more]);
        #[cfg(windows)]
        {
            use tao::platform::windows::WindowExtWindows;
            use tray_icon::menu::ContextMenu;
            let pos = tray_icon::dpi::Position::Logical(tray_icon::dpi::LogicalPosition::new(x, y));
            unsafe {
                self.menu_win.set(self.win_no);
                m.show_context_menu_for_hwnd(self.window.hwnd() as isize, Some(pos));
            }
        }
        #[cfg(not(windows))]
        {
            let _ = (x, y, m);
        }
    }

    /// Tampilkan menu sementara di posisi (x, y) toolbar.
    fn popup(&self, m: &Menu, x: f64, y: f64) {
        #[cfg(windows)]
        {
            use tao::platform::windows::WindowExtWindows;
            use tray_icon::menu::ContextMenu;
            let pos = tray_icon::dpi::Position::Logical(tray_icon::dpi::LogicalPosition::new(x, y));
            unsafe {
                self.menu_win.set(self.win_no);
                m.show_context_menu_for_hwnd(self.window.hwnd() as isize, Some(pos));
            }
        }
        #[cfg(not(windows))]
        {
            let _ = (m, x, y);
        }
    }

    /// Isi akar bilah bookmark (bookmark dan folder tingkat pertama sesuai urutan), lalu
    /// "Bookmark lain".
    fn fill_bookmark_root(&self, push: &mut dyn FnMut(&dyn IsMenuItem)) {
        let mut seen: Vec<String> = Vec::new();
        for (i, b) in self.store.bookmarks.iter().enumerate() {
            if b.folder.is_empty() {
                push(&MenuItem::with_id(format!("bmo:{i}"), menu_label(&b.title), true, None));
                continue;
            }
            let top = b.folder.split('/').next().unwrap_or("").to_string();
            if top == "@other" || seen.contains(&top) {
                continue;
            }
            seen.push(top.clone());
            let sm = Submenu::new(menu_label(&top), true);
            self.fill_bookmark_folder(&top, &mut |it| {
                let _ = sm.append(it);
            });
            push(&sm);
        }
        for f in self.store.all_folders() {
            if !f.contains('/') && f != "@other" && !seen.contains(&f) {
                push(&Submenu::new(menu_label(&f), true));
            }
        }
        if self.store.bookmarks.iter().any(|b| b.folder == "@other" || b.folder.starts_with("@other/")) {
            let sm = Submenu::new(menu_label(&self.folder_label("@other")), true);
            self.fill_bookmark_folder("@other", &mut |it| {
                let _ = sm.append(it);
            });
            push(&sm);
        }
    }

    /// Isi satu folder bookmark: subfolder sebagai submenu, lalu bookmark-nya.
    fn fill_bookmark_folder(&self, path: &str, push: &mut dyn FnMut(&dyn IsMenuItem)) {
        let prefix = format!("{path}/");
        let mut subs: Vec<String> = Vec::new();
        for b in &self.store.bookmarks {
            if let Some(rest) = b.folder.strip_prefix(&prefix) {
                let name = rest.split('/').next().unwrap_or("").to_string();
                if !subs.contains(&name) {
                    subs.push(name);
                }
            }
        }
        for name in subs {
            let sm = Submenu::new(menu_label(&name), true);
            self.fill_bookmark_folder(&format!("{prefix}{name}"), &mut |it| {
                let _ = sm.append(it);
            });
            push(&sm);
        }
        for (i, b) in self.store.bookmarks.iter().enumerate().filter(|(_, b)| b.folder == path) {
            let item = MenuItem::with_id(format!("bmo:{i}"), menu_label(&b.title), true, None);
            push(&item);
        }
    }

    fn folder_label(&self, folder: &str) -> String {
        if folder == "@other" {
            t(self.lang, "m.other_bookmarks").to_string()
        } else {
            folder.to_string()
        }
    }

    /// Menu satu folder bilah bookmark.
    fn show_bookmark_folder(&self, folder: &str, x: f64, y: f64) {
        let m = Menu::new();
        self.fill_bookmark_folder(folder, &mut |it| {
            let _ = m.append(it);
        });
        self.popup(&m, x, y);
    }

    /// Menu » untuk item bilah bookmark yang tidak muat.
    fn show_bookmark_overflow(&self, items: &Value, x: f64, y: f64) {
        let m = Menu::new();
        for it in items.as_array().into_iter().flatten() {
            if let Some(f) = it["f"].as_str() {
                let sm = Submenu::new(menu_label(&self.folder_label(f)), true);
                self.fill_bookmark_folder(f, &mut |x| {
                    let _ = sm.append(x);
                });
                let _ = m.append(&sm);
            } else if let Some(i) = it["i"].as_u64().map(|i| i as usize) {
                if let Some(b) = self.store.bookmarks.get(i) {
                    let _ = m.append(&MenuItem::with_id(format!("bmo:{i}"), menu_label(&b.title), true, None));
                }
            }
        }
        self.popup(&m, (x - 280.0).max(0.0), y);
    }

    fn show_menu(&self, tab_menu: bool, x: f64, y: f64) {
        if !tab_menu {
            // Daftar bookmark di submenu ☰ → Bookmark, seperti Brave.
            let sm = &self.menus.bookmarks;
            while sm.items().len() > 5 {
                sm.remove_at(5);
            }
            self.fill_bookmark_root(&mut |it| {
                let _ = sm.append(it);
            });
        }
        #[cfg(windows)]
        {
            use tao::platform::windows::WindowExtWindows;
            use tray_icon::menu::ContextMenu;
            let pos = tray_icon::dpi::Position::Logical(tray_icon::dpi::LogicalPosition::new(if tab_menu { x } else { (x - 260.0).max(0.0) }, y));
            let m = if tab_menu { &self.menus.tab } else { &self.menus.main };
            unsafe {
                self.menu_win.set(self.win_no);
                m.show_context_menu_for_hwnd(self.window.hwnd() as isize, Some(pos));
            }
        }
        #[cfg(not(windows))]
        {
            let _ = (&self.menus, tab_menu, x, y);
            log("menu hanya tersedia di Windows");
        }
    }

    fn check_update_now(&self) {
        let p = self.proxy.clone();
        let dir = data_dir().to_path_buf();
        let (channel, lang) = (self.settings.borrow().update_channel.clone(), self.lang);
        std::thread::spawn(move || match update::check_and_stage(&dir, &channel) {
            Ok(Some(v)) => send(&p, UserEvent::UpdateReady(v)),
            Ok(None) => send(&p, UserEvent::UpdateInfo(t(lang, "d.up_to_date").into())),
            Err(e) => send(&p, UserEvent::UpdateInfo(tf(lang, "d.update_failed", &[("e", &e)]))),
        });
    }

    /// Mengembalikan true bila Ovner harus keluar.
    fn on_menu(&mut self, id: &str) -> bool {
        let tab_target = self.menu_tab.and_then(|id| self.tab_index(id));
        if let Some(i) = id.strip_prefix("bmo:").and_then(|i| i.parse::<usize>().ok()) {
            if let Some(url) = self.store.bookmarks.get(i).map(|b| b.url.clone()) {
                self.navigate_active(&url, false);
            }
            return false;
        }
        match id {
            "newtab" => {
                if self.in_tray {
                    self.show_from_tray();
                }
                self.open_tab("", true, false)
            }
            "private" => self.open_tab("", true, true),
            "reopen" => {
                let _ = self.on_key("reopen");
            }
            "show" => self.show_from_tray(),
            "hide" => self.hide_to_tray(),
            "home" => {
                let home = self.settings.borrow().home_url.trim().to_string();
                let url = if home.is_empty() { None } else { to_url(&home, &self.settings.borrow()) };
                let url = url.unwrap_or_else(|| self.newtab_url());
                self.navigate_active(&url, false);
            }
            "bookmark" => {
                if let Some(t) = self.tabs.get(self.active) {
                    if !t.url.is_empty() && !is_newtab(&t.url) {
                        // Seperti Chrome/Brave: bintang langsung menyimpan, lalu kotak kecil untuk
                        // mengganti nama dan memilih folder (atau menghapus).
                        let (u, ti) = (t.url.clone(), t.title.clone());
                        let fresh = !self.store.is_bookmarked(&u);
                        if fresh {
                            let f = self.last_bm_folder.clone();
                            self.store.add_bookmark(&ti, &u, &f);
                        }
                        self.bm_edit = Some((u, fresh));
                        self.bm_edit_from = None;
                        self.dl_bubble = false;
                        self.refresh_pages("bookmarks");
                        self.last_render_key.clear();
                        self.render();
                    }
                }
            }
            "bookmark-bar" => {
                let v = !self.settings.borrow().bookmark_bar;
                self.settings.borrow_mut().bookmark_bar = v;
                self.settings.borrow().save(data_dir());
                self.refresh_pages("settings");
                self.last_render_key.clear();
                self.render();
            }
            "bookmarks" | "history" | "settings" | "tasks" | "passwords" | "diagnostics" => self.open_internal(id),
            "shield-settings" => self.open_internal("settings"),
            "shield-allow-js" => {
                let host = self.tabs.get(self.active).and_then(|t| host_of(&t.url));
                if let Some(h) = host {
                    toggle_host(&mut self.settings.borrow_mut().script_allowed, &h);
                    self.settings.borrow().save(data_dir());
                    if let Some(wv) = self.active_webview() {
                        let _ = wv.reload();
                    }
                    self.refresh_pages("settings");
                }
            }
            "copy-urls" => {
                let text: String = self.tabs.iter().filter(|t| !is_newtab(&t.url)).map(|t| format!("{}\n", display_url(&t.url))).collect();
                win::set_clipboard(&text);
            }
            "new-identity" => {
                // Seperti Mullvad/Tor: tutup semua tab, hapus cookie, cache, dan riwayat.
                if win::confirm(t(self.lang, "d.new_identity")) {
                    self.new_identity();
                }
            }
            "shield-scripts" => self.toggle_site("scripts"),
            "shield-fp" => self.toggle_site("fp"),
            "shield-dark" => {
                if let Some(h) = self.active_site() {
                    let on = self.settings.borrow().dark_for(&h, shields::system_dark()).is_some();
                    self.set_dark_site(&h, if on { "off" } else { "on" });
                }
            }
            "shield-copy" => {
                if let Some(h) = self.active_site() {
                    let now = {
                        let mut st = self.settings.borrow_mut();
                        let now = toggle_host(&mut st.allow_copy_sites, &h);
                        st.save(data_dir());
                        now
                    };
                    if let Some(wv) = self.active_webview() {
                        if now {
                            let _ = wv.evaluate_script(shields::allow_copy_script());
                        } else {
                            let _ = wv.reload();
                        }
                    }
                    self.refresh_pages("settings");
                }
            }
            "shield-nosleep" => {
                if let Some(h) = self.active_site() {
                    let mut st = self.settings.borrow_mut();
                    toggle_host(&mut st.sleep_never_sites, &h);
                    st.save(data_dir());
                    drop(st);
                    self.refresh_pages("settings");
                }
            }
            "close-duplicates" => self.close_duplicates(),
            "sessions" => self.open_internal("sessions"),
            "shield-ads" => {
                let host = self.tabs.get(self.active).and_then(|t| host_of(&t.url));
                if let Some(h) = host {
                    self.settings.borrow_mut().toggle_exception(&h);
                    self.settings.borrow().save(data_dir());
                    // Pengecualian juga mematikan farbling: bangun ulang tab agar skripnya ikut.
                    self.script_gen += 1;
                    let a = self.active;
                    self.rebuild_tab(a);
                    self.refresh_pages("settings");
                    self.last_render_key.clear();
                    self.render();
                }
            }
            "import" => {
                let m = self.import_bookmarks(None);
                win::info(&m);
            }
            "pick" => {
                if let Some(wv) = self.active_webview() {
                    let _ = wv.evaluate_script("window.__ovnerPick && __ovnerPick()");
                    let _ = wv.focus();
                }
            }
            "split" => {
                if self.split.is_some() {
                    self.stop_split();
                } else {
                    // Pasangan: tab lain yang paling baru dibuka.
                    let a = self.active;
                    let partner = (0..self.tabs.len()).filter(|&j| j != a).max_by_key(|&j| self.tabs[j].last_active);
                    match partner {
                        Some(p) => self.start_split(p),
                        None => {
                            self.open_tab("", false, false);
                            let p = self.active + 1;
                            self.start_split(p);
                        }
                    }
                }
            }
            "tab-split" => {
                if let Some(j) = tab_target {
                    if j == self.active {
                        let _ = self.on_menu("split");
                    } else {
                        self.start_split(j);
                    }
                }
            }
            "downloads" => self.open_internal("downloads"),
            "zoom-in" => self.zoom_active(1),
            "zoom-out" => self.zoom_active(-1),
            "zoom-reset" => self.zoom_active(0),
            "print" => {
                if let Some(wv) = self.active_webview() {
                    let _ = wv.print();
                }
            }
            "findbar" => {
                let _ = self.on_key("findbar");
            }
            "devtools" => {
                if !self.settings.borrow().devtools {
                    win::info(t(self.lang, "d.devtools_off"));
                } else if let Some(wv) = self.active_webview() {
                    wv.open_devtools();
                }
            }
            "sleep-all" => {
                self.each_window(|a| {
                    for j in 0..a.tabs.len() {
                        a.sleep_tab(j);
                    }
                    a.render();
                });
                self.trim_now();
                self.render();
            }
            "trim" => {
                for t in self.all_tabs() {
                    if let Some(wv) = &t.webview {
                        set_memory_low(wv, true);
                    }
                }
                self.next_trim = Some(Instant::now() + Duration::from_secs(2));
            }
            "filters" => {
                start_filter_update_forced(self.proxy.clone(), self.settings.borrow().clone());
                win::info(t(self.lang, "d.filters"));
            }
            "data" => win::open_folder(data_dir()),
            "clear" => {
                if win::confirm(t(self.lang, "d.clear")) {
                    if let Some(wv) = self.all_tabs().find_map(|t| t.webview.as_ref()) {
                        let _ = wv.clear_all_browsing_data();
                    }
                    self.store.clear_history();
                    self.store.flush();
                    self.refresh_pages("history");
                }
            }
            "about" => {
                let mut engine = wry::webview_version().unwrap_or_else(|_| "?".into());
                if let Some(latest) = versions::cached_latest(&data_dir()) {
                    engine = versions::status(&engine, &latest, self.lang == Lang::Id).1;
                }
                let lists = t(self.lang, if self.blocker.borrow().full { "d.lists_full" } else { "d.lists_builtin" });
                let data = data_dir().display().to_string();
                let text = tf(
                    self.lang,
                    "d.about",
                    &[("v", &update::display_version()), ("e", &engine), ("l", lists), ("c", versions::COMPONENTS), ("d", &data), ("r", update::REPO)],
                );
                // Tentang Ovner: sekaligus tawarkan periksa dan pasang update.
                if win::confirm(&format!("{text}{}", t(self.lang, "d.about_check"))) {
                    self.update_prompt = true;
                    self.check_update_now();
                }
            }
            "check-update" => {
                self.update_prompt = true;
                if let Some(v) = self.update_ready.clone() {
                    // Sudah diunduh sebelumnya: langsung tawarkan pasang.
                    send(&self.proxy, UserEvent::UpdateReady(v));
                } else {
                    self.check_update_now();
                }
            }
            "shield-record" => {
                let a = self.tabs.iter().position(|t| t.recording).unwrap_or(self.active);
                if self.tabs.get(a).is_some_and(|t| t.recording) {
                    for t in &mut self.tabs {
                        t.recording = false;
                    }
                    let report = json!({
                        "ovner": update::display_version(),
                        "engine": wry::webview_version().unwrap_or_default(),
                        "url": self.tabs[a].url,
                        "settings": {
                            "blocking": self.settings.borrow().blocking_mode, "dark": self.settings.borrow().dark_mode,
                            "farbling": self.settings.borrow().farbling, "hideWebView2": self.settings.borrow().hide_webview2,
                        },
                        "blocked": self.tabs[a].blocked.get(),
                        "events": self.problems,
                    });
                    win::set_clipboard(&serde_json::to_string_pretty(&report).unwrap_or_default());
                    win::info(&tf(self.lang, "d.record_copied", &[("n", &self.problems.len().to_string())]));
                } else if self.tabs.get(a).is_some_and(|t| t.webview.is_some()) {
                    self.problems.clear();
                    self.record_at = Instant::now();
                    self.tabs[a].recording = true;
                    let (p, id) = (self.proxy.clone(), self.tabs[a].id);
                    let wv = self.tabs[a].webview.as_ref().unwrap();
                    // Langganan kejadian cukup sekali per tab (memulai lagi tidak menggandakan).
                    if self.tabs[a].rec_hooked {
                        win::record_enable(wv);
                    } else {
                        win::record_problems(wv, move |name, j| send(&p, UserEvent::Problem(id, name.to_string(), j)));
                    }
                    let _ = wv.reload();
                    self.tabs[a].rec_hooked = true;
                    win::info(t(self.lang, "d.record_started"));
                }
            }
            "shield-clear-site" => {
                if let Some(h) = self.active_site() {
                    if win::confirm(&tf(self.lang, "d.clear_site", &[("h", &h)])) {
                        self.clear_site_data(&h);
                    }
                }
            }
            "tab-reload" => {
                if let Some(wv) = tab_target.and_then(|j| self.tabs[j].webview.as_ref()) {
                    let _ = wv.reload();
                }
            }
            "tab-new-window" => {
                if let Some(j) = tab_target {
                    self.detach_tab(j, None);
                }
            }
            "new-window" => {
                if self.in_tray {
                    self.show_from_tray();
                }
                self.new_window("", false);
            }
            "new-private-window" => self.new_window("", true),
            "merge-windows" => self.merge_windows(),
            "tab-duplicate" => {
                if let Some(j) = tab_target {
                    let (u, p) = (self.tabs[j].url.clone(), self.tabs[j].private);
                    self.open_tab(&u, true, p);
                }
            }
            "tab-sleep" => {
                if let Some(j) = tab_target {
                    if j == self.active {
                        // Tab aktif tidak bisa tidur: pindah dulu ke tab tetangga.
                        let other = if j + 1 < self.tabs.len() { j + 1 } else { j.saturating_sub(1) };
                        if other != j {
                            self.activate(other);
                            self.sleep_tab(j);
                        }
                    } else {
                        self.sleep_tab(j);
                    }
                    self.render();
                }
            }
            "tab-close" => {
                if let Some(j) = tab_target {
                    self.close_tab(j);
                }
            }
            "tab-close-others" => {
                if let Some(j) = tab_target {
                    let keep = self.tabs[j].id;
                    self.tabs.retain(|t| t.id == keep);
                    self.active = 0;
                    self.activate(0);
                }
            }
            "quit" => return true,
            _ => {}
        }
        self.menu_tab = None;
        false
    }
}

#[cfg(windows)]
fn wry_theme(t: &str) -> wry::Theme {
    match t {
        "light" => wry::Theme::Light,
        "dark" => wry::Theme::Dark,
        _ => wry::Theme::Auto,
    }
}

/// (bisa kembali, bisa maju) untuk tombol toolbar.
fn nav_state(wv: &WebView) -> (bool, bool) {
    #[cfg(windows)]
    {
        use wry::WebViewExtWindows;
        unsafe {
            let core = wv.webview();
            let mut b = windows::core::BOOL::default();
            let mut f = windows::core::BOOL::default();
            let _ = core.CanGoBack(&mut b);
            let _ = core.CanGoForward(&mut f);
            (b.as_bool(), f.as_bool())
        }
    }
    #[cfg(not(windows))]
    {
        let _ = wv;
        (true, true)
    }
}

/// Siapkan daftar filter sesuai setelan lalu susun ulang mesinnya di latar bila berubah:
/// buang daftar yang tidak dipakai, unduh yang belum ada, dan (bila otomatis) perbarui yang
/// lebih tua dari seminggu.
fn start_filter_update(proxy: EventLoopProxy<UserEvent>, s: Settings) {
    filter_update(proxy, s, false);
}

/// Seperti `start_filter_update`, tetapi semua daftar diunduh ulang sekarang.
fn start_filter_update_forced(proxy: EventLoopProxy<UserEvent>, s: Settings) {
    filter_update(proxy, s, true);
}

fn filter_update(proxy: EventLoopProxy<UserEvent>, s: Settings, force: bool) {
    let dir = data_dir().to_path_buf();
    std::thread::spawn(move || {
        let wanted = blocker::wanted_lists(&s);
        blocker::write_custom_rules(&dir, &s.custom_rules);
        blocker::prune_lists(&dir, &wanted);
        let had_cache = blocker::load_cached(&dir).is_some();
        let stale = if force { wanted.clone() } else { blocker::stale_lists(&dir, &wanted, s.filter_update_hours) };
        let mut changed = false;
        if force || (!stale.is_empty() && s.auto_update_filters) {
            if !force {
                std::thread::sleep(Duration::from_secs(3));
            }
            let n = blocker::download_lists(&dir, &stale, false);
            log(&format!("daftar filter berubah: {n}/{}", stale.len()));
            changed = n > 0;
        } else if !stale.is_empty() {
            // Pembaruan otomatis mati: hanya unduh daftar yang belum ada sama sekali.
            changed = blocker::download_lists(&dir, &stale, true) > 0;
        }
        if changed || !had_cache {
            let started = Instant::now();
            if let Some(bytes) = blocker::build_and_cache(&dir) {
                log(&format!("mesin filter disusun dalam {:?}", started.elapsed()));
                send(&proxy, UserEvent::EngineReady(bytes));
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Satu instance

enum Lock {
    Primary(TcpListener),
    Secondary,
    None,
}

fn acquire_lock(cmd: &str, restarted: bool) -> Lock {
    let addr = SocketAddr::from(([127, 0, 0, 1], INSTANCE_PORT));
    let attempts = if restarted { 40 } else { 2 };
    for _ in 0..attempts {
        if let Ok(l) = TcpListener::bind(addr) {
            return Lock::Primary(l);
        }
        let mut ours = false;
        if let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_millis(800)) {
            let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
            let mut line = String::new();
            if BufReader::new(&s).read_line(&mut line).is_ok() && line.trim() == MAGIC {
                ours = true;
                if !restarted {
                    let _ = s.write_all(format!("{cmd}\n").as_bytes());
                    return Lock::Secondary;
                }
            }
        }
        if !ours && !restarted {
            return Lock::None;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Lock::None
}

fn stop_running_instance() {
    let addr = SocketAddr::from(([127, 0, 0, 1], INSTANCE_PORT));
    let mut asked = false;
    if let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_millis(800)) {
        let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
        let mut line = String::new();
        if BufReader::new(&s).read_line(&mut line).is_ok() && line.trim() == MAGIC {
            let _ = s.write_all(b"exit\n");
            asked = true;
        }
    }
    if asked {
        for _ in 0..50 {
            if TcpListener::bind(addr).is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        std::thread::sleep(Duration::from_millis(800));
    }
}

fn serve_lock(listener: TcpListener, proxy: EventLoopProxy<UserEvent>) {
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut s) = conn else { continue };
            let _ = s.write_all(format!("{MAGIC}\n").as_bytes());
            let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
            let mut line = String::new();
            if BufReader::new(&s).read_line(&mut line).is_err() {
                continue;
            }
            let line = line.trim();
            let cmd = match line {
                "show" => Some(Remote::Show),
                "newwin" => Some(Remote::NewWindow),
                "exit" => Some(Remote::Exit),
                _ => line.strip_prefix("open ").filter(|u| u.starts_with("http://") || u.starts_with("https://")).map(|u| Remote::Open(u.to_string())),
            };
            if let Some(c) = cmd {
                send(&proxy, UserEvent::Remote(c));
            }
        }
    });
}


// ---------------------------------------------------------------------------
// Banyak jendela

impl App {
    /// Tukar keadaan jendela saat ini dengan `w`.
    fn swap_state(&mut self, w: &mut WinState) {
        use std::mem::swap;
        swap(&mut self.win_no, &mut w.no);
        swap(&mut self.window, &mut w.window);
        swap(&mut self.toolbar, &mut w.toolbar);
        swap(&mut self.sidebar, &mut w.sidebar);
        swap(&mut self.toolbar_ready, &mut w.toolbar_ready);
        swap(&mut self.toolbar_h, &mut w.toolbar_h);
        swap(&mut self.toolbar_over, &mut w.toolbar_over);
        swap(&mut self.tabs, &mut w.tabs);
        swap(&mut self.active, &mut w.active);
        swap(&mut self.split, &mut w.split);
        swap(&mut self.find_open, &mut w.find_open);
        swap(&mut self.last_render_key, &mut w.last_render_key);
        swap(&mut self.last_icons_key, &mut w.last_icons_key);
        swap(&mut self.background, &mut w.background);
        swap(&mut self.bg_since, &mut w.bg_since);
        swap(&mut self.bg_suspended, &mut w.bg_suspended);
    }

    /// Jadikan jendela nomor `no` sebagai jendela saat ini. False bila tidak ada.
    fn switch_to(&mut self, no: u32) -> bool {
        if self.win_no == no {
            return true;
        }
        let Some(k) = self.others.iter().position(|w| w.no == no) else { return false };
        let mut w = self.others.remove(k);
        self.swap_state(&mut w);
        self.others.insert(k, w);
        true
    }

    fn window_count(&self) -> usize {
        1 + self.others.len()
    }

    /// Nomor semua jendela, jendela saat ini lebih dulu.
    fn window_nos(&self) -> Vec<u32> {
        std::iter::once(self.win_no).chain(self.others.iter().map(|w| w.no)).collect()
    }

    /// Jalankan `f` untuk tiap jendela (masing-masing sebagai jendela saat ini), lalu kembali.
    fn each_window(&mut self, mut f: impl FnMut(&mut App)) {
        let cur = self.win_no;
        if self.others.is_empty() {
            f(self);
            return;
        }
        for no in self.window_nos() {
            if self.switch_to(no) {
                f(self);
            }
        }
        self.switch_to(cur);
    }

    /// Jendela yang memuat tab `id`.
    fn win_of_tab(&self, id: u64) -> Option<u32> {
        if self.tabs.iter().any(|t| t.id == id) {
            return Some(self.win_no);
        }
        self.others.iter().find(|w| w.tabs.iter().any(|t| t.id == id)).map(|w| w.no)
    }

    fn win_of_window(&self, id: tao::window::WindowId) -> Option<u32> {
        if self.window.id() == id {
            return Some(self.win_no);
        }
        self.others.iter().find(|w| w.window.id() == id).map(|w| w.no)
    }

    /// Jendela tujuan sebuah kejadian (None = tidak terikat jendela).
    fn route(&self, ev: &UserEvent) -> Option<u32> {
        use UserEvent::*;
        let tab = match ev {
            Toolbar(no, _) | CloseWindow(no) => return Some(*no),
            Menu(_) => return Some(self.menu_win.get()),
            TrayClick | Remote(_) => return Some(self.focused_win),
            Content(id, _) | Title(id, _) | PageLoad(id, ..) | NewTab(id, _) | Navigate(id, _) | ScriptNav(id, ..) | ProcFailed(id, ..)
            | TabMetrics(id, _) | Anubis(id, _) | Problem(id, ..) | ScriptClose(id) | NavDone(id, ..) | Upgraded(id, _) | Favicon(id, _)
            | Download(id, _) => *id,
            DlProbe(_, tab, ..) => *tab,
            _ => return None,
        };
        self.win_of_tab(tab)
    }

    /// Semua tab beserta keadaan tampilnya (aktif/pasangan split di jendelanya).
    fn tabs_with_shown(&self) -> Vec<(&Tab, bool)> {
        let mut v: Vec<(&Tab, bool)> = self.tabs.iter().enumerate().map(|(j, t)| (t, self.is_shown(j))).collect();
        for w in &self.others {
            v.extend(w.tabs.iter().enumerate().map(|(j, t)| (t, j == w.active || w.split == Some(t.id))));
        }
        v
    }

    /// Semua tab di semua jendela.
    fn all_tabs(&self) -> impl Iterator<Item = &Tab> {
        self.tabs.iter().chain(self.others.iter().flat_map(|w| w.tabs.iter()))
    }

    /// Semua jendela sedang hemat (diminimalkan atau di tray).
    fn all_background(&self) -> bool {
        self.background && self.others.iter().all(|w| w.background)
    }

    /// Pasang WebView tab ke jendela saat ini (tab dipindah dari jendela lain).
    fn attach_webview(&self, wv: &WebView) {
        #[cfg(windows)]
        {
            use tao::platform::windows::WindowExtWindows;
            use wry::WebViewExtWindows;
            if let Err(e) = wv.reparent(self.window.hwnd() as isize) {
                log(&format!("pindah tab ke jendela lain gagal: {e}"));
            }
        }
        #[cfg(not(windows))]
        let _ = wv;
    }

    /// Minta jendela baru dengan satu tab baru (`url` kosong = halaman tab baru).
    fn new_window(&mut self, url: &str, private: bool) {
        let mut tabs = Vec::new();
        if !url.is_empty() {
            let t = self.new_tab_struct(url, "", private || self.settings.borrow().always_private);
            tabs.push(t);
        }
        self.want_windows.push(WinRequest { tabs, active: 0, private, pos: None, maximized: false, from: self.win_no, focus: true });
        log("jendela baru diminta");
    }

    /// Pindahkan tab `i` ke jendela baru (dari menu tab atau ditarik keluar dari daftar tab).
    fn detach_tab(&mut self, i: usize, pos: Option<(f64, f64)>) {
        if self.tabs.len() < 2 || i >= self.tabs.len() {
            return;
        }
        #[allow(unused_mut)]
        let mut t = self.tabs.remove(i);
        if self.split == Some(t.id) {
            self.split = None;
        }
        if let Some(wv) = &t.webview {
            let _ = wv.set_visible(false);
        }
        // Linux (pengujian): WebView tidak dipindah, tab dibangun lagi di jendela baru.
        #[cfg(not(windows))]
        {
            t.webview = None;
        }
        if i < self.active || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1).min(self.tabs.len() - 1);
        }
        let a = self.active;
        self.activate(a);
        log(&format!("tab dipindah ke jendela baru: {}", display_url(&t.url)));
        self.want_windows.push(WinRequest { tabs: vec![t], active: 0, private: false, pos, maximized: false, from: self.win_no, focus: true });
    }

    /// Jendela Ovner lain yang baris tabnya (atau tab vertikalnya) berada di titik layar (logis)
    /// `x`,`y`. Jendela yang paling baru difokuskan didahulukan bila bertumpuk.
    fn window_at(&self, x: f64, y: f64) -> Option<u32> {
        let mut cands: Vec<&WinState> = self.others.iter().filter(|w| !w.window.is_minimized() && w.window.is_visible()).collect();
        cands.sort_by_key(|w| self.z_order.iter().position(|&n| n == w.no).unwrap_or(usize::MAX));
        cands.into_iter().find(|w| {
            let scale = w.window.scale_factor();
            let Ok(p) = w.window.inner_position() else { return false };
            let p: tao::dpi::LogicalPosition<f64> = p.to_logical(scale);
            let size: LogicalSize<f64> = w.window.inner_size().to_logical(scale);
            let (lx, ly) = (x - p.x, y - p.y);
            if lx < 0.0 || ly < -8.0 || lx > size.width || ly > size.height {
                return false;
            }
            ly <= w.toolbar_h + 24.0 || (w.sidebar.is_some() && lx <= SIDEBAR_W + 24.0)
        })
        .map(|w| w.no)
    }

    /// Pindahkan tab `i` ke jendela `no` (diseret ke baris tabnya), di dekat titik jatuhnya.
    /// Jendela asal yang menjadi kosong ditutup.
    fn move_tab_to_window(&mut self, i: usize, no: u32, at: (f64, f64)) {
        if i >= self.tabs.len() || no == self.win_no {
            return;
        }
        let from = self.win_no;
        #[allow(unused_mut)]
        let mut t = self.tabs.remove(i);
        if self.split == Some(t.id) {
            self.split = None;
        }
        if let Some(wv) = &t.webview {
            let _ = wv.set_visible(false);
        }
        #[cfg(not(windows))]
        {
            t.webview = None;
        }
        let empty = self.tabs.is_empty();
        if !empty {
            if i < self.active || self.active >= self.tabs.len() {
                self.active = self.active.saturating_sub(1).min(self.tabs.len() - 1);
            }
            let a = self.active;
            self.activate(a);
        }
        let id = t.id;
        log(&format!("tab dipindah ke jendela {no}: {}", display_url(&t.url)));
        if !self.switch_to(no) {
            // Jendela tujuan hilang: kembalikan tab.
            self.tabs.insert(i.min(self.tabs.len()), t);
            return;
        }
        if let Some(wv) = &t.webview {
            self.attach_webview(wv);
        }
        self.tabs.push(t);
        let k = self.tabs.len() - 1;
        self.layout();
        self.activate(k);
        // Urutan di baris tab mengikuti titik jatuhnya (dihitung halaman toolbar/tab vertikal).
        let scale = self.window.scale_factor();
        if let Ok(p) = self.window.inner_position() {
            let p: tao::dpi::LogicalPosition<f64> = p.to_logical(scale);
            let (x, y) = (at.0 - p.x, at.1 - p.y);
            let js = format!("window.ovner && ovner.dropAt && ovner.dropAt({id},{x},{})", y - self.toolbar_h);
            match &self.sidebar {
                Some(sb) if x <= SIDEBAR_W + 24.0 && y > self.toolbar_h => {
                    let _ = sb.evaluate_script(&js);
                }
                _ => {
                    let _ = self.toolbar.evaluate_script(&format!("window.ovner && ovner.dropAt && ovner.dropAt({id},{x},{y})"));
                }
            }
        }
        self.focused_win = no;
        self.window.set_focus();
        if empty && self.switch_to(from) {
            // Jendela asal kosong: tutup (close_current_window memilih jendela tujuan sebagai pengganti).
            self.close_current_window();
            self.switch_to(no);
        }
        self.save_session();
    }

    /// Gabungkan semua jendela lain ke jendela saat ini.
    fn merge_windows(&mut self) {
        let mut moved: Vec<Tab> = Vec::new();
        for mut w in std::mem::take(&mut self.others) {
            #[allow(unused_mut)]
            for mut t in w.tabs.drain(..) {
                if let Some(wv) = &t.webview {
                    let _ = wv.set_visible(false);
                    #[cfg(windows)]
                    self.attach_webview(wv);
                }
                #[cfg(not(windows))]
                {
                    t.webview = None;
                }
                moved.push(t);
            }
            w.sidebar = None;
            w.window.set_visible(false);
            drop(w);
        }
        if moved.is_empty() {
            return;
        }
        log(&format!("{} tab digabung ke jendela ini", moved.len()));
        self.tabs.extend(moved);
        self.focused_win = self.win_no;
        self.layout();
        let a = self.active;
        self.activate(a);
    }

    /// Tutup jendela saat ini beserta tabnya. False bila ini jendela terakhir (perilaku lama:
    /// ke tray atau keluar).
    fn close_current_window(&mut self) -> bool {
        if self.others.is_empty() {
            return false;
        }
        let busy = self.tabs.iter().filter(|t| t.downloads_live > 0).count();
        if busy > 0 && !win::confirm(&tf(self.lang, "d.close_window_dl", &[("n", &busy.to_string())])) {
            return true;
        }
        for t in &self.tabs {
            if !is_newtab(&t.url) && !t.private && internal_page(&t.url).is_none() {
                self.closed.push(t.url.clone());
            }
        }
        let over = self.closed.len().saturating_sub(25);
        self.closed.drain(..over);
        // Jendela lain yang terakhir dipakai menjadi jendela saat ini.
        let k = self.others.iter().position(|w| w.no == self.focused_win).unwrap_or(self.others.len() - 1);
        let mut w = self.others.remove(k);
        self.swap_state(&mut w);
        w.window.set_visible(false);
        w.tabs.clear();
        w.sidebar = None;
        drop(w);
        log(&format!("jendela ditutup, sisa {}", self.window_count()));
        self.focused_win = self.win_no;
        self.last_render_key.clear();
        self.render();
        self.window.set_focus();
        self.save_session();
        true
    }

    /// Buat satu jendela baru beserta toolbarnya, lalu isi dengan tab permintaan itu.
    fn create_window(&mut self, target: &EventLoopWindowTarget<UserEvent>, req: WinRequest) -> Option<u32> {
        let back = self.win_no;
        // Ukuran seperti jendela saat ini; posisi bertingkat sedikit ke kanan bawah.
        let scale = self.window.scale_factor();
        let cur: LogicalSize<f64> = self.window.inner_size().to_logical(scale);
        let size = if cur.width >= 480.0 && cur.height >= 320.0 && !self.window.is_maximized() { (cur.width, cur.height) } else { (1100.0, 720.0) };
        let pos = req.pos.map(|(x, y)| (x - 120.0, (y - 20.0).max(0.0))).or_else(|| {
            let p: tao::dpi::LogicalPosition<f64> = self.window.outer_position().ok()?.to_logical(scale);
            let n = self.others.len() as f64 + 1.0;
            Some((p.x.max(0.0) + 32.0 * n, p.y.max(0.0) + 32.0 * n))
        });
        let theme = self.settings.borrow().theme.clone();
        let restore = |app: &mut App, tabs: Vec<Tab>| {
            if app.switch_to(req.from) || app.switch_to(back) {
                for t in tabs {
                    if let Some(wv) = &t.webview {
                        app.attach_webview(wv);
                    }
                    app.tabs.push(t);
                }
                app.render();
            }
        };
        let window = match build_window(target, &theme, !self.in_tray, req.maximized, size, pos) {
            Ok(w) => Rc::new(w),
            Err(e) => {
                log(&format!("jendela baru gagal dibuat: {e}"));
                restore(self, req.tabs);
                return None;
            }
        };
        let no = self.next_win;
        self.next_win += 1;
        let toolbar = match build_toolbar(
            &window,
            &mut self.web_context,
            &self.proxy,
            no,
            &self.browser_args,
            &theme,
            #[cfg(windows)]
            self.env.as_ref(),
        ) {
            Ok(t) => t,
            Err(e) => {
                log(&format!("toolbar jendela baru gagal dibuat: {e}"));
                restore(self, req.tabs);
                return None;
            }
        };
        self.others.push(WinState {
            tabs: Vec::new(),
            toolbar,
            sidebar: None,
            window,
            no,
            toolbar_ready: false,
            toolbar_h: 77.0,
            toolbar_over: 0.0,
            active: 0,
            split: None,
            find_open: false,
            last_render_key: String::new(),
            last_icons_key: String::new(),
            background: self.in_tray,
            bg_since: self.in_tray.then(Instant::now),
            bg_suspended: false,
        });
        self.switch_to(no);
        self.ensure_sidebar();
        for t in req.tabs {
            if let Some(wv) = &t.webview {
                self.attach_webview(wv);
            }
            self.tabs.push(t);
        }
        if self.tabs.is_empty() {
            let nt = self.newtab_url();
            let private = req.private || self.settings.borrow().always_private;
            let t = self.new_tab_struct(&nt, t(self.lang, "p.newtab"), private);
            self.tabs.push(t);
        }
        let a = req.active.min(self.tabs.len() - 1);
        // Linux (pengujian): naikkan toolbar di atas kotak GTK jendela (lihat StartCause::Init).
        #[cfg(not(windows))]
        {
            let _ = self.toolbar.set_visible(false);
            let _ = self.toolbar.set_visible(true);
        }
        self.layout();
        if self.in_tray {
            // Ovner di tray: tab dimuat saat jendela dibuka.
            self.active = a;
        } else {
            self.activate(a);
            if is_newtab(&self.tabs[a].url) && req.focus {
                self.focus_address();
            }
        }
        log(&format!("jendela {no} dibuka ({} tab), total {} jendela", self.tabs.len(), self.window_count()));
        Some(no)
    }

    /// Buat jendela yang diminta (dipanggil dari event loop, yang punya target jendelanya).
    fn create_pending_windows(&mut self, target: &EventLoopWindowTarget<UserEvent>) {
        let back = self.win_no;
        let mut focus_to = None;
        for req in std::mem::take(&mut self.want_windows) {
            let focus = req.focus;
            if let Some(no) = self.create_window(target, req) {
                if focus {
                    focus_to = Some(no);
                }
            }
        }
        match focus_to {
            Some(no) => {
                self.switch_to(no);
                self.focused_win = no;
                if !self.in_tray {
                    self.window.set_focus();
                }
            }
            None => {
                self.switch_to(back);
            }
        }
        self.save_session();
    }
}

// ---------------------------------------------------------------------------
// Start

struct Args {
    portable: bool,
    uninstall: bool,
    restarted: bool,
    tray: bool,
    /// Proses admin dari pemasang/update: salin exe ini ke Program Files lalu keluar.
    install_machine: bool,
    desktop: bool,
    /// Dibuka ulang dari "pasang update sekarang".
    apply_update: bool,
    /// `--new-window`: minta Ovner yang sedang jalan membuka jendela baru.
    new_window: bool,
    urls: Vec<String>,
}

fn parse_args() -> Args {
    let mut a = Args { portable: false, uninstall: false, restarted: false, tray: false, install_machine: false, desktop: false, apply_update: false, new_window: false, urls: Vec::new() };
    for v in std::env::args().skip(1) {
        match v.as_str() {
            "--portable" => a.portable = true,
            "--uninstall" => a.uninstall = true,
            "--restarted" => a.restarted = true,
            "--tray" => a.tray = true,
            "--install-machine" => a.install_machine = true,
            "--desktop" => a.desktop = true,
            "--apply-update" => a.apply_update = true,
            "--new-window" => a.new_window = true,
            s if s.starts_with("http://") || s.starts_with("https://") => a.urls.push(s.to_string()),
            s if s.starts_with("ovner://") => a.urls.push(internal_url(s.trim_start_matches("ovner://").trim_end_matches('/'))),
            _ => {}
        }
    }
    a
}

/// Jendela pemasang. Mengembalikan exe terpasang yang harus dijalankan (None = batal).
fn run_setup(exe: &Path) -> Option<PathBuf> {
    let lang = i18n::system_lang();
    let id = matches!(lang, i18n::Lang::Id);
    let user = install::installed_exe().exists().then(install::installed_version).flatten();
    let machine = install::machine_exe().exists().then(install::machine_installed_version).flatten();
    let html = setup_ui::page(if id { "id" } else { "en" }, &install::default_portable_dir(exe), machine.as_deref(), user.as_deref(), win::is_elevated());
    match setup_ui::run(html, |a| perform_install(exe, a, id)) {
        Ok(r) => r,
        Err(e) => {
            // WebView2 tidak bisa dipakai untuk jendela pemasang: tanya lewat kotak pesan.
            let text = if id {
                format!("Pasang Ovner {}?\n\nYa = Install di Program Files (perlu admin)\nTidak = Portable di {}\nBatal = keluar\n\n({e})", env!("CARGO_PKG_VERSION"), install::default_portable_dir(exe).display())
            } else {
                format!("Install Ovner {}?\n\nYes = Install in Program Files (needs admin)\nNo = Portable in {}\nCancel = quit\n\n({e})", env!("CARGO_PKG_VERSION"), install::default_portable_dir(exe).display())
            };
            let action = match win::ask_yes_no_cancel(&text)? {
                true => setup_ui::Action::Machine { desktop: true },
                false => setup_ui::Action::Portable { dir: install::default_portable_dir(exe), desktop: false },
            };
            match perform_install(exe, action, id) {
                Ok(t) => Some(t),
                Err(e) => {
                    win::info(&tf(lang, "d.install_failed", &[("e", &e)]));
                    None
                }
            }
        }
    }
}

/// Kerjakan pilihan pemasang. Ovner yang sedang terbuka ditutup dulu.
fn perform_install(exe: &Path, action: setup_ui::Action, id: bool) -> Result<PathBuf, String> {
    match action {
        setup_ui::Action::User => install::install_self(exe, stop_running_instance),
        setup_ui::Action::Portable { dir, desktop } => {
            stop_running_instance();
            install::install_portable(exe, &dir, desktop)
        }
        setup_ui::Action::Machine { desktop } => {
            stop_running_instance();
            if win::is_elevated() {
                install::install_machine(exe, desktop)?;
            } else {
                let args = if desktop { "--install-machine --desktop" } else { "--install-machine" };
                match win::run_elevated(exe, args) {
                    Ok(0) => {}
                    Ok(code) => return Err(if id { format!("Pemasangan di Program Files gagal (kode {code}).") } else { format!("Installing in Program Files failed (code {code}).") }),
                    Err(_) => {
                        return Err(if id {
                            "Izin admin dibatalkan atau ditolak. Tanpa akses admin, pilih Portable.".into()
                        } else {
                            "Admin permission was cancelled or denied. Without admin access, choose Portable.".into()
                        })
                    }
                }
            }
            // Instalasi per-pengguna lama tidak dipakai lagi (datanya tetap, folder data sama).
            if install::installed_exe().exists() {
                install::remove_user_install();
            }
            Ok(install::machine_exe())
        }
    }
}

/// Pasang update Program Files yang sudah diunduh (perlu admin). `asked` = pengguna baru saja
/// memilih "pasang update sekarang". True bila update terpasang (Ovner harus dibuka ulang).
fn apply_machine_update(exe: &Path, asked: bool) -> bool {
    let Some((staged, v)) = update::machine_staged(data_dir()) else { return false };
    let declined = data_dir().join("update").join("declined.txt");
    if !asked {
        // Ditolak sebelumnya untuk versi ini: tunggu pengguna memilih sendiri lewat menu.
        if std::fs::read_to_string(&declined).is_ok_and(|d| d.trim() == v) {
            return false;
        }
        let lang = i18n::resolve(&Settings::load(data_dir()).language);
        let q = if matches!(lang, i18n::Lang::Id) {
            format!("Update Ovner {v} sudah diunduh. Pasang sekarang? (Ovner terpasang di Program Files, jadi perlu izin admin.)")
        } else {
            format!("Ovner update {v} is downloaded. Install it now? (Ovner is installed in Program Files, so admin permission is needed.)")
        };
        if !win::confirm(&q) {
            let _ = std::fs::write(&declined, &v);
            return false;
        }
    }
    match win::run_elevated(&staged, "--install-machine --update") {
        Ok(0) => {
            log(&format!("update {v} terpasang di {}", exe.display()));
            let _ = std::fs::remove_dir_all(data_dir().join("update"));
            true
        }
        Ok(c) => {
            log(&format!("update {v} gagal dipasang (kode {c})"));
            false
        }
        Err(e) => {
            log(&format!("update {v} tidak dipasang: {e}"));
            false
        }
    }
}

fn run_uninstall(exe: &Path, portable: bool) {
    // Instalasi Program Files: hapus sebagai admin (Apps & features belum tentu meminta UAC).
    if install::is_machine_copy(exe) && !win::is_elevated() {
        if let Err(e) = win::run_elevated(exe, "--uninstall") {
            win::info(&format!("Ovner: {e}"));
        }
        return;
    }
    let lang = i18n::resolve(&Settings::load(&install::data_dir(exe, portable)).language);
    if !win::confirm(t(lang, "d.uninstall")) {
        return;
    }
    stop_running_instance();
    let _ = win::set_autostart(false, exe);
    install::remove_integration();
    let data = install::data_dir(exe, portable);
    if install::is_machine_copy(exe) {
        install::remove_machine_integration();
        win::delete_dirs_later(&[&data, &install::machine_dir()]);
        win::info(t(lang, "d.uninstalled"));
    } else if install::is_installed_copy(exe) {
        win::delete_dirs_later(&[&data, &install::install_dir()]);
        win::info(t(lang, "d.uninstalled"));
    } else {
        win::delete_dirs_later(&[&data]);
        win::info(t(lang, "d.uninstalled_portable"));
    }
}

fn main() {
    // Catat panic ke ovner.log sebelum proses berhenti, supaya penyebab "Ovner tiba-tiba
    // tertutup" bisa dilacak.
    std::panic::set_hook(Box::new(|info| {
        log(&format!("PANIC: {info}"));
    }));
    let args = parse_args();
    let exe = std::env::current_exe().unwrap_or_default();
    let portable = install::is_portable(&exe, args.portable);
    update::cleanup_old();

    if args.uninstall {
        run_uninstall(&exe, portable);
        return;
    }
    // Proses admin dari pemasang / update Program Files: salin exe ini lalu keluar.
    if args.install_machine {
        let tmp = std::env::temp_dir().join("ovner-setup");
        let _ = std::fs::create_dir_all(&tmp);
        let _ = DATA_DIR.set(tmp);
        let code = match install::install_machine(&exe, args.desktop) {
            Ok(t) => {
                log(&format!("terpasang di {}", t.display()));
                0
            }
            Err(e) => {
                win::info(&tf(i18n::system_lang(), "d.install_failed", &[("e", &e)]));
                1
            }
        };
        std::process::exit(code);
    }
    // Dijalankan dari Downloads (Ovner-Setup.exe): pilih Install (Program Files) / Portable.
    if install::should_install(&exe, args.portable) {
        if let Some(target) = run_setup(&exe) {
            if std::process::Command::new(&target).args(&args.urls).spawn().is_err() {
                win::info(&format!("Ovner: {}", target.display()));
            }
        }
        return;
    }

    let data = std::env::var_os("OVNER_DATA").map(PathBuf::from).unwrap_or_else(|| install::data_dir(&exe, portable));
    let _ = std::fs::create_dir_all(&data);
    let _ = DATA_DIR.set(data);
    win::install_crash_logger(&data_dir().join("ovner.log"));
    if install::is_installed_copy(&exe) {
        install::ensure_integration(&exe, false);
    }
    // Instalasi Program Files: update yang sudah diunduh dipasang dengan izin admin.
    if install::is_machine_copy(&exe) && apply_machine_update(&exe, args.apply_update) {
        let mut c = std::process::Command::new(&exe);
        c.arg("--restarted").args(&args.urls);
        let _ = c.spawn();
        return;
    }

    let cmd = args.urls.first().map(|u| format!("open {u}")).unwrap_or_else(|| if args.new_window { "newwin".into() } else { "show".into() });
    let lock = acquire_lock(&cmd, args.restarted);
    if matches!(lock, Lock::Secondary) {
        return;
    }
    win::init_identity();
    log(&format!("Ovner {} mulai (portabel: {portable}, tray: {})", update::display_version(), args.tray));

    let event_loop: EventLoop<UserEvent> = EventLoopBuilder::with_user_event().build();
    let proxy = event_loop.create_proxy();
    if let Lock::Primary(l) = lock {
        serve_lock(l, proxy.clone());
    }
    {
        let p = Mutex::new(proxy.clone());
        tray_icon::menu::MenuEvent::set_event_handler(Some(move |e: tray_icon::menu::MenuEvent| {
            if let Ok(p) = p.lock() {
                send(&p, UserEvent::Menu(e.id.0.clone()));
            }
        }));
        let p = Mutex::new(proxy.clone());
        TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
            if let TrayIconEvent::Click { button: tray_icon::MouseButton::Left, button_state: tray_icon::MouseButtonState::Up, .. } = e {
                if let Ok(p) = p.lock() {
                    send(&p, UserEvent::TrayClick);
                }
            }
        }));
    }

    let settings = Settings::load(data_dir());
    // Simpan selalu: menulis versi skema setelah migrasi.
    settings.save(data_dir());
    let lang = i18n::resolve(&settings.language);
    let cached = blocker::load_cached(data_dir());
    let full = cached.is_some();
    let blocker = Rc::new(RefCell::new(Blocker::new(cached.unwrap_or_else(blocker::builtin_engine), full, data_dir())));
    start_filter_update(proxy.clone(), settings.clone());
    if settings.check_updates && cfg!(windows) {
        let p = proxy.clone();
        let dir = data_dir().to_path_buf();
        let channel = settings.update_channel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(20));
            match update::check_and_stage(&dir, &channel) {
                Ok(Some(v)) => {
                    log(&format!("update {v} siap"));
                    send(&p, UserEvent::UpdateReady(v));
                }
                Ok(None) => {}
                Err(e) => log(&format!("cek update: {e}")),
            }
            // WebView2 terbaru dari Microsoft (paling sering sehari sekali), untuk Tentang dan Diagnostik.
            let installed = wry::webview_version().unwrap_or_default();
            match versions::refresh_latest(&dir, Some(Duration::from_secs(20 * 3600)), win::download) {
                Ok(latest) => log(&format!("WebView2: {}", versions::status(&installed, &latest, true).1)),
                Err(e) => log(&format!("cek WebView2 terbaru: {e}")),
            }
        });
    }

    let window = Rc::new(build_window(&event_loop, &settings.theme, !args.tray, !args.tray, (1280.0, 800.0), None).expect("jendela Ovner"));

    let menus = build_menus(lang);
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menus.tray.clone()))
        .with_tooltip(format!("Ovner {}", update::display_version()))
        .with_icon(tray_icon::Icon::from_rgba(icon::rgba(32), 32, 32).expect("ikon tray"))
        .with_menu_on_left_click(false)
        .build()
        .map_err(|e| log(&format!("tray: {e}")))
        .ok();

    usage::set_user_data_dir(&data_dir().join("WebView2"));
    let mut args_str = settings.browser_args();
    // Anti-blokir: proxy lokal harus jalan sebelum WebView2 dibuat (port masuk argumen).
    let antiblock_port = if settings.antiblock {
        match antiblock::start() {
            Ok(port) => {
                log(&format!("anti-blokir aktif di 127.0.0.1:{port}"));
                args_str.push_str(&antiblock::browser_args(port));
                Some(port)
            }
            Err(e) => {
                log(&format!("anti-blokir gagal dijalankan: {e}"));
                None
            }
        }
    } else {
        None
    };
    #[allow(unused_mut)]
    let mut env_error: Option<String> = None;
    let mut web_context = WebContext::new(Some(data_dir().join("WebView2")));
    #[cfg(windows)]
    let env = match win::create_environment(
        &data_dir().join("WebView2"),
        &args_str,
        settings.edge_tracking_prevention,
        settings.local_crash_reports,
        settings.extensions_enabled,
    ) {
        Ok(e) => Some(e),
        Err(e) => {
            log(&format!("environment WebView2 sendiri gagal ({e}); memakai bawaan wry"));
            env_error = Some(e.to_string());
            None
        }
    };
    let toolbar = match build_toolbar(
        &window,
        &mut web_context,
        &proxy,
        0,
        &args_str,
        &settings.theme,
        #[cfg(windows)]
        env.as_ref(),
    ) {
        Ok(w) => w,
        Err(e) => {
            win::info(&tf(lang, "d.need_webview2", &[("e", &e.to_string())]));
            return;
        }
    };

    // Pemetaan proses per tab: bila Ovner pernah mati saat menunggu hasilnya (penanda
    // tertinggal), fitur ini dimatikan supaya Ovner tidak tertutup lagi.
    shields::SYSTEM_DARK.store(win::system_dark(), std::sync::atomic::Ordering::Relaxed);
    let frames_sentinel = data_dir().join(".proses-tab");
    let frames_ok = if frames_sentinel.exists() {
        let _ = std::fs::remove_file(&frames_sentinel);
        log("pemetaan proses per tab dimatikan: Ovner pernah tertutup saat memintanya");
        let _ = std::fs::write(data_dir().join(".proses-tab-mati"), b"1");
        false
    } else {
        !data_dir().join(".proses-tab-mati").exists()
    };
    {
        let p = proxy.clone();
        usage::start_monitor(move |stats| send(&p, UserEvent::Usage(stats)));
    }

    let mut app = App {
        window,
        toolbar,
        sidebar: None,
        toolbar_ready: false,
        toolbar_h: 77.0,
        tabs: Vec::new(),
        active: 0,
        next_id: Rc::new(Cell::new(0)),
        popups: Rc::new(RefCell::new(Vec::new())),
        closed: Vec::new(),
        settings: Rc::new(RefCell::new(settings)),
        blocker,
        store: Store::load(data_dir()),
        vault: vault::Vault::load(data_dir()),
        pw_offer: None,
        downloads: {
            let mut v: Vec<Download> =
                std::fs::read_to_string(data_dir().join("downloads.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
            // Unduhan yang belum selesai saat Ovner ditutup tidak bisa dilanjutkan lagi.
            for d in &mut v {
                // Video yang ditunda tetap menunggu dilanjutkan.
                if d.state == "paused" && d.vspec.is_some() {
                    continue;
                }
                if d.state == "progress" || d.state == "paused" {
                    d.state = "failed".into();
                    // Sisa berkas setengah jadi (.ovnerpart/.crdownload) tidak bisa dilanjutkan:
                    // jangan biarkan memakan ruang disk (mis. 4 GB Parrot).
                    if !d.path.is_empty() && !Path::new(&d.path).exists() {
                        delete_download_files(&d.path);
                    }
                }
            }
            v
        },
        dl_bubble: false,
        dl_jobs: HashMap::new(),
        dl_skip: Rc::new(RefCell::new(HashSet::new())),
        dl_render_at: Instant::now(),
        dl_page_at: Instant::now(),
        dl_meta: HashMap::new(),
        restarting: false,
        apply_update: false,
        dl_cancelled: HashSet::new(),
        dl_forced: HashSet::new(),
        dl_waiting: HashMap::new(),
        dl_yield: HashSet::new(),
        dl_no_yield: HashSet::new(),
        eco_on: false,
        pdf_race: HashMap::new(),
        pdf_learned: std::fs::read_to_string(data_dir().join("pdf_endpoints.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default(),
        bm_edit: None,
        bm_edit_from: None,
        last_bm_folder: String::new(),
        record_at: Instant::now(),
        video_jobs: HashMap::new(),
        toolbar_over: 0.0,
        web_context,
        #[cfg(windows)]
        env,
        proxy,
        browser_args: args_str,
        update_ready: None,
        update_prompt: false,
        last_sleep_check: Instant::now(),
        last_diag: Instant::now(),
        last_render_key: String::new(),
        last_icons_key: String::new(),
        ui_frame: None,
        ui_pids: Vec::new(),
        menus,
        menu_tab: None,
        _tray: tray,
        in_tray: args.tray,
        background: false,
        next_trim: None,
        find_open: false,
        win_no: 0,
        others: Vec::new(),
        next_win: 1,
        focused_win: 0,
        menu_win: Cell::new(0),
        want_windows: Vec::new(),
        bg_since: None,
        bg_suspended: false,
        z_order: vec![0],
        eco_pids: HashSet::new(),
        eco_at: Instant::now(),
        video_req: 0,
        video_win: 0,
        lang,
        session_key: shields::session_key(),
        script_gen: 0,
        split: None,
        cname: Arc::new(Mutex::new(HashMap::new())),
        last_unresponsive: HashMap::new(),
        extensions_loaded: false,
        sampler: usage::Sampler::new(),
        stats: Vec::new(),
        stats_at: Instant::now(),
        renderer_frames: Vec::new(),
        frames_pending: false,
        frames_at: Instant::now(),
        frames_ok,
        problems: Vec::new(),
        tab_stats: std::fs::read_to_string(data_dir().join("tab-stats.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default(),
        sessions: std::fs::read_to_string(data_dir().join("sessions.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default(),
        antiblock_port,
        env_error,
        diag: Vec::new(),
        quit_requested: false,
        icons_due: Some(Instant::now() + Duration::from_secs(20)),
        icons_tried: HashSet::new(),
    };
    app.ensure_sidebar();
    // Unduhan dari riwayat mendapat id sendiri (id unduhan baru dimulai dari 1).
    for (k, d) in app.downloads.iter_mut().enumerate() {
        d.id = (1 << 40) + k as u64;
    }

    // Pulihkan tab terakhir: hanya tab aktif yang dimuat, sisanya tidur sampai diklik.
    let restore = app.settings.borrow().restore_tabs;
    let session: Session = if restore {
        std::fs::read_to_string(data_dir().join("session.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    } else {
        Session::default()
    };
    for st in &session.tabs {
        let t = app.new_tab_struct(&st.url, &st.title, false);
        app.tabs.push(t);
    }
    for u in &args.urls {
        let t = app.new_tab_struct(u, "", false);
        *t.allow_http.borrow_mut() = Some(u.clone());
        app.tabs.push(t);
    }
    // Pertama kali (atau setelah update dari versi tanpa setup): halaman selamat datang.
    let show_setup = !app.settings.borrow().setup_done && args.urls.is_empty();
    if show_setup {
        let t = app.new_tab_struct(&internal_url("setup"), t(lang, "p.setup"), false);
        app.tabs.push(t);
    }
    if app.tabs.is_empty() {
        let nt = app.newtab_url();
        let t = app.new_tab_struct(&nt, t(lang, "p.newtab"), false);
        app.tabs.push(t);
    }
    let first = if show_setup || !args.urls.is_empty() { app.tabs.len() - 1 } else { session.active.min(app.tabs.len() - 1) };
    // Jendela Ovner lain dari sesi terakhir: dibuat di awal event loop (tab tidur sampai diklik).
    for w in &session.windows {
        let tabs: Vec<Tab> = w.tabs.iter().map(|st| app.new_tab_struct(&st.url, &st.title, false)).collect();
        if !tabs.is_empty() {
            app.want_windows.push(WinRequest { tabs, active: w.active, private: false, pos: None, maximized: false, from: 0, focus: false });
        }
    }
    if args.tray {
        // Mulai di tray: tidak ada tab yang dimuat sampai jendela dibuka.
        app.active = first;
        app.background = true;
        if app.settings.borrow().efficiency_mode {
            win::set_background_mode(true);
        }
    } else {
        app.activate(first);
        if is_newtab(&app.tabs[first].url) {
            app.focus_address();
        }
    }

    // Tenggat tick tetap: dulu tiap kejadian (gerak mouse, ketikan) menunda tick 2 detik lagi,
    // sehingga indikator pemakaian dan task manager tidak pernah diperbarui saat dipakai.
    let mut next_tick = Instant::now() + TICK;
    event_loop.run(move |event, target, control_flow| {
        let mut quit = false;
        let mut restart = false;
        let tick_due = Instant::now() >= next_tick;
        if tick_due {
            next_tick = Instant::now() + app.tick_every();
        }
        *control_flow = ControlFlow::WaitUntil(next_tick);
        if tick_due && !matches!(event, Event::NewEvents(StartCause::ResumeTimeReached { .. })) {
            app.on_tick();
        }
        // Arahkan kejadian ke jendelanya: jendela itu menjadi "jendela saat ini".
        match &event {
            Event::WindowEvent { window_id, .. } => match app.win_of_window(*window_id) {
                Some(no) => {
                    app.switch_to(no);
                }
                None => return,
            },
            Event::UserEvent(ev) => {
                if let Some(no) = app.route(ev) {
                    // Pesan terlambat dari jendela yang sudah ditutup: abaikan.
                    if !app.switch_to(no) && matches!(ev, UserEvent::Toolbar(..) | UserEvent::CloseWindow(_)) {
                        return;
                    }
                }
            }
            _ => {}
        }
        match event {
            // Linux (pengujian): toolbar dibuat sebelum jendela tampil sehingga tertutup kotak
            // GTK jendela; tampilkan ulang supaya naik ke atas.
            #[cfg(not(windows))]
            Event::NewEvents(StartCause::Init) => {
                let _ = app.toolbar.set_visible(false);
                let _ = app.toolbar.set_visible(true);
                app.layout();
            }
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
                if tick_due {
                    app.on_tick();
                }
            }
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => {
                    if app.close_current_window() {
                        // Jendela lain masih terbuka.
                    } else if app.settings.borrow().close_to_tray && app._tray.is_some() {
                        app.hide_to_tray();
                    } else {
                        quit = true;
                    }
                }
                WindowEvent::Resized(size) => {
                    if size.width == 0 || size.height == 0 || app.window.is_minimized() {
                        app.enter_background();
                    } else {
                        if !app.in_tray {
                            app.leave_background();
                        }
                        app.layout();
                        app.push_usage();
                    }
                }
                WindowEvent::Moved(_) => app.push_usage(),
                WindowEvent::ScaleFactorChanged { .. } => app.layout(),
                WindowEvent::ThemeChanged(_) => {
                    let dark = win::system_dark();
                    if dark != shields::system_dark() {
                        shields::SYSTEM_DARK.store(dark, std::sync::atomic::Ordering::Relaxed);
                        if app.settings.borrow().dark_mode == "system" {
                            app.apply_dark_live();
                        }
                    }
                }
                WindowEvent::Focused(true) => {
                    app.focused_win = app.win_no;
                    let no = app.win_no;
                    app.z_order.retain(|&w| w != no);
                    app.z_order.insert(0, no);
                    if !app.in_tray {
                        app.leave_background();
                    }
                    if let Some(wv) = app.active_webview() {
                        let _ = wv.focus();
                    }
                }
                _ => {}
            },
            Event::UserEvent(ev) => match ev {
                UserEvent::Toolbar(_, body) => restart = app.on_toolbar(&body),
                UserEvent::Content(id, body) => restart = app.on_content(id, &body),
                UserEvent::Title(id, title) => {
                    if let Some(i) = app.tab_index(id) {
                        app.tabs[i].title = title.clone();
                        if !app.tabs[i].private && app.settings.borrow().save_history {
                            let u = app.tabs[i].url.clone();
                            app.store.set_title(&u, &title);
                        }
                        app.render();
                    }
                }
                UserEvent::PageLoad(id, ev, url) => {
                    if let Some(i) = app.tab_index(id) {
                        let started = matches!(ev, PageLoadEvent::Started);
                        let t = &mut app.tabs[i];
                        t.loading = started;
                        if started {
                            t.form_dirty = false;
                        }
                        // Rekam masalah: hidupkan lagi DevTools di halaman baru (lihat win::record_enable).
                        if t.recording {
                            if let Some(wv) = &t.webview {
                                win::record_enable(wv);
                            }
                        }
                        if url.starts_with("http") && t.recent_urls.last() != Some(&url) {
                            t.recent_urls.push(url.clone());
                            if t.recent_urls.len() > 6 {
                                t.recent_urls.remove(0);
                            }
                        }
                        if !url.is_empty() {
                            t.url = url.clone();
                        }
                        if is_newtab(&t.url) {
                            t.title = i18n::t(app.lang, "p.newtab").into();
                        } else if let Some(name) = internal_page(&t.url) {
                            t.title = i18n::t(app.lang, &format!("p.{name}")).into();
                        }
                        if started && !t.private && internal_page(&t.url).is_none() && app.settings.borrow().save_history {
                            let (u, ti) = (t.url.clone(), t.title.clone());
                            app.store.visit(&u, &ti);
                        }
                        if !started {
                            app.save_session();
                        }
                        app.render();
                    }
                }
                UserEvent::NewTab(opener, url) => {
                    let private = app.tab_index(opener).is_some_and(|i| app.tabs[i].private);
                    app.open_tab(&url, true, private);
                }
                UserEvent::Navigate(id, url) => {
                    if let Some(i) = app.tab_index(id) {
                        if let Some(wv) = &app.tabs[i].webview {
                            let _ = wv.load_url(&url);
                        }
                    }
                }
                UserEvent::Download(tab, ev) => app.on_download(tab, ev),
                UserEvent::DlProbe(id, tab, req, path, res) => app.on_dl_probe(id, tab, req, path, res),
                UserEvent::DlUpdate(id, u) => app.on_dl_update(id, u),
                UserEvent::PdfRace(id, m) => app.on_pdf_race(id, m),
                UserEvent::DlServer(id, info) => {
                    app.dl_meta.entry(id).or_default().server = info;
                    app.last_render_key.clear();
                    app.render();
                }
                UserEvent::Video(id, ev) => app.on_video(id, ev),
                UserEvent::VideoInfo(req, r) => app.on_video_info(req, r),
                UserEvent::VideoJob(id, job) => {
                    app.video_jobs.insert(id, job);
                }
                UserEvent::Favicon(id, png) => {
                    if let Some(i) = app.tab_index(id) {
                        let url = app.tabs[i].webview.as_ref().and_then(|w| w.url().ok()).unwrap_or_default();
                        if url.starts_with("http") && internal_page(&url).is_none() {
                            app.store.set_icon(&url, &png);
                            app.render();
                        }
                    }
                }
                UserEvent::CloseWindow(_) => {
                    if app.close_current_window() {
                    } else if app.settings.borrow().close_to_tray && app._tray.is_some() {
                        app.hide_to_tray();
                    } else {
                        quit = true;
                    }
                }
                UserEvent::ScriptNav(id, url, enabled) => {
                    if let Some(i) = app.tab_index(id) {
                        let t = &app.tabs[i];
                        t.scripts_on.set(enabled);
                        if let Some(wv) = &t.webview {
                            win::set_script_enabled(wv, enabled);
                            let _ = wv.load_url(&url);
                        }
                    }
                }
                UserEvent::IconFetched(host, bytes) => {
                    if app.store.set_icon_host(&host, &bytes) {
                        app.last_render_key.clear();
                        app.render();
                    }
                }
                UserEvent::DiagResult(name, ok, msg) => {
                    log(&format!("diagnostik {name}: {} {msg}", if ok { "OK" } else { "GAGAL" }));
                    app.diag.retain(|d| d.0 != name);
                    app.diag.push((name, ok, msg));
                    app.refresh_pages("diagnostics");
                }
                UserEvent::TabMetrics(id, json_text) => {
                    if let Some(i) = app.tab_index(id) {
                        let v: Value = serde_json::from_str(&json_text).unwrap_or_default();
                        let metric = |name: &str| {
                            v["metrics"].as_array().and_then(|a| a.iter().find(|m| m["name"] == name)).and_then(|m| m["value"].as_f64())
                        };
                        let t = &mut app.tabs[i];
                        if let Some(h) = metric("JSHeapUsedSize") {
                            t.js_heap = h as u64;
                        }
                        if let Some(task) = metric("TaskDuration") {
                            // Waktu kerja utas utama halaman (detik) → persen dari satu inti,
                            // dibagi jumlah inti agar sebanding dengan Task Manager.
                            let now = Instant::now();
                            if let Some((prev, at)) = t.perf_prev {
                                let wall = now.duration_since(at).as_secs_f64();
                                let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f64;
                                if wall > 0.2 {
                                    t.tab_cpu = ((task - prev).max(0.0) / wall / cores * 100.0).clamp(0.0, 100.0);
                                }
                            }
                            t.perf_prev = Some((task, now));
                        }
                    }
                    app.refresh_pages("tasks");
                }
                UserEvent::Usage(stats) => {
                    app.stats = stats;
                    app.assign_tab_procs();
                    let cur = app.win_no;
                    app.each_window(|a| {
                        if a.win_no == cur || !a.background {
                            a.push_usage();
                        }
                    });
                    app.refresh_pages("tasks");
                }
                UserEvent::RendererFrames(frames) => {
                    app.frames_pending = false;
                    app.renderer_frames = frames;
                    app.assign_tab_procs();
                }
                UserEvent::PopupReady => app.adopt_popups(),
                UserEvent::Problem(id, name, json_text) => app.on_problem(id, &name, &json_text),
                UserEvent::DeleteCookies(base, json_text) => {
                    let v: Value = serde_json::from_str(&json_text).unwrap_or_default();
                    if let Some(wv) = app.all_tabs().find_map(|t| t.webview.as_ref()) {
                        let mut n = 0;
                        for c in v["cookies"].as_array().into_iter().flatten() {
                            let domain = c["domain"].as_str().unwrap_or("").trim_start_matches('.');
                            if netfilter_base_domain(domain) == base {
                                let p = json!({"name": c["name"], "domain": c["domain"], "path": c["path"]}).to_string();
                                win::cdp(wv, "Network.deleteCookies", &p);
                                n += 1;
                            }
                        }
                        log(&format!("cookie dihapus: {base} ({n})"));
                    }
                    app.reload_site(&base);
                }
                UserEvent::ScriptClose(id) => {
                    if let Some(i) = app.tab_index(id).filter(|&i| app.tabs[i].popup) {
                        app.close_tab(i);
                    }
                }
                UserEvent::Anubis(id, js) => {
                    if let Some(i) = app.tab_index(id) {
                        if let Some(wv) = &app.tabs[i].webview {
                            let _ = wv.evaluate_script(&js);
                        }
                    }
                }
                UserEvent::ProcFailed(id, kind, detail) => {
                    log(&format!("proses tab {id} gagal: {kind:?} ({detail})"));
                    if kind == win::ProcFail::Browser && !app.restarting {
                        log_crash_report();
                    }
                    match kind {
                        win::ProcFail::Browser => {
                            // Seluruh mesin berhenti: buka ulang Ovner dengan tab yang sama.
                            restart = true;
                        }
                        win::ProcFail::Render => {
                            if let Some(i) = app.tab_index(id) {
                                if app.is_shown(i) {
                                    if let Some(wv) = &app.tabs[i].webview {
                                        let _ = wv.reload();
                                    }
                                } else {
                                    app.sleep_tab(i);
                                    app.render();
                                }
                            }
                        }
                        win::ProcFail::Unresponsive => {
                            let recent = app.last_unresponsive.get(&id).is_some_and(|t| t.elapsed() < Duration::from_secs(60));
                            if let Some(i) = app.tab_index(id).filter(|_| !recent) {
                                app.last_unresponsive.insert(id, Instant::now());
                                let title = app.tabs[i].title.clone();
                                if win::confirm(&tf(app.lang, "d.unresponsive", &[("t", &title)])) {
                                    if let Some(wv) = &app.tabs[i].webview {
                                        let _ = wv.reload();
                                    }
                                }
                            }
                        }
                        win::ProcFail::Other => {}
                    }
                }
                UserEvent::Upgraded(id, from) => {
                    if let Some(i) = app.tab_index(id) {
                        app.tabs[i].upgraded_from = Some(from);
                    }
                }
                UserEvent::NavDone(id, ok, status) => {
                    // Navigasi tab selesai tanpa menjadi unduhan: hapus tampilan menunggunya.
                    let stale: Vec<u64> = app.dl_waiting.iter().filter(|(_, w)| w.0 == id).map(|(k, _)| *k).collect();
                    for k in stale {
                        app.dl_waiting.remove(&k);
                        app.drop_waiting_entry(k);
                    }
                    if let Some(i) = app.tab_index(id) {
                        let t = &mut app.tabs[i];
                        // Situs tanpa HTTPS: kembali ke alamat http:// aslinya (seperti Brave).
                        // Galat koneksi/sertifikat (kode 1-12), bukan galat HTTP dari server.
                        if !ok && (1..=12).contains(&status) {
                            if let Some(orig) = t.upgraded_from.take() {
                                let strict = app.settings.borrow().https_strict;
                                log(&format!("HTTPS gagal ({status}), {} {orig}", if strict { "peringatan untuk" } else { "kembali ke" }));
                                let to = if strict {
                                    // HTTPS-only ketat: tanya dulu sebelum membuka versi http.
                                    let enc: String = url::form_urlencoded::byte_serialize(orig.as_bytes()).collect();
                                    format!("{}#{enc}", internal_url("https"))
                                } else {
                                    *t.allow_http.borrow_mut() = Some(orig.clone());
                                    orig
                                };
                                if let Some(wv) = &t.webview {
                                    let _ = wv.load_url(&to);
                                }
                            }
                        } else if ok {
                            t.upgraded_from = None;
                        }
                    }
                }
                UserEvent::EngineReady(bytes) => {
                    if let Some(engine) = blocker::engine_from_bytes(&bytes) {
                        *app.blocker.borrow_mut() = Blocker::new(engine, true, data_dir());
                        log("pemblokir memakai daftar filter lengkap");
                        app.refresh_pages("settings");
                        app.render();
                    }
                }
                UserEvent::UpdateReady(v) => {
                    app.update_ready = Some(v.clone());
                    app.render();
                    // Diminta lewat menu/Tentang: langsung tawarkan pasang dan buka ulang.
                    if std::mem::take(&mut app.update_prompt) && win::confirm(&tf(app.lang, "d.update_now", &[("v", &v)])) {
                        app.save_session();
                        app.apply_update = true;
                        restart = true;
                    }
                }
                UserEvent::UpdateInfo(msg) if app.update_prompt || !msg.is_empty() => {
                    app.update_prompt = false;
                    win::info(&msg);
                }
                UserEvent::UpdateInfo(_) => {}
                UserEvent::Menu(id) => quit = app.on_menu(&id),
                UserEvent::TrayClick => {
                    if app.in_tray {
                        app.show_from_tray();
                    } else {
                        app.hide_to_tray();
                    }
                }
                UserEvent::Remote(r) => match r {
                    // Ovner dibuka lagi (ikon/menu Start): seperti Brave, buka jendela baru;
                    // bila Ovner di tray, cukup tampilkan lagi.
                    Remote::Show if app.in_tray => app.show_from_tray(),
                    Remote::Show | Remote::NewWindow => {
                        if app.in_tray {
                            app.show_from_tray();
                        }
                        app.new_window("", false);
                    }
                    Remote::Open(u) => {
                        app.show_from_tray();
                        app.open_tab(&u, true, false);
                        let a = app.active;
                        *app.tabs[a].allow_http.borrow_mut() = Some(u);
                    }
                    Remote::Exit => quit = true,
                },
            },
            _ => {}
        }
        // Baru keluar dari mode diam (jendela dibuka lagi, unduhan mulai): kembali ke tick 1 detik.
        let every = app.tick_every();
        if next_tick > Instant::now() + every {
            next_tick = Instant::now() + every;
            *control_flow = ControlFlow::WaitUntil(next_tick);
        }
        if !app.want_windows.is_empty() && !quit && !app.quit_requested {
            app.create_pending_windows(target);
        }
        if app.quit_requested {
            quit = true;
        }
        // Mesin WebView2 mati: semua tab melapor gagal sekaligus. Buka ulang Ovner sekali saja
        // (dulu tiap tab membuka ulang sendiri sehingga muncul 5 jendela Ovner).
        if restart && !app.restarting {
            app.restarting = true;
            app.save_session();
            app.save_downloads();
            if let Ok(exe) = std::env::current_exe() {
                let mut c = std::process::Command::new(exe);
                c.arg("--restarted");
                if app.apply_update {
                    c.arg("--apply-update");
                }
                let _ = c.spawn();
            }
            quit = true;
        } else if restart {
            quit = true;
        }
        if quit {
            app.save_session();
            app.store.flush();
            if app.settings.borrow().clear_on_exit {
                if let Some(wv) = app.all_tabs().find_map(|t| t.webview.as_ref()) {
                    let _ = wv.clear_all_browsing_data();
                    std::thread::sleep(Duration::from_millis(1200));
                }
            }
            // Keluar normal: penanda pemetaan proses per tab bukan tanda crash.
            let _ = std::fs::remove_file(data_dir().join(".proses-tab"));
            // Unduhan mesin Ovner yang belum selesai dibatalkan (berkas .ovnerpart dibuang).
            for (job, _) in app.dl_jobs.values() {
                job.cancel();
            }
            // Buang semua WebView sebelum keluar supaya proses WebView2 ikut berhenti.
            app.tabs.clear();
            for w in &mut app.others {
                w.tabs.clear();
                w.sidebar = None;
            }
            app.others.clear();
            *control_flow = ControlFlow::Exit;
        }
    });
}

#[cfg(test)]
mod pdf_key_tests {
    #[test]
    fn keys() {
        assert_eq!(super::pdf_key("https://SIPP.bpjsketenagakerjaan.go.id/kartu/download?x=1"), "sipp.bpjsketenagakerjaan.go.id/kartu/download");
        assert_eq!(super::pdf_key("http://a.b/c.pdf#p"), "a.b/c.pdf");
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn deletes_download_leftovers_only() {
        let d = std::env::temp_dir().join(format!("ovner-del-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        let names = ["Lagu [abc].mp4", "Lagu [abc].f137.mp4.part", "Lagu [abc].f140.m4a", "Lagu [abc].mp4.ytdl", "big.iso.ovnerpart", "Lagu lain.mp4", "Lagu [abc] (1).mp4"];
        for n in names {
            std::fs::write(d.join(n), b"x").unwrap();
        }
        delete_download_files(&d.join("Lagu [abc].mp4").display().to_string());
        delete_download_files(&d.join("big.iso").display().to_string());
        std::thread::sleep(Duration::from_millis(300));
        let mut left: Vec<String> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        left.sort();
        assert_eq!(left, vec!["Lagu [abc] (1).mp4", "Lagu lain.mp4"]);
        let _ = std::fs::remove_dir_all(&d);
    }
    use super::*;

    #[test]
    fn address_text_to_url() {
        let s = Settings::default();
        assert_eq!(to_url("kompas.com", &s).as_deref(), Some("https://kompas.com"));
        assert_eq!(to_url("detik.com/berita?id=1", &s).as_deref(), Some("https://detik.com/berita?id=1"));
        assert_eq!(to_url("https://a.id/x", &s).as_deref(), Some("https://a.id/x"));
        assert_eq!(to_url("localhost:8080", &s).as_deref(), Some("http://localhost:8080"));
        assert_eq!(to_url("192.168.1.1", &s).as_deref(), Some("http://192.168.1.1"));
        assert_eq!(to_url("harga emas hari ini", &s).as_deref(), Some("https://www.google.com/search?q=harga+emas+hari+ini"));
        assert_eq!(to_url("rust", &s).as_deref(), Some("https://www.google.com/search?q=rust"));
        assert_eq!(to_url("8.8.8.8", &s).as_deref(), Some("https://8.8.8.8"));
        assert_eq!(to_url("ovner://settings", &s).as_deref(), Some(internal_url("settings").as_ref()));
        // !bangs dan No Search
        assert_eq!(to_url("rust !w", &s).as_deref(), Some("https://en.wikipedia.org/w/index.php?search=rust"));
        let mut ns = Settings::default();
        ns.no_search = true;
        assert_eq!(to_url("harga emas", &ns), None);
        assert_eq!(to_url("kompas.com", &ns).as_deref(), Some("https://kompas.com"));
    }

    #[test]
    fn internal_pages_and_display() {
        assert_eq!(internal_page(&internal_url("settings")), Some("settings"));
        assert_eq!(internal_page("https://ovner.example/"), None);
        assert_eq!(display_url(&internal_url("newtab")), "");
        assert_eq!(display_url(&internal_url("history")), "ovner://history");
        assert_eq!(display_url("about:blank"), "");
        assert_eq!(display_url("https://a.id/"), "https://a.id/");
    }

    #[test]
    fn https_upgrade_skips_local_and_internal() {
        assert_eq!(upgrade_https("http://kompas.com/x").as_deref(), Some("https://kompas.com/x"));
        assert_eq!(upgrade_https("http://localhost:8080/"), None);
        assert_eq!(upgrade_https("http://192.168.1.5/"), None);
        assert_eq!(upgrade_https("https://a.id/"), None);
        assert_eq!(upgrade_https(&internal_url("settings")), None);
    }
}
