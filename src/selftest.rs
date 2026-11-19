//! "Uji semua fitur" di halaman Diagnostik.
//!
//! - Katalog setelan dibaca dari halaman Pengaturan sendiri (`ui/settings.html`), jadi setiap
//!   setelan baru otomatis ikut diuji; uji unit memastikan tidak ada field `Settings` yang
//!   terlewat (yang tidak tampil di Pengaturan harus terdaftar di `HIDDEN`).
//! - Setiap setelan diperiksa: nilainya sah (normalize tidak mengubahnya), lalu efeknya dilacak
//!   otomatis dengan membandingkan keluaran Ovner saat nilainya diubah: argumen mesin WebView2,
//!   skrip halaman, skrip per situs, penulisan ulang alamat/pencarian, identitas browser.
//!   Setelan yang dipakai langsung oleh Ovner (tidur tab, rapikan RAM, PDF, unduhan, …) diberi
//!   keterangan di mana efeknya terlihat (kategori log); yang punya uji khusus diberi hasilnya.
//! - Uji fitur tanpa internet (berkas, aturan PDF, subtitle, ffmpeg, log, pengukuran) dan dengan
//!   internet (DNS aman, unduhan, update + tanda tangan, WebView2) berjalan di thread latar.

use crate::settings::Settings;
use serde_json::{json, Value};
use std::path::Path;

pub const SETTINGS_HTML: &str = include_str!("ui/settings.html");

/// Nilai tersimpan yang tidak tampil sebagai baris di Pengaturan, dan alasannya.
pub const HIDDEN: &[(&str, &str)] = &[
    ("schema", "internal: versi format setelan"),
    ("setup_done", "internal: layar sambutan sudah selesai"),
    ("yt_codec_migrated", "internal: penanda migrasi codec"),
    ("preset", "tombol Seimbang/Ketat/Maksimal di atas Pengaturan"),
    ("filter_lists", "katalog daftar filter opsional di Shields"),
    ("password_autosignin", "halaman Kata sandi"),
    ("default_browser_registered", "tombol Browser default"),
    ("codec_hw", "hasil uji dekoder GPU (diisi otomatis)"),
    ("dl_box_v2", "dipaksa menyala sejak 0.8.18"),
    ("force_dark", "lama: hanya dibaca untuk migrasi"),
    ("no_gpu", "lama: hanya dibaca untuk migrasi"),
    ("youtube_h264", "lama: hanya dibaca untuk migrasi"),
    ("tts_voice", "dipilih di bilah Baca keras (Opsi suara)"),
    ("tts_rate", "dipilih di bilah Baca keras (Kecepatan)"),
];

/// Satu baris di Pengaturan: (bagian, kunci, jenis, label).
#[derive(Debug, Clone)]
pub struct Row {
    pub section: String,
    pub key: String,
    pub kind: String,
    pub label: String,
}

fn quoted(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start();
    let s = s.strip_prefix('"')?;
    let end = s.find('"')?;
    Some((&s[..end], &s[end + 1..]))
}

/// Baca katalog setelan dari halaman Pengaturan (bagian dan baris berformat tetap).
pub fn catalog() -> Vec<Row> {
    let mut out = Vec::new();
    let mut section = String::new();
    for line in SETTINGS_HTML.lines() {
        if let Some(rest) = line.strip_prefix("    [\"") {
            // Bagian: ["id", ["Nama", "Name"], [
            if let Some((_, tail)) = rest.split_once('"') {
                if let Some(t) = tail.trim_start().strip_prefix(", [") {
                    if let Some((name, _)) = quoted(t) {
                        if tail.trim_end().ends_with('[') {
                            section = name.to_string();
                        }
                    }
                }
            }
        } else if let Some(rest) = line.strip_prefix("      [\"") {
            let Some((key, tail)) = rest.split_once('"') else { continue };
            let Some((kind, tail)) = tail.trim_start().strip_prefix(',').and_then(quoted) else { continue };
            let label = tail.find("yes(\"").map(|i| &tail[i + 5..]).and_then(|t| t.find('"').map(|e| t[..e].to_string())).unwrap_or_default();
            out.push(Row { section: section.clone(), key: key.to_string(), kind: kind.to_string(), label });
        }
    }
    out
}

/// Pilihan teks yang dicoba untuk melacak efek setelan berupa pilihan (nilai-nilai yang dipakai
/// di Pengaturan Ovner).
const STRING_TRIES: &[&str] = &[
    "off", "on", "auto", "none", "standard", "strict", "aggressive", "safer", "safest", "pause", "keep", "timer", "chromium", "filter", "dynamic", "dark", "light", "system",
    "duckduckgo", "google", "bing", "brave", "startpage", "ecosia", "disable_non_proxied_udp", "default_public_interface_only", "default", "blank", "home", "download", "show",
    "measure", "manual", "hardware", "software", "no-referrer", "strict-origin", "en", "id", "https://contoh.id/?q=%s",
];

