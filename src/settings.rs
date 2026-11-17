//! Pengaturan Ovner, disimpan di `<data>/settings.json` dan diubah lewat halaman
//! `ovner://settings`. Bagian-bagiannya mengikuti Brave Origin: tampilan, pencarian,
//! Shields, privasi, unduhan, saat mulai, sistem; ditambah hemat RAM dari Outlook Lite/ZFPlus
//! dan fitur dari ungoogled-chromium, Helium, dan Servo.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Versi skema setelan. Naik bila bawaan lama perlu diubah untuk pengguna yang sudah ada.
pub const SCHEMA: u32 = 7;

fn zero() -> u32 {
    0
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Settings {
    /// Versi skema file ini (0 = dibuat sebelum ada migrasi).
    #[serde(default = "zero")]
    pub schema: u32,
    /// Halaman setup awal sudah dilewati.
    pub setup_done: bool,

    /// Preset perlindungan: "balanced" (Seimbang), "strict" (Ketat), "max" (Maksimal), atau
    /// "custom" bila ada setelan yang diubah sendiri.
    pub preset: String,

    // ---------- tampilan ----------
    /// "auto" (ikut Windows), "id", atau "en".
    pub language: String,
    /// "system", "light", atau "dark".
    pub theme: String,
    pub bookmark_bar: bool,
    pub home_button: bool,
    pub home_url: String,
    /// Halaman tab baru: "blank" (kosong, seperti Brave Origin) atau "ovner" (kotak cari).
    pub newtab: String,
    pub default_zoom: u32,
    /// Lama: digantikan `dark_mode` + `dark_engine` "chromium" (tetap dibaca untuk migrasi).
    pub force_dark: bool,
    /// Mode gelap halaman web (seperti Dark Reader): "off", "on" (semua situs), "system"
    /// (ikut tema Windows). Situs bisa dinyalakan/dimatikan sendiri.
    pub dark_mode: String,
    /// Cara menggelapkan: "dynamic" (Dark Reader dinamis, paling rapi), "filter" (balik warna,
    /// cepat), "chromium" (mode gelap otomatis Chromium, paling ringan).
    pub dark_engine: String,
    /// Kecerahan, kontras, sepia, dan skala abu-abu mode gelap (persen, seperti Dark Reader).
    pub dark_brightness: u32,
    pub dark_contrast: u32,
    pub dark_sepia: u32,
    pub dark_grayscale: u32,
    /// Situs yang selalu gelap / tidak pernah gelap (mengalahkan `dark_mode`).
    pub dark_on_sites: Vec<String>,
    pub dark_off_sites: Vec<String>,
    /// CSS sendiri yang dipasang di semua halaman.
    pub user_css: String,
    /// CSS sendiri untuk toolbar Ovner (seperti userChrome.css Waterfox).
    pub toolbar_css: String,
    /// Tab di sisi kiri (vertikal), bukan di atas.
    pub vertical_tabs: bool,

    // ---------- pencarian dan kolom alamat ----------
    /// google, duckduckgo, bing, brave, atau URL berisi `%s`.
    pub search_engine: String,
    /// Pintasan !bang (contoh: `!w rust`).
    pub bangs: bool,
    /// Bang sendiri, tiap baris `!kode https://alamat?q=%s`.
    pub custom_bangs: Vec<String>,
    /// Saran dari riwayat dan bookmark saat mengetik (lokal, tanpa koneksi).
    pub address_suggestions: bool,
    /// Kolom alamat hanya menerima alamat, tidak pernah mencari ("No Search").
    pub no_search: bool,
    /// Mesin pencari tab privat; kosong = sama dengan mesin biasa.
    pub private_search_engine: String,

    // ---------- Shields ----------
    pub block_ads: bool,
    /// "standard" (seperti Brave Standar: permintaan ke situs itu sendiri tidak diblokir, hanya
    /// iklan/pelacak pihak ketiga) atau "aggressive" (semua permintaan diperiksa).
    pub blocking_mode: String,
    pub cosmetic_filtering: bool,
    /// Daftar filter popup persetujuan cookie.
    pub block_cookie_notices: bool,
    /// Acak sidik jari: "off", "standard", "strict".
    pub farbling: String,
    pub strip_tracking_params: bool,
    /// Lewati halaman AMP Google dan buka halaman aslinya.
    pub de_amp: bool,
    /// Lewati link pengalih pelacak (l.facebook.com, google.com/url, ...).
    pub debounce: bool,
    pub https_only: bool,
    pub gpc: bool,
    pub block_telemetry: bool,
    /// Matikan API iklan Chromium (Topics, FLEDGE, Attribution Reporting) dan eksperimen.
    pub block_privacy_sandbox: bool,
    /// Ungkap pelacak yang menyamar lewat CNAME (pencarian DNS di latar).
    pub cname_uncloak: bool,
    pub auto_update_filters: bool,
    /// Daftar kecil yang cepat berubah (perbaikan YouTube/anti-adblock) diperbarui tiap N jam.
    pub filter_update_hours: u32,
    /// Daftar gangguan: notifikasi, widget sosial, popup newsletter, "pasang aplikasi kami".
    pub block_annoyances: bool,
    /// Daftar filter opsional dari katalog (id seperti Brave, mis. "2F3DCE16" = promo aplikasi).
    pub filter_lists: Vec<String>,
    /// Alamat daftar filter tambahan (format EasyList/uBlock).
    pub custom_lists: Vec<String>,
    /// Aturan filter sendiri (format EasyList/uBlock), satu per baris.
    pub custom_rules: String,
    /// Situs tanpa pemblokir iklan.
    pub exceptions: Vec<String>,
    /// Situs yang JavaScript-nya diblokir.
    pub script_blocked: Vec<String>,
    /// Situs tanpa pengacakan sidik jari.
    pub farbling_off: Vec<String>,

    // ---------- privasi dan keamanan ----------
    /// Blokir situs publik yang mencoba mengakses jaringan lokal / komputer ini (0.0.0.0, 127.x, 192.168.x).
    pub block_local_network: bool,
    /// Referrer ke situs lain: "default", "origin" (hanya asal situs), "none" (kosong).
    pub referrer_policy: String,
    /// HTTPS-only ketat: tampilkan peringatan sebelum membuka situs http.
    pub https_strict: bool,
    /// Secure DNS tanpa fallback ke DNS Windows.
    pub dns_strict: bool,
    /// Bulatkan ukuran area halaman ke kelipatan 200×100 (seperti Mullvad/Tor).
    pub letterboxing: bool,
    /// Mode seragam: identitas generik (user agent, inti CPU, timer kasar, API langka mati).
    pub uniform_fingerprint: bool,
    /// Bagian dari mode seragam: laporkan zona waktu UTC (jam di situs bisa bergeser).
    pub uniform_utc: bool,
    /// Sembunyikan tanda bahwa ini aplikasi WebView2 (chrome.webview, merek user agent).
    pub hide_webview2: bool,
    /// Semua tab dibuka privat: tanpa riwayat, cookie hilang saat Ovner ditutup.
    pub always_private: bool,
    /// Level keamanan: "standard", "safer" (JS mati di http, JIT mati, media klik dulu),
    /// "safest" (JS mati di semua situs kecuali yang diizinkan).
    pub security_level: String,
    /// Situs yang tetap boleh menjalankan JavaScript di level "Paling aman".
    pub script_allowed: Vec<String>,
    /// Matikan JIT JavaScript (lebih aman dari celah V8, tetapi YouTube dan situs berat jauh
    /// lebih lambat karena WebAssembly ikut mati). Tidak termasuk preset.
    pub disable_jit: bool,
    pub clear_on_exit: bool,
    pub save_history: bool,
    pub deny_sensitive_permissions: bool,
    pub webrtc_policy: String,
    /// Prediksi jaringan (DNS prefetch, preconnect). Brave Origin: nyala (speed fix).
    pub network_prediction: bool,
    /// Secure DNS: "off", "cloudflare", "quad9", "adguard", atau URL DoH.
    pub secure_dns: String,
    pub autoplay: bool,
    /// Tawarkan simpan sandi dan isi otomatis (pengelola sandi Ovner).
    pub password_manager: bool,
    /// Isi login otomatis bila situs hanya punya satu login tersimpan ("Sign in automatically").
    pub password_autosignin: bool,
    /// Isi otomatis alamat dan kartu pembayaran (fitur WebView2/Edge).
    pub autofill: bool,
    pub spellcheck: bool,
    pub smartscreen: bool,
    pub edge_tracking_prevention: bool,
    pub local_crash_reports: bool,
    /// Matikan WebGL (mengurangi sidik jari dan RAM; game/peta 3D tidak jalan).
    pub disable_webgl: bool,
    /// Semua pop-up dibuka sebagai tab (termasuk jendela login berukuran kecil).
    pub popups_to_tabs: bool,
    /// Abaikan dialog "Tinggalkan situs?" (beforeunload).
    pub disable_beforeunload: bool,

    // ---------- izinkan salin (seperti Allow Copy+) ----------
    /// Izinkan pilih teks, salin, tempel, dan klik kanan di semua situs.
    pub allow_copy_all: bool,
    /// Situs yang dibuka larangan salin/klik kanannya.
    pub allow_copy_sites: Vec<String>,

    // ---------- unduhan ----------
    pub download_dir: String,
    /// Mesin unduh Ovner (seperti Mister Download Manager/IDM): berkas besar diunduh dengan
    /// beberapa koneksi sekaligus, bisa dijeda/dilanjutkan.
    pub dl_engine: bool,
    /// Jumlah koneksi per unduhan.
    pub dl_connections: u32,
    /// Hanya berkas sebesar ini (MB) ke atas yang diambil alih mesin unduh Ovner.
    pub dl_min_mb: u32,
    /// Tampilkan kotak unduhan (jendela kecil seperti IDM) saat unduhan dimulai.
    pub dl_box: bool,
    /// Cari lokasi server unduhan (negara/kota/penyedia) lewat ipwho.is.
    pub dl_server_info: bool,

    // ---------- saat mulai ----------
    pub restore_tabs: bool,
    pub autostart: bool,

    // ---------- hemat RAM ----------
    pub sleep_tabs_minutes: u64,
    /// Situs yang tidak pernah ditidurkan (seperti whitelist TabWars).
    pub sleep_never_sites: Vec<String>,
    /// Jangan tidurkan tab yang formulirnya sedang diisi.
    pub sleep_skip_forms: bool,
    pub suspend_hidden_tabs: bool,
    pub close_to_tray: bool,
    pub low_memory_when_hidden: bool,
    pub trim_when_hidden: bool,
    pub trim_every_minutes: u32,
    pub efficiency_mode: bool,
    pub simulate_memory_pressure: bool,
    pub low_end_mode: bool,
    pub v8_optimize_for_size: bool,
    pub site_isolation: bool,
    pub spare_renderer: bool,
    /// Batas jumlah proses halaman (0 = otomatis).
    pub renderer_process_limit: u32,
    /// Satu proses per situs (bukan per tab).
    pub process_per_site: bool,
    /// Indikator pemakaian (RAM, CPU, GPU) Ovner total di toolbar, diperbarui tiap 2 detik.
    pub usage_overlay_app: bool,
    /// Indikator pemakaian tab yang sedang dibuka.
    pub usage_overlay_tab: bool,
    /// Media (video/musik) di tab yang ditinggal: "pause" (langsung berhenti), "keep" (tetap
    /// berputar), "timer" (berhenti setelah `background_media_minutes` menit).
    pub background_media: String,
    pub background_media_minutes: u32,
    /// Anti-blokir: proxy lokal yang mencari alamat situs lewat DNS aman (DoH) dan memecah
    /// salam TLS supaya filter SNI ISP (Internet Positif) tidak mengenali situsnya.
    pub antiblock: bool,
    /// Lama: digantikan `gpu_mode` (tetap dibaca untuk migrasi).
    pub no_gpu: bool,
    /// "full" (GPU penuh), "lite" (GPU untuk video dan komposisi, gambar halaman di CPU),
    /// "off" (tanpa GPU, paling hemat RAM tapi video lebih berat).
    pub gpu_mode: String,
    /// Paksa YouTube memakai H.264 (lebih ringan didekode CPU): "auto" (saat GPU mati), "on", "off".
    pub youtube_h264: String,
    pub zero_copy: bool,
    pub parallel_downloading: bool,
    pub extra_browser_args: String,

    // ---------- sistem ----------
    pub check_updates: bool,
    /// "stable" atau "nightly".
    pub update_channel: String,
    pub diagnostics: bool,
    pub devtools: bool,
    /// Muat ekstensi (eksperimen).
    pub extensions_enabled: bool,
    /// Folder ekstensi yang sudah diekstrak (unpacked).
    pub extension_dirs: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            schema: SCHEMA,
            setup_done: false,
            preset: "balanced".into(),
            toolbar_css: String::new(),
            vertical_tabs: false,
            private_search_engine: String::new(),
            filter_update_hours: 6,
            block_annoyances: false,
            block_local_network: true,
            referrer_policy: "default".into(),
            https_strict: false,
            dns_strict: false,
            letterboxing: false,
            uniform_fingerprint: false,
            uniform_utc: false,
            hide_webview2: false,
            always_private: false,
            security_level: "standard".into(),
            script_allowed: Vec::new(),
            disable_jit: false,
            gpu_mode: "full".into(),
            usage_overlay_app: false,
            usage_overlay_tab: false,
            background_media: "keep".into(),
            background_media_minutes: 10,
            antiblock: false,
            youtube_h264: "auto".into(),
            language: "auto".into(),
            theme: "system".into(),
            bookmark_bar: false,
            home_button: false,
            home_url: String::new(),
            newtab: "blank".into(),
            default_zoom: 100,
            force_dark: false,
            dark_mode: "off".into(),
            dark_engine: "dynamic".into(),
            dark_brightness: 100,
            dark_contrast: 100,
            dark_sepia: 0,
            dark_grayscale: 0,
            dark_on_sites: Vec::new(),
            dark_off_sites: Vec::new(),
            allow_copy_all: false,
            allow_copy_sites: Vec::new(),
            dl_engine: true,
            dl_connections: 8,
            dl_min_mb: 2,
            dl_box: true,
            dl_server_info: true,
            sleep_never_sites: Vec::new(),
            sleep_skip_forms: true,
            user_css: String::new(),
            search_engine: "google".into(),
            bangs: true,
            custom_bangs: Vec::new(),
            address_suggestions: true,
            no_search: false,
            block_ads: true,
            blocking_mode: "standard".into(),
            cosmetic_filtering: true,
            block_cookie_notices: true,
            farbling: "standard".into(),
            strip_tracking_params: true,
            de_amp: true,
            debounce: true,
            https_only: true,
            gpc: true,
            block_telemetry: true,
            block_privacy_sandbox: true,
            cname_uncloak: false,
            auto_update_filters: true,
            custom_lists: Vec::new(),
            filter_lists: crate::catalog::default_ids(),
            custom_rules: String::new(),
            exceptions: Vec::new(),
            script_blocked: Vec::new(),
            farbling_off: Vec::new(),
            clear_on_exit: false,
            save_history: true,
            deny_sensitive_permissions: false,
            webrtc_policy: "default".into(),
            network_prediction: true,
            secure_dns: "off".into(),
            autoplay: true,
            password_manager: true,
            password_autosignin: true,
            autofill: true,
            spellcheck: false,
            smartscreen: false,
            edge_tracking_prevention: false,
            local_crash_reports: true,
            disable_webgl: false,
            popups_to_tabs: false,
            disable_beforeunload: false,
            download_dir: String::new(),
            restore_tabs: true,
            autostart: false,
            sleep_tabs_minutes: 15,
            suspend_hidden_tabs: true,
            close_to_tray: true,
            low_memory_when_hidden: true,
            trim_when_hidden: true,
            trim_every_minutes: 5,
            efficiency_mode: true,
            simulate_memory_pressure: true,
            low_end_mode: false,
            v8_optimize_for_size: true,
            site_isolation: true,
            spare_renderer: false,
            renderer_process_limit: 0,
            process_per_site: false,
            no_gpu: false,
            zero_copy: true,
            parallel_downloading: true,
            extra_browser_args: String::new(),
            check_updates: true,
            update_channel: "stable".into(),
            diagnostics: false,
            devtools: false,
            extensions_enabled: false,
            extension_dirs: Vec::new(),
        }
    }
}

