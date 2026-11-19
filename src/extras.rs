//! Fitur 0.8.25: menu klik kanan Ovner, lihat sumber halaman, tangkapan layar, kode QR,
//! terjemahan halaman, baca keras, dan penangkap media (unduh video seperti IDM).

use super::*;
use base64::Engine;

pub const TRANSLATE_JS: &str = include_str!("ui/translate.js");
pub const READER_JS: &str = include_str!("ui/reader.js");
pub const SOURCE_HTML: &str = include_str!("ui/source.html");

/// Item menu klik kanan untuk sasaran ini (kosong = menu bawaan saja).
pub fn ctx_items(en: bool, info: &Value) -> Vec<win::CtxItem> {
    use win::CtxItem as C;
    let l = |id: &str, e: &str| if en { e.to_string() } else { id.to_string() };
    let page = info["page"].as_str().unwrap_or("");
    if !(page.starts_with("http://") || page.starts_with("https://")) || page.starts_with(INTERNAL_PREFIX) || info["editable"].as_bool() == Some(true) {
        return Vec::new();
    }
    let kind = info["kind"].as_str().unwrap_or("page");
    let mut v = vec![C::sep()];
    if kind == "text" || !info["sel"].as_str().unwrap_or("").trim().is_empty() {
        v.push(C::cmd(&l("Baca keras teks yang dipilih", "Read aloud selection"), "read-sel"));
        v.push(C::cmd(&l("Terjemahkan teks yang dipilih", "Translate selection"), "translate-sel"));
        return v;
    }
    // Tautan dan gambar: menu bawaan sudah lengkap; tambahkan seperlunya saja.
    if !info["link"].as_str().unwrap_or("").is_empty() && !matches!(kind, "video" | "audio") {
        v.push(C::cmd(&l("Buat kode QR untuk tautan ini", "Create QR code for this link"), "qr-link"));
        if kind == "image" {
            v.push(C::cmd(&l("Blokir elemen…", "Block element…"), "pick"));
        }
        return v;
    }
    if kind == "image" {
        v.push(C::cmd(&l("Blokir elemen…", "Block element…"), "pick"));
        return v;
    }
    if matches!(kind, "video" | "audio") {
        v.push(C::cmd(&l("Unduh video ini", "Download this video"), "video-dl"));
        v.push(C::sep());
    }
    v.push(C::cmd(&l("Terjemahkan halaman…", "Translate page…"), "translate"));
    v.push(C::cmd(&l("Baca keras halaman ini", "Read aloud this page"), "read").key("Ctrl+Shift+U"));
    v.push(C::sub(
        &l("Tidur & daftar tab", "Sleep & tab list"),
        vec![
            C::cmd(&l("Tidurkan tab ini", "Sleep this tab"), "sleep-this"),
            C::cmd(&l("Tidurkan semua tab lain", "Sleep all other tabs"), "sleep-all"),
            C::sep(),
            C::cmd(&l("Pindahkan tab lain ke daftar", "Move other tabs to the list"), "list-others"),
            C::cmd(&l("Pindahkan semua tab ke daftar", "Move all tabs to the list"), "list-all"),
            C::cmd(&l("Buka daftar tab", "Open tab list"), "list-open"),
        ],
    ));
    v.push(C::sub(
        &l("Tangkapan layar", "Screenshot"),
        vec![C::cmd(&l("Area yang terlihat", "Visible area"), "shot"), C::cmd(&l("Satu halaman penuh", "Full page"), "shot-full")],
    ));
    let mut qr = C::cmd(&l("Buat kode QR untuk halaman ini", "Create QR code for this page"), "qr");
    qr.unless = "createQrCode";
    v.push(qr);
    v.push(C::cmd(&l("Blokir elemen…", "Block element…"), "pick"));
    v.push(C::cmd(&l("Lihat sumber halaman", "View page source"), "source").key("Ctrl+U"));
    v.push(C::sep());
    v
}

