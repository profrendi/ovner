//! Pengaturan Ovner, disimpan di `<data>/settings.json` dan diubah lewat halaman
//! `ovner://settings`. Bagian-bagiannya mengikuti halaman pengaturan Brave (Origin):
//! tampilan, pencarian, Shields, privasi, unduhan, saat mulai, sistem, ditambah bagian
//! hemat RAM dari Outlook Lite/ZFPlus.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Settings {
    // ---------- tampilan ----------
    /// "system", "light", atau "dark".
    pub theme: String,
    pub bookmark_bar: bool,
    pub home_button: bool,
    /// Alamat tombol beranda (kosong = halaman tab baru).
    pub home_url: String,
    /// Halaman tab baru: "ovner" (kotak cari) atau "blank" (kosong, seperti Brave Origin).
    pub newtab: String,
    /// Zoom bawaan halaman, persen.
    pub default_zoom: u32,

    // ---------- pencarian ----------
    /// google, duckduckgo, bing, brave, atau URL berisi `%s`.
    pub search_engine: String,

    // ---------- Shields (pemblokir) ----------
    pub block_ads: bool,
    /// Sembunyikan kotak iklan yang kosong (aturan kosmetik EasyList).
    pub cosmetic_filtering: bool,
    /// Buang parameter pelacak klik (fbclid, utm_*, ...) dari alamat.
    pub strip_tracking_params: bool,
    /// Naikkan http:// ke https:// (alamat lokal dikecualikan).
    pub https_only: bool,
    /// Kirim tajuk Global Privacy Control dan Do Not Track.
    pub gpc: bool,
    /// Blokir telemetri Microsoft Edge/WebView2 dan Office.
    pub block_telemetry: bool,
    /// Daftar filter diperbarui otomatis seminggu sekali.
    pub auto_update_filters: bool,
    /// Situs (host) tempat pemblokir dimatikan.
    pub exceptions: Vec<String>,

    // ---------- privasi & keamanan ----------
    /// Hapus cookie, cache, dan riwayat situs saat Ovner ditutup.
    pub clear_on_exit: bool,
    /// Simpan riwayat kunjungan di Ovner.
    pub save_history: bool,
    /// Tolak akses kamera, mikrofon, lokasi, dan sensor tanpa bertanya.
    pub deny_sensitive_permissions: bool,
    /// Kebijakan WebRTC: "default" atau "proxy_only" (sembunyikan IP lokal).
    pub webrtc_policy: String,
    /// Prediksi jaringan (prefetch DNS, preconnect). Mati = seperti Brave Origin.
    pub network_prediction: bool,
    /// Putar video/audio otomatis tanpa klik.
    pub autoplay: bool,
    /// Pengelola kata sandi bawaan WebView2 (Brave Origin: mati).
    pub password_manager: bool,
    /// Isi otomatis alamat/formulir (Brave Origin: mati).
    pub autofill: bool,
    /// SmartScreen (cek reputasi alamat ke Microsoft).
    pub smartscreen: bool,
    /// Tracking prevention bawaan Edge (pemblokir sudah ada di Ovner; mati = lebih ringan).
    pub edge_tracking_prevention: bool,
    /// Laporan crash WebView2 disimpan lokal, bukan dikirim ke Microsoft.
    pub local_crash_reports: bool,

    // ---------- unduhan ----------
    /// Folder unduhan (kosong = bawaan Windows).
    pub download_dir: String,

    // ---------- saat mulai ----------
    pub restore_tabs: bool,
    /// Mulai bersama Windows (di tray).
    pub autostart: bool,

    // ---------- hemat RAM ----------
    /// Tab yang tidak dibuka selama ini (menit) ditidurkan. 0 = tidak pernah.
    pub sleep_tabs_minutes: u64,
    /// Tab di belakang (tidak terlihat) ditangguhkan WebView2 (TrySuspend): JavaScript dan
    /// timer berhenti, RAM dilepas, halaman tetap utuh dan langsung tampil saat diklik.
    pub suspend_hidden_tabs: bool,
    /// Tombol X menyembunyikan Ovner ke tray (bukan keluar).
    pub close_to_tray: bool,
    /// Saat diminimalkan/di tray: mode memori rendah WebView2 untuk semua tab.
    pub low_memory_when_hidden: bool,
    /// Saat diminimalkan/di tray: rapikan working set semua proses (RAM di Task Manager turun).
    pub trim_when_hidden: bool,
    /// Rapikan ulang tiap N menit selama tersembunyi. 0 = sekali saja.
    pub trim_every_minutes: u32,
    /// Mode efisiensi Windows (EcoQoS) + prioritas rendah untuk proses WebView2 saat tersembunyi.
    pub efficiency_mode: bool,
    /// Flag WebView2: tab tidak aktif diperlakukan seperti kekurangan memori (bersih-bersih sendiri).
    pub simulate_memory_pressure: bool,
    /// Mode perangkat RAM kecil Chromium: cache gambar/grafis lebih kecil.
    pub low_end_mode: bool,
    /// V8 mengutamakan ukuran memori daripada kecepatan.
    pub v8_optimize_for_size: bool,
    /// Pisahkan iframe lintas-situs ke proses sendiri (lebih aman, lebih banyak RAM).
    pub site_isolation: bool,
    /// Proses renderer cadangan Chromium (tab baru terbuka lebih cepat, +RAM).
    pub spare_renderer: bool,
    /// Matikan akselerasi GPU (hemat ~50-80 MB; video jadi lebih berat ke CPU).
    pub no_gpu: bool,
    /// Zero-copy rasterizer (saran Brave Origin).
    pub zero_copy: bool,
    /// Unduhan paralel (saran Brave Origin).
    pub parallel_downloading: bool,
    /// Argumen tambahan WebView2 (lanjutan).
    pub extra_browser_args: String,

    // ---------- sistem ----------
    pub check_updates: bool,
    /// Catat pemakaian RAM ke log tiap 5 menit.
    pub diagnostics: bool,
    /// Izinkan DevTools (F12).
    pub devtools: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: "system".into(),
            bookmark_bar: false,
            home_button: false,
            home_url: String::new(),
            newtab: "ovner".into(),
            default_zoom: 100,
            search_engine: "google".into(),
            block_ads: true,
            cosmetic_filtering: true,
            strip_tracking_params: true,
            https_only: true,
            gpc: true,
            block_telemetry: true,
            auto_update_filters: true,
            exceptions: Vec::new(),
            clear_on_exit: false,
            save_history: true,
            deny_sensitive_permissions: false,
            webrtc_policy: "default".into(),
            network_prediction: false,
            autoplay: true,
            password_manager: false,
            autofill: false,
            smartscreen: false,
            edge_tracking_prevention: false,
            local_crash_reports: true,
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
            no_gpu: false,
            zero_copy: false,
            parallel_downloading: true,
            extra_browser_args: String::new(),
            check_updates: true,
            diagnostics: false,
            devtools: true,
        }
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
        s.normalize();
        s
    }

    pub fn save(&self, data_dir: &Path) {
        let _ = std::fs::create_dir_all(data_dir);
        if let Ok(t) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(Self::path(data_dir), t);
        }
    }

    fn normalize(&mut self) {
        if !["system", "light", "dark"].contains(&self.theme.as_str()) {
            self.theme = "system".into();
        }
        if !["ovner", "blank"].contains(&self.newtab.as_str()) {
            self.newtab = "ovner".into();
        }
        if !["default", "proxy_only"].contains(&self.webrtc_policy.as_str()) {
            self.webrtc_policy = "default".into();
        }
        self.default_zoom = self.default_zoom.clamp(25, 500);
        self.sleep_tabs_minutes = self.sleep_tabs_minutes.min(24 * 60);
        self.trim_every_minutes = self.trim_every_minutes.min(24 * 60);
        self.exceptions = self.exceptions.iter().map(|h| norm_host(h)).filter(|h| !h.is_empty()).collect();
        self.exceptions.dedup();
    }

    /// Ubah satu pengaturan dari halaman pengaturan (nilai JSON). Mengembalikan false bila
    /// kunci tidak dikenal atau nilainya salah jenis.
    pub fn set(&mut self, key: &str, value: &serde_json::Value) -> bool {
        let Ok(mut map) = serde_json::to_value(&*self) else { return false };
        let Some(obj) = map.as_object_mut() else { return false };
        let Some(slot) = obj.get_mut(key) else { return false };
        let same_kind = match (&*slot, value) {
            (serde_json::Value::Bool(_), serde_json::Value::Bool(_)) => true,
            (serde_json::Value::Number(_), serde_json::Value::Number(_)) => true,
            (serde_json::Value::String(_), serde_json::Value::String(_)) => true,
            (serde_json::Value::Array(_), serde_json::Value::Array(_)) => true,
            _ => false,
        };
        if !same_kind {
            return false;
        }
        *slot = value.clone();
        match serde_json::from_value::<Settings>(map) {
            Ok(mut s) => {
                s.normalize();
                *self = s;
                true
            }
            Err(_) => false,
        }
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
        let mut on: Vec<&str> = Vec::new();
        let mut extra = String::new();
        if !self.spare_renderer {
            off.push("SpareRendererForSitePerProcess");
        }
        if !self.site_isolation {
            off.push("IsolateOrigins");
            off.push("site-per-process");
            extra.push_str(" --disable-site-isolation-trials");
        }
        if !self.smartscreen {
            off.push("msSmartScreenProtection");
        }
        if self.simulate_memory_pressure {
            on.push("msWebView2SimulateMemoryPressureWhenInactive");
        }
        if self.parallel_downloading {
            on.push("ParallelDownloading");
        }
        if !self.network_prediction {
            extra.push_str(" --dns-prefetch-disable");
        }
        if self.low_end_mode {
            extra.push_str(" --enable-low-end-device-mode");
        }
        if self.v8_optimize_for_size {
            extra.push_str(" --js-flags=--optimize-for-size");
        }
        if self.no_gpu {
            extra.push_str(" --disable-gpu");
        }
        if self.zero_copy {
            extra.push_str(" --enable-zero-copy");
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
            other if other.contains("%s") && other.starts_with("http") => self.search_engine.as_str(),
            _ => "https://www.google.com/search?q=%s",
        };
        let q: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
        template.replacen("%s", &q, 1)
    }

    pub fn is_exception(&self, host: &str) -> bool {
        let host = norm_host(host);
        self.exceptions.iter().any(|e| host == *e || host.ends_with(&format!(".{e}")))
    }

    /// Nyalakan/matikan pemblokir di situs ini. Mengembalikan true bila sekarang dikecualikan.
    pub fn toggle_exception(&mut self, host: &str) -> bool {
        let host = norm_host(host);
        if host.is_empty() {
            return false;
        }
        if self.is_exception(&host) {
            self.exceptions.retain(|e| !(host == *e || host.ends_with(&format!(".{e}"))));
            false
        } else {
            self.exceptions.push(host);
            true
        }
    }
}