/// Isi preset perlindungan: (kunci, Seimbang, Ketat, Maksimal). Setelan lain tidak disentuh.
pub const PRESETS: &[(&str, &str, &str, &str)] = &[
    ("block_ads", "true", "true", "true"),
    ("blocking_mode", "\"standard\"", "\"aggressive\"", "\"aggressive\""),
    ("cosmetic_filtering", "true", "true", "true"),
    ("block_cookie_notices", "true", "true", "true"),
    ("block_annoyances", "false", "true", "true"),
    ("farbling", "\"standard\"", "\"strict\"", "\"strict\""),
    ("strip_tracking_params", "true", "true", "true"),
    ("debounce", "true", "true", "true"),
    ("de_amp", "true", "true", "true"),
    ("https_only", "true", "true", "true"),
    ("https_strict", "false", "false", "true"),
    ("gpc", "true", "true", "true"),
    ("block_telemetry", "true", "true", "true"),
    ("block_privacy_sandbox", "true", "true", "true"),
    ("cname_uncloak", "false", "true", "true"),
    ("block_local_network", "true", "true", "true"),
    ("referrer_policy", "\"default\"", "\"origin\"", "\"none\""),
    ("webrtc_policy", "\"default\"", "\"proxy_only\"", "\"proxy_only\""),
    ("deny_sensitive_permissions", "false", "false", "true"),
    ("letterboxing", "false", "false", "true"),
    ("uniform_fingerprint", "false", "false", "true"),
    ("hide_webview2", "false", "false", "true"),
    ("security_level", "\"standard\"", "\"standard\"", "\"safer\""),
    ("disable_webgl", "false", "false", "true"),
    ("save_history", "true", "true", "false"),
    ("clear_on_exit", "false", "false", "true"),
];

