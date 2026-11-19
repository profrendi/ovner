//! `ovner --self-test <folder>`: uji Ovner sungguhan dengan WebView2 (dipakai di GitHub Actions).
//!
//! Ovner berjalan normal dengan folder data sementara (OVNER_DATA) dan server uji lokal
//! (testserver.rs), lalu menjalankan skenario langkah demi langkah lewat fungsi Ovner yang sama
//! dengan yang dipakai pengguna. Setiap langkah dinilai dari keadaan Ovner dan dari log (baris
//! log dibaca dari memori sejak langkah dimulai). Hasil: `hasil.json`, `hasil.txt`, tangkapan
//! layar PNG per langkah, log, dan pengukuran — lalu Ovner keluar dengan kode 0 (semua lulus)
//! atau 1.

use crate::{display_url, internal_page, log, logging, testserver, video, App, UserEvent};
use std::time::{Duration, Instant};
use serde_json::{json, Value};
use std::path::PathBuf;

pub struct CiRun {
    pub out: PathBuf,
    pub port: u16,
    step: usize,
    sub: u32,
    started: Instant,
    seq0: u64,
    dl0: usize,
    results: Vec<Value>,
    bench: Vec<(String, u64)>,
    note: String,
    pub eval: Option<String>,
    nonce: String,
}

/// (nama, batas waktu detik)
const STEPS: &[(&str, u64)] = &[
    ("mulai", 60),
    ("patokan_ram_awal", 30),
    ("isi_ram_ovner_dan_pemblokir", 20),
    ("pdf_tanpa_ukuran_kecil_tampil", 30),
    ("pdf_dalam_bingkai_seperti_infor", 30),
    ("pdf_tanpa_ukuran_besar_jadi_unduhan", 90),
    ("pdf_dengan_ukuran_besar_diunduh", 90),
    ("pdf_file_lokal_tidak_dicegat", 20),
    ("unduh_mesin_ovner_20mb_utuh", 120),
    ("iklan_diblokir", 30),
    ("subtitle_lewat_halaman_youtube_tiruan", 40),
    ("tidur_n_tab_terakhir", 40),
    ("formulir_dilindungi_dari_tidur", 40),
    ("tidurkan_semua_tab_lain", 20),
    ("pindah_ke_daftar_dan_buka_lagi", 30),
    ("rapikan_ram_cerdas", 20),
    ("browser_default_daftar_dan_lepas", 20),
    // 0.8.25 (masa percobaan: dilaporkan, belum menghentikan rilis)
    ("tombol_video_di_halaman_trusted_types", 30),
    ("tombol_video_di_dalam_iframe", 30),
    ("video_hls_tertangkap", 30),
    ("terjemahan_halaman_dan_kembalikan", 40),
    ("baca_keras_ambil_paragraf", 30),
    ("lihat_sumber_halaman", 30),
    ("tangkapan_layar_disimpan", 30),
    ("gpc_dnt_terkirim", 25),
    ("parameter_pelacak_dibuang", 25),
    ("font_tampilan_ovner", 20),
    ("layanan_online_terjemah_dan_suara", 60),
    ("patokan_ram_10_tab", 90),
    // 0.8.26 (masa percobaan): ekstensi Chrome. Dijalankan setelah patokan RAM supaya proses
    // ekstensi tidak ikut terhitung di patokan.
    ("ekstensi_pasang_dari_crx", 40),
    ("ekstensi_skrip_konten_jalan", 30),
    ("ekstensi_panel_samping_terbuka", 30),
    ("ekstensi_copyclip_dari_web_store", 90),
];

/// Langkah baru dalam masa percobaan: hasilnya dilaporkan, tetapi belum ikut menentukan lulus
/// (kode keluar) sampai lulus di 3 rilis berturut-turut.
const PROBATION: &[&str] = &[
    "tombol_video_di_halaman_trusted_types",
    "tombol_video_di_dalam_iframe",
    "video_hls_tertangkap",
    "terjemahan_halaman_dan_kembalikan",
    "baca_keras_ambil_paragraf",
    "lihat_sumber_halaman",
    "tangkapan_layar_disimpan",
    "gpc_dnt_terkirim",
    "parameter_pelacak_dibuang",
    "font_tampilan_ovner",
    "layanan_online_terjemah_dan_suara",
    "isi_ram_ovner_dan_pemblokir",
    "ekstensi_pasang_dari_crx",
    "ekstensi_skrip_konten_jalan",
    "ekstensi_panel_samping_terbuka",
    "ekstensi_copyclip_dari_web_store",
];

/// CopyClip (panel samping riwayat clipboard) di Chrome Web Store.
pub const COPYCLIP_ID: &str = "pgjckoablokakigdlgbhdmeohbklphli";

enum Out {
    Wait,
    Pass(String),
    Fail(String),
}

impl CiRun {
    pub fn new(out: PathBuf) -> Result<CiRun, String> {
        std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        let port = testserver::start().map_err(|e| format!("server uji: {e}"))?;
        log(&format!("[diag] SELF-TEST mulai: server uji di 127.0.0.1:{port}, hasil ke {}", out.display()));
        Ok(CiRun { out, port, step: 0, sub: 0, started: Instant::now(), seq0: logging::seq(), dl0: 0, results: Vec::new(), bench: Vec::new(), note: String::new(), eval: None, nonce: String::new() })
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    fn logs(&self) -> Vec<String> {
        logging::since(self.seq0)
    }

    fn logged(&self, needle: &str) -> bool {
        self.logs().iter().any(|l| l.contains(needle))
    }

    fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }
}

