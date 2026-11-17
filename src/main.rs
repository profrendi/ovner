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
mod usage;
mod settings;
mod vault;
mod shields;
mod store;
mod update;
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
use tao::event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy};
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
    Open(String),
    Exit,
}

enum UserEvent {
    Toolbar(String),
    Content(u64, String),
    Title(u64, String),
    PageLoad(u64, PageLoadEvent, String),
    /// Jendela baru diminta halaman tab `opener`.
    NewTab(u64, String),
    Navigate(u64, String),
    /// Navigasi ulang dengan JavaScript dinyalakan/dimatikan (shields per situs).
    ScriptNav(u64, String, bool),
    /// Proses WebView2 tab ini crash/macet.
    ProcFailed(u64, win::ProcFail),
    /// Hasil CDP Performance.getMetrics untuk satu tab (JSON).
    TabMetrics(u64, String),
    /// Sampel pemakaian semua proses dari thread pemantau (tiap detik).
    Usage(Vec<usage::ProcStat>),
    /// Proses halaman WebView2 dan bingkai utama yang dilayaninya: (pid, [frame id]).
    RendererFrames(Vec<(u32, Vec<u32>)>),
    /// Jawaban tantangan Anubis (anti-bot Startpage dan situs lain) dari pemecah native.
    Anubis(u64, String),
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
    /// Tab terakhir ditutup: tutup jendela seperti tombol X.
    CloseWindow,
    /// Ikon situs (PNG) tab ini berubah.
    Favicon(u64, Vec<u8>),
    Download(u64, win::DlEvent),
    /// Hasil uji server untuk unduhan yang ditahan: (id unduhan, tab, permintaan, ukuran/galat).
    DlProbe(u64, u64, dlengine::Request, String, Result<u64, String>),
    /// Kabar dari mesin unduh Ovner.
    DlUpdate(u64, dlengine::Update),
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
    last_sleep_check: Instant,
    last_diag: Instant,
    last_render_key: String,
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
}

/// Bahan bersama pembuat WebView tab.
#[derive(Clone)]
struct TabEnv {
    window: Rc<Window>,
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
    let init = shields::page_scripts(&s, tenv.session_key, private);
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
    if let Some(u) = url {
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
        .with_ipc_handler(move |req| send(&p_ipc, UserEvent::Content(id, req.body().clone())))
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
                send(&p_nav, UserEvent::Navigate(id, t));
                return false;
            }
            // 4. Blokir JavaScript per situs: setelan berlaku untuk navigasi berikutnya,
            //    jadi navigasi ini dibatalkan lalu diulang dengan setelan yang benar.
            let want = internal_page(&url).is_some() || s.scripts_allowed(&url);
            if want != scripts_on.get() {
                send(&p_nav, UserEvent::ScriptNav(id, url, want));
                return false;
            }
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