/// Nilai pengganti untuk melacak efek sebuah setelan (bool dibalik, angka diubah, teks dicoba
/// beberapa pilihan, daftar ditambah satu situs).
fn alternatives(v: &Value) -> Vec<Value> {
    match v {
        Value::Bool(b) => vec![Value::Bool(!b)],
        Value::Number(n) => {
            let x = n.as_u64().unwrap_or(0);
            vec![json!(x + 7), json!(x.saturating_sub(1).max(1)), json!(0)]
        }
        Value::String(s) => STRING_TRIES.iter().filter(|t| **t != s.as_str()).map(|t| json!(t)).chain([json!("")]).collect(),
        Value::Array(a) => {
            let mut a = a.clone();
            a.push(json!("contoh.id"));
            vec![Value::Array(a), json!([])]
        }
        _ => Vec::new(),
    }
}

fn with(s: &Settings, key: &str, v: Value) -> Option<Settings> {
    let mut m = serde_json::to_value(s).ok()?;
    *m.as_object_mut()?.get_mut(key)? = v;
    serde_json::from_value(m).ok()
}

/// Jejak keluaran Ovner yang bergantung pada setelan (untuk dibandingkan).
struct Effects {
    args: String,
    page: String,
    site: String,
    nav: String,
    ident: String,
}

fn effects(s: &Settings) -> Effects {
    let nav = [
        "https://www.google.com/amp/s/contoh.id/berita?utm_source=x&fbclid=y",
        "http://contoh.id/",
        "https://contoh.id/?gclid=1",
    ]
    .iter()
    .map(|u| crate::shields::rewrite_navigation(u, s).unwrap_or_default())
    .chain(["cuaca jakarta", "!w ovner", "contoh.id"].iter().map(|q| crate::to_url(q, s).unwrap_or_default()))
    .collect::<Vec<_>>()
    .join("\n");
    Effects {
        args: s.browser_args(),
        page: crate::shields::page_scripts(s, 1, false),
        site: format!(
            "{}{}{}{}",
            crate::shields::site_feature_script(s, "contoh.id"),
            crate::shields::dark_live_script(s, None),
            crate::shields::dark_live_script(s, Some("dynamic")),
            crate::shields::dark_live_script(s, Some("filter"))
        ),
        nav,
        ident: format!("{:?}{}", crate::shields::identity_overrides(s, "Mozilla/5.0"), crate::shields::needs_real_identity(s, "contoh.id")),
    }
}

/// Di mana efek setelan yang dipakai langsung oleh Ovner terlihat.
fn used_by(key: &str) -> &'static str {
    match key {
        k if k.starts_with("sleep_") || k == "keep_live_tabs" || k == "background_media" || k == "background_media_minutes" => "tidur tab — log [tidur], keadaan tab di Diagnostik",
        k if k.starts_with("trim_") || k.starts_with("bloat") || k == "low_mem_sleep" || k == "low_memory_when_hidden" || k == "suspend_hidden_tabs" || k == "efficiency_mode" || k == "simulate_memory_pressure" => "hemat RAM — log [ram], pengukuran per menit",
        k if k.starts_with("pdf_") => "PDF — log [pdf], uji aturan PDF",
        k if k.starts_with("dl_") || k == "download_dir" || k == "parallel_downloading" => "unduhan — log [unduh]",
        k if k.starts_with("video_") => "video — log [video]/[subtitle]",
        k if k.starts_with("yt_") => "codec YouTube — log [codec] (codec yang benar-benar diputar)",
        "list_remove_on_open" | "restore_tabs" => "daftar/sesi tab — log [sesi]",
        "check_updates" | "update_channel" => "update — log [update]",
        "diagnostics" | "metrics" => "diagnostik — log [diag], folder metrics",
        "autostart" => "registri Windows (Run)",
        "close_to_tray" | "usage_overlay_app" | "usage_overlay_tab" | "vertical_tabs" | "bookmark_bar" | "home_button" | "theme" | "language" | "toolbar_css" => "tampilan Ovner (berlaku langsung)",
        "save_history" | "clear_on_exit" => "riwayat/data saat keluar",
        "password_manager" | "autofill" => "kata sandi & isi otomatis",
        "extensions_enabled" | "extension_dirs" | "devtools" => "dipakai saat tab dibuat",
        "auto_update_filters" | "filter_update_hours" | "custom_lists" | "custom_rules" | "block_ads" | "blocking_mode" | "cosmetic_filtering" | "block_cookie_notices" | "block_annoyances" | "exceptions" | "cname_uncloak" | "block_telemetry" => "pemblokir — log [shields], uji blokir",
        "antiblock" => "anti-blokir — log [shields]",
        "secure_dns" | "dns_strict" => "DNS aman — uji DoH",
        "default_zoom" | "newtab" | "home_url" => "tab baru / zoom",
        "strip_tracking_params" | "https_only" | "https_strict" => "saat navigasi (penyaring permintaan)",
        "gpc" | "block_local_network" => "penyaring permintaan (header/jaringan lokal)",
        "letterboxing" | "uniform_utc" => "sidik jari (saat tab dibuat)",
        "deny_sensitive_permissions" | "popups_to_tabs" | "autoplay" | "always_private" | "disable_beforeunload" => "saat tab dibuat",
        "local_crash_reports" | "edge_tracking_prevention" => "opsi mesin WebView2 saat Ovner mulai",
        "address_suggestions" | "search_engine" | "private_search_engine" | "custom_bangs" | "bangs" | "no_search" => "kolom alamat",
        k if k.starts_with("dark_") => "mode gelap (berlaku langsung)",
        "security_level" | "script_allowed" | "script_blocked" | "disable_jit" | "disable_webgl" => "keamanan skrip per situs",
        "allow_copy_all" | "allow_copy_sites" => "Izinkan salin (skrip per situs)",
        _ => "dipakai langsung oleh Ovner",
    }
}