/// Aturan host-resolver untuk uji iklan: domain iklan diarahkan ke server uji.
pub fn browser_args(port: u16) -> String {
    format!(" --host-resolver-rules=\"MAP pagead2.googlesyndication.com 127.0.0.1:{port}\"")
}

impl App {
    fn ci_tab(&self, path_part: &str) -> Option<usize> {
        self.tabs.iter().position(|t| t.url.contains(path_part))
    }

    fn ci_shot(&self, name: &str) {
        let Some(ci) = &self.ci else { return };
        if let Some(wv) = self.tabs.get(self.active).and_then(|t| t.webview.as_ref()) {
            crate::win::capture_png(wv, &ci.out.join(format!("{:02}-{name}.png", ci.step)));
        }
    }

    fn ci_ram(&mut self) -> u64 {
        self.stats = self.sampler.sample(false);
        self.stats_at = Instant::now();
        crate::usage::total(&self.stats).working / 1_048_576
    }

    /// Dipanggil tiap 300 ms selama --self-test.
    pub fn ci_tick(&mut self) {
        let Some(ci) = self.ci.as_ref() else { return };
        if ci.step >= STEPS.len() {
            return;
        }
        let (name, limit) = STEPS[ci.step];
        let out = if ci.elapsed() > Duration::from_secs(limit) { Out::Fail(format!("batas waktu {limit} dtk habis{}", if ci.note.is_empty() { String::new() } else { format!(" ({})", ci.note) })) } else { self.ci_step(name) };
        let (status, msg) = match out {
            Out::Wait => return,
            Out::Pass(m) => ("lulus", m),
            Out::Fail(m) => ("GAGAL", m),
        };
        self.ci_shot(name);
        let ci = self.ci.as_mut().unwrap();
        let ms = ci.elapsed().as_millis();
        log(&format!("[diag] SELF-TEST {name}: {status} ({ms} ms) {msg}"));
        let tail: Vec<String> = if status == "GAGAL" { ci.logs().into_iter().rev().take(15).collect::<Vec<_>>().into_iter().rev().collect() } else { Vec::new() };
        ci.results.push(json!({"langkah": name, "status": status, "ms": ms, "pesan": msg, "log": tail}));
        ci.step += 1;
        ci.sub = 0;
        ci.started = Instant::now();
        ci.seq0 = logging::seq();
        ci.dl0 = self.downloads.len();
        ci.note.clear();
        ci.eval = None;
        if ci.step >= STEPS.len() {
            self.ci_finish();
        }
    }

    fn ci_open(&mut self, path: &str) {
        let u = self.ci.as_ref().unwrap().url(path);
        self.open_tab(&u, true, false);
    }

    fn ci_new_download(&self) -> Option<&crate::Download> {
        let dl0 = self.ci.as_ref()?.dl0;
        self.downloads.get(dl0..)?.iter().last()
    }