    let wv = builder.build_as_child(&*tenv.window)?;
    win::tune_webview(&wv, &s);
    for (method, params) in shields::identity_overrides(&s, &win::user_agent(&wv)) {
        win::cdp(&wv, method, &params);
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
    win::on_process_failed(&wv, move |k| send(&p_fail, UserEvent::ProcFailed(id, k)));
    let p_close = tenv.proxy.clone();
    win::on_window_close(&wv, move || send(&p_close, UserEvent::ScriptClose(id)));
    let p_dl = tenv.proxy.clone();
    let (dl_settings, dl_skip) = (tenv.settings.clone(), tenv.dl_skip.clone());
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
    let size: LogicalSize<f64> = tenv.window.inner_size().to_logical(tenv.window.scale_factor());
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
        &PredefinedMenuItem::separator(),
        &item("copy-urls", "tm.copy_urls"),
        &item("close-duplicates", "m.close_duplicates"),
        &item("tab-close", "tm.close"),
        &item("tab-close-others", "tm.close_others"),
    ]);
    let tray = Menu::new();
    let _ = tray.append_items(&[&item("show", "tray.show"), &item("newtab", "tray.newtab"), &PredefinedMenuItem::separator(), &item("quit", "m.quit")]);
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
        let theme = self.settings.borrow().theme.clone();
        let rect = self.sidebar_rect();
        let b = WebViewBuilder::new_with_web_context(&mut self.web_context)
            .with_html(SIDEBAR_HTML)
            .with_bounds(rect)
            .with_hotkeys_zoom(false)
            .with_devtools(cfg!(debug_assertions))
            .with_ipc_handler(move |req| send(&p, UserEvent::Toolbar(req.body().clone())));
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
        let shown: Vec<usize> = (0..self.tabs.len()).filter(|&j| self.is_shown(j) && internal_page(&self.tabs[j].url).is_none()).collect();
        for j in shown {
            self.rebuild_tab(j);
        }
        self.ensure_sidebar();
        self.layout();
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
            window: self.window.clone(),
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
                    } else if suspend && !t.loading {
                        if playing {
                            pause_media(wv);
                        }
                        win::try_suspend(wv);
                        t.suspended = true;
                    }
                }
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
            send(&self.proxy, UserEvent::CloseWindow);
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
                    win::try_suspend(wv);
                    t.suspended = true;
                }
            }
        }
        if s.efficiency_mode {
            win::set_background_mode(true);
        }
        if s.trim_when_hidden {
            // Beri waktu WebView2 membuang cache dulu, baru working set dikosongkan.
            self.next_trim = Some(Instant::now() + Duration::from_secs(3));
        }
        log(&format!("mode hemat: {}", if self.in_tray { "tray" } else { "diminimalkan" }));
    }

    fn leave_background(&mut self) {
        if !self.background {
            return;
        }
        self.background = false;
        self.next_trim = None;
        if self.settings.borrow().efficiency_mode {
            win::set_background_mode(false);
        }
        // activate() membangunkan tab aktif (dan pasangan split) dan menampilkannya lagi.
        let a = self.active;
        self.activate(a);
    }

    fn hide_to_tray(&mut self) {
        self.in_tray = true;
        self.window.set_visible(false);
        self.enter_background();
    }

    fn show_from_tray(&mut self) {
        self.in_tray = false;
        self.window.set_visible(true);
        self.window.set_minimized(false);
        self.window.set_focus();
        self.leave_background();
        let a = self.active;
        self.activate(a);
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
        let limit = Duration::from_secs(mins * 60);
        let mut changed = false;
        for j in 0..self.tabs.len() {
            let t = &self.tabs[j];
            if self.is_shown(j) || t.webview.is_none() || t.last_active.elapsed() < limit {
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
            "icons": self.store.icons_for(self.store.bookmarks.iter().map(|b| b.url.as_str()).chain(self.tabs.iter().map(|t| t.url.as_str()))),
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
            "downloads": self.downloads.iter().rev().take(6).map(Self::download_view).collect::<Vec<_>>(),
            "dlBubble": self.dl_bubble,
            "pwOffer": self.pw_offer.as_ref().map(|(u, user, _, update)| json!({"site": vault::site_of(u), "user": user, "update": update})),
            "toolbarCss": s.toolbar_css,
            "dark": dark,
        });
        drop(s);
        let text = state.to_string();
        if text == self.last_render_key {
            return;
        }
        let _ = self.toolbar.evaluate_script(&format!("window.ovner && ovner.render({text})"));
        if let Some(sb) = &self.sidebar {
            let _ = sb.evaluate_script(&format!("window.ovner && ovner.render({text})"));
        }
        if let Some(t) = tab {
            let title = if t.title.is_empty() { "Ovner".to_string() } else { format!("{} - Ovner", t.title) };
            self.window.set_title(&title);
        }
        self.last_render_key = text;
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

    fn download_view(d: &Download) -> Value {
        json!({"id": d.id, "name": d.name, "url": d.url, "path": d.path, "total": d.total, "received": d.received, "state": d.state, "time": d.time,
            "engine": d.engine, "speed": d.speed, "conns": d.conns,
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
            win::DlEvent::Offer { id, url, path, total, cookie } => {
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
                let req = dlengine::Request { url, user_agent: ua, headers };
                let p = self.proxy.clone();
                let _ = total;
                std::thread::spawn(move || {
                    let r = dlengine::probe(&req).map(|pr| pr.total);
                    send(&p, UserEvent::DlProbe(id, tab, req, path, r));
                });
                return;
            }
            win::DlEvent::Started { id, url, path, total } => {
                let name = Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| url.clone());
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
                });
                self.dl_bubble = true;
            }
            win::DlEvent::Progress { id, received, total } => {
                if let Some(d) = self.downloads.iter_mut().rev().find(|d| d.id == id) {
                    d.received = received;
                    d.total = total;
                }
            }
            win::DlEvent::Done { id, ok, path, received } => {
                if let Some(d) = self.downloads.iter_mut().rev().find(|d| d.id == id) {
                    d.state = if ok { "done".into() } else { "failed".into() };
                    d.received = received;
                    if !path.is_empty() {
                        d.name = Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| d.name.clone());
                        d.path = path;
                    }
                }
                self.save_downloads();
            }
        }
        self.refresh_pages("downloads");
        self.last_render_key.clear();
        self.render();
    }

    /// Jawaban uji server: ambil alih dengan mesin unduh Ovner, atau biarkan WebView2.
    fn on_dl_probe(&mut self, id: u64, tab: u64, req: dlengine::Request, path: String, res: Result<u64, String>) {
        let total = match res {
            Ok(t) => t,
            Err(e) => {
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
                log(&format!("mesin unduh Ovner: {} ({} MB, {conns} koneksi)", req.url, total / 1_048_576));
                let name = target.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| req.url.clone());
                self.downloads.push(Download {
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
                });
                self.dl_jobs.insert(id, (job, tab));
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
                if !ok && err != "dibatalkan" {
                    // Gagal di tengah jalan (mis. tautan sekali pakai): ulangi lewat WebView2.
                    log(&format!("mesin unduh Ovner gagal: {err}; diulang dengan WebView2"));
                    self.dl_skip.borrow_mut().insert(url.clone());
                    let tab = job.map(|j| j.1).and_then(|t| self.tab_index(t)).filter(|&i| self.tabs[i].webview.is_some()).unwrap_or(self.active);
                    if let Some(wv) = self.tabs.get(tab).and_then(|t| t.webview.as_ref()) {
                        let _ = wv.load_url(&url);
                    }
                }
                self.save_downloads();
                self.last_render_key.clear();
                self.render();
            }
        }
        self.refresh_pages("downloads");
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
        match kind {
            "dl-open" if Path::new(&path).exists() => win::open_path(&path, false),
            "dl-show" if Path::new(&path).exists() => win::open_path(&path, true),
            "dl-cancel" => match self.dl_jobs.get(&id) {
                Some((job, _)) => job.cancel(),
                None => win::cancel_download(id),
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
            }
            "dl-remove" => {
                self.downloads.remove(pos);
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
        let s = Session {
            tabs: self.tabs.iter().filter(|t| keep(t)).map(|t| SessionTab { url: t.url.clone(), title: t.title.clone() }).collect(),
            active: self.tabs.iter().take(self.active).filter(|t| keep(t)).count(),
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
                let list: Vec<Value> = self.downloads.iter().rev().map(Self::download_view).collect();
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
                    .tabs
                    .iter()
                    .enumerate()
                    .map(|(j, t)| {
                        json!({
                            "id": t.id, "title": t.title, "url": display_url(&t.url), "active": self.is_shown(j),
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
                        let titles: Vec<String> = self
                            .tabs
                            .iter()
                            .filter(|t| t.proc_pid == p.pid && p.pid != 0)
                            .map(|t| if t.title.is_empty() { display_url(&t.url) } else { t.title.clone() })
                            .collect();
                        json!({"pid": p.pid, "kind": p.kind, "working": p.working, "commit": p.commit, "cpu": p.cpu, "gpu": p.gpu, "gpuMem": p.gpu_mem, "tabs": titles})
                    })
                    .collect();
                let mapped = !self.renderer_frames.is_empty();
                json!({ "theme": theme, "lang": lang, "tabs": tabs, "procs": procs, "mapped": mapped, "mapOff": !self.frames_ok,
                        "stats": self.tab_stats })
            }
            "sessions" => {
                let open: Vec<Value> = self
                    .tabs
                    .iter()
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
            let _ = wv.evaluate_script(&format!("window.ovnerPage && ovnerPage({state})"));
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
        let mode = if tasks || app_on { 2 } else if tab_on { 1 } else { 0 };
        usage::set_monitor(mode);
        if mode == 0 {
            return;
        }
        if self.stats_at.elapsed() < Duration::from_millis(900) {
            return;
        }
        self.stats_at = Instant::now();
        for j in 0..self.tabs.len() {
            let shown = self.is_shown(j);
            let t = &mut self.tabs[j];
            let Some(wv) = &t.webview else { continue };
            if t.suspended || (!shown && !tasks) {
                continue;
            }
            if !t.perf_on {
                win::cdp(wv, "Performance.enable", "{}");
                t.perf_on = true;
            }
            let (p, id) = (self.proxy.clone(), t.id);
            win::cdp_call(wv, "Performance.getMetrics", "{}", move |r| send(&p, UserEvent::TabMetrics(id, r)));
        }
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
            for t in &mut self.tabs {
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
        let mut shared: HashMap<u32, usize> = HashMap::new();
        for t in &self.tabs {
            if let Some(pid) = t.frame_id.and_then(|f| owner.get(&f)).filter(|_| t.webview.is_some()) {
                *shared.entry(*pid).or_default() += 1;
            }
        }
        for t in &mut self.tabs {
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
        let js = format!("window.ovner && ovner.usage && ovner.usage({v})");
        let _ = self.toolbar.evaluate_script(&js);
        if let Some(sb) = &self.sidebar {
            let _ = sb.evaluate_script(&js);
        }
    }

    /// Pekerjaan berkala (tiap 2 detik): tidurkan tab, rapikan RAM, ukur pemakaian, dsb.
    fn on_tick(&mut self) {
        if self.last_sleep_check.elapsed() >= SLEEP_CHECK {
            self.last_sleep_check = Instant::now();
            self.sleep_idle_tabs();
        }
        if let Some(due) = self.next_trim {
            if Instant::now() >= due {
                self.trim_now();
                let every = self.settings.borrow().trim_every_minutes;
                self.next_trim = if self.background && every > 0 { Some(Instant::now() + Duration::from_secs(every as u64 * 60)) } else { None };
            }
        }
        self.sample_usage();
        self.check_background_media();
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
            let kinds: Vec<String> = self.stats.iter().map(|p| p.kind.clone()).collect();
            log(&format!(
                "RAM: {} MB, CPU {:.1}%, {} proses [{}], {} tab ({} hidup)",
                t.working / 1_048_576,
                t.cpu,
                t.procs,
                kinds.join(","),
                self.tabs.len(),
                self.tabs.iter().filter(|t| t.webview.is_some()).count()
            ));
        }
        self.store.flush();
        self.render();
    }

    fn tasks_open(&self) -> bool {
        self.tabs.iter().any(|t| internal_page(&t.url) == Some("tasks") && t.webview.is_some())
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
                if suspend {
                    win::try_suspend(wv);
                    t.suspended = true;
                }
            }
        }
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
        json!({"app": app, "tab": tab})
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
            "tabs": self.tabs.len(), "tabsLive": self.tabs.iter().filter(|t| t.webview.is_some()).count(),
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
        });
    }

    /// Segarkan semua tab yang menampilkan halaman internal `name`.
    fn refresh_pages(&mut self, name: &str) {
        for i in 0..self.tabs.len() {
            if internal_page(&self.tabs[i].url) == Some(name) {
                self.push_page_state(i);
            }
        }
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
                    let _ = self.toolbar.set_theme(th);
                    for t in &self.tabs {
                        if let Some(wv) = &t.webview {
                            let _ = wv.set_theme(th);
                        }
                    }
                }
                self.window.set_theme(match s.theme.as_str() {
                    "light" => Some(tao::window::Theme::Light),
                    "dark" => Some(tao::window::Theme::Dark),
                    _ => None,
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
            }
            "custom_rules" => {
                if blocker::write_custom_rules(data_dir(), &s.custom_rules) {
                    start_filter_update(self.proxy.clone(), s.clone());
                }
            }
            "block_cookie_notices" | "custom_lists" | "filter_lists" => start_filter_update(self.proxy.clone(), s.clone()),
            // Skrip halaman dibuat saat tab dibangun: tandai semua tab untuk dibangun ulang.
            "autofill" | "password_manager" | "password_autosignin" => {
                for t in &self.tabs {
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
                self.ensure_sidebar();
                self.layout();
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
                let shown: Vec<usize> = (0..self.tabs.len()).filter(|&j| self.is_shown(j) && internal_page(&self.tabs[j].url).is_none()).collect();
                for j in shown {
                    self.rebuild_tab(j);
                }
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
        for t in &self.tabs {
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
        if internal.is_none() && !matches!(kind, "key" | "url" | "page" | "cos" | "pick" | "pw-query" | "pw-capture" | "pw-fill" | "anubis" | "paste" | "dirty" | "clean") {
            return false;
        }
        match kind {
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
            "dl-open" | "dl-show" | "dl-remove" | "dl-cancel" | "dl-pause" | "dl-resume" => self.download_action(kind, msg["id"].as_u64().unwrap_or(0)),
            "dl-clear" => {
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
                if let Some(j) = msg["id"].as_u64().and_then(|id| self.tab_index(id)) {
                    if kind == "tab-sleep" {
                        self.sleep_tab(j);
                    } else if j != self.active {
                        self.close_tab(j);
                    }
                    self.render();
                    self.refresh_pages("tasks");
                }
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
                }
            }
            "dl-open" | "dl-show" | "dl-cancel" | "dl-remove" | "dl-pause" | "dl-resume" => self.download_action(&cmd, id),
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
        let _ = m.append_items(&[&PredefinedMenuItem::separator(), &dark, &copy, &nosleep]);
        let _ = m.append_items(&[&PredefinedMenuItem::separator(), &pick, &more]);
        #[cfg(windows)]
        {
            use tao::platform::windows::WindowExtWindows;
            use tray_icon::menu::ContextMenu;
            let pos = tray_icon::dpi::Position::Logical(tray_icon::dpi::LogicalPosition::new(x, y));
            unsafe {
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
                        let (u, ti) = (t.url.clone(), t.title.clone());
                        self.store.toggle_bookmark(&u, &ti);
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
                for j in 0..self.tabs.len() {
                    self.sleep_tab(j);
                }
                self.trim_now();
                self.render();
            }
            "trim" => {
                for t in &self.tabs {
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
                    if let Some(wv) = self.tabs.iter().find_map(|t| t.webview.as_ref()) {
                        let _ = wv.clear_all_browsing_data();
                    }
                    self.store.clear_history();
                    self.store.flush();
                    self.refresh_pages("history");
                }
            }
            "about" => {
                let engine = wry::webview_version().unwrap_or_else(|_| "?".into());
                let lists = t(self.lang, if self.blocker.borrow().full { "d.lists_full" } else { "d.lists_builtin" });
                let data = data_dir().display().to_string();
                win::info(&tf(
                    self.lang,
                    "d.about",
                    &[("v", &update::display_version()), ("e", &engine), ("l", lists), ("d", &data), ("r", update::REPO)],
                ));
            }
            "tab-reload" => {
                if let Some(wv) = tab_target.and_then(|j| self.tabs[j].webview.as_ref()) {
                    let _ = wv.reload();
                }
            }
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
// Start

struct Args {
    portable: bool,
    uninstall: bool,
    restarted: bool,
    tray: bool,
    urls: Vec<String>,
}

fn parse_args() -> Args {
    let mut a = Args { portable: false, uninstall: false, restarted: false, tray: false, urls: Vec::new() };
    for v in std::env::args().skip(1) {
        match v.as_str() {
            "--portable" => a.portable = true,
            "--uninstall" => a.uninstall = true,
            "--restarted" => a.restarted = true,
            "--tray" => a.tray = true,
            s if s.starts_with("http://") || s.starts_with("https://") => a.urls.push(s.to_string()),
            s if s.starts_with("ovner://") => a.urls.push(internal_url(s.trim_start_matches("ovner://").trim_end_matches('/'))),
            _ => {}
        }
    }
    a
}

fn run_uninstall(exe: &Path, portable: bool) {
    let lang = i18n::resolve(&Settings::load(&install::data_dir(exe, portable)).language);
    if !win::confirm(t(lang, "d.uninstall")) {
        return;
    }
    stop_running_instance();
    let _ = win::set_autostart(false, exe);
    install::remove_integration();
    let data = install::data_dir(exe, portable);
    if install::is_installed_copy(exe) {
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
    // Dijalankan dari Downloads (Ovner-Setup.exe): pasang ke LocalAppData lalu jalankan salinannya.
    if install::should_install(&exe, args.portable) {
        match install::install_self(&exe, stop_running_instance) {
            Ok(target) => {
                if std::process::Command::new(&target).args(&args.urls).spawn().is_ok() {
                    return;
                }
            }
            Err(e) => win::info(&tf(i18n::system_lang(), "d.install_failed", &[("e", &e)])),
        }
    }

    let data = std::env::var_os("OVNER_DATA").map(PathBuf::from).unwrap_or_else(|| install::data_dir(&exe, portable));
    let _ = std::fs::create_dir_all(&data);
    let _ = DATA_DIR.set(data);
    win::install_crash_logger(&data_dir().join("ovner.log"));
    if install::is_installed_copy(&exe) {
        install::ensure_integration(&exe, false);
    }

    let cmd = args.urls.first().map(|u| format!("open {u}")).unwrap_or_else(|| "show".into());
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
        });
    }

    let window = Rc::new(
        WindowBuilder::new()
        .with_title("Ovner")
        .with_inner_size(LogicalSize::new(1280.0, 800.0))
        .with_min_inner_size(LogicalSize::new(480.0, 320.0))
        .with_maximized(!args.tray)
        .with_visible(!args.tray)
        .with_theme(match settings.theme.as_str() {
            "light" => Some(tao::window::Theme::Light),
            "dark" => Some(tao::window::Theme::Dark),
            _ => None,
        })
        .with_window_icon(Icon::from_rgba(icon::rgba(64), 64, 64).ok())
        .build(&event_loop)
        .expect("jendela Ovner"),
    );

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
    let toolbar = {
        let p = proxy.clone();
        let size: LogicalSize<f64> = window.inner_size().to_logical(window.scale_factor());
        let b = WebViewBuilder::new_with_web_context(&mut web_context)
            .with_html(TOOLBAR_HTML)
            .with_bounds(Rect { position: WLogicalPosition::new(0.0, 0.0).into(), size: WLogicalSize::new(size.width, 77.0).into() })
            .with_hotkeys_zoom(false)
            .with_devtools(cfg!(debug_assertions))
            .with_ipc_handler(move |req| send(&p, UserEvent::Toolbar(req.body().clone())));
        #[cfg(windows)]
        let b = {
            use wry::WebViewBuilderExtWindows;
            let mut b = b
                .with_additional_browser_args(args_str.clone())
                .with_browser_accelerator_keys(false)
                .with_default_context_menus(false)
                .with_theme(wry_theme(&settings.theme));
            if let Some(env) = &env {
                b = b.with_environment(env.clone());
            }
            b
        };
        match b.build_as_child(&*window) {
            Ok(w) => w,
            Err(e) => {
                win::info(&tf(lang, "d.need_webview2", &[("e", &e.to_string())]));
                return;
            }
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
                if d.state == "progress" || d.state == "paused" {
                    d.state = "failed".into();
                }
            }
            v
        },
        dl_bubble: false,
        dl_jobs: HashMap::new(),
        dl_skip: Rc::new(RefCell::new(HashSet::new())),
        dl_render_at: Instant::now(),
        toolbar_over: 0.0,
        web_context,
        #[cfg(windows)]
        env,
        proxy,
        browser_args: args_str,
        update_ready: None,
        last_sleep_check: Instant::now(),
        last_diag: Instant::now(),
        last_render_key: String::new(),
        menus,
        menu_tab: None,
        _tray: tray,
        in_tray: args.tray,
        background: false,
        next_trim: None,
        find_open: false,
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
    event_loop.run(move |event, _, control_flow| {
        let mut quit = false;
        let mut restart = false;
        let tick_due = Instant::now() >= next_tick;
        if tick_due {
            next_tick = Instant::now() + TICK;
        }
        *control_flow = ControlFlow::WaitUntil(next_tick);
        if tick_due && !matches!(event, Event::NewEvents(StartCause::ResumeTimeReached { .. })) {
            app.on_tick();
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
                    if app.settings.borrow().close_to_tray && app._tray.is_some() {
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
                    }
                }
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
                UserEvent::Toolbar(body) => restart = app.on_toolbar(&body),
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
                UserEvent::Favicon(id, png) => {
                    if let Some(i) = app.tab_index(id) {
                        let url = app.tabs[i].webview.as_ref().and_then(|w| w.url().ok()).unwrap_or_default();
                        if url.starts_with("http") && internal_page(&url).is_none() {
                            app.store.set_icon(&url, &png);
                            app.render();
                        }
                    }
                }
                UserEvent::CloseWindow => {
                    if app.settings.borrow().close_to_tray && app._tray.is_some() {
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
                    app.push_usage();
                    app.refresh_pages("tasks");
                }
                UserEvent::RendererFrames(frames) => {
                    app.frames_pending = false;
                    app.renderer_frames = frames;
                    app.assign_tab_procs();
                }
                UserEvent::PopupReady => app.adopt_popups(),
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
                UserEvent::ProcFailed(id, kind) => {
                    log(&format!("proses tab {id} gagal: {kind:?}"));
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
                    app.update_ready = Some(v);
                    app.render();
                }
                UserEvent::UpdateInfo(msg) => win::info(&msg),
                UserEvent::Menu(id) => quit = app.on_menu(&id),
                UserEvent::TrayClick => {
                    if app.in_tray {
                        app.show_from_tray();
                    } else {
                        app.hide_to_tray();
                    }
                }
                UserEvent::Remote(r) => match r {
                    Remote::Show => {
                        app.show_from_tray();
                        app.open_tab("", true, false);
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
        if app.quit_requested {
            quit = true;
        }
        if restart {
            app.save_session();
            if let Ok(exe) = std::env::current_exe() {
                let _ = std::process::Command::new(exe).arg("--restarted").spawn();
            }
            quit = true;
        }
        if quit {
            app.save_session();
            app.store.flush();
            if app.settings.borrow().clear_on_exit {
                if let Some(wv) = app.tabs.iter().find_map(|t| t.webview.as_ref()) {
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
            *control_flow = ControlFlow::Exit;
        }
    });
}

#[cfg(test)]
mod tests {
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