fn norm_host(h: &str) -> String {
    h.trim().trim_start_matches("www.").trim_end_matches('/').to_ascii_lowercase()
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
        assert_eq!(serde_json::from_str::<Settings>("{}").unwrap(), Settings::default());
    }

    #[test]
    fn set_checks_key_and_kind() {
        let mut s = Settings::default();
        assert!(s.set("block_ads", &serde_json::json!(false)));
        assert!(!s.block_ads);
        assert!(!s.set("block_ads", &serde_json::json!("ya")));
        assert!(!s.set("tidak_ada", &serde_json::json!(true)));
        assert!(s.set("default_zoom", &serde_json::json!(900)));
        assert_eq!(s.default_zoom, 500);
        assert!(s.set("theme", &serde_json::json!("ungu")));
        assert_eq!(s.theme, "system");
        assert!(s.set("exceptions", &serde_json::json!(["www.Detik.com/", ""])));
        assert_eq!(s.exceptions, vec!["detik.com"]);
    }

    #[test]
    fn browser_args_follow_settings() {
        let s = Settings::default();
        let a = s.browser_args();
        assert!(a.contains("SpareRendererForSitePerProcess"));
        assert!(a.contains("msWebView2SimulateMemoryPressureWhenInactive"));
        assert!(a.contains("--js-flags=--optimize-for-size"));
        assert!(!a.contains("--disable-gpu"));
        let s = Settings { no_gpu: true, site_isolation: false, webrtc_policy: "proxy_only".into(), extra_browser_args: "--foo".into(), ..Settings::default() };
        let a = s.browser_args();
        assert!(a.contains("--disable-gpu") && a.contains("site-per-process") && a.contains("disable_non_proxied_udp") && a.ends_with("--foo"));
    }

    #[test]
    fn search_urls() {
        let s = Settings::default();
        assert_eq!(s.search_url("harga cabai"), "https://www.google.com/search?q=harga+cabai");
        let s = Settings { search_engine: "https://x.id/?q=%s".into(), ..Settings::default() };
        assert_eq!(s.search_url("a&b"), "https://x.id/?q=a%26b");
    }

    #[test]
    fn exceptions_match_subdomains_and_toggle() {
        let mut s = Settings::default();
        assert!(s.toggle_exception("www.kompas.com"));
        assert!(s.is_exception("kompas.com"));
        assert!(s.is_exception("money.kompas.com"));
        assert!(!s.is_exception("notkompas.com"));
        assert!(!s.toggle_exception("money.kompas.com"));
        assert!(!s.is_exception("kompas.com"));
    }
}