/// Hasil khusus yang dihitung App (keadaan saat ini) untuk setelan tertentu: (status, pesan).
pub type Special = std::collections::HashMap<String, (String, String)>;

/// Baris tabel setelan: nilai, bawaan, sah atau tidak, efek yang terlacak, status.
pub fn setting_rows(s: &Settings, running_args: &str, special: &Special) -> Vec<Value> {
    let def = serde_json::to_value(Settings::default()).unwrap_or_default();
    let cur = serde_json::to_value(s).unwrap_or_default();
    let base = effects(s);
    let mut fixed = s.clone();
    fixed.normalize();
    let fixed = serde_json::to_value(&fixed).unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    let rows = catalog();
    for r in rows.iter().filter(|r| !matches!(r.kind.as_str(), "action" | "defaultbrowser")) {
        if !seen.insert(r.key.clone()) {
            continue;
        }
        let v = cur.get(&r.key).cloned().unwrap_or(Value::Null);
        let mut how: Vec<String> = Vec::new();
        let mut status = "ok";
        let mut msg: Vec<String> = Vec::new();
        if v.is_null() {
            status = "gagal";
            msg.push("tidak ada di file setelan".into());
        } else if fixed.get(&r.key) != Some(&v) {
            status = "gagal";
            msg.push(format!("nilai tidak sah, seharusnya {}", crate::settings::short_value(fixed.get(&r.key).unwrap_or(&Value::Null))));
        }
        let mut changed = [false; 5];
        for e in alternatives(&v).into_iter().filter_map(|a| with(s, &r.key, a)).map(|t| effects(&t)) {
            changed[0] |= e.args != base.args;
            changed[1] |= e.page != base.page;
            changed[2] |= e.site != base.site;
            changed[3] |= e.nav != base.nav;
            changed[4] |= e.ident != base.ident;
            if changed.iter().all(|c| *c) {
                break;
            }
        }
        {
            if changed[0] {
                how.push("argumen mesin WebView2".into());
                if !running_args.starts_with(base.args.trim()) && status == "ok" {
                    status = "peringatan";
                    msg.push("diubah setelah Ovner mulai: berlaku setelah Ovner dimulai ulang".into());
                }
            }
            if changed[1] {
                how.push("skrip halaman (halaman yang dimuat setelah diubah)".into());
            }
            if changed[2] {
                how.push("skrip per situs".into());
            }
            if changed[3] {
                how.push("penulisan alamat/pencarian".into());
            }
            if changed[4] {
                how.push("identitas browser".into());
            }
        }
        if how.is_empty() {
            how.push(used_by(&r.key).to_string());
        }
        if let Some((st, m)) = special.get(&r.key) {
            msg.push(m.clone());
            if status == "ok" || st == "gagal" {
                status = match st.as_str() {
                    "gagal" => "gagal",
                    "peringatan" => "peringatan",
                    _ => status,
                };
            }
        } else if status == "ok" && how.len() == 1 && how[0] == used_by(&r.key) {
            // Tidak ada keluaran yang bisa dibandingkan: nilai sah, efeknya dibuktikan lewat log.
            status = "info";
        }
        out.push(json!({
            "section": r.section,
            "key": r.key,
            "label": r.label,
            "value": if r.key == "tts_azure_key" { if v.as_str().is_some_and(|x| !x.is_empty()) { "\"(ada, terenkripsi)\"".to_string() } else { "\"\"".to_string() } } else { crate::settings::short_value(&v) },
            "default": def.get(&r.key) == Some(&v),
            "how": how.join(" · "),
            "status": status,
            "msg": msg.join("; "),
        }));
    }
    for (k, why) in HIDDEN {
        let v = cur.get(*k).cloned().unwrap_or(Value::Null);
        out.push(json!({"section": "Tidak tampil di Pengaturan", "key": k, "label": why, "value": crate::settings::short_value(&v), "default": def.get(*k) == Some(&v), "how": why, "status": special.get(*k).map(|x| x.0.as_str()).unwrap_or("info"), "msg": special.get(*k).map(|x| x.1.clone()).unwrap_or_default()}));
    }
    out
}