/// Alamat DoH untuk pilihan Secure DNS.
pub fn doh_template(choice: &str) -> Option<String> {
    match choice {
        "cloudflare" => Some("https://cloudflare-dns.com/dns-query".into()),
        "quad9" => Some("https://dns.quad9.net/dns-query".into()),
        "adguard" => Some("https://dns.adguard-dns.com/dns-query".into()),
        "mullvad" => Some("https://dns.mullvad.net/dns-query".into()),
        "mullvad_adblock" => Some("https://adblock.dns.mullvad.net/dns-query".into()),
        c if c.starts_with("https://") => Some(c.to_string()),
        _ => None,
    }
}

fn escape_param(v: &str) -> String {
    url::form_urlencoded::byte_serialize(v.as_bytes()).collect()
}

pub fn norm_host(h: &str) -> String {
    h.trim().trim_start_matches("www.").trim_end_matches('/').to_ascii_lowercase()
}

/// `host` (atau domain induknya) ada di daftar.
pub fn host_in(list: &[String], host: &str) -> bool {
    let host = norm_host(host);
    !host.is_empty() && list.iter().any(|e| host == *e || host.ends_with(&format!(".{e}")))
}

/// Tambah/hapus `host` dari daftar. Mengembalikan true bila sekarang ada di daftar.
pub fn toggle_host(list: &mut Vec<String>, host: &str) -> bool {
    let host = norm_host(host);
    if host.is_empty() {
        return false;
    }
    if host_in(list, &host) {
        list.retain(|e| !(host == *e || host.ends_with(&format!(".{e}"))));
        false
    } else {
        list.push(host);
        true
    }
}

