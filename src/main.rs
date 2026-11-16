//! Ovner: browser ringan untuk Windows di atas WebView2 (mesin Edge yang sudah ada di Windows)
//! dengan pemblokir iklan Brave (`adblock-rust`), tab yang ditidurkan otomatis, tray yang
//! menekan RAM, dan instalasi tanpa hak admin.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
// Sebagian kode hanya dipakai di Windows; di Linux (untuk pengujian) jangan dianggap mati.
#![cfg_attr(not(windows), allow(dead_code))]

mod blocker;
mod i18n;
mod icon;
mod import;
mod install;
#[cfg(windows)]
mod netfilter;
mod settings;
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
const PAGE_CSS: &str = include_str!("ui/page.css");
/// Halaman internal disajikan lewat protokol `ovner://`. Di Windows, wry memetakannya ke
/// `http://ovner.<nama>/` (WebView2 belum mendukung skema sendiri secara penuh).
#[cfg(windows)]
const INTERNAL_PREFIX: &str = "http://ovner.";
#[cfg(not(windows))]
const INTERNAL_PREFIX: &str = "ovner://";
const INSTANCE_PORT: u16 = 47893;
const MAGIC: &str = "OVNER-1";
const TICK: Duration = Duration::from_secs(2);
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
    /// Hasil daftar proses WebView2 (task manager per tab).
    ProcInfos(Vec<win::ProcExt>),
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
}

#[derive(Serialize, Deserialize, Default)]
struct Session {
    tabs: Vec<SessionTab>,
    active: usize,
}

#[derive(Serialize, Deserialize)]
struct SessionTab {
    url: String,
    title: String,
}

struct Menus {
    main: Menu,
    tab: Menu,
    tray: Menu,
}

struct App {
    window: Window,
    toolbar: WebView,
    /// Daftar tab vertikal di kiri (bila dinyalakan).
    sidebar: Option<WebView>,
    toolbar_ready: bool,
    toolbar_h: f64,
    tabs: Vec<Tab>,
    active: usize,
    next_id: u64,
    closed: Vec<String>,
    settings: Rc<RefCell<Settings>>,
    blocker: Rc<RefCell<Blocker>>,
    store: Store,
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
    proc_infos: Vec<win::ProcExt>,
    last_unresponsive: HashMap<u64, Instant>,
    extensions_loaded: bool,
    /// Tersembunyi di tray.
    in_tray: bool,
    /// Sedang dalam keadaan hemat (tray atau diminimalkan).
    background: bool,
    next_trim: Option<Instant>,
    find_open: bool,
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
        _ => wry::http::Response::builder().status(404).body(Cow::Borrowed(&b"Tidak ada"[..])).unwrap(),
    }
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
    ]);
    let zoom = Submenu::new(t(lang, "m.zoom"), true);
    let _ = zoom.append_items(&[&item("zoom-out", "m.zoom_out"), &item("zoom-reset", "m.zoom_reset"), &item("zoom-in", "m.zoom_in")]);
    let tools = Submenu::new(t(lang, "m.tools"), true);
    let _ = tools.append_items(&[
        &item("tasks", "m.tasks"),
        &item("sleep-all", "m.sleep_all"),
        &item("trim", "m.trim"),
        &PredefinedMenuItem::separator(),
        &item("pick", "m.pick"),
        &item("filters", "m.filters"),
        &item("clear", "m.clear"),
        &item("data", "m.data"),
        &item("devtools", "m.devtools"),
    ]);
    let _ = main.append_items(&[
        &item("newtab", "m.newtab"),
        &item("private", "m.private"),
        &item("reopen", "m.reopen"),
        &item("new-identity", "m.new_identity"),
        &PredefinedMenuItem::separator(),
        &bookmarks,
        &item("history", "m.history"),
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
        &item("tab-close", "tm.close"),
        &item("tab-close-others", "tm.close_others"),
    ]);
    let tray = Menu::new();
    let _ = tray.append_items(&[&item("show", "tray.show"), &item("newtab", "tray.newtab"), &PredefinedMenuItem::separator(), &item("quit", "m.quit")]);
    Menus { main, tab, tray }
}