/// SVG kode QR (modul gelap sebagai satu path).
pub fn qr_svg(text: &str) -> Option<String> {
    let qr = qrcodegen::QrCode::encode_text(text, qrcodegen::QrCodeEcc::Medium).ok()?;
    let n = qr.size();
    let b = 4;
    let mut d = String::new();
    for y in 0..n {
        for x in 0..n {
            if qr.get_module(x, y) {
                d.push_str(&format!("M{},{}h1v1h-1z", x + b, y + b));
            }
        }
    }
    let w = n + 2 * b;
    Some(format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w} {w}\" shape-rendering=\"crispEdges\"><rect width=\"{w}\" height=\"{w}\" fill=\"#fff\"/><path d=\"{d}\" fill=\"#000\"/></svg>"))
}

/// Nama berkas aman dari teks bebas.
fn file_stem(s: &str) -> String {
    let t: String = s.chars().map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { ' ' } else { c }).collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    t.chars().take(80).collect::<String>().trim().to_string()
}

impl App {
    // ---- penangkap media ----

    pub(crate) fn on_media_hit(&mut self, id: u64, hit: win::MediaHit) {
        let Some(i) = self.tab_index(id) else { return };
        let t = &mut self.tabs[i];
        if t.media.iter().any(|m| m.url == hit.url) {
            return;
        }
        // Daftar putar HLS/DASH lebih dulu (master playlist = semua kualitas), lalu berkas.
        if t.media.len() >= 25 {
            t.media.remove(0);
        }
        let first = t.media.is_empty();
        log(&format!("[video] media terdeteksi ({}{}): {}", hit.kind, if hit.len > 0 { format!(", {} KB", hit.len / 1024) } else { String::new() }, hit.url.chars().take(300).collect::<String>()));
        t.media.push(hit);
        if first && i == self.active {
            self.render();
        }
    }

    /// Calon alamat video untuk tab `i` (urutan dicoba): HLS/DASH, lalu berkas terbesar.
    pub(crate) fn media_candidates(&self, i: usize, extra_src: &str) -> Vec<win::MediaHit> {
        let mut v: Vec<win::MediaHit> = self.tabs.get(i).map(|t| t.media.clone()).unwrap_or_default();
        if extra_src.starts_with("http") && !v.iter().any(|m| m.url == extra_src) {
            let page = self.tabs.get(i).map(|t| t.url.clone()).unwrap_or_default();
            v.push(win::MediaHit { url: extra_src.to_string(), kind: "file", mime: String::new(), len: 0, headers: vec![("Referer".into(), page)] });
        }
        let rank = |m: &win::MediaHit| match m.kind {
            "hls" => 0,
            "dash" => 1,
            _ => 2,
        };
        // HLS: daftar putar "master" (berisi kualitas lain) biasanya dimuat pertama.
        v.sort_by(|a, b| rank(a).cmp(&rank(b)).then(b.len.cmp(&a.len)));
        v.truncate(6);
        v
    }

    // ---- menu klik kanan ----

    pub(crate) fn on_ctx(&mut self, id: u64, cmd: &str, info: &Value) {
        let Some(i) = self.tab_index(id) else { return };
        if i != self.active {
            self.activate(i);
        }
        let page = info["page"].as_str().unwrap_or("").to_string();
        match cmd {
            "translate" => {
                self.start_translate(i, None);
                let _ = self.toolbar.evaluate_script("window.ovner && ovner.trOpen && ovner.trOpen()");
            }
            "translate-sel" => self.translate_selection(info["sel"].as_str().unwrap_or("")),
            "read" => self.start_read(i, false),
            "read-sel" => self.start_read(i, true),
            "sleep-this" => {
                let other = if i + 1 < self.tabs.len() { i + 1 } else { i.saturating_sub(1) };
                if other != i {
                    self.activate(other);
                    self.sleep_why = "diminta (klik kanan)".into();
                    self.sleep_tab(i);
                    self.render();
                }
            }
            "sleep-all" => {
                self.on_menu("sleep-all");
            }
            "list-others" | "list-all" => {
                self.on_toolbar(&json!({"cmd": "tablist-move", "all": cmd == "list-all"}).to_string());
            }
            "list-open" => {
                self.on_toolbar(&json!({"cmd": "tablist-open"}).to_string());
            }
            "shot" | "shot-full" => self.screenshot(i, cmd == "shot-full"),
            "qr" => self.show_qr(&page),
            "qr-link" => self.show_qr(info["link"].as_str().unwrap_or("")),
            "pick" => {
                self.on_menu("pick");
            }
            "source" => self.view_source(i),
            "video-dl" => {
                let src = info["src"].as_str().unwrap_or("");
                if src.starts_with("http") {
                    let hit = win::MediaHit { url: src.to_string(), kind: if src.contains(".m3u8") { "hls" } else { "file" }, mime: String::new(), len: 0, headers: vec![("Referer".into(), page.clone())] };
                    let id = self.tabs[i].id;
                    self.on_media_hit(id, hit);
                }
                self.open_video_dialog(&page);
            }
            _ => {}
        }
    }