/// Bukti setelan dari log (sesi ini) dan penghitung harian: (kunci, keterangan).
pub fn proofs(lines: &[String], counts: &std::collections::BTreeMap<String, u64>) -> Vec<(String, String)> {
    // Baris sejak Ovner terakhir dibuka.
    let start = lines.iter().rposition(|l| l.contains("==== Ovner ") && l.contains(" mulai")).unwrap_or(0);
    let session = &lines[start..];
    let find = |pats: &[&str]| -> Option<(usize, String)> {
        let hits: Vec<&String> = session.iter().filter(|l| pats.iter().all(|p| l.contains(p))).collect();
        let last = hits.last()?;
        let time = last.get(11..19).unwrap_or("").to_string();
        Some((hits.len(), time))
    };
    let mut out = Vec::new();
    let mut by_log = |keys: &[&str], pats: &[&str], what: &str| {
        if let Some((n, t)) = find(pats) {
            for k in keys {
                out.push((k.to_string(), format!("terbukti di log: {what} {n}× sesi ini, terakhir {t}")));
            }
        }
    };
    by_log(&["sleep_tabs_minutes"], &["[tidur] tab ditidurkan", "tidak dibuka >"], "tab ditidurkan karena tidak dibuka");
    by_log(&["keep_live_tabs"], &["[tidur] tab ditidurkan", "tab terakhir"], "tab di luar N tab terakhir ditidurkan");
    by_log(&["sleep_minimized", "sleep_minimized_secs"], &["[tidur] tab ditidurkan", "diminimalkan/di tray"], "tab ditidurkan saat diminimalkan");
    by_log(&["trim_every_minutes"], &["RAM dirapikan"], "RAM dirapikan");
    by_log(&["low_memory_when_hidden", "trim_when_hidden"], &["mode hemat:"], "mode hemat saat diminimalkan/di tray");
    by_log(&["efficiency_mode"], &["efisiensi per proses"], "mode efisiensi Windows");
    by_log(&["suspend_hidden_tabs"], &["ditangguhkan"], "tab di belakang ditangguhkan");
    by_log(&["bloat_guard", "bloat_mb"], &["membengkak"], "penjaga tab membengkak");
    by_log(&["background_media", "background_media_minutes"], &["dijeda"], "video/musik di latar dijeda");
    by_log(&["pdf_download", "pdf_download_mb", "pdf_unknown", "pdf_show_sites", "pdf_download_sites"], &["[pdf]"], "aturan PDF dipakai");
    by_log(&["dl_engine", "dl_connections", "dl_min_mb"], &["mesin unduh Ovner"], "mesin unduh multi-koneksi");
    by_log(&["video_subs_page"], &["subtitle lewat halaman YouTube berhasil"], "subtitle lewat halaman YouTube");
    by_log(&["video_button"], &["telusuri video"], "unduh video");
    by_log(&["video_sniff"], &["media terdeteksi"], "video tertangkap di halaman");
    by_log(&["yt_codec", "yt_block_av1", "yt_block_vp9", "yt_block_vp8", "yt_block_h264", "yt_block_60fps", "yt_block_opus", "yt_block_aac"], &["[codec] YouTube"], "codec YouTube tercatat");
    by_log(&["popups_to_tabs"], &["pop-up dibuka sebagai tab"], "pop-up jadi tab");
    by_log(&["translate_engine", "translate_to"], &["[terjemah] halaman diterjemahkan"], "halaman diterjemahkan");
    by_log(&["tts_engine"], &["[baca] baca keras dimulai"], "baca keras");
    by_log(&["auto_update_filters", "filter_update_hours"], &["[shields] daftar filter"], "daftar filter diperbarui");
    by_log(&["diagnostics"], &["catatan RAM"], "catatan RAM");
    by_log(&["check_updates", "update_channel"], &["[update]"], "pemeriksaan update");
    by_log(&["restore_tabs"], &["[sesi]"], "sesi tab");
    if let Some(n) = counts.get("diblokir").filter(|n| **n > 0) {
        for k in ["block_ads", "blocking_mode", "cosmetic_filtering", "block_cookie_notices", "block_annoyances", "block_telemetry", "custom_rules", "custom_lists"] {
            out.push((k.to_string(), format!("terbukti: {n} permintaan diblokir hari ini")));
        }
    }
    // Satu kunci cukup satu bukti (yang pertama).
    let mut seen = std::collections::HashSet::new();
    out.retain(|(k, _)| seen.insert(k.clone()));
    out
}

/// Satu hasil uji fitur.
pub fn item(group: &str, name: &str, status: &str, msg: impl Into<String>) -> Value {
    json!({"group": group, "name": name, "status": status, "msg": msg.into()})
}