impl App {
    fn toolbar_rect(&self) -> Rect {
        let size: LogicalSize<f64> = self.window.inner_size().to_logical(self.window.scale_factor());
        Rect { position: WLogicalPosition::new(0.0, 0.0).into(), size: WLogicalSize::new(size.width.max(1.0), self.toolbar_h).into() }
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
        match b.build_as_child(&self.window) {
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
        self.next_id += 1;
        Tab {
            id: self.next_id,
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
    fn wake(&mut self, i: usize) {
        if self.tabs[i].webview.is_some() {
            return;
        }
        let rect = self.rect_for(i);
        let s = self.settings.borrow().clone();
        let tab = &self.tabs[i];
        let (id, private) = (tab.id, tab.private);
        let url = if tab.url.is_empty() { self.newtab_url() } else { tab.url.clone() };
        let (blocked, page) = (tab.blocked.clone(), tab.page.clone());
        *page.borrow_mut() = url.clone();
        let visible = self.is_shown(i) && !self.in_tray;
        let zoom = tab.zoom;
        let allow_http = tab.allow_http.clone();
        let scripts_on = tab.scripts_on.clone();
        scripts_on.set(true);
        let init = shields::page_scripts(&s, self.session_key, private);

        let p_ipc = self.proxy.clone();
        let p_title = self.proxy.clone();
        let p_load = self.proxy.clone();
        let p_nav = self.proxy.clone();
        let p_new = self.proxy.clone();
        let (nav_blocked, nav_page) = (blocked.clone(), page.clone());
        let nav_settings = self.settings.clone();
        let nav_blocker = self.blocker.clone();
        let (deny_perm, autoplay, popups_to_tabs) = (s.deny_sensitive_permissions, s.autoplay, s.popups_to_tabs);

        let mut builder = if private { WebViewBuilder::new().with_incognito(true) } else { WebViewBuilder::new_with_web_context(&mut self.web_context) };
        builder = builder
            .with_url(url)
            .with_bounds(rect)
            .with_visible(visible)
            .with_focused(visible && i == self.active)
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
                    NewWindowResponse::Allow
                } else {
                    send(&p_new, UserEvent::NewTab(id, url));
                    NewWindowResponse::Deny
                }
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
                .with_additional_browser_args(self.browser_args.clone())
                .with_browser_accelerator_keys(true)
                .with_default_context_menus(true)
                .with_theme(wry_theme(&s.theme));
            if let Some(env) = &self.env {
                b = b.with_environment(env.clone());
            }
            b
        };

        match builder.build_as_child(&self.window) {
            Ok(wv) => {
                win::tune_webview(&wv, &s);
                for (method, params) in shields::identity_overrides(&s, &win::user_agent(&wv)) {
                    win::cdp(&wv, method, &params);
                }
                #[cfg(windows)]
                {
                    let ctx = netfilter::FilterCtx {
                        blocker: self.blocker.clone(),
                        settings: self.settings.clone(),
                        blocked: blocked.clone(),
                        page: page.clone(),
                        cname: self.cname.clone(),
                    };
                    netfilter::attach_scriptlets(&wv, ctx.clone());
                    if let Err(e) = netfilter::attach(&wv, ctx) {
                        log(&format!("pemblokir tidak terpasang: {e}"));
                    }
                    let p_done = self.proxy.clone();
                    netfilter::on_navigation_completed(&wv, move |ok, status| send(&p_done, UserEvent::NavDone(id, ok, status)));
                }
                let p_fail = self.proxy.clone();
                win::on_process_failed(&wv, move |k| send(&p_fail, UserEvent::ProcFailed(id, k)));
                if s.extensions_enabled && !self.extensions_loaded && !private {
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
        let rects: Vec<Rect> = (0..self.tabs.len()).map(|j| self.rect_for(j)).collect();
        for (j, t) in self.tabs.iter_mut().enumerate() {
            if let Some(wv) = &t.webview {
                let on = j == i || Some(j) == partner;
                if on {
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
                    if suspend && !t.loading {
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
        log(&format!("tab ditidurkan: {}", display_url(&t.url)));
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
            #[cfg(windows)]
            if t.webview.as_ref().is_some_and(netfilter::is_playing_audio) {
                continue;
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
            "toolbarCss": s.toolbar_css,
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

    fn set_toolbar_height(&mut self, h: f64) {
        let h = h.clamp(60.0, 640.0);
        if (h - self.toolbar_h).abs() > 0.5 {
            self.toolbar_h = h;
            self.layout();
        }
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
                json!({
                    "settings": &*self.settings.borrow(),
                    "lang": lang,
                    "version": update::display_version(),
                    "engine": wry::webview_version().unwrap_or_else(|_| "?".into()),
                    "lists": i18n::t(self.lang, if self.blocker.borrow().full { "d.lists_full" } else { "d.lists_builtin" }),
                    "data": data_dir().display().to_string(),
                    "browsers": browsers,
                })
            }
            "newtab" => json!({ "theme": theme, "lang": lang }),
            "history" => json!({ "theme": theme, "lang": lang, "history": self.store.search_history(&t.page_query, 500) }),
            "bookmarks" => json!({ "theme": theme, "lang": lang, "bookmarks": self.store.bookmarks }),
            "tasks" => {
                let snap = win::process_snapshot();
                let mem: HashMap<u32, (u64, u64)> = snap.iter().map(|p| (p.pid, (p.working, p.commit))).collect();
                // RAM per tab: proses renderer yang melayani halaman tab itu (seperti task
                // manager Chrome). Proses yang melayani beberapa tab ditandai "bersama".
                let same = |a: &str, b: &str| a == b || (host_of(a).is_some() && host_of(a) == host_of(b));
                let tabs: Vec<Value> = self
                    .tabs
                    .iter()
                    .enumerate()
                    .map(|(j, t)| {
                        let procs: Vec<&win::ProcExt> = if t.webview.is_some() {
                            self.proc_infos.iter().filter(|p| p.kind == "renderer" && p.mains.iter().any(|m| same(m, &t.url))).collect()
                        } else {
                            Vec::new()
                        };
                        let ram: u64 = procs.iter().filter_map(|p| mem.get(&p.pid)).map(|m| m.0).sum();
                        let shared = procs.iter().any(|p| p.mains.len() > 1);
                        json!({
                            "id": t.id, "title": t.title, "url": display_url(&t.url), "active": self.is_shown(j),
                            "sleeping": t.webview.is_none(), "suspended": t.suspended, "blocked": t.blocked.get(),
                            "zoom": t.zoom, "private": t.private, "ram": ram, "shared": shared,
                            "pids": procs.iter().map(|p| p.pid).collect::<Vec<_>>(),
                        })
                    })
                    .collect();
                let kinds: HashMap<u32, &str> = self.proc_infos.iter().map(|p| (p.pid, p.kind)).collect();
                let procs: Vec<Value> = snap
                    .iter()
                    .map(|p| {
                        let kind = if p.is_self { "ovner" } else { kinds.get(&p.pid).copied().unwrap_or("webview2") };
                        json!({"pid": p.pid, "working": p.working, "commit": p.commit, "kind": kind})
                    })
                    .collect();
                json!({ "theme": theme, "lang": lang, "tabs": tabs, "procs": procs })
            }
            _ => return,
        };
        if let Some(wv) = &self.tabs[i].webview {
            let _ = wv.evaluate_script(&format!("window.ovnerPage && ovnerPage({state})"));
        }
    }

    /// Minta daftar proses WebView2 terbaru (untuk task manager per tab).
    fn refresh_proc_infos(&self) {
        #[cfg(windows)]
        {
            use wry::WebViewExtWindows;
            let env = self.env.clone().unwrap_or_else(|| self.toolbar.environment());
            let p = self.proxy.clone();
            win::request_process_infos(&env, move |v| send(&p, UserEvent::ProcInfos(v)));
        }
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
            "block_cookie_notices" | "custom_lists" => start_filter_update(self.proxy.clone(), s.clone()),
            // Skrip halaman dibuat saat tab dibangun: tandai semua tab untuk dibangun ulang.
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
        if internal.is_none() && !matches!(kind, "key" | "url" | "page" | "cos" | "pick") {
            return false;
        }
        match kind {
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
                self.refresh_pages("bookmarks");
                self.last_render_key.clear();
                self.render();
            }
            "trim" => {
                self.trim_now();
                self.push_page_state(i);
            }
            "sleep-all" => {
                self.on_menu("sleep-all");
                self.push_page_state(i);
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
        match msg["cmd"].as_str().unwrap_or("") {
            "ready" => {
                self.toolbar_ready = true;
                self.last_render_key.clear();
                self.render();
            }
            "height" => {
                if let Some(h) = msg["h"].as_f64() {
                    self.set_toolbar_height(h);
                }
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
            "pick" => {
                let _ = self.on_menu("pick");
            }
            "split-swap" => {
                if let Some(p) = self.split.and_then(|id| self.tab_index(id)) {
                    self.activate(p);
                }
            }
            "split-close" => self.stop_split(),
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
        drop(s);
        let _ = m.append_items(&[&head, &count, &PredefinedMenuItem::separator(), &ads, &scripts, &fp]);
        if level != "standard" {
            let _ = m.append(&allow_js);
        }
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
            "bookmarks" | "history" | "settings" | "tasks" => self.open_internal(id),
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
            "downloads" => {
                #[cfg(windows)]
                if let Some(wv) = self.active_webview() {
                    netfilter::open_downloads(wv);
                }
            }
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

    let window = WindowBuilder::new()
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
        .expect("jendela Ovner");

    let menus = build_menus(lang);
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menus.tray.clone()))
        .with_tooltip(format!("Ovner {}", update::display_version()))
        .with_icon(tray_icon::Icon::from_rgba(icon::rgba(32), 32, 32).expect("ikon tray"))
        .with_menu_on_left_click(false)
        .build()
        .map_err(|e| log(&format!("tray: {e}")))
        .ok();

    let args_str = settings.browser_args();
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
        match b.build_as_child(&window) {
            Ok(w) => w,
            Err(e) => {
                win::info(&tf(lang, "d.need_webview2", &[("e", &e.to_string())]));
                return;
            }
        }
    };

    let mut app = App {
        window,
        toolbar,
        sidebar: None,
        toolbar_ready: false,
        toolbar_h: 77.0,
        tabs: Vec::new(),
        active: 0,
        next_id: 0,
        closed: Vec::new(),
        settings: Rc::new(RefCell::new(settings)),
        blocker,
        store: Store::load(data_dir()),
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
        proc_infos: Vec::new(),
        last_unresponsive: HashMap::new(),
        extensions_loaded: false,
    };
    app.ensure_sidebar();

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

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + TICK);
        let mut quit = false;
        let mut restart = false;
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
                if app.last_sleep_check.elapsed() >= SLEEP_CHECK {
                    app.last_sleep_check = Instant::now();
                    app.sleep_idle_tabs();
                }
                if let Some(due) = app.next_trim {
                    if Instant::now() >= due {
                        app.trim_now();
                        let every = app.settings.borrow().trim_every_minutes;
                        app.next_trim = if app.background && every > 0 { Some(Instant::now() + Duration::from_secs(every as u64 * 60)) } else { None };
                    }
                }
                if app.settings.borrow().diagnostics && app.last_diag.elapsed() >= DIAG_EVERY {
                    app.last_diag = Instant::now();
                    let procs = win::process_snapshot();
                    let total: u64 = procs.iter().map(|p| p.working).sum();
                    log(&format!("RAM: {} MB working set, {} proses, {} tab ({} hidup)", total / 1_048_576, procs.len(), app.tabs.len(), app.tabs.iter().filter(|t| t.webview.is_some()).count()));
                }
                app.store.flush();
                if app.tabs.iter().any(|t| internal_page(&t.url) == Some("tasks") && t.webview.is_some()) {
                    app.refresh_proc_infos();
                }
                app.refresh_pages("tasks");
                app.render();
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
                UserEvent::ProcInfos(v) => {
                    app.proc_infos = v;
                    app.refresh_pages("tasks");
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