    // ---- lihat sumber halaman ----

    pub(crate) fn view_source(&mut self, i: usize) {
        let Some(wv) = self.tabs.get(i).and_then(|t| t.webview.as_ref()) else { return };
        let url = self.tabs[i].url.clone();
        if !url.starts_with("http") {
            return;
        }
        let (p, id) = (self.proxy.clone(), self.tabs[i].id);
        win::cdp_result(wv, "Page.getFrameTree", "{}", move |r| send(&p, UserEvent::Source(id, url, 1, r.unwrap_or_default())));
    }

    pub(crate) fn on_source_step(&mut self, id: u64, url: String, step: u8, data: String) {
        let Some(i) = self.tab_index(id) else { return };
        let Some(wv) = self.tabs[i].webview.as_ref() else { return };
        let p = self.proxy.clone();
        match step {
            1 => {
                let v: Value = serde_json::from_str(&data).unwrap_or_default();
                let frame = v["frameTree"]["frame"]["id"].as_str().unwrap_or("").to_string();
                let doc = v["frameTree"]["frame"]["url"].as_str().unwrap_or(&url).to_string();
                if frame.is_empty() {
                    self.on_source_step(id, url, 3, String::new());
                    return;
                }
                let params = json!({"frameId": frame, "url": doc}).to_string();
                win::cdp_result(wv, "Page.getResourceContent", &params, move |r| send(&p, UserEvent::Source(id, url, 2, r.unwrap_or_default())));
            }
            2 => {
                let v: Value = serde_json::from_str(&data).unwrap_or_default();
                match v["content"].as_str() {
                    Some(c) => {
                        let text = if v["base64Encoded"].as_bool() == Some(true) {
                            base64::engine::general_purpose::STANDARD.decode(c).map(|b| String::from_utf8_lossy(&b).to_string()).unwrap_or_default()
                        } else {
                            c.to_string()
                        };
                        self.open_source_tab(url, text, false);
                    }
                    None => self.on_source_step(id, url, 3, String::new()),
                }
            }
            3 => {
                // Tidak ada di cache (mis. server mengirim no-store): ambil ulang dari server
                // dengan cookie halaman, seperti view-source Chrome.
                let params = json!({"expression": "fetch(location.href,{credentials:'include',cache:'force-cache'}).then(r=>r.ok?r.text():Promise.reject(r.status))", "awaitPromise": true, "returnByValue": true}).to_string();
                win::cdp_result(wv, "Runtime.evaluate", &params, move |r| send(&p, UserEvent::Source(id, url, 4, r.unwrap_or_default())));
            }
            4 => {
                let v: Value = serde_json::from_str(&data).unwrap_or_default();
                match v["result"]["value"].as_str().filter(|_| v.get("exceptionDetails").is_none()) {
                    Some(text) => self.open_source_tab(url, text.to_string(), false),
                    None => {
                        // Halaman POST atau server menolak: tampilkan DOM saat ini.
                        let _ = wv.evaluate_script_with_callback("document.documentElement.outerHTML", move |r| send(&p, UserEvent::Source(id, url.clone(), 5, r)));
                    }
                }
            }
            _ => {
                let text: String = serde_json::from_str(&data).unwrap_or_default();
                self.open_source_tab(url, text, true);
            }
        }
    }

    fn open_source_tab(&mut self, url: String, text: String, dom: bool) {
        self.source_seq += 1;
        let k = self.source_seq;
        log(&format!("[umum] lihat sumber halaman{}: {} ({} KB)", if dom { " (DOM)" } else { "" }, display_url(&url), text.len() / 1024));
        self.sources.retain(|(n, _)| k - *n < 5);
        self.sources.push((k, json!({"url": url, "text": text, "dom": dom})));
        let u = format!("{}?k={k}", internal_url("source"));
        self.open_tab(&u, true, false);
    }