/// Uji fitur tanpa internet (dijalankan di thread latar). `s` = salinan setelan saat ini.
pub fn offline_tests(data: &Path, s: &Settings) -> Vec<Value> {
    let mut out = Vec::new();
    // Folder data bisa ditulis dan dibaca.
    let probe = data.join(".uji-tulis");
    let w = std::fs::write(&probe, b"ovner").and_then(|_| std::fs::read(&probe)).map(|b| b == b"ovner");
    let _ = std::fs::remove_file(&probe);
    out.push(item("Data", "Folder data bisa ditulis", if matches!(w, Ok(true)) { "ok" } else { "gagal" }, format!("{} ({w:?})", data.display())));
    // Setelan di disk sama dengan yang dipakai.
    let disk = std::fs::read_to_string(Settings::path(data)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
    let same = disk.as_ref() == serde_json::to_value(s).ok().as_ref();
    out.push(item("Data", "settings.json sama dengan setelan yang dipakai", if same { "ok" } else { "peringatan" }, if same { "cocok".to_string() } else { "berbeda (mungkin baru diubah dan belum disimpan)".into() }));
    for f in ["downloads.json", "sessions.json", "tab-stats.json"] {
        let p = data.join(f);
        let st = match std::fs::read_to_string(&p) {
            Ok(t) if serde_json::from_str::<Value>(&t).is_ok() => ("ok", format!("{} KB", t.len() / 1024)),
            Ok(_) => ("gagal", "isi rusak (bukan JSON)".into()),
            Err(_) => ("info", "belum ada (belum pernah dipakai)".into()),
        };
        out.push(item("Data", f, st.0, st.1));
    }
    // Log dan pengukuran.
    let logs = crate::logging::files(data);
    let size: u64 = logs.iter().filter_map(|p| std::fs::metadata(p).ok()).map(|m| m.len()).sum();
    out.push(item("Log", "Berkas log", if logs.is_empty() { "gagal" } else { "ok" }, format!("{} berkas, {} KB (batas {} × {} MB)", logs.len(), size / 1024, crate::logging::KEEP, crate::logging::MAX_BYTES / 1_048_576)));
    let m = crate::metrics::files(data);
    let age = m.first().and_then(|p| std::fs::metadata(p).ok()).and_then(|x| x.modified().ok()).and_then(|t| t.elapsed().ok()).map(|d| d.as_secs());
    out.push(item(
        "Log",
        "Pengukuran tiap menit",
        match (s.metrics, age) {
            (false, _) => "info",
            (true, Some(a)) if a < 150 => "ok",
            (true, None) => "peringatan",
            _ => "peringatan",
        },
        match (s.metrics, age) {
            (false, _) => "dimatikan di Pengaturan".to_string(),
            (true, Some(a)) => format!("{} hari tersimpan, terakhir ditulis {a} dtk lalu", m.len()),
            (true, None) => "belum ada (ditulis 1 menit setelah Ovner mulai)".into(),
        },
    ));
    // Aturan PDF dengan setelan saat ini.
    use crate::settings::PdfAction;
    let mb = 1_048_576u64;
    let show = |a: PdfAction| match a {
        PdfAction::Show => "tampil".to_string(),
        PdfAction::Download => "unduh".into(),
        PdfAction::Measure(n) => format!("ukur dulu ({} MB)", n / mb),
    };
    let infor = s.pdf_action("eln.ne1.inforcloudsuite.com", None);
    let sipp = s.pdf_action("sipp.bpjsketenagakerjaan.go.id", Some(17 * mb));
    let small = s.pdf_action("contoh.go.id", Some(mb));
    let nolen = s.pdf_action("contoh.go.id", None);
    let infor_ok = !s.pdf_download || infor == PdfAction::Show || s.pdf_unknown == "download";
    let sipp_ok = !s.pdf_download || sipp == PdfAction::Download || s.pdf_download_mb > 17;
    out.push(item(
        "PDF",
        "Aturan PDF (setelan saat ini)",
        if infor_ok && sipp_ok && small != PdfAction::Download { "ok" } else { "peringatan" },
        format!("Infor tanpa ukuran → {}; SIPP 17 MB → {}; PDF 1 MB → {}; situs lain tanpa ukuran → {}; file:// → tampil", show(infor), show(sipp), show(small), show(nolen)),
    ));
    // Subtitle: json3 → SRT.
    let j = r#"{"events":[{"tStartMs":0,"dDurationMs":900,"segs":[{"utf8":"uji"}]}]}"#;
    let srt = crate::video::json3_to_srt(j);
    out.push(item("Video", "Subtitle YouTube (json3 → SRT)", if srt.as_ref().is_ok_and(|t| t.contains("00:00:00,000 --> 00:00:00,900")) { "ok" } else { "gagal" }, format!("{:?}", srt.map(|t| t.lines().count()))));
    // Alat video + uji ffmpeg sungguhan (video 1 detik → subtitle masuk → diperiksa → diambil lagi).
    for v in crate::video::tool_versions(data) {
        let st = if v.contains("GAGAL") { "gagal" } else if v.contains("belum") { "info" } else { "ok" };
        out.push(item("Video", v.split(':').next().unwrap_or("alat"), st, v.split_once(": ").map(|x| x.1).unwrap_or("").to_string() + if st == "info" { " — dipasang otomatis saat pertama kali mengunduh video" } else { "" }));
    }
    out.push(ffmpeg_roundtrip(data));
    out.extend(ram_items(data));
    out
}

/// Komposisi RAM proses Ovner sendiri, ukuran mesin pemblokir iklan, dan tren kebocoran.
pub fn ram_items(data: &Path) -> Vec<Value> {
    let mut out = Vec::new();
    let mb = |b: u64| b as f64 / 1_048_576.0;
    let heap = crate::heap::now();
    let engine = crate::blocker::measure_engine(data);
    match crate::win::self_memory() {
        Some((commit, threads)) => {
            let eng = engine.map(|e| e.0).unwrap_or(0);
            out.push(item(
                "RAM",
                "Isi proses ovner.exe",
                "info",
                format!(
                    "komit {:.0} MB = heap Rust Ovner {:.1} MB (di dalamnya mesin pemblokir ±{:.1} MB) + DLL WebView2/Windows, tumpukan {threads} thread, dll. ±{:.0} MB · puncak heap Rust {:.1} MB",
                    mb(commit),
                    mb(heap),
                    mb(eng),
                    mb(commit.saturating_sub(heap)),
                    mb(crate::heap::peak())
                ),
            ));
        }
        None => out.push(item("RAM", "Isi proses ovner.exe", "info", format!("heap Rust Ovner {:.1} MB", mb(heap)))),
    }
    out.push(match engine {
        Some((used, ms, cache)) => item("RAM", "Mesin pemblokir iklan", "ok", format!("±{:.1} MB di memori (cache {:.1} MB di disk), dimuat {ms} ms", mb(used), mb(cache))),
        None => item("RAM", "Mesin pemblokir iklan", "info", "belum ada cache daftar filter (dibuat setelah daftar terunduh)"),
    });
    let (msg, warn) = crate::metrics::analyze(data);
    out.push(item("RAM", "Tren RAM (deteksi kebocoran)", if warn { "peringatan" } else if msg.starts_with("tidak ada") { "ok" } else { "info" }, msg));
    out
}

/// Uji ffmpeg/ffprobe Ovner: buat video 1 detik, masukkan subtitle, periksa, ambil lagi.
fn ffmpeg_roundtrip(data: &Path) -> Value {
    let name = "ffmpeg: subtitle masuk → diperiksa → diambil lagi";
    let ff = crate::video::ffmpeg(data);
    if !ff.exists() {
        return item("Video", name, "lewat", "ffmpeg belum terpasang (dipasang otomatis saat pertama kali mengunduh video)");
    }
    let dir = std::env::temp_dir().join(format!("ovner-uji-ffmpeg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    let v = dir.join("uji.mp4");
    let mut cmd = std::process::Command::new(&ff);
    cmd.args(["-v", "error", "-y", "-f", "lavfi", "-i", "color=c=black:s=160x90:d=1", "-c:v", "mpeg4"]).arg(&v);
    crate::video::hide_window(&mut cmd);
    let made = cmd.status().map(|s| s.success()).unwrap_or(false);
    let r = (|| -> Result<String, String> {
        if !made {
            return Err("ffmpeg tidak bisa membuat video uji".into());
        }
        let srt = dir.join("in.srt");
        std::fs::write(&srt, "1\n00:00:00,000 --> 00:00:00,900\nuji\n\n").map_err(|e| e.to_string())?;
        crate::video::embed_subs(data, &v, &srt, "id", None)?;
        let n = crate::video::verify_embedded(data, &v)?;
        let (got, _) = crate::video::extract_subs(data, &v)?;
        let text = got.first().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        if text.contains("uji") {
            Ok(format!("{n} trek, .srt terambil kembali"))
        } else {
            Err("isi .srt hasil ambil tidak cocok".into())
        }
    })();
    let _ = std::fs::remove_dir_all(&dir);
    match r {
        Ok(m) => item("Video", name, "ok", m),
        Err(e) => item("Video", name, "gagal", e),
    }
}

/// Uji yang butuh internet (thread latar).
pub fn network_tests(data: &Path, lang_id: bool) -> Vec<Value> {
    let mut out = Vec::new();
    let (ok, msg) = crate::antiblock::self_test();
    out.push(item("Jaringan", "DNS aman (DoH 1.1.1.1 / 8.8.8.8)", if ok { "ok" } else { "gagal" }, msg));
    let tmp = std::env::temp_dir().join("ovner-uji-unduh.txt");
    let r = crate::win::download("https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/quick-fixes.txt", &tmp);
    let size = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
    let _ = std::fs::remove_file(&tmp);
    out.push(item("Jaringan", "Unduh daftar filter", if r.is_ok() && size > 1000 { "ok" } else { "gagal" }, format!("{r:?}, {size} byte")));
    // Penerjemah dan suara online (jalur tidak resmi, bisa berubah): diuji dengan satu kalimat.
    for (key, engine) in [("Microsoft", "microsoft"), ("Google", "google")] {
        let r = crate::translate::translate_only(engine, &["Good morning".to_string()], "en", "id");
        let (st, m) = match r {
            Ok(o) if !o.texts.is_empty() && !o.texts[0].is_empty() => ("ok", format!("\"Good morning\" → \"{}\"", o.texts[0])),
            Ok(_) => ("peringatan", "jawaban kosong".to_string()),
            Err(e) => ("peringatan", e),
        };
        out.push(item("Jaringan", &format!("Penerjemah {key}"), st, m));
    }
    let r = crate::tts::edge_speak("Halo", "id-ID-GadisNeural", 100);
    out.push(match r {
        Ok(sp) => item("Jaringan", "Suara Natural online (seperti Edge)", "ok", format!("{} KB audio, {} batas kata", sp.mp3.len() / 1024, sp.marks.len())),
        Err(e) => item("Jaringan", "Suara Natural online (seperti Edge)", "peringatan", format!("{e} (suara Windows tetap bisa dipakai)")),
    });
    // Update: manifest terbaru + tanda tangannya diperiksa dengan kunci publik Ovner.
    let tmp2 = std::env::temp_dir().join("ovner-uji-update.json");
    let r2 = crate::win::download(&crate::update::latest_url("stable"), &tmp2);
    let body = std::fs::read_to_string(&tmp2).unwrap_or_default();
    let _ = std::fs::remove_file(&tmp2);
    let man: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    let sig = crate::update::verify_signature(
        man["version"].as_str().unwrap_or(""),
        man["build"].as_u64().unwrap_or(0),
        man["sha256"].as_str().unwrap_or(""),
        man["sig"].as_str().unwrap_or(""),
    );
    out.push(item(
        "Jaringan",
        "Cek update + tanda tangan",
        if r2.is_ok() && matches!(sig, Ok(true)) { "ok" } else { "gagal" },
        format!("versi terbaru {} · tanda tangan {:?}", man["version"].as_str().unwrap_or("?"), sig),
    ));
    let installed = wry::webview_version().unwrap_or_default();
    let (ok, msg) = match crate::versions::refresh_latest(data, None, crate::win::download) {
        Ok(latest) => crate::versions::status(&installed, &latest, lang_id),
        Err(e) => (false, format!("{e} ({})", crate::versions::EDGE_API)),
    };
    out.push(item("Jaringan", "WebView2 terbaru (Microsoft)", if ok { "ok" } else { "peringatan" }, msg));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_covers_every_setting() {
        let rows = catalog();
        let sections: std::collections::HashSet<&str> = rows.iter().map(|r| r.section.as_str()).collect();
        assert_eq!(sections.len(), 11, "{sections:?}");
        let keys: std::collections::HashSet<&str> = rows.iter().map(|r| r.key.as_str()).collect();
        let fields = serde_json::to_value(Settings::default()).unwrap();
        let mut missing = Vec::new();
        for k in fields.as_object().unwrap().keys() {
            if !keys.contains(k.as_str()) && !HIDDEN.iter().any(|(h, _)| h == k) {
                missing.push(k.clone());
            }
        }
        assert!(missing.is_empty(), "setelan tanpa baris di Pengaturan dan tidak ada di HIDDEN: {missing:?}");
        assert!(rows.iter().all(|r| !r.label.is_empty()), "label kosong");
    }

    #[test]
    fn rows_have_status_and_effects() {
        let s = Settings::default();
        let rows = setting_rows(&s, &s.browser_args(), &Special::new());
        let n = rows.iter().filter(|r| r["section"] != "Tidak tampil di Pengaturan").count();
        assert!(n >= 138, "{n}");
        let find = |k: &str| rows.iter().find(|r| r["key"] == k).unwrap().clone();
        assert!(find("spare_renderer")["how"].as_str().unwrap().contains("argumen"), "{}", find("spare_renderer"));
        assert!(find("de_amp")["how"].as_str().unwrap().contains("skrip halaman"), "{}", find("de_amp"));
        assert_eq!(find("sleep_tabs_minutes")["status"], "info");
        assert!(rows.iter().all(|r| r["status"] != "gagal"), "{:?}", rows.iter().filter(|r| r["status"] == "gagal").collect::<Vec<_>>());
        // Diubah setelah mulai → perlu mulai ulang.
        let mut t = s.clone();
        t.spare_renderer = !t.spare_renderer;
        let rows = setting_rows(&t, &s.browser_args(), &Special::new());
        assert_eq!(rows.iter().find(|r| r["key"] == "spare_renderer").unwrap()["status"], "peringatan");
    }
}
#[cfg(test)]
mod dump {
    #[test]
    #[ignore]
    fn dump_rows() {
        let s = crate::settings::Settings::default();
        for r in super::setting_rows(&s, &s.browser_args(), &super::Special::new()) {
            println!("{:<12} {:<28} {}", r["status"].as_str().unwrap(), r["key"].as_str().unwrap(), r["how"].as_str().unwrap());
        }
    }
}

/// Sembunyikan alamat situs (hanya nama situsnya yang tersisa) dan nama pengguna Windows di
/// jalur berkas, untuk paket diagnostik yang akan dikirim ke orang lain.
pub fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let next = ["https://", "http://", "file:///"].iter().filter_map(|p| rest.find(p).map(|i| (i, *p))).min();
        let Some((i, p)) = next else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..i]);
        let tail = &rest[i + p.len()..];
        let end = tail.find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | ')' | ',' | ';' | '\\')).unwrap_or(tail.len());
        let token = &tail[..end];
        if p == "file:///" {
            out.push_str("file:///…");
        } else {
            let host_end = token.find(['/', '?', '#']).unwrap_or(token.len());
            out.push_str(p);
            out.push_str(&token[..host_end]);
            if host_end < token.len() {
                out.push_str("/…");
            }
        }
        rest = &tail[end..];
    }
    // C:\Users\nama\… → C:\Users\…\… (juga bentuk JSON "C:\\Users\\nama\\…").
    let mut res = String::with_capacity(out.len());
    let mut s = out.as_str();
    while let Some(i) = s.find("Users\\") {
        let ok = s[..i].ends_with('\\');
        let after = &s[i + 5..];
        let slashes = after.len() - after.trim_start_matches('\\').len();
        res.push_str(&s[..i + 5 + slashes]);
        let t = &after[slashes..];
        if !ok {
            s = t;
            continue;
        }
        let end = t.find(['\\', '"', ' ', '\n']).unwrap_or(t.len());
        res.push('…');
        s = &t[end..];
    }
    res.push_str(s);
    res
}