impl Settings {
    pub fn path(data_dir: &Path) -> PathBuf {
        data_dir.join("settings.json")
    }

    pub fn load(data_dir: &Path) -> Settings {
        let mut s: Settings = std::fs::read_to_string(Self::path(data_dir))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        s.migrate();
        s.normalize();
        s
    }

    pub fn save(&self, data_dir: &Path) {
        let _ = std::fs::create_dir_all(data_dir);
        if let Ok(t) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(Self::path(data_dir), t);
        }
    }

    /// Ubah bawaan lama yang ternyata keliru untuk pengguna versi sebelumnya.
    fn migrate(&mut self) {
        if self.schema < 1 {
            // 0.3.0: disamakan dengan Brave Origin (repo flyingPenguinW/brave-origin).
            self.network_prediction = true;
            self.zero_copy = true;
            self.newtab = "blank".into();
            self.devtools = false;
            self.spellcheck = false;
        }
        if self.schema < 3 {
            // 0.5: pengelola sandi dan isi otomatis seperti Brave (diminta pengguna).
            self.password_manager = true;
            self.password_autosignin = true;
            self.autofill = true;
        }
        if self.schema < 4 && self.secure_dns == ["off", "quad9", "mullvad_adblock"][match self.preset.as_str() {
            "strict" => 1,
            "max" => 2,
            _ => 0,
        }] {
            // 0.5.1: preset tidak lagi mengganti DNS. DNS yang dipasang preset Ketat/Maksimal
            // dikembalikan ke DNS Windows, karena sebagian jaringan (kantor, ISP) memaksa
            // YouTube Restricted Mode (error 151) atau memblokir DNS aman.
            self.secure_dns = "off".into();
        }
        if self.schema < 7 && !self.uniform_fingerprint {
            // 0.7.5: samaran "Microsoft Edge" lewat DevTools hanya berlaku di halaman, tidak di
            // service worker/worker, sehingga satu situs menerima dua user agent berbeda dan
            // sesi yang terikat user agent rusak (SIPP BPJS, Infor). Identitas asli WebView2
            // (Edge) kini dipakai; samaran hanya di mode seragam/preset Maksimal.
            self.hide_webview2 = false;
        }
        if self.schema < 6 {
            // 0.7.1: daftar opsional seperti Brave; yang bawaan Brave ikut dinyalakan.
            for id in crate::catalog::default_ids() {
                if !self.filter_lists.contains(&id) {
                    self.filter_lists.push(id);
                }
            }
        }
        if self.schema < 5 && self.force_dark {
            // 0.7: "Paksa mode gelap" menjadi mode gelap per situs (mesin Chromium, sama persis
            // seperti sebelumnya, tetapi sekarang bisa diatur per situs tanpa buka ulang).
            self.dark_mode = "on".into();
            self.dark_engine = "chromium".into();
        }
        if self.schema < 5 {
            self.force_dark = false;
        }
        if self.schema < 2 {
            // 0.3.1: "Matikan GPU" menjadi pilihan mode GPU.
            self.gpu_mode = if self.no_gpu { "off".into() } else { "full".into() };
            self.no_gpu = false;
            self.preset = "custom".into();
        }
        self.schema = SCHEMA;
    }

    fn normalize(&mut self) {
        let pick = |v: &mut String, ok: &[&str], def: &str| {
            if !ok.contains(&v.as_str()) {
                *v = def.into();
            }
        };
        pick(&mut self.language, &["auto", "id", "en"], "auto");
        pick(&mut self.theme, &["system", "light", "dark"], "system");
        pick(&mut self.newtab, &["blank", "ovner"], "blank");
        pick(&mut self.webrtc_policy, &["default", "proxy_only"], "default");
        pick(&mut self.farbling, &["off", "standard", "strict"], "standard");
        pick(&mut self.preset, &["balanced", "strict", "max", "custom"], "custom");
        pick(&mut self.referrer_policy, &["default", "origin", "none"], "default");
        pick(&mut self.security_level, &["standard", "safer", "safest"], "standard");
        pick(&mut self.gpu_mode, &["full", "lite", "off"], "full");
        pick(&mut self.background_media, &["pause", "keep", "timer"], "keep");
        pick(&mut self.dark_mode, &["off", "on", "system"], "off");
        pick(&mut self.blocking_mode, &["standard", "aggressive"], "standard");
        pick(&mut self.dark_engine, &["dynamic", "filter", "chromium"], "dynamic");
        self.dark_brightness = self.dark_brightness.clamp(30, 150);
        self.dark_contrast = self.dark_contrast.clamp(30, 150);
        self.dark_sepia = self.dark_sepia.min(100);
        self.dark_grayscale = self.dark_grayscale.min(100);
        self.dl_connections = self.dl_connections.clamp(1, 32);
        self.dl_min_mb = self.dl_min_mb.min(10_000);
        self.background_media_minutes = self.background_media_minutes.clamp(1, 24 * 60);
        pick(&mut self.youtube_h264, &["auto", "on", "off"], "auto");
        self.filter_update_hours = self.filter_update_hours.clamp(1, 24 * 7);
        pick(&mut self.update_channel, &["stable", "nightly"], "stable");
        if !["off", "cloudflare", "quad9", "adguard", "mullvad", "mullvad_adblock"].contains(&self.secure_dns.as_str()) && !self.secure_dns.starts_with("https://") {
            self.secure_dns = "off".into();
        }
        self.default_zoom = self.default_zoom.clamp(25, 500);
        self.sleep_tabs_minutes = self.sleep_tabs_minutes.min(24 * 60);
        self.trim_every_minutes = self.trim_every_minutes.min(24 * 60);
        self.renderer_process_limit = self.renderer_process_limit.min(64);
        for list in [
            &mut self.exceptions,
            &mut self.script_blocked,
            &mut self.farbling_off,
            &mut self.script_allowed,
            &mut self.dark_on_sites,
            &mut self.dark_off_sites,
            &mut self.allow_copy_sites,
            &mut self.sleep_never_sites,
        ] {
            *list = list.iter().map(|h| norm_host(h)).filter(|h| !h.is_empty()).collect();
            list.sort();
            list.dedup();
        }
        let clean = |v: &mut Vec<String>| {
            *v = v.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        };
        clean(&mut self.custom_lists);
        self.filter_lists.retain(|id| crate::catalog::find(id).is_some());
        self.filter_lists.sort();
        self.filter_lists.dedup();
        clean(&mut self.custom_bangs);
        clean(&mut self.extension_dirs);
    }

    /// Ubah satu pengaturan dari halaman pengaturan (nilai JSON). Mengembalikan false bila
    /// kunci tidak dikenal atau nilainya salah jenis.
    pub fn set(&mut self, key: &str, value: &serde_json::Value) -> bool {
        if key == "schema" {
            return false;
        }
        let Ok(mut map) = serde_json::to_value(&*self) else { return false };
        let Some(obj) = map.as_object_mut() else { return false };
        let Some(slot) = obj.get_mut(key) else { return false };
        let same_kind = matches!(
            (&*slot, value),
            (serde_json::Value::Bool(_), serde_json::Value::Bool(_))
                | (serde_json::Value::Number(_), serde_json::Value::Number(_))
                | (serde_json::Value::String(_), serde_json::Value::String(_))
                | (serde_json::Value::Array(_), serde_json::Value::Array(_))
        );
        if !same_kind {
            return false;
        }
        *slot = value.clone();
        match serde_json::from_value::<Settings>(map) {
            Ok(mut s) => {
                s.normalize();
                if PRESETS.iter().any(|p| p.0 == key) {
                    s.preset = s.detect_preset().into();
                }
                *self = s;
                true
            }
            Err(_) => false,
        }
    }

    /// Apakah JavaScript boleh jalan di alamat ini (blokir per situs + level keamanan).
    pub fn scripts_allowed(&self, url: &str) -> bool {
        let host = url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
        if host_in(&self.script_blocked, &host) {
            return false;
        }
        match self.security_level.as_str() {
            "safer" => !url.starts_with("http://") || host_in(&self.script_allowed, &host),
            "safest" => host_in(&self.script_allowed, &host),
            _ => true,
        }
    }

    /// Terapkan preset "balanced" / "strict" / "max".
    pub fn apply_preset(&mut self, name: &str) -> bool {
        let col = match name {
            "balanced" => 0,
            "strict" => 1,
            "max" => 2,
            _ => return false,
        };
        let Ok(mut map) = serde_json::to_value(&*self) else { return false };
        for (key, a, b, c) in PRESETS {
            let v = [a, b, c][col];
            if let (Some(slot), Ok(val)) = (map.get_mut(*key), serde_json::from_str::<serde_json::Value>(v)) {
                *slot = val;
            }
        }
        match serde_json::from_value::<Settings>(map) {
            Ok(mut s) => {
                s.normalize();
                s.preset = name.into();
                *self = s;
                true
            }
            Err(_) => false,
        }
    }

    /// Preset yang cocok persis dengan setelan sekarang, atau "custom".
    pub fn detect_preset(&self) -> &'static str {
        let Ok(map) = serde_json::to_value(self) else { return "custom" };
        for (col, name) in ["balanced", "strict", "max"].iter().enumerate() {
            let all = PRESETS.iter().all(|(key, a, b, c)| {
                let want: serde_json::Value = serde_json::from_str([a, b, c][col]).unwrap_or_default();
                map.get(*key) == Some(&want)
            });
            if all {
                return name;
            }
        }
        "custom"
    }

    /// Argumen baris perintah WebView2 dari pengaturan (berlaku saat Ovner dibuka).
    pub fn browser_args(&self) -> String {
        let mut off: Vec<&str> = vec![
            // Fitur Edge yang tidak dipakai Ovner tetapi memakan RAM/CPU.
            "msWebOOUI",
            "msPdfOOUI",
            "msEdgeShoppingAssistant",
            "Translate",
            "MediaRouter",
            "OptimizationHints",
            "AutofillServerCommunication",
        ];
        let mut on: Vec<String> = Vec::new();
        let mut extra = String::new();
        if !self.spare_renderer {
            off.push("SpareRendererForSitePerProcess");
        }
        if !self.site_isolation {
            off.extend(["IsolateOrigins", "site-per-process"]);
            extra.push_str(" --disable-site-isolation-trials");
        }
        if !self.smartscreen {
            off.push("msSmartScreenProtection");
        }
        if self.block_privacy_sandbox {
            // Seperti Helium: API iklan Chromium dan FedCM dimatikan.
            off.extend([
                "BrowsingTopics",
                "BrowsingTopicsXHR",
                "Fledge",
                "InterestGroupStorage",
                "PrivacySandboxAdsAPIs",
                "PrivacySandboxAdsAPIsOverride",
                "AttributionReportingCrossAppWeb",
                "ConversionMeasurement",
                "SharedStorageAPI",
                "FedCm",
            ]);
            extra.push_str(" --disable-field-trial-config");
        }
        if self.simulate_memory_pressure {
            on.push("msWebView2SimulateMemoryPressureWhenInactive".into());
        }
        if self.parallel_downloading {
            on.push("ParallelDownloading".into());
        }
        if self.force_dark {
            on.push("WebContentsForceDark".into());
        }
        if let Some(t) = doh_template(&self.secure_dns) {
            let fallback = if self.dns_strict { "false" } else { "true" };
            on.push(format!("DnsOverHttps:Fallback/{fallback}/Templates/{}", escape_param(&t)));
        }
        if !self.network_prediction {
            extra.push_str(" --dns-prefetch-disable");
        }
        if self.low_end_mode {
            extra.push_str(" --enable-low-end-device-mode");
        }
        // Semua flag V8 harus dalam satu --js-flags.
        let mut js: Vec<&str> = Vec::new();
        if self.v8_optimize_for_size {
            js.push("--optimize-for-size");
        }
        if self.disable_jit {
            // Seperti "Enhance security" Edge: JIT JavaScript mati.
            js.push("--jitless");
        }
        if !js.is_empty() {
            if js.len() == 1 {
                extra.push_str(&format!(" --js-flags={}", js[0]));
            } else {
                extra.push_str(&format!(" --js-flags=\"{}\"", js.join(" ")));
            }
        }
        match self.gpu_mode.as_str() {
            "off" => extra.push_str(" --disable-gpu"),
            // GPU tetap untuk video, kanvas, dan komposisi (video tajam); hanya gambar halaman
            // (teks, tata letak) yang dirender CPU.
            "lite" => extra.push_str(" --disable-gpu-rasterization"),
            _ => {
                if self.zero_copy {
                    extra.push_str(" --enable-zero-copy");
                }
            }
        }

        if self.disable_webgl {
            extra.push_str(" --disable-webgl");
        }
        if self.renderer_process_limit > 0 {
            extra.push_str(&format!(" --renderer-process-limit={}", self.renderer_process_limit));
        }
        if self.process_per_site {
            extra.push_str(" --process-per-site");
        }
        if self.webrtc_policy == "proxy_only" {
            extra.push_str(" --force-webrtc-ip-handling-policy=disable_non_proxied_udp");
        }
        if !self.autoplay {
            extra.push_str(" --autoplay-policy=user-gesture-required");
        }
        let mut a = format!("--disable-features={} --disable-background-networking", off.join(","));
        if !on.is_empty() {
            a.push_str(&format!(" --enable-features={}", on.join(",")));
        }
        a.push_str(&extra);
        if !self.extra_browser_args.trim().is_empty() {
            a.push(' ');
            a.push_str(self.extra_browser_args.trim());
        }
        a
    }

    /// URL pencarian untuk teks yang diketik.
    pub fn search_url(&self, query: &str) -> String {
        let template = match self.search_engine.to_ascii_lowercase().as_str() {
            "duckduckgo" | "ddg" => "https://duckduckgo.com/?q=%s",
            "bing" => "https://www.bing.com/search?q=%s",
            "brave" => "https://search.brave.com/search?q=%s",
            "google" => "https://www.google.com/search?q=%s",
            "qwant" => "https://www.qwant.com/?q=%s",
            // Alamat formulir pencarian Startpage sendiri (seperti mengetik di startpage.com).
            "startpage" => "https://www.startpage.com/sp/search?query=%s&cat=web",
            "leta" => "https://leta.mullvad.net/search?q=%s",
            "ecosia" => "https://www.ecosia.org/search?q=%s",
            "mojeek" => "https://www.mojeek.com/search?q=%s",
            other if other.contains("%s") && other.starts_with("http") => self.search_engine.as_str(),
            _ => "https://www.google.com/search?q=%s",
        };
        let q: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
        template.replacen("%s", &q, 1)
    }

    pub fn is_exception(&self, host: &str) -> bool {
        host_in(&self.exceptions, host)
    }

    pub fn toggle_exception(&mut self, host: &str) -> bool {
        toggle_host(&mut self.exceptions, host)
    }

    /// Mesin mode gelap untuk situs ini, atau `None` bila situs ini tidak digelapkan.
    pub fn dark_for(&self, host: &str, system_dark: bool) -> Option<&str> {
        if host.is_empty() || host_in(&self.dark_off_sites, host) {
            return None;
        }
        let on = host_in(&self.dark_on_sites, host)
            || match self.dark_mode.as_str() {
                "on" => true,
                "system" => system_dark,
                _ => false,
            };
        on.then_some(self.dark_engine.as_str())
    }

    /// Larangan salin/klik kanan dibuka di situs ini?
    pub fn allow_copy_for(&self, host: &str) -> bool {
        !host.is_empty() && (self.allow_copy_all || host_in(&self.allow_copy_sites, host))
    }

    /// Pengacakan sidik jari yang berlaku untuk situs ini ("off", "standard", "strict").
    pub fn farbling_for(&self, host: &str) -> &str {
        if host_in(&self.farbling_off, host) || self.is_exception(host) {
            "off"
        } else {
            &self.farbling
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_defaults() {
        let mut s = Settings::default();
        s.exceptions = vec!["kompas.com".into()];
        s.sleep_tabs_minutes = 5;
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), s);
    }

    #[test]
    fn migration_applies_brave_origin_defaults_once() {
        // File 0.2.0: tanpa "schema", nilai lama keliru.
        let old = r#"{"network_prediction":false,"zero_copy":false,"newtab":"ovner","devtools":true,"block_ads":false}"#;
        let mut s: Settings = serde_json::from_str(old).unwrap();
        assert_eq!(s.schema, 0);
        s.migrate();
        assert!(s.network_prediction && s.zero_copy && !s.devtools);
        assert_eq!(s.newtab, "blank");
        assert!(!s.block_ads, "setelan lain tidak disentuh");
        assert_eq!(s.schema, SCHEMA);
        // Setelah migrasi, pilihan pengguna dihormati.
        s.newtab = "ovner".into();
        let again: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        let mut again = again;
        again.migrate();
        assert_eq!(again.newtab, "ovner");
    }

    #[test]
    fn set_checks_key_and_kind() {
        let mut s = Settings::default();
        assert!(s.set("block_ads", &serde_json::json!(false)));
        assert!(!s.block_ads);
        assert!(!s.set("block_ads", &serde_json::json!("ya")));
        assert!(!s.set("tidak_ada", &serde_json::json!(true)));
        assert!(!s.set("schema", &serde_json::json!(0)));
        assert!(s.set("default_zoom", &serde_json::json!(900)));
        assert_eq!(s.default_zoom, 500);
        assert!(s.set("theme", &serde_json::json!("ungu")));
        assert_eq!(s.theme, "system");
        assert!(s.set("exceptions", &serde_json::json!(["www.Detik.com/", ""])));
        assert_eq!(s.exceptions, vec!["detik.com"]);
    }

    #[test]
    fn browser_args_follow_settings() {
        let a = Settings::default().browser_args();
        assert!(a.contains("SpareRendererForSitePerProcess"));
        assert!(a.contains("msWebView2SimulateMemoryPressureWhenInactive"));
        assert!(a.contains("--js-flags=--optimize-for-size"));
        assert!(a.contains("BrowsingTopics") && a.contains("--disable-field-trial-config"));
        assert!(a.contains("--enable-zero-copy"));
        assert!(!a.contains("--dns-prefetch-disable"));
        assert!(!a.contains("--disable-gpu") && !a.contains("DnsOverHttps"));
        let s = Settings {
            gpu_mode: "off".into(),
            site_isolation: false,
            webrtc_policy: "proxy_only".into(),
            secure_dns: "quad9".into(),
            renderer_process_limit: 4,
            force_dark: true,
            extra_browser_args: "--foo".into(),
            ..Settings::default()
        };
        let a = s.browser_args();
        assert!(a.contains("--disable-gpu") && a.contains("site-per-process") && a.contains("disable_non_proxied_udp"));
        assert!(a.contains("DnsOverHttps:Fallback/true/Templates/https%3A%2F%2Fdns.quad9.net%2Fdns-query"));
        assert!(a.contains("--renderer-process-limit=4") && a.contains("WebContentsForceDark") && a.ends_with("--foo"));
        // Hanya satu --enable-features (Chromium memakai yang terakhir).
        assert_eq!(a.matches("--enable-features=").count(), 1);
    }

    #[test]
    fn presets_and_gpu() {
        let mut s = Settings::default();
        assert_eq!(s.detect_preset(), "balanced");
        assert!(s.apply_preset("max"));
        assert_eq!((s.preset.as_str(), s.farbling.as_str(), s.security_level.as_str()), ("max", "strict", "safer"));
        // Level keamanan tidak lagi mematikan JIT (YouTube jadi lambat/hitam); itu setelan sendiri.
        // Preset tidak menyentuh DNS (jaringan tertentu memaksa YouTube Restricted Mode).
        assert!(!s.browser_args().contains("--jitless") && !s.browser_args().contains("DnsOverHttps"));
        let mut m: Settings = serde_json::from_str(r#"{"schema":3,"preset":"max","secure_dns":"mullvad_adblock"}"#).unwrap();
        m.migrate();
        assert_eq!(m.secure_dns, "off");
        let mut k: Settings = serde_json::from_str(r#"{"schema":3,"preset":"custom","secure_dns":"quad9"}"#).unwrap();
        k.migrate();
        assert_eq!(k.secure_dns, "quad9");
        assert!(Settings { disable_jit: true, ..Settings::default() }.browser_args().contains("--jitless"));
        assert!(s.set("letterboxing", &serde_json::json!(false)));
        assert_eq!(s.preset, "custom");
        assert!(s.set("letterboxing", &serde_json::json!(true)));
        assert_eq!(s.preset, "max");
        let lite = Settings { gpu_mode: "lite".into(), ..Settings::default() }.browser_args();
        assert!(lite.contains("--disable-gpu-rasterization") && !lite.contains("--disable-gpu "));
        // Migrasi 0.3: "Matikan GPU" → mode GPU mati.
        let mut old: Settings = serde_json::from_str(r#"{"schema":1,"no_gpu":true}"#).unwrap();
        old.migrate();
        assert_eq!(old.gpu_mode, "off");
    }

    #[test]
    fn security_levels() {
        let mut s = Settings::default();
        assert!(s.scripts_allowed("http://a.id/") && s.scripts_allowed("https://a.id/"));
        s.security_level = "safer".into();
        assert!(!s.scripts_allowed("http://a.id/") && s.scripts_allowed("https://a.id/"));
        s.security_level = "safest".into();
        s.script_allowed = vec!["bank.co.id".into()];
        assert!(!s.scripts_allowed("https://a.id/") && s.scripts_allowed("https://ib.bank.co.id/"));
        s.script_blocked = vec!["bank.co.id".into()];
        assert!(!s.scripts_allowed("https://ib.bank.co.id/"));
    }

    #[test]
    fn search_urls() {
        let s = Settings::default();
        assert_eq!(s.search_url("harga cabai"), "https://www.google.com/search?q=harga+cabai");
        let s = Settings { search_engine: "https://x.id/?q=%s".into(), ..Settings::default() };
        assert_eq!(s.search_url("a&b"), "https://x.id/?q=a%26b");
    }

    #[test]
    fn dark_mode_and_allow_copy_per_site() {
        let mut s = Settings::default();
        assert_eq!(s.dark_for("example.com", true), None);
        s.dark_on_sites.push("example.com".into());
        assert_eq!(s.dark_for("example.com", false), Some("dynamic"));
        assert_eq!(s.dark_for("sub.example.com", false), Some("dynamic"));
        s.dark_mode = "system".into();
        assert_eq!(s.dark_for("other.id", true), Some("dynamic"));
        assert_eq!(s.dark_for("other.id", false), None);
        s.dark_off_sites.push("other.id".into());
        assert_eq!(s.dark_for("other.id", true), None);
        assert!(!s.allow_copy_for("bpjs.go.id"));
        s.allow_copy_sites.push("bpjs.go.id".into());
        assert!(s.allow_copy_for("www.bpjs.go.id") || s.allow_copy_for("bpjs.go.id"));
        // 0.7: "Paksa mode gelap" lama berpindah ke mode gelap per situs (mesin Chromium).
        let old = r#"{"schema":4,"force_dark":true}"#;
        let dir = std::env::temp_dir().join(format!("ovner-dark-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(Settings::path(&dir), old).unwrap();
        let m = Settings::load(&dir);
        assert_eq!((m.dark_mode.as_str(), m.dark_engine.as_str(), m.force_dark), ("on", "chromium", false));
        assert!(!m.browser_args().contains("WebContentsForceDark"));
        let _ = std::fs::remove_dir_all(&dir);
        let mut c = Settings::default();
        assert!(c.set("dark_brightness", &serde_json::json!(999)));
        assert_eq!(c.dark_brightness, 150);
        c.search_engine = "startpage".into();
        assert!(c.search_url("kartu bpjs").starts_with("https://www.startpage.com/sp/search?query=kartu+bpjs"));
    }

    #[test]
    fn per_site_lists() {
        let mut s = Settings::default();
        assert!(s.toggle_exception("www.kompas.com"));
        assert!(s.is_exception("money.kompas.com"));
        assert!(!s.is_exception("notkompas.com"));
        assert!(!s.toggle_exception("money.kompas.com"));
        assert!(toggle_host(&mut s.farbling_off, "bank.co.id"));
        assert_eq!(s.farbling_for("ib.bank.co.id"), "off");
        assert_eq!(s.farbling_for("kompas.com"), "standard");
    }
}
