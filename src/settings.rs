//! Pengaturan Ovner, disimpan di `<data>/settings.json` dan diubah lewat halaman
//! `ovner://settings`. Bagian-bagiannya mengikuti Brave Origin: tampilan, pencarian,
//! Shields, privasi, unduhan, saat mulai, sistem; ditambah hemat RAM dari Outlook Lite/ZFPlus
//! dan fitur dari ungoogled-chromium, Helium, dan Servo.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Versi skema setelan. Naik bila bawaan lama perlu diubah untuk pengguna yang sudah ada.
pub const SCHEMA: u32 = 1;

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
    /// Paksa mode gelap di semua situs (fitur Chromium).
    pub force_dark: bool,
    /// CSS sendiri yang dipasang di semua halaman.
    pub user_css: String,

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

    // ---------- Shields ----------
    pub block_ads: bool,
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
    pub clear_on_exit: bool,
    pub save_history: bool,
    pub deny_sensitive_permissions: bool,
    pub webrtc_policy: String,
    /// Prediksi jaringan (DNS prefetch, preconnect). Brave Origin: nyala (speed fix).
    pub network_prediction: bool,
    /// Secure DNS: "off", "cloudflare", "quad9", "adguard", atau URL DoH.
    pub secure_dns: String,
    pub autoplay: bool,
    pub password_manager: bool,
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

    // ---------- unduhan ----------
    pub download_dir: String,

    // ---------- saat mulai ----------
    pub restore_tabs: bool,
    pub autostart: bool,

    // ---------- hemat RAM ----------
    pub sleep_tabs_minutes: u64,
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
    pub no_gpu: bool,
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
            language: "auto".into(),
            theme: "system".into(),
            bookmark_bar: false,
            home_button: false,
            home_url: String::new(),
            newtab: "blank".into(),
            default_zoom: 100,
            force_dark: false,
            user_css: String::new(),
            search_engine: "google".into(),
            bangs: true,
            custom_bangs: Vec::new(),
            address_suggestions: true,
            no_search: false,
            block_ads: true,
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
            password_manager: false,
            autofill: false,
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

/// Alamat DoH untuk pilihan Secure DNS.
pub fn doh_template(choice: &str) -> Option<String> {
    match choice {
        "cloudflare" => Some("https://cloudflare-dns.com/dns-query".into()),
        "quad9" => Some("https://dns.quad9.net/dns-query".into()),
        "adguard" => Some("https://dns.adguard-dns.com/dns-query".into()),
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
        pick(&mut self.update_channel, &["stable", "nightly"], "stable");
        if !["off", "cloudflare", "quad9", "adguard"].contains(&self.secure_dns.as_str()) && !self.secure_dns.starts_with("https://") {
            self.secure_dns = "off".into();
        }
        self.default_zoom = self.default_zoom.clamp(25, 500);
        self.sleep_tabs_minutes = self.sleep_tabs_minutes.min(24 * 60);
        self.trim_every_minutes = self.trim_every_minutes.min(24 * 60);
        self.renderer_process_limit = self.renderer_process_limit.min(64);
        for list in [&mut self.exceptions, &mut self.script_blocked, &mut self.farbling_off] {
            *list = list.iter().map(|h| norm_host(h)).filter(|h| !h.is_empty()).collect();
            list.sort();
            list.dedup();
        }
        let clean = |v: &mut Vec<String>| {
            *v = v.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        };
        clean(&mut self.custom_lists);
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
            on.push(format!("DnsOverHttps:Fallback/true/Templates/{}", escape_param(&t)));
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
            no_gpu: true,
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
    fn search_urls() {
        let s = Settings::default();
        assert_eq!(s.search_url("harga cabai"), "https://www.google.com/search?q=harga+cabai");
        let s = Settings { search_engine: "https://x.id/?q=%s".into(), ..Settings::default() };
        assert_eq!(s.search_url("a&b"), "https://x.id/?q=a%26b");
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