#[cfg(test)]
mod redact_tests {
    #[test]
    fn proofs_from_log() {
        let lines: Vec<String> = [
            "2026-11-19 14:40:01.693 INFO       [sistem] ==== Ovner 0.8.23 mulai (portabel: false)",
            "2026-11-19 14:44:25.027 INFO       [tidur] tab ditidurkan: https://a/ (±2 MB; alasan: tidak dibuka > 2 menit)",
            "2026-11-19 16:17:39.925 INFO       [sistem] ==== Ovner 0.8.24 mulai (portabel: false)",
            "2026-11-19 16:19:51.781 INFO       [tidur] tab ditidurkan: https://b/ (±6 MB; alasan: tidak dibuka > 2 menit)",
            "2026-11-19 16:22:42.608 INFO       [ram] RAM dirapikan (1 proses): 73 MB dilepas",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let mut c = std::collections::BTreeMap::new();
        c.insert("diblokir".to_string(), 43u64);
        let p: std::collections::HashMap<String, String> = super::proofs(&lines, &c).into_iter().collect();
        assert!(p["sleep_tabs_minutes"].contains("1× sesi ini, terakhir 16:19:51"), "{:?}", p["sleep_tabs_minutes"]);
        assert!(p["trim_every_minutes"].contains("16:22:42"));
        assert!(p["block_ads"].contains("43"));
        assert!(!p.contains_key("keep_live_tabs"));
    }

    #[test]
    fn hides_paths_and_users() {
        let t = "PDF tampil: https://eln.inforcloudsuite.com/webui/gwtui/download?id=9 dan http://contoh.id ok; file:///C:/x.pdf C:\\Users\\Rendi\\Downloads\\a.mp4";
        assert_eq!(
            super::redact(t),
            "PDF tampil: https://eln.inforcloudsuite.com/… dan http://contoh.id ok; file:///… C:\\Users\\…\\Downloads\\a.mp4"
        );
        // Bentuk JSON tetap JSON yang sah.
        let j = serde_json::json!({"data": "C:\\Users\\14245\\AppData\\Local\\Ovner"}).to_string();
        let r = super::redact(&j);
        let v: serde_json::Value = serde_json::from_str(&r).expect(&r);
        assert_eq!(v["data"], "C:\\Users\\…\\AppData\\Local\\Ovner");
    }
}

#[cfg(test)]
mod sample {
    /// Contoh laporan untuk mencoba halaman Diagnostik di browser biasa.
    #[test]
    #[ignore]
    fn write_sample_report() {
        let s = crate::settings::Settings::default();
        let data = std::env::temp_dir().join("ovner-sample-data");
        let _ = std::fs::create_dir_all(&data);
        let mut tests = vec![super::item("Tab & tidur", "3 tab hidup dari 9", "info", "youtube.com — dilindungi: memutar suara · 420 MB\naas.awi.co.id/AMS — hidup (slot N tab terakhir / tidur dalam ±2 menit)")];
        tests.extend(super::offline_tests(&data, &s));
        let rows = super::setting_rows(&s, &s.browser_args(), &super::Special::new());
        let r = serde_json::json!({"version": "0.8.22", "channel": "stable", "tests": tests, "settingRows": rows, "diagState": "2026-11-19 14:02:11",
            "counts": {"diblokir": 1234, "tab_ditidurkan": 12, "log_pdf": 3, "galat_subtitle": 1},
            "logCategories": crate::logging::CATEGORIES,
            "log": ["2026-11-19 13:31:05.120 INFO       [pdf] PDF ditampilkan di tab: 40 KB", "2026-11-19 13:32:05.120 GALAT      [subtitle] subtitle gagal: HTTP 429", "2026-11-19 13:33:05.120 PERINGATAN [ram] hasil rapikan: memantul"],
            "crashes": [], "lists": [], "kinds": []});
        std::fs::write("/tmp/pt/diag-sample.json", serde_json::to_string(&r).unwrap()).unwrap();
    }
}