    fn ci_step(&mut self, name: &str) -> Out {
        let (sub, waited) = {
            let ci = self.ci.as_mut().unwrap();
            ci.sub += 1;
            (ci.sub, ci.elapsed())
        };
        match name {
            "mulai" => {
                if self.toolbar_ready && !self.tabs.is_empty() && self.tabs[self.active].webview.is_some() {
                    Out::Pass(format!("jendela + toolbar + tab pertama siap {} ms sejak Ovner mulai", self.started.elapsed().as_millis()))
                } else {
                    Out::Wait
                }
            }
            "patokan_ram_awal" => {
                if sub == 1 {
                    self.request_renderer_frames(true);
                }
                if waited < Duration::from_secs(8) {
                    return Out::Wait;
                }
                let mb = self.ci_ram();
                self.ci.as_mut().unwrap().bench.push(("RAM diam, 1 tab kosong (MB)".into(), mb));
                Out::Pass(format!("{mb} MB, {} proses — {}", self.stats.len(), self.ram_breakdown()))
            }
            "isi_ram_ovner_dan_pemblokir" => {
                let rows = crate::selftest::ram_items(crate::data_dir());
                let txt: Vec<String> = rows.iter().take(2).map(|r| format!("{}: {}", r["name"].as_str().unwrap_or(""), r["msg"].as_str().unwrap_or(""))).collect();
                if let Some(e) = crate::blocker::measure_engine(crate::data_dir()) {
                    self.ci.as_mut().unwrap().bench.push(("Mesin pemblokir iklan (KB)".into(), e.0 / 1024));
                }
                self.ci.as_mut().unwrap().bench.push(("Heap Rust Ovner (KB)".into(), crate::heap::now() / 1024));
                Out::Pass(txt.join(" | "))
            }
            "pdf_tanpa_ukuran_kecil_tampil" | "pdf_dalam_bingkai_seperti_infor" => {
                if sub == 1 {
                    self.ci_open(if name.starts_with("pdf_tanpa") { "/pdf-small-nolen" } else { "/infor" });
                    return Out::Wait;
                }
                let ci = self.ci.as_ref().unwrap();
                if ci.logged("dialihkan ke unduhan") || ci.logged("PDF diunduh langsung") {
                    return Out::Fail("PDF kecil malah diunduh".into());
                }
                if ci.logged("PDF ditampilkan di tab") && waited > Duration::from_secs(3) {
                    Out::Pass("diukur lalu ditampilkan di penampil PDF (preview + tombol unduh/cetak)".into())
                } else {
                    Out::Wait
                }
            }
            "pdf_tanpa_ukuran_besar_jadi_unduhan" | "pdf_dengan_ukuran_besar_diunduh" => {
                let nolen = name.contains("tanpa");
                if sub == 1 {
                    self.ci_open(if nolen { "/pdf-big-nolen" } else { "/pdf-big-len" });
                    return Out::Wait;
                }
                let need = if nolen { "dialihkan ke unduhan" } else { "PDF diunduh langsung" };
                let Some(d) = self.ci_new_download() else { return Out::Wait };
                if d.state == "failed" {
                    return Out::Fail(format!("unduhan gagal: {}", d.error));
                }
                if d.state != "done" {
                    return Out::Wait;
                }
                let size = std::fs::metadata(&d.path).map(|m| m.len()).unwrap_or(0);
                let want = if nolen { 7 } else { 6 } * 1_048_576;
                if !self.ci.as_ref().unwrap().logged(need) {
                    return Out::Fail(format!("selesai tetapi log \"{need}\" tidak ada"));
                }
                if size < want {
                    return Out::Fail(format!("berkas {size} byte, seharusnya ≥ {want}"));
                }
                let head = std::fs::read(&d.path).map(|b| b.starts_with(b"%PDF")).unwrap_or(false);
                if head {
                    Out::Pass(format!("{} ({} KB) utuh", d.name, size / 1024))
                } else {
                    Out::Fail("berkas bukan PDF".into())
                }
            }
            "pdf_file_lokal_tidak_dicegat" => {
                if sub == 1 {
                    let p = self.ci.as_ref().unwrap().out.join("lokal.pdf");
                    let _ = std::fs::write(&p, testserver::pdf(30 * 1024));
                    let u = url::Url::from_file_path(&p).map(|u| u.to_string()).unwrap_or_default();
                    self.open_tab(&u, true, false);
                    return Out::Wait;
                }
                if waited < Duration::from_secs(6) {
                    return Out::Wait;
                }
                let ci = self.ci.as_ref().unwrap();
                if ci.logged("PDF tanpa ukuran") || ci.logged("PDF diunduh") || self.downloads.len() > ci.dl0 {
                    Out::Fail("PDF lokal ikut dicegat/diunduh".into())
                } else {
                    Out::Pass("dibuka langsung di penampil".into())
                }
            }
            "unduh_mesin_ovner_20mb_utuh" => {
                if sub == 1 {
                    self.ci_open("/file-20mb");
                    return Out::Wait;
                }
                let Some(d) = self.ci_new_download() else { return Out::Wait };
                if d.state == "failed" {
                    return Out::Fail(format!("gagal: {}", d.error));
                }
                if d.state != "done" {
                    return Out::Wait;
                }
                let Ok(b) = std::fs::read(&d.path) else { return Out::Fail("berkas tidak terbaca".into()) };
                let bad = b.iter().enumerate().position(|(i, x)| *x != testserver::big_byte(i));
                match (b.len() == testserver::BIG_FILE, bad) {
                    (true, None) => Out::Pass(format!("20 MB utuh (isi dicek byte per byte), mesin: {}", if d.engine { "Ovner multi-koneksi" } else { "WebView2" })),
                    (false, _) => Out::Fail(format!("ukuran {} byte", b.len())),
                    (_, Some(i)) => Out::Fail(format!("isi berbeda mulai byte {i}")),
                }
            }
            "iklan_diblokir" => {
                if sub == 1 {
                    self.ci_open("/ads");
                    return Out::Wait;
                }
                if waited < Duration::from_secs(5) {
                    return Out::Wait;
                }
                let Some(j) = self.ci_tab("/ads") else { return Out::Fail("tab hilang".into()) };
                let t = &self.tabs[j];
                if t.title.contains("IKLAN LOLOS") {
                    Out::Fail("skrip iklan berjalan (tidak diblokir)".into())
                } else if t.blocked.get() >= 1 {
                    Out::Pass(format!("{} permintaan diblokir", t.blocked.get()))
                } else {
                    Out::Fail("tidak ada yang tercatat diblokir".into())
                }
            }
            "subtitle_lewat_halaman_youtube_tiruan" => {
                if sub == 1 {
                    self.ci_open("/yt-mock");
                    self.ci.as_mut().unwrap().nonce = format!("ci{}", crate::store::now());
                    return Out::Wait;
                }
                if let Some(r) = self.ci.as_mut().unwrap().eval.take() {
                    let outer: Value = serde_json::from_str(&r).unwrap_or(Value::Null);
                    let v: Value = outer.as_str().and_then(|s| serde_json::from_str(s).ok()).unwrap_or(Value::Null);
                    match v["state"].as_str() {
                        Some("ok") => {
                            return match video::json3_to_srt(v["json"].as_str().unwrap_or("")) {
                                Ok(srt) if srt.contains("halo uji") => Out::Pass(format!("subtitle didapat lewat {} (bahasa {}), jadi SRT", v["via"].as_str().unwrap_or("?"), v["lang"].as_str().unwrap_or("?"))),
                                Ok(srt) => Out::Fail(format!("isi tidak cocok: {srt}")),
                                Err(e) => Out::Fail(e),
                            }
                        }
                        Some("fail") => return Out::Fail(v["error"].as_str().unwrap_or("gagal").to_string()),
                        _ => {}
                    }
                }
                // Jalankan skrip yang sama dengan pengambilan subtitle sungguhan, tiap ±0,6 dtk.
                if sub % 2 == 0 {
                    if let Some(j) = self.ci_tab("/yt-mock") {
                        if let Some(wv) = &self.tabs[j].webview {
                            let js = video::page_subs_js(&self.ci.as_ref().unwrap().nonce, "wtp3OolekmA", "id", true, false);
                            let p = self.proxy.clone();
                            let _ = wv.evaluate_script_with_callback(&js, move |r| crate::send(&p, UserEvent::CiEval(r)));
                        }
                    }
                }
                Out::Wait
            }
            "tidur_n_tab_terakhir" => {
                // keep_live_tabs = 1, aturan menit 30: setelah membuka 3 halaman berurutan, yang
                // terlihat (page/3) + 1 tab terakhir sebelumnya (page/2) hidup, page/1 tidur
                // "di luar 1 tab terakhir".
                match sub {
                    1 => {
                        {
                            let mut s = self.settings.borrow_mut();
                            s.keep_live_tabs = 1;
                            s.sleep_tabs_minutes = 30;
                        }
                        self.ci_open("/page/1");
                    }
                    8 => self.ci_open("/page/2"),
                    16 => self.ci_open("/page/3"),
                    _ => {}
                }
                if waited < Duration::from_secs(16) {
                    return Out::Wait;
                }
                let state = |p: &str| self.ci_tab(p).map(|j| self.tabs[j].webview.is_some());
                match (state("/page/1"), state("/page/2"), state("/page/3")) {
                    (Some(false), Some(true), Some(true)) if self.ci.as_ref().unwrap().logged("di luar 1 tab terakhir") => Out::Pass("tab yang dilihat + 1 tab terakhir hidup, tab lebih lama tidur, alasan tercatat".into()),
                    (a, b, c) => {
                        self.ci.as_mut().unwrap().note = format!("hidup: page1 {a:?}, page2 {b:?}, page3 {c:?}");
                        Out::Wait
                    }
                }
            }
            "formulir_dilindungi_dari_tidur" => {
                match sub {
                    1 => self.ci_open("/form"),
                    10 => {
                        if let Some(wv) = self.ci_tab("/form").and_then(|j| self.tabs[j].webview.as_ref()) {
                            let _ = wv.evaluate_script(r#"(function(){var e=document.getElementById("nama");e.focus();e.value="Rendi";e.dispatchEvent(new Event("input",{bubbles:true}));})()"#);
                        }
                    }
                    16 => self.ci_open("/page/4"),
                    _ => {}
                }
                if waited < Duration::from_secs(16) {
                    return Out::Wait;
                }
                let Some(j) = self.ci_tab("/form") else { return Out::Fail("tab formulir hilang".into()) };
                let (alive, dirty) = (self.tabs[j].webview.is_some(), self.tabs[j].form_dirty);
                if !dirty {
                    self.ci.as_mut().unwrap().note = "Ovner belum menerima tanda formulir diisi".into();
                    return Out::Wait;
                }
                if alive {
                    Out::Pass("formulir diisi → tidak ditidurkan walau di luar N tab terakhir".into())
                } else {
                    Out::Fail("tab berisi formulir ikut ditidurkan".into())
                }
            }
            "tidurkan_semua_tab_lain" => {
                if sub == 1 {
                    for k in 5..8 {
                        let u = self.ci.as_ref().unwrap().url(&format!("/page/{k}"));
                        self.open_tab(&u, false, false);
                    }
                    return Out::Wait;
                }
                if sub == 12 {
                    self.on_menu("sleep-all");
                    return Out::Wait;
                }
                if sub < 18 {
                    return Out::Wait;
                }
                let awake: Vec<String> = (0..self.tabs.len()).filter(|&j| self.tabs[j].webview.is_some() && !self.is_shown(j)).map(|j| display_url(&self.tabs[j].url)).collect();
                let only_form = awake.iter().all(|u| u.contains("/form"));
                if only_form && self.ci.as_ref().unwrap().logged("tidurkan semua tab lain") {
                    Out::Pass(format!("semua tab lain tidur; dilindungi: {}", if awake.is_empty() { "-".into() } else { awake.join(", ") }))
                } else {
                    Out::Fail(format!("masih hidup: {awake:?}"))
                }
            }
            "pindah_ke_daftar_dan_buka_lagi" => {
                if sub == 1 {
                    self.move_to_list(false);
                    return Out::Wait;
                }
                if sub < 6 {
                    return Out::Wait;
                }
                if sub == 6 {
                    let Some(k) = self.sessions.iter().position(|s| s.list) else { return Out::Fail("daftar tidak tersimpan".into()) };
                    let n = self.sessions[k].tabs.len();
                    let Some(page) = self.tabs.iter().find(|t| internal_page(&t.url) == Some("sessions")).map(|t| t.id) else { return Out::Fail("halaman daftar tidak terbuka".into()) };
                    self.ci.as_mut().unwrap().note = n.to_string();
                    self.on_content(page, &json!({"t": "session-item", "idx": k, "item": 0}).to_string());
                    return Out::Wait;
                }
                if sub < 10 {
                    return Out::Wait;
                }
                let before: usize = self.ci.as_ref().unwrap().note.parse().unwrap_or(0);
                let after = self.sessions.iter().find(|s| s.list).map(|s| s.tabs.len()).unwrap_or(0);
                if before > 0 && after + 1 == before {
                    Out::Pass(format!("{before} tab masuk daftar; satu dibuka lagi dan otomatis dihapus dari daftar"))
                } else {
                    Out::Fail(format!("isi daftar {before} → {after}"))
                }
            }
            "rapikan_ram_cerdas" => {
                if sub == 1 {
                    let n = self.trim_smart(true);
                    self.ci.as_mut().unwrap().note = n.to_string();
                    return Out::Wait;
                }
                if self.ci.as_ref().unwrap().logged("RAM dirapikan") {
                    Out::Pass(format!("{} proses dirapikan (proses yang dilindungi dilewati dengan alasan)", self.ci.as_ref().unwrap().note))
                } else {
                    Out::Fail("tidak ada catatan rapikan".into())
                }
            }
            "browser_default_daftar_dan_lepas" => {
                let exe = std::env::current_exe().unwrap_or_default();
                if !cfg!(windows) {
                    return Out::Pass("bukan Windows: dilewati".into());
                }
                let reg = crate::defaults::register(&exe);
                let on = crate::defaults::status().registered;
                crate::defaults::unregister();
                let off = !crate::defaults::status().registered;
                match (reg, on, off) {
                    (Ok(_), true, true) => Out::Pass("terdaftar (http/https/.html/.pdf) lalu dilepas bersih".into()),
                    (r, a, b) => Out::Fail(format!("daftar {r:?}, terdaftar {a}, lepas {b}")),
                }
            }
            "tombol_video_di_halaman_trusted_types" | "tombol_video_di_dalam_iframe" => {
                let iframe = name.contains("iframe");
                let path = if iframe { "/iframe-video" } else { "/tt-video" };
                if sub == 1 {
                    self.ci_open(path);
                    return Out::Wait;
                }
                if let Some(r) = self.ci.as_mut().unwrap().eval.take() {
                    let v: Value = serde_json::from_str(&r).ok().and_then(|o: Value| o.as_str().and_then(|s| serde_json::from_str(s).ok())).unwrap_or(Value::Null);
                    if v["btn"] == true && v["tt"] == true {
                        return Out::Pass(if iframe { "tombol unduh muncul di video dalam iframe (halaman Trusted Types)".into() } else { "Trusted Types aktif (seperti YouTube) dan tombol unduh tetap muncul".into() });
                    }
                    self.ci.as_mut().unwrap().note = format!("hasil: {v}");
                }
                if sub >= 8 && sub % 3 == 0 {
                    if let Some(wv) = self.ci_tab(path).and_then(|j| self.tabs[j].webview.as_ref()) {
                        let js = r#"(function(){
                          var w = %W%; var d = w.document; var v = d.getElementById('v'); if (!v) return JSON.stringify({btn:false,tt:false});
                          var r = v.getBoundingClientRect();
                          d.dispatchEvent(new w.MouseEvent('mousemove', {clientX: r.left + 60, clientY: r.top + 60, bubbles: true}));
                          var x = d.createElement('div'), tt = false; try { x.innerHTML = '<b>x</b>'; } catch (e) { tt = true; }
                          return JSON.stringify({btn: !!d.querySelector('ovner-video-button'), tt: tt});
                        })()"#
                            .replace("%W%", if iframe { "document.getElementById('f').contentWindow" } else { "window" });
                        let p = self.proxy.clone();
                        let _ = wv.evaluate_script_with_callback(&js, move |r| crate::send(&p, UserEvent::CiEval(r)));
                    }
                }
                Out::Wait
            }
            "video_hls_tertangkap" => {
                if sub == 1 {
                    self.ci_open("/hls-page");
                    return Out::Wait;
                }
                let Some(j) = self.ci_tab("/hls-page") else { return Out::Wait };
                let c = self.media_candidates(j, "");
                match c.first() {
                    Some(m) if m.kind == "hls" && m.url.contains("master.m3u8") => {
                        let referer = m.headers.iter().any(|(k, _)| k == "Referer");
                        Out::Pass(format!("daftar putar HLS tertangkap{}: {}", if referer { " beserta Referer" } else { "" }, display_url(&m.url)))
                    }
                    _ => Out::Wait,
                }
            }
            "terjemahan_halaman_dan_kembalikan" => {
                crate::translate::MOCK.store(true, std::sync::atomic::Ordering::Relaxed);
                if sub == 1 {
                    self.ci_open("/tr");
                    return Out::Wait;
                }
                let Some(j) = self.ci_tab("/tr") else { return Out::Wait };
                let phase = self.ci.as_ref().unwrap().note.clone();
                if sub == 8 {
                    self.start_translate(j, Some("id".into()));
                    self.ci.as_mut().unwrap().note = "terjemah".into();
                    return Out::Wait;
                }
                if let Some(r) = self.ci.as_mut().unwrap().eval.take() {
                    let txt: String = serde_json::from_str(&r).unwrap_or_default();
                    if phase == "terjemah" && txt.starts_with("[id] HELLO WORLD|") && txt.ends_with("|let x = 1;") {
                        self.restore_translate(j);
                        self.ci.as_mut().unwrap().note = "kembali".into();
                    } else if phase == "kembali" && txt == "Hello world|let x = 1;" {
                        crate::translate::MOCK.store(false, std::sync::atomic::Ordering::Relaxed);
                        return Out::Pass("teks diganti terjemahan (kode tidak ikut), lalu Tampilkan asli mengembalikan teks persis".into());
                    }
                }
                if sub > 8 && sub % 2 == 0 {
                    if let Some(wv) = &self.tabs[j].webview {
                        let p = self.proxy.clone();
                        let _ = wv.evaluate_script_with_callback("document.getElementById('a').textContent + '|' + document.getElementById('c').textContent", move |r| crate::send(&p, UserEvent::CiEval(r)));
                    }
                }
                Out::Wait
            }
            "baca_keras_ambil_paragraf" => {
                if sub == 1 {
                    self.ci_open("/read");
                    return Out::Wait;
                }
                if sub == 8 {
                    if let Some(j) = self.ci_tab("/read") {
                        self.start_read(j, false);
                    }
                    return Out::Wait;
                }
                let line = self.ci.as_ref().unwrap().logs().into_iter().find(|l| l.contains("baca keras dimulai"));
                match line {
                    // Judul halaman + 3 paragraf artikel.
                    Some(l) if l.contains(": 4 paragraf") || l.contains(": 3 paragraf") => {
                        self.on_reader_cmd("read-stop", &Value::Null);
                        Out::Pass(format!("teks halaman diambil per paragraf dan dikirim ke pemutar toolbar ({})", l.split("dimulai: ").nth(1).unwrap_or("").split(',').next().unwrap_or("")))
                    }
                    Some(l) => Out::Fail(l),
                    None => Out::Wait,
                }
            }
            "lihat_sumber_halaman" => {
                if sub == 1 {
                    self.ci_open("/page/31");
                    return Out::Wait;
                }
                if sub == 8 {
                    if let Some(j) = self.ci_tab("/page/31") {
                        self.view_source(j);
                    }
                    return Out::Wait;
                }
                let opened = self.tabs.iter().any(|t| internal_page(&t.url) == Some("source"));
                let text = self.sources.last().map(|(_, v)| v["text"].as_str().unwrap_or("").to_string()).unwrap_or_default();
                if opened && text.contains("halaman biasa") {
                    let dom = self.sources.last().map(|(_, v)| v["dom"] == true).unwrap_or(false);
                    Out::Pass(format!("tab sumber halaman terbuka ({}, {} byte)", if dom { "DOM" } else { "sumber asli dari cache" }, text.len()))
                } else {
                    Out::Wait
                }
            }
            "tangkapan_layar_disimpan" => {
                if sub == 1 {
                    self.ci_open("/page/32");
                    return Out::Wait;
                }
                if sub == 8 {
                    if let Some(j) = self.ci_tab("/page/32") {
                        self.screenshot(j, true);
                    }
                    return Out::Wait;
                }
                let Some(d) = self.ci_new_download() else { return Out::Wait };
                let ok = std::fs::read(&d.path).map(|b| b.starts_with(b"\x89PNG") && b.len() > 2000).unwrap_or(false);
                if ok {
                    Out::Pass(format!("PNG halaman penuh tersimpan: {}", d.name))
                } else {
                    Out::Fail(format!("berkas tidak valid: {}", d.path))
                }
            }
            "gpc_dnt_terkirim" => {
                if sub == 1 {
                    self.ci_open("/echo");
                    return Out::Wait;
                }
                if let Some(r) = self.ci.as_mut().unwrap().eval.take() {
                    let txt: String = serde_json::from_str(&r).unwrap_or_default();
                    if txt.contains("sec-gpc: 1") && txt.contains("dnt: 1") {
                        return Out::Pass("header Sec-GPC: 1 dan DNT: 1 diterima server".into());
                    }
                    self.ci.as_mut().unwrap().note = txt.replace('\n', " ").chars().take(200).collect();
                }
                if sub >= 6 && sub % 3 == 0 {
                    if let Some(wv) = self.ci_tab("/echo").and_then(|j| self.tabs[j].webview.as_ref()) {
                        let p = self.proxy.clone();
                        let _ = wv.evaluate_script_with_callback("document.body ? document.body.innerText : ''", move |r| crate::send(&p, UserEvent::CiEval(r)));
                    }
                }
                Out::Wait
            }
            "parameter_pelacak_dibuang" => {
                if sub == 1 {
                    self.ci_open("/page/33?utm_source=koran&fbclid=abc123&id=5");
                    return Out::Wait;
                }
                let Some(j) = self.ci_tab("/page/33") else { return Out::Wait };
                let u = self.tabs[j].url.clone();
                if sub < 8 {
                    return Out::Wait;
                }
                if !u.contains("utm_source") && !u.contains("fbclid") && u.contains("id=5") {
                    Out::Pass(format!("alamat jadi {}", display_url(&u)))
                } else {
                    self.ci.as_mut().unwrap().note = u;
                    Out::Wait
                }
            }
            "font_tampilan_ovner" => {
                if sub == 1 {
                    let _ = self.settings.borrow_mut().set("ui_font", &json!("albert-sans"));
                    self.apply_setting_side_effects("ui_font");
                    return Out::Wait;
                }
                if let Some(r) = self.ci.as_mut().unwrap().eval.take() {
                    let txt: String = serde_json::from_str(&r).unwrap_or_default();
                    if txt.contains("Albert Sans Variable") && txt.ends_with("|true") {
                        let _ = self.settings.borrow_mut().set("ui_font", &json!("segoe"));
                        self.apply_setting_side_effects("ui_font");
                        return Out::Pass("toolbar memakai Albert Sans (dimuat dari dalam Ovner), lalu kembali ke Segoe UI".into());
                    }
                    self.ci.as_mut().unwrap().note = txt;
                }
                if sub >= 4 && sub % 2 == 0 {
                    let p = self.proxy.clone();
                    let _ = self.toolbar.evaluate_script_with_callback(
                        "getComputedStyle(document.body).fontFamily + '|' + document.fonts.check('13px \"Albert Sans Variable\"')",
                        move |r| crate::send(&p, UserEvent::CiEval(r)),
                    );
                }
                Out::Wait
            }
            "layanan_online_terjemah_dan_suara" => {
                // Jalur tidak resmi Microsoft/Google dari server GitHub: tanda dini bila berubah.
                if sub == 1 {
                    let p = self.proxy.clone();
                    std::thread::spawn(move || {
                        let mut parts = Vec::new();
                        for e in ["microsoft", "google"] {
                            parts.push(match crate::translate::translate_only(e, &["Good morning".to_string()], "en", "id") {
                                Ok(o) => format!("{e}: ok \"{}\"", o.texts.first().cloned().unwrap_or_default()),
                                Err(x) => format!("{e}: GAGAL {x}"),
                            });
                        }
                        parts.push(match crate::tts::edge_speak("Halo, ini uji suara.", "id-ID-GadisNeural", 100) {
                            Ok(sp) => format!("suara edge: ok {} KB, {} kata", sp.mp3.len() / 1024, sp.marks.len()),
                            Err(x) => format!("suara edge: GAGAL {x}"),
                        });
                        crate::send(&p, UserEvent::CiEval(serde_json::to_string(&parts.join("; ")).unwrap_or_default()));
                    });
                    return Out::Wait;
                }
                match self.ci.as_mut().unwrap().eval.take() {
                    Some(r) => {
                        let txt: String = serde_json::from_str(&r).unwrap_or_default();
                        if txt.contains("GAGAL") {
                            Out::Fail(txt)
                        } else {
                            Out::Pass(txt)
                        }
                    }
                    None => Out::Wait,
                }
            }
            "patokan_ram_10_tab" => {
                if sub == 1 {
                    for k in 10..20 {
                        let u = self.ci.as_ref().unwrap().url(&format!("/page/{k}"));
                        self.open_tab(&u, false, false);
                    }
                    return Out::Wait;
                }
                if sub == 30 || sub == 70 {
                    self.request_renderer_frames(true);
                }
                if sub == 40 {
                    let mb = self.ci_ram();
                    let live = self.tabs.iter().filter(|t| t.webview.is_some()).count();
                    self.ci.as_mut().unwrap().bench.push((format!("RAM dengan {live} tab hidup (MB)"), mb));
                    self.ci.as_mut().unwrap().note = format!("rincian {live} tab hidup: {}", self.ram_breakdown());
                    self.on_menu("sleep-all");
                    return Out::Wait;
                }
                if sub < 80 {
                    return Out::Wait;
                }
                let mb = self.ci_ram();
                let live = self.tabs.iter().filter(|t| t.webview.is_some()).count();
                self.ci.as_mut().unwrap().bench.push((format!("RAM setelah tidurkan semua ({live} tab hidup) (MB)"), mb));
                let ci = self.ci.as_ref().unwrap();
                let first = ci.note.clone();
                let b = &ci.bench;
                Out::Pass(format!(
                    "{} | {first} | rincian setelah tidur: {}",
                    b.iter().filter(|(k, _)| k.starts_with("RAM")).map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join("; "),
                    self.ram_breakdown()
                ))
            }
            "ekstensi_pasang_dari_crx" | "ekstensi_copyclip_dari_web_store" => {
                let store = name.contains("copyclip");
                let (id, url) = if store { (COPYCLIP_ID.to_string(), None) } else { (crate::extensions::test_crx().0, Some(self.ci.as_ref().unwrap().url("/ext.crx"))) };
                if sub == 1 {
                    if !self.ext_env {
                        return Out::Fail("mesin WebView2 dibuat tanpa dukungan ekstensi".into());
                    }
                    self.ext_install(&id, url);
                    return Out::Wait;
                }
                if let Some((eid, e)) = self.ext_err.clone().filter(|(eid, _)| *eid == id) {
                    return Out::Fail(format!("{eid}: {e}"));
                }
                let Some(info) = self.ext_infos.iter().find(|i| i.id == id).cloned() else { return Out::Wait };
                match self.ext_loaded.get(&info.dir) {
                    Some(Ok((wid, wname))) => {
                        if *wid != id {
                            return Out::Fail(format!("WebView2 memberi ID {wid}, seharusnya {id} (kunci manifest tidak dipakai)"));
                        }
                        Out::Pass(format!(
                            "{wname} v{} terpasang dan dimuat WebView2 dengan ID Web Store yang sama; {}: {}; izin: {}",
                            info.version,
                            if info.kind == "side" { "panel samping" } else if info.kind == "popup" { "popup" } else { "tanpa panel" },
                            if info.panel.is_empty() { "-" } else { info.panel.as_str() },
                            if info.perms.is_empty() { "-".to_string() } else { info.perms.join(", ") }
                        ))
                    }
                    Some(Err(e)) => Out::Fail(format!("dipasang, tetapi WebView2 menolak memuat: {e}")),
                    None => Out::Wait,
                }
            }
            "ekstensi_skrip_konten_jalan" => {
                if sub == 1 {
                    self.ci_open("/page/41");
                    return Out::Wait;
                }
                if let Some(v) = self.ci.as_ref().unwrap().eval.clone() {
                    if v.contains('1') {
                        return Out::Pass("skrip konten ekstensi berjalan di halaman (penanda data-ovner-ext=1 ada)".into());
                    }
                    self.ci.as_mut().unwrap().note = format!("hasil: {v}");
                }
                if sub >= 6 && sub % 4 == 0 {
                    if let Some(wv) = self.ci_tab("/page/41").and_then(|j| self.tabs[j].webview.as_ref()) {
                        let p = self.proxy.clone();
                        let _ = wv.evaluate_script_with_callback("String(document.documentElement.getAttribute('data-ovner-ext'))", move |r| crate::send(&p, UserEvent::CiEval(r)));
                    }
                }
                Out::Wait
            }
            "ekstensi_panel_samping_terbuka" => {
                let id = crate::extensions::test_crx().0;
                if sub == 1 {
                    self.ext_open(&id);
                    return Out::Wait;
                }
                if self.ext_panel.is_none() {
                    return if sub > 5 { Out::Fail("panel ekstensi tidak terbuka".into()) } else { Out::Wait };
                }
                if let Some(v) = self.ci.as_ref().unwrap().eval.clone() {
                    if v.contains("Panel Uji Ovner") {
                        self.ext_close();
                        return Out::Pass("panel samping ekstensi (chrome-extension://…/panel.html) terbuka di kanan jendela".into());
                    }
                    self.ci.as_mut().unwrap().note = format!("judul panel: {v}");
                }
                if sub >= 6 && sub % 4 == 0 {
                    if let Some((_, wv)) = &self.ext_panel {
                        let p = self.proxy.clone();
                        let _ = wv.evaluate_script_with_callback("document.title + ' @ ' + location.href", move |r| crate::send(&p, UserEvent::CiEval(r)));
                    }
                }
                Out::Wait
            }
            _ => Out::Fail("langkah tidak dikenal".into()),
        }
    }

    fn ci_finish(&mut self) {
        let Some(ci) = self.ci.as_ref() else { return };
        let core_ok = ci.results.iter().filter(|r| !PROBATION.contains(&r["langkah"].as_str().unwrap_or(""))).all(|r| r["status"] == "lulus");
        let ok = ci.results.iter().all(|r| r["status"] == "lulus");
        let pass = ci.results.iter().filter(|r| r["status"] == "lulus").count();
        let bench: serde_json::Map<String, Value> = ci.bench.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
        let report = json!({"versi": crate::update::display_version(), "webview2": wry::webview_version().unwrap_or_default(), "lulus": pass, "jumlah": ci.results.len(), "semua_lulus": ok, "inti_lulus": core_ok, "masa_percobaan": PROBATION, "patokan": bench, "langkah": ci.results});
        let _ = std::fs::write(ci.out.join("hasil.json"), serde_json::to_string_pretty(&report).unwrap_or_default());
        let mut txt = format!("Ovner {} self-test: {pass}/{} lulus\n\n", crate::update::display_version(), ci.results.len());
        for r in &ci.results {
            let trial = if PROBATION.contains(&r["langkah"].as_str().unwrap_or("")) { "*" } else { " " };
            txt.push_str(&format!("{:<6}{trial}{:<42} {:>7} ms  {}\n", r["status"].as_str().unwrap_or(""), r["langkah"].as_str().unwrap_or(""), r["ms"], r["pesan"].as_str().unwrap_or("")));
            for l in r["log"].as_array().into_iter().flatten() {
                txt.push_str(&format!("         | {}\n", l.as_str().unwrap_or("")));
            }
        }
        txt.push_str("\n* = masa percobaan (dilaporkan, belum menentukan lulus)\n\nPatokan:\n");
        for (k, v) in &ci.bench {
            txt.push_str(&format!("  {k}: {v}\n"));
        }
        let _ = std::fs::write(ci.out.join("hasil.txt"), &txt);
        log(&format!("[diag] SELF-TEST selesai: {pass}/{} lulus", ci.results.len()));
        logging::flush();
        let data = crate::data_dir();
        for f in logging::files(data).into_iter().chain(crate::metrics::files(data)) {
            if let Some(n) = f.file_name() {
                let _ = std::fs::copy(&f, ci.out.join(n));
            }
        }
        std::process::exit(if core_ok { 0 } else { 1 });
    }
}