    pub(crate) fn source_state(&self, tab_url: &str) -> Value {
        let k: u64 = tab_url.split("k=").nth(1).and_then(|x| x.split(['&', '#']).next()).and_then(|x| x.parse().ok()).unwrap_or(0);
        self.sources.iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone()).unwrap_or(Value::Null)
    }

    // ---- tangkapan layar ----

    pub(crate) fn screenshot(&mut self, i: usize, full: bool) {
        let Some(wv) = self.tabs.get(i).and_then(|t| t.webview.as_ref()) else { return };
        let (p, id) = (self.proxy.clone(), self.tabs[i].id);
        if full {
            win::cdp_result(wv, "Page.getLayoutMetrics", "{}", move |r| send(&p, UserEvent::Shot(id, 1, r.unwrap_or_default())));
        } else {
            win::cdp_result(wv, "Page.captureScreenshot", r#"{"format":"png"}"#, move |r| send(&p, UserEvent::Shot(id, 2, r.unwrap_or_default())));
        }
    }

    pub(crate) fn on_shot(&mut self, id: u64, step: u8, data: String) {
        let Some(i) = self.tab_index(id) else { return };
        let v: Value = serde_json::from_str(&data).unwrap_or_default();
        if step == 1 {
            let Some(wv) = self.tabs[i].webview.as_ref() else { return };
            let size = &v["cssContentSize"];
            let (w, h) = (size["width"].as_f64().unwrap_or(0.0), size["height"].as_f64().unwrap_or(0.0).min(16_000.0));
            let p = self.proxy.clone();
            let params = if w > 0.0 && h > 0.0 {
                json!({"format": "png", "captureBeyondViewport": true, "clip": {"x": 0, "y": 0, "width": w, "height": h, "scale": 1}}).to_string()
            } else {
                r#"{"format":"png"}"#.to_string()
            };
            win::cdp_result(wv, "Page.captureScreenshot", &params, move |r| send(&p, UserEvent::Shot(id, 2, r.unwrap_or_default())));
            return;
        }
        let Some(b64) = v["data"].as_str() else {
            log("[umum] GALAT: tangkapan layar gagal");
            return;
        };
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) else { return };
        let title = file_stem(&self.tabs[i].title);
        let name = format!("{} {}.png", if self.lang.code() == "en" { "Screenshot" } else { "Tangkapan layar" }, if title.is_empty() { win::local_time().replace(':', ".") } else { title });
        self.save_file_download(&name, &bytes, &self.tabs[i].url.clone());
    }

    /// Simpan berkas buatan Ovner (tangkapan layar, kode QR) ke folder unduhan dan tampilkan di
    /// daftar unduhan.
    pub(crate) fn save_file_download(&mut self, name: &str, bytes: &[u8], url: &str) {
        let dir = download_dir_of(&self.settings.borrow());
        let path = dlengine::categorized(&dir.join(name));
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let path = free_download_path(&path);
        if let Err(e) = std::fs::write(&path, bytes) {
            log(&format!("[unduh] GALAT: gagal menyimpan {}: {e}", path.display()));
            return;
        }
        log(&format!("[unduh] disimpan: {} ({} KB)", path.display(), bytes.len() / 1024));
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        self.downloads.push(Download {
            id: win::next_download_id(),
            url: url.to_string(),
            path: path.display().to_string(),
            name,
            total: bytes.len() as i64,
            received: bytes.len() as i64,
            state: "done".into(),
            time: store::now(),
            engine: false,
            speed: 0,
            conns: 0,
            video: false,
            tab: 0,
            vspec: None,
            error: String::new(),
            subs_retry: None,
        });
        self.save_downloads();
        self.dl_bubble = true;
        self.last_render_key.clear();
        self.render();
        self.refresh_pages("downloads");
    }

    // ---- kode QR ----

    pub(crate) fn show_qr(&mut self, url: &str) {
        let url = if url.is_empty() { self.tabs.get(self.active).map(|t| t.url.clone()).unwrap_or_default() } else { url.to_string() };
        let Some(svg) = qr_svg(&url) else { return };
        post_page(&self.toolbar, "qr", &json!({"url": url, "svg": svg}).to_string(), "window.ovner && ovner.qr");
    }

    // ---- terjemahan ----

    pub(crate) fn start_translate(&mut self, i: usize, to: Option<String>) {
        let (engine, default_to) = {
            let s = self.settings.borrow();
            (s.translate_engine.clone(), s.translate_to.clone())
        };
        let Some(t) = self.tabs.get_mut(i) else { return };
        let Some(wv) = t.webview.as_ref() else { return };
        if !t.url.starts_with("http") {
            return;
        }
        let to = to.filter(|x| !x.is_empty()).unwrap_or_else(|| if default_to == "auto" { translate::auto_target(&t.page_lang).to_string() } else { default_to });
        self.tr_seq += 1;
        let n = self.tr_seq;
        t.tr = TrState { n, on: true, to: to.clone(), busy: true, ..Default::default() };
        let from = t.page_lang.split('-').next().unwrap_or("").to_string();
        let js = format!("{TRANSLATE_JS}\n;window.__ovnerTr && __ovnerTr.start({n},{},{})", json!(to), json!(from));
        let _ = wv.evaluate_script(&js);
        log(&format!("[terjemah] halaman diterjemahkan ke {to} lewat {engine}{}: {}", if from.is_empty() { String::new() } else { format!(" (dari {from})") }, display_url(&t.url)));
        self.last_render_key.clear();
        self.render();
    }

    pub(crate) fn restore_translate(&mut self, i: usize) {
        let Some(t) = self.tabs.get_mut(i) else { return };
        if let Some(wv) = t.webview.as_ref() {
            let _ = wv.evaluate_script("window.__ovnerTr && __ovnerTr.restore()");
        }
        t.tr = TrState::default();
        self.last_render_key.clear();
        self.render();
    }

    pub(crate) fn on_tr_req(&mut self, i: usize, msg: &Value) {
        let t = &self.tabs[i];
        let n = msg["n"].as_u64().unwrap_or(0);
        if !t.tr.on || n != t.tr.n {
            return;
        }
        let b = msg["b"].as_u64().unwrap_or(0);
        let texts: Vec<String> = msg["texts"].as_array().map(|a| a.iter().take(60).filter_map(|x| x.as_str()).map(|x| x.chars().take(5000).collect()).collect()).unwrap_or_default();
        if texts.iter().map(|x| x.len()).sum::<usize>() > 60_000 {
            return;
        }
        let to = t.tr.to.clone();
        let from = msg["from"].as_str().unwrap_or("").split('-').next().unwrap_or("").to_string();
        let engine = self.settings.borrow().translate_engine.clone();
        let (p, id) = (self.proxy.clone(), t.id);
        std::thread::spawn(move || {
            let r = translate::translate(&engine, &texts, &from, &to).map(|o| (o.texts, o.detected));
            send(&p, UserEvent::Translated(id, n, b, r));
        });
    }

    pub(crate) fn on_translated(&mut self, id: u64, n: u64, b: u64, r: Result<(Vec<String>, Option<String>), String>) {
        let Some(i) = self.tab_index(id) else { return };
        let t = &mut self.tabs[i];
        if t.tr.n != n {
            return;
        }
        let Some(wv) = t.webview.as_ref() else { return };
        match r {
            Ok((texts, det)) => {
                if let Some(d) = &det {
                    if t.page_lang.is_empty() {
                        t.page_lang = d.clone();
                    }
                }
                let _ = wv.evaluate_script(&format!("window.__ovnerTr && __ovnerTr.apply({n},{b},{},{})", json!(texts), json!(det.unwrap_or_default())));
            }
            Err(e) => {
                if t.tr.err.is_empty() {
                    log(&format!("[terjemah] GALAT: {e}"));
                }
                t.tr.err = e.clone();
                let _ = wv.evaluate_script(&format!("window.__ovnerTr && __ovnerTr.fail({n},{b},{})", json!(e)));
                self.last_render_key.clear();
                self.render();
            }
        }
    }

    pub(crate) fn translate_selection(&mut self, text: &str) {
        let text: String = text.chars().take(5000).collect();
        if text.trim().is_empty() {
            return;
        }
        let (engine, to_set) = {
            let s = self.settings.borrow();
            (s.translate_engine.clone(), s.translate_to.clone())
        };
        let p = self.proxy.clone();
        post_page(&self.toolbar, "trSel", &json!({"text": text, "busy": true}).to_string(), "window.ovner && ovner.trSel");
        std::thread::spawn(move || {
            // Bahasa tujuan otomatis: deteksi dulu lewat terjemahan ke Indonesia.
            let to = if to_set == "auto" { "id".to_string() } else { to_set };
            let mut r = translate::translate(&engine, std::slice::from_ref(&text), "", &to);
            if let Ok(o) = &r {
                if to == "id" && o.detected.as_deref().is_some_and(|d| d.starts_with("id")) {
                    r = translate::translate(&engine, std::slice::from_ref(&text), "id", "en");
                }
            }
            let v = match r {
                Ok(o) => json!({"text": text, "out": o.texts.first().cloned().unwrap_or_default(), "from": o.detected.unwrap_or_default()}),
                Err(e) => json!({"text": text, "err": e}),
            };
            send(&p, UserEvent::TrSel(v));
        });
    }

    // ---- baca keras ----

    pub(crate) fn start_read(&mut self, i: usize, selection: bool) {
        let Some(t) = self.tabs.get(i) else { return };
        let Some(wv) = t.webview.as_ref() else { return };
        if !t.url.starts_with("http") {
            return;
        }
        self.read_seq += 1;
        let n = self.read_seq;
        // Hentikan sorotan di tab yang sebelumnya dibaca.
        if let Some(prev) = self.reading.and_then(|id| self.tab_index(id)) {
            if let Some(w) = self.tabs[prev].webview.as_ref() {
                let _ = w.evaluate_script("window.__ovnerRead && __ovnerRead.stop()");
            }
        }
        self.reading = Some(t.id);
        let _ = wv.evaluate_script(&format!("{READER_JS}\n;window.__ovnerRead && __ovnerRead.start({n},{selection})"));
    }

    pub(crate) fn on_read_text(&mut self, i: usize, msg: &Value) {
        if self.reading != Some(self.tabs[i].id) {
            return;
        }
        let s = self.settings.borrow();
        let v = json!({
            "open": true,
            "n": msg["n"], "start": msg["start"], "paras": msg["paras"], "lang": msg["lang"].as_str().filter(|l| !l.is_empty()).unwrap_or(&self.tabs[i].page_lang),
            "title": msg["title"],
            "engine": s.tts_engine, "voice": s.tts_voice, "rate": s.tts_rate,
            "azure": !s.tts_azure_key.is_empty(),
        });
        drop(s);
        let paras = msg["paras"].as_array().map(|a| a.len()).unwrap_or(0);
        log(&format!("[baca] baca keras dimulai: {paras} paragraf, suara {}: {}", self.settings.borrow().tts_engine, display_url(&self.tabs[i].url)));
        post_page(&self.toolbar, "reader", &v.to_string(), "window.ovner && ovner.reader");
    }

    fn azure_key(&self) -> String {
        let enc = self.settings.borrow().tts_azure_key.clone();
        base64::engine::general_purpose::STANDARD
            .decode(enc)
            .ok()
            .and_then(|b| win::protect(&b, false))
            .map(|b| String::from_utf8_lossy(&b).to_string())
            .unwrap_or_default()
    }

    /// Perintah bilah Baca keras dari toolbar.
    pub(crate) fn on_reader_cmd(&mut self, cmd: &str, msg: &Value) {
        match cmd {
            "read-mark" => {
                if let Some(wv) = self.reading.and_then(|id| self.tab_index(id)).and_then(|j| self.tabs[j].webview.as_ref()) {
                    let n = msg["n"].as_u64().unwrap_or(0);
                    let (p, a, l) = (msg["i"].as_u64().unwrap_or(0), msg["a"].as_u64().unwrap_or(0), msg["len"].as_u64().unwrap_or(0));
                    let _ = wv.evaluate_script(&format!("window.__ovnerRead && __ovnerRead.mark({n},{p},{a},{l})"));
                }
            }
            "read-stop" => {
                if let Some(wv) = self.reading.and_then(|id| self.tab_index(id)).and_then(|j| self.tabs[j].webview.as_ref()) {
                    let _ = wv.evaluate_script("window.__ovnerRead && __ovnerRead.stop()");
                }
                if self.reading.take().is_some() {
                    log("[baca] baca keras dihentikan");
                }
            }
            "tts-set" => {
                let mut s = self.settings.borrow_mut();
                let mut changed = false;
                if let Some(v) = msg["voice"].as_str() {
                    changed |= s.set("tts_voice", &json!(v));
                }
                if let Some(r) = msg["rate"].as_u64() {
                    changed |= s.set("tts_rate", &json!(r));
                }
                if let Some(e) = msg["engine"].as_str() {
                    changed |= s.set("tts_engine", &json!(e));
                }
                if changed {
                    s.save(data_dir());
                }
            }
            "tts-voices" => {
                let engine = msg["engine"].as_str().unwrap_or("edge").to_string();
                let (key, region) = (self.azure_key(), self.settings.borrow().tts_azure_region.clone());
                let p = self.proxy.clone();
                std::thread::spawn(move || {
                    let r = if engine == "azure" {
                        if key.is_empty() {
                            Err("Kunci Azure Speech belum diisi di Pengaturan".to_string())
                        } else {
                            tts::azure_voices(&key, &region)
                        }
                    } else {
                        tts::edge_voices()
                    };
                    let v = match r {
                        Ok(list) => json!({"engine": engine, "voices": list}),
                        Err(e) => json!({"engine": engine, "err": e}),
                    };
                    send(&p, UserEvent::TtsResult("ttsVoices", v));
                });
            }
            "tts" => {
                let engine = msg["engine"].as_str().unwrap_or("edge").to_string();
                let voice = msg["voice"].as_str().unwrap_or("").to_string();
                let text: String = msg["text"].as_str().unwrap_or("").chars().take(3000).collect();
                let rate = msg["rate"].as_u64().unwrap_or(100).clamp(50, 300) as u32;
                let req = msg["req"].clone();
                let (key, region) = if engine == "azure" { (self.azure_key(), self.settings.borrow().tts_azure_region.clone()) } else { (String::new(), String::new()) };
                let p = self.proxy.clone();
                std::thread::spawn(move || {
                    let r = if engine == "azure" { tts::azure_speak(&key, &region, &text, &voice, rate) } else { tts::edge_speak(&text, &voice, rate) };
                    let v = match r {
                        Ok(sp) => json!({"req": req, "mp3": base64::engine::general_purpose::STANDARD.encode(&sp.mp3), "marks": sp.marks}),
                        Err(e) => {
                            crate::log(&format!("[baca] GALAT: suara {engine} gagal: {e}"));
                            json!({"req": req, "err": e})
                        }
                    };
                    send(&p, UserEvent::TtsResult("ttsAudio", v));
                });
            }
            _ => {}
        }
    }
}

/// Keadaan terjemahan satu tab.
#[derive(Default, Clone)]
pub struct TrState {
    pub n: u64,
    pub on: bool,
    pub to: String,
    pub busy: bool,
    pub done: u64,
    pub total: u64,
    pub err: String,
    pub device: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_and_menu() {
        let s = qr_svg("https://example.com/").unwrap();
        assert!(s.starts_with("<svg") && s.contains("M4,4h1v1h-1z"));
        let page = json!({"kind": "page", "page": "https://example.com/", "sel": "", "editable": false});
        let items = ctx_items(false, &page);
        assert!(items.iter().any(|x| x.cmd == "source" && x.shortcut == "Ctrl+U"));
        assert!(items.iter().any(|x| x.children.iter().any(|c| c.cmd == "sleep-all")));
        let sel = json!({"kind": "text", "page": "https://example.com/", "sel": "halo", "editable": false});
        assert!(ctx_items(true, &sel).iter().any(|x| x.cmd == "read-sel"));
        let internal = json!({"kind": "page", "page": format!("{INTERNAL_PREFIX}settings/"), "sel": "", "editable": false});
        assert!(ctx_items(false, &internal).is_empty());
        let link = json!({"kind": "page", "page": "https://example.com/", "link": "https://example.com/x", "sel": "", "editable": false});
        let li = ctx_items(false, &link);
        assert!(li.iter().any(|x| x.cmd == "qr-link") && !li.iter().any(|x| x.cmd == "source"));
        let edit = json!({"kind": "page", "page": "https://example.com/", "editable": true});
        assert!(ctx_items(false, &edit).is_empty());
        assert_eq!(file_stem("a/b: c?"), "a b c");
    }
}
