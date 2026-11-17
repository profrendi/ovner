//! Riwayat kunjungan (`history.json`) dan bookmark (`bookmarks.json`).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const MAX_HISTORY: usize = 5000;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Visit {
    pub url: String,
    pub title: String,
    /// Detik Unix.
    pub time: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Bookmark {
    pub url: String,
    pub title: String,
    pub added: u64,
    /// Folder di bilah bookmark, mis. "Kerja" atau "Kerja/HR"; kosong = langsung di bilah.
    /// "@other" = folder "Bookmark lain" di ujung kanan (seperti Chrome/Brave).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub folder: String,
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn read<T: for<'a> Deserialize<'a> + Default>(p: &Path) -> T {
    std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn write<T: Serialize>(p: &Path, v: &T) {
    if let Ok(t) = serde_json::to_string(v) {
        let _ = std::fs::write(p, t);
    }
}

pub struct Store {
    history_path: PathBuf,
    bookmarks_path: PathBuf,
    folders_path: PathBuf,
    icons_path: PathBuf,
    pub history: Vec<Visit>,
    pub bookmarks: Vec<Bookmark>,
    /// Folder yang dibuat sendiri (boleh kosong); folder lain tersirat dari bookmark.
    pub folders: Vec<String>,
    /// Ikon situs: host → data URL PNG kecil.
    pub icons: std::collections::HashMap<String, String>,
    dirty_history: bool,
    dirty_icons: bool,
}

/// Host tanpa "www." untuk kunci ikon.
/// Jenis gambar ikon dari isinya; None bila bukan gambar (mis. halaman galat HTML).
pub fn icon_mime(b: &[u8]) -> Option<&'static str> {
    if b.len() < 16 {
        return None;
    }
    if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("image/png")
    } else if b.starts_with(&[0, 0, 1, 0]) {
        Some("image/x-icon")
    } else if b.starts_with(b"GIF8") {
        Some("image/gif")
    } else if b.starts_with(&[0xFF, 0xD8]) {
        Some("image/jpeg")
    } else if b.starts_with(b"RIFF") && b.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        let head = String::from_utf8_lossy(&b[..b.len().min(512)]).to_ascii_lowercase();
        (head.contains("<svg") && !head.contains("<html")).then_some("image/svg+xml")
    }
}

pub fn icon_key(url: &str) -> String {
    url::Url::parse(url).ok().and_then(|u| u.host_str().map(|h| h.trim_start_matches("www.").to_string())).unwrap_or_default()
}

fn esc_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn unesc_html(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&amp;", "&")
}

/// Bookmark dari berkas HTML (format Netscape: ekspor Chrome, Edge, Brave, Firefox):
/// (judul, alamat, folder "A/B"). Folder "Bookmarks bar"/"Bilah bookmark" teratas diratakan.
pub fn parse_bookmarks_html(html: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut pending: Option<String> = None;
    let lower = html.to_ascii_lowercase();
    let mut i = 0;
    while let Some(off) = lower[i..].find('<') {
        let at = i + off;
        let rest = &lower[at..];
        if rest.starts_with("<h3") {
            let start = html[at..].find('>').map(|p| at + p + 1).unwrap_or(at);
            let end = lower[start..].find("</h3>").map(|p| start + p).unwrap_or(start);
            pending = Some(unesc_html(html[start..end].trim()));
            i = end;
        } else if rest.starts_with("<dl") {
            stack.push(pending.take().unwrap_or_default());
            i = at + 3;
        } else if rest.starts_with("</dl") {
            stack.pop();
            i = at + 4;
        } else if rest.starts_with("<a ") {
            let tag_end = html[at..].find('>').map(|p| at + p).unwrap_or(at);
            let tag = &html[at..tag_end];
            let href = tag
                .to_ascii_lowercase()
                .find("href=\"")
                .map(|p| p + 6)
                .and_then(|p| tag[p..].find('"').map(|q| unesc_html(&tag[p..p + q])))
                .unwrap_or_default();
            let close = lower[tag_end..].find("</a>").map(|p| tag_end + p).unwrap_or(tag_end);
            let title = unesc_html(html[(tag_end + 1).min(close)..close].trim());
            if href.starts_with("http://") || href.starts_with("https://") {
                // Lewati folder akar dokumen dan folder "bilah bookmark" bawaan browser.
                let parts: Vec<&String> = stack.iter().skip(1).filter(|f| !f.is_empty()).collect();
                let mut parts: Vec<String> = parts.iter().map(|f| f.replace('/', "\u{2215}")).collect();
                if parts.first().is_some_and(|f| {
                    let f = f.to_lowercase();
                    f.contains("bookmarks bar") || f.contains("bilah bookmark") || f.contains("favorites bar") || f == "bookmarks toolbar"
                }) {
                    parts.remove(0);
                } else if parts.first().is_some_and(|f| {
                    let f = f.to_lowercase();
                    f.contains("other bookmarks") || f.contains("bookmark lain") || f.contains("other favorites")
                }) {
                    parts[0] = "@other".into();
                }
                out.push((title, href, parts.join("/")));
            }
            i = close;
        } else {
            i = at + 1;
        }
    }
    out
}

impl Store {
    pub fn load(data_dir: &Path) -> Store {
        let history_path = data_dir.join("history.json");
        let bookmarks_path = data_dir.join("bookmarks.json");
        let folders_path = data_dir.join("bookmark-folders.json");
        let icons_path = data_dir.join("favicons.json");
        Store {
            history: read(&history_path),
            bookmarks: read(&bookmarks_path),
            folders: read(&folders_path),
            icons: read(&icons_path),
            history_path,
            bookmarks_path,
            folders_path,
            icons_path,
            dirty_history: false,
            dirty_icons: false,
        }
    }

    /// Catat kunjungan; kunjungan berulang ke alamat yang sama dalam semenit digabung.
    pub fn visit(&mut self, url: &str, title: &str) {
        if url.is_empty() || !(url.starts_with("http://") || url.starts_with("https://")) {
            return;
        }
        let t = now();
        if let Some(last) = self.history.last_mut() {
            if last.url == url && t.saturating_sub(last.time) < 60 {
                if !title.is_empty() {
                    last.title = title.to_string();
                }
                last.time = t;
                self.dirty_history = true;
                return;
            }
        }
        self.history.push(Visit { url: url.to_string(), title: title.to_string(), time: t });
        if self.history.len() > MAX_HISTORY {
            let cut = self.history.len() - MAX_HISTORY;
            self.history.drain(..cut);
        }
        self.dirty_history = true;
    }

    /// Perbarui judul kunjungan terakhir ke alamat ini (judul sering datang setelah navigasi).
    pub fn set_title(&mut self, url: &str, title: &str) {
        if title.is_empty() {
            return;
        }
        if let Some(v) = self.history.iter_mut().rev().find(|v| v.url == url) {
            if v.title != title {
                v.title = title.to_string();
                self.dirty_history = true;
            }
        }
    }

    pub fn remove_visits(&mut self, url: &str) {
        self.history.retain(|v| v.url != url);
        self.dirty_history = true;
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
        self.dirty_history = true;
    }

    /// Riwayat terbaru dulu, disaring dengan kata kunci (judul atau alamat).
    pub fn search_history(&self, query: &str, limit: usize) -> Vec<&Visit> {
        let q = query.trim().to_lowercase();
        self.history
            .iter()
            .rev()
            .filter(|v| q.is_empty() || v.url.to_lowercase().contains(&q) || v.title.to_lowercase().contains(&q))
            .take(limit)
            .collect()
    }

    pub fn is_bookmarked(&self, url: &str) -> bool {
        self.bookmarks.iter().any(|b| b.url == url)
    }

    /// Tambah/hapus bookmark. Mengembalikan true bila sekarang tersimpan.
    pub fn toggle_bookmark(&mut self, url: &str, title: &str) -> bool {
        if url.is_empty() {
            return false;
        }
        let added = if self.is_bookmarked(url) {
            self.bookmarks.retain(|b| b.url != url);
            false
        } else {
            let title = if title.is_empty() { url.to_string() } else { title.to_string() };
            self.bookmarks.push(Bookmark { url: url.to_string(), title, added: now(), folder: String::new() });
            true
        };
        write(&self.bookmarks_path, &self.bookmarks);
        added
    }

    /// Tambah banyak bookmark sekaligus (impor); alamat yang sudah ada dilewati.
    /// Mengembalikan jumlah yang ditambahkan.
    /// Bookmark yang sudah ada tanpa folder ikut mendapat foldernya (impor ulang dari 0.3).
    pub fn add_bookmarks(&mut self, items: &[(String, String, String)]) -> usize {
        let mut n = 0;
        let mut changed = false;
        for (title, url, folder) in items {
            if url.is_empty() {
                continue;
            }
            if let Some(b) = self.bookmarks.iter_mut().find(|b| &b.url == url) {
                if b.folder.is_empty() && !folder.is_empty() {
                    b.folder = folder.clone();
                    changed = true;
                }
                continue;
            }
            let title = if title.trim().is_empty() { url.clone() } else { title.trim().to_string() };
            self.bookmarks.push(Bookmark { url: url.clone(), title, added: now(), folder: folder.clone() });
            n += 1;
        }
        if n > 0 || changed {
            write(&self.bookmarks_path, &self.bookmarks);
        }
        n
    }

    /// Saran kolom alamat dari bookmark dan riwayat: (judul, alamat, bookmark?).
    /// Cocok bila kata kunci ada di judul atau alamat; urut: bookmark, awalan host, sering dikunjungi.
    pub fn suggest(&self, query: &str, limit: usize) -> Vec<(String, String, bool)> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return Vec::new();
        }
        let bare = |u: &str| u.trim_start_matches("https://").trim_start_matches("http://").trim_start_matches("www.").to_lowercase();
        let mut scored: Vec<(i64, String, String, bool)> = Vec::new();
        let mut push = |title: &str, url: &str, bm: bool, visits: i64| {
            let (t, b) = (title.to_lowercase(), bare(url));
            if !(t.contains(&q) || b.contains(&q)) {
                return;
            }
            let mut score = visits + if bm { 50 } else { 0 };
            if b.starts_with(&q) {
                score += 100;
            }
            if let Some(e) = scored.iter_mut().find(|e| e.2 == url) {
                e.0 = e.0.max(score);
                e.3 |= bm;
            } else {
                scored.push((score, title.to_string(), url.to_string(), bm));
            }
        };
        for b in &self.bookmarks {
            push(&b.title, &b.url, true, 0);
        }
        let mut counts: std::collections::HashMap<&str, (i64, &str)> = std::collections::HashMap::new();
        for v in &self.history {
            let e = counts.entry(v.url.as_str()).or_insert((0, v.title.as_str()));
            e.0 += 1;
            if !v.title.is_empty() {
                e.1 = v.title.as_str();
            }
        }
        for (url, (n, title)) in counts {
            push(title, url, false, n.min(40));
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.2.len().cmp(&b.2.len())));
        scored.into_iter().take(limit).map(|(_, t, u, bm)| (t, u, bm)).collect()
    }

    /// Ganti judul bookmark `url` yang ada di folder `folder`.
    pub fn rename_bookmark_in(&mut self, url: &str, folder: &str, title: &str) {
        if let Some(b) = self.bookmarks.iter_mut().find(|b| b.url == url && b.folder == folder) {
            b.title = if title.trim().is_empty() { url.to_string() } else { title.trim().to_string() };
            write(&self.bookmarks_path, &self.bookmarks);
        }
    }

    pub fn rename_bookmark(&mut self, url: &str, title: &str) {
        if let Some(b) = self.bookmarks.iter_mut().find(|b| b.url == url) {
            b.title = if title.trim().is_empty() { url.to_string() } else { title.trim().to_string() };
            write(&self.bookmarks_path, &self.bookmarks);
        }
    }

    /// Hapus satu bookmark: alamat `url` di folder `folder` saja (salinannya di folder lain tetap).
    pub fn remove_bookmark_in(&mut self, url: &str, folder: &str) {
        if let Some(i) = self.bookmarks.iter().position(|b| b.url == url && b.folder == folder) {
            self.bookmarks.remove(i);
            self.save_bookmarks();
        }
    }

    pub fn remove_bookmark(&mut self, url: &str) {
        self.bookmarks.retain(|b| b.url != url);
        write(&self.bookmarks_path, &self.bookmarks);
    }

    fn save_bookmarks(&self) {
        write(&self.bookmarks_path, &self.bookmarks);
        write(&self.folders_path, &self.folders);
    }

    /// Semua folder (dibuat sendiri + tersirat dari bookmark), termasuk induknya, sesuai urutan.
    pub fn all_folders(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut add = |f: &str| {
            let mut acc = String::new();
            for part in f.split('/').filter(|p| !p.is_empty()) {
                if !acc.is_empty() {
                    acc.push('/');
                }
                acc.push_str(part);
                if !out.contains(&acc) {
                    out.push(acc.clone());
                }
            }
        };
        for b in &self.bookmarks {
            add(&b.folder);
        }
        for f in &self.folders {
            add(f);
        }
        out
    }

    /// Tambah bookmark baru, atau perbarui judul/folder bila alamatnya sudah ada.
    pub fn add_bookmark(&mut self, title: &str, url: &str, folder: &str) {
        if url.trim().is_empty() {
            return;
        }
        let title = if title.trim().is_empty() { url.trim().to_string() } else { title.trim().to_string() };
        if let Some(b) = self.bookmarks.iter_mut().find(|b| b.url == url.trim()) {
            b.title = title;
            b.folder = folder.to_string();
        } else {
            self.bookmarks.push(Bookmark { url: url.trim().to_string(), title, added: now(), folder: folder.to_string() });
        }
        self.save_bookmarks();
    }

    /// Ubah judul, alamat, dan folder bookmark `old_url`.
    pub fn edit_bookmark(&mut self, old_url: &str, title: &str, url: &str, folder: &str) {
        let url = url.trim();
        if url.is_empty() {
            return;
        }
        if url != old_url {
            self.bookmarks.retain(|b| b.url != url);
        }
        if let Some(b) = self.bookmarks.iter_mut().find(|b| b.url == old_url) {
            b.url = url.to_string();
            b.title = if title.trim().is_empty() { url.to_string() } else { title.trim().to_string() };
            b.folder = folder.to_string();
            self.save_bookmarks();
        }
    }

    pub fn add_folder(&mut self, path: &str) {
        let p = path.trim_matches('/').to_string();
        if !p.is_empty() && !self.all_folders().contains(&p) {
            self.folders.push(p);
            self.save_bookmarks();
        }
    }

    /// Ganti nama folder (dan semua isinya ikut pindah).
    pub fn rename_folder(&mut self, path: &str, name: &str) {
        let name = name.trim().replace('/', "\u{2215}");
        if name.is_empty() || path.is_empty() || path == "@other" {
            return;
        }
        let new = match path.rsplit_once('/') {
            Some((parent, _)) => format!("{parent}/{name}"),
            None => name,
        };
        let swap = |f: &mut String| {
            if f == path {
                *f = new.clone();
            } else if let Some(rest) = f.strip_prefix(&format!("{path}/")) {
                *f = format!("{new}/{rest}");
            }
        };
        self.bookmarks.iter_mut().for_each(|b| swap(&mut b.folder));
        self.folders.iter_mut().for_each(swap);
        self.save_bookmarks();
    }

    /// Hapus folder beserta isinya.
    pub fn remove_folder(&mut self, path: &str) {
        let inside = |f: &str| f == path || f.starts_with(&format!("{path}/"));
        self.bookmarks.retain(|b| !inside(&b.folder));
        self.folders.retain(|f| !inside(f));
        self.save_bookmarks();
    }

    /// Pindahkan bookmark (`url`) atau folder (`folder`) ke folder `dest` ("" = bilah bookmark),
    /// tepat sebelum bookmark `before_url` atau folder `before_folder` di sana; tanpa keduanya
    /// diletakkan di akhir folder itu. Dipakai untuk seret-dan-lepas seperti di Chrome/Brave.
    pub fn move_item(&mut self, url: Option<&str>, folder: Option<&str>, dest: &str, before_url: Option<&str>, before_folder: Option<&str>) {
        self.relocate(url, None, folder, dest, before_url, before_folder, false);
    }

    /// Seperti [`Store::move_item`], tetapi bisa memilih bookmark di folder tertentu (`from`, untuk
    /// alamat yang ada di beberapa folder) dan bisa menyalin (`copy`) alih-alih memindah.
    #[allow(clippy::too_many_arguments)]
    pub fn relocate(&mut self, url: Option<&str>, from: Option<&str>, folder: Option<&str>, dest: &str, before_url: Option<&str>, before_folder: Option<&str>, copy: bool) {
        let dest = dest.trim_matches('/');
        let inside = |f: &str, p: &str| f == p || f.starts_with(&format!("{p}/"));
        // Ambil yang dipindah (urutan relatifnya tetap) dan tentukan foldernya yang baru.
        let mut moving: Vec<Bookmark> = Vec::new();
        match (url, folder) {
            (Some(u), _) => {
                let Some(i) = self.bookmarks.iter().position(|b| b.url == u && from.is_none_or(|f| b.folder == f)) else { return };
                let mut b = if copy { self.bookmarks[i].clone() } else { self.bookmarks.remove(i) };
                if copy && b.folder == dest {
                    return; // salinan di folder yang sama tidak berguna
                }
                b.folder = dest.to_string();
                moving.push(b);
            }
            (None, Some(p)) => {
                let p = p.trim_matches('/');
                if p.is_empty() || p == "@other" || inside(dest, p) {
                    return; // tidak bisa masuk ke dirinya sendiri
                }
                let name = p.rsplit('/').next().unwrap_or(p);
                let new = if dest.is_empty() { name.to_string() } else { format!("{dest}/{name}") };
                let re = |f: &str| -> String { format!("{new}{}", &f[p.len()..]) };
                if copy && new == p {
                    return;
                }
                let mut keep = Vec::new();
                for b in std::mem::take(&mut self.bookmarks) {
                    if inside(&b.folder, p) {
                        let mut c = b.clone();
                        c.folder = re(&b.folder);
                        moving.push(c);
                        if copy {
                            keep.push(b);
                        }
                    } else {
                        keep.push(b);
                    }
                }
                self.bookmarks = keep;
                let mut fs = Vec::new();
                for f in std::mem::take(&mut self.folders) {
                    if inside(&f, p) {
                        fs.push(re(&f));
                        if copy {
                            self.folders.push(f);
                        }
                    } else {
                        self.folders.push(f);
                    }
                }
                // Folder (kosong) tetap tercatat di tempat barunya.
                for f in fs {
                    if !self.folders.contains(&f) {
                        self.folders.push(f);
                    }
                }
                if moving.is_empty() {
                    self.save_bookmarks();
                    return;
                }
            }
            _ => return,
        }
        let at = if let Some(bu) = before_url {
            self.bookmarks.iter().position(|b| b.url == bu)
        } else if let Some(bf) = before_folder {
            self.bookmarks.iter().position(|b| inside(&b.folder, bf))
        } else {
            None
        };
        let at = at.unwrap_or_else(|| {
            // Akhir folder tujuan (setelah isi terakhirnya), atau akhir daftar.
            self.bookmarks
                .iter()
                .rposition(|b| if dest.is_empty() { !b.folder.starts_with("@other") } else { inside(&b.folder, dest) })
                .map(|i| i + 1)
                .unwrap_or(self.bookmarks.len())
        });
        for (k, b) in moving.into_iter().enumerate() {
            self.bookmarks.insert(at + k, b);
        }
        self.save_bookmarks();
    }

    /// Urutkan isi satu folder berdasarkan nama (seperti "Sort by name" Brave).
    pub fn sort_folder(&mut self, folder: &str) {
        let mut idx: Vec<usize> = (0..self.bookmarks.len()).filter(|&i| self.bookmarks[i].folder == folder).collect();
        let mut sorted: Vec<Bookmark> = idx.iter().map(|&i| self.bookmarks[i].clone()).collect();
        sorted.sort_by_key(|b| b.title.to_lowercase());
        idx.sort();
        for (slot, b) in idx.into_iter().zip(sorted) {
            self.bookmarks[slot] = b;
        }
        self.folders.sort_by_key(|f| f.to_lowercase());
        self.save_bookmarks();
    }

    /// Ekspor ke HTML format Netscape (bisa diimpor Brave, Chrome, Edge, Firefox).
    pub fn export_html(&self) -> String {
        fn emit(store: &Store, path: &str, depth: usize, out: &mut String) {
            let pad = "    ".repeat(depth);
            let prefix = if path.is_empty() { String::new() } else { format!("{path}/") };
            let mut subs: Vec<String> = Vec::new();
            for f in store.all_folders() {
                if let Some(rest) = f.strip_prefix(&prefix) {
                    if !rest.contains('/') && !rest.is_empty() && rest != "@other" && !subs.contains(&rest.to_string()) {
                        subs.push(rest.to_string());
                    }
                }
            }
            for s in subs {
                out.push_str(&format!("{pad}<DT><H3>{}</H3>\n{pad}<DL><p>\n", esc_html(&s.replace('\u{2215}', "/"))));
                emit(store, &format!("{prefix}{s}"), depth + 1, out);
                out.push_str(&format!("{pad}</DL><p>\n"));
            }
            for b in store.bookmarks.iter().filter(|b| b.folder == path) {
                out.push_str(&format!("{pad}<DT><A HREF=\"{}\" ADD_DATE=\"{}\">{}</A>\n", esc_html(&b.url), b.added, esc_html(&b.title)));
            }
        }
        let mut body = String::new();
        body.push_str("    <DT><H3 PERSONAL_TOOLBAR_FOLDER=\"true\">Bookmarks bar</H3>\n    <DL><p>\n");
        emit(self, "", 2, &mut body);
        body.push_str("    </DL><p>\n");
        if self.all_folders().iter().any(|f| f == "@other") {
            body.push_str("    <DT><H3>Other bookmarks</H3>\n    <DL><p>\n");
            emit(self, "@other", 2, &mut body);
            body.push_str("    </DL><p>\n");
        }
        format!(
            "<!DOCTYPE NETSCAPE-Bookmark-file-1>\n<META HTTP-EQUIV=\"Content-Type\" CONTENT=\"text/html; charset=UTF-8\">\n<TITLE>Bookmarks</TITLE>\n<H1>Bookmarks</H1>\n<DL><p>\n{body}</DL><p>\n"
        )
    }

    /// Simpan ikon situs (PNG) untuk host halaman ini.
    /// Simpan ikon hasil unduhan (favicon.ico/PNG/SVG) untuk host tertentu.
    pub fn set_icon_host(&mut self, host: &str, bytes: &[u8]) -> bool {
        use base64::Engine as _;
        let Some(mime) = icon_mime(bytes) else { return false };
        let key = host.trim_start_matches("www.").to_ascii_lowercase();
        if key.is_empty() || bytes.len() > 64 * 1024 || self.icons.contains_key(&key) {
            return false;
        }
        self.icons.insert(key, format!("data:{mime};base64,{}", base64::prelude::BASE64_STANDARD.encode(bytes)));
        self.dirty_icons = true;
        true
    }

    /// Host bookmark yang belum punya ikon.
    pub fn hosts_without_icon(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for b in &self.bookmarks {
            let k = icon_key(&b.url);
            if !k.is_empty() && !self.icons.contains_key(&k) && !out.contains(&k) {
                out.push(k);
            }
        }
        out
    }

    pub fn icon_count(&self) -> usize {
        self.icons.len()
    }

    pub fn set_icon(&mut self, url: &str, png: &[u8]) {
        use base64::Engine as _;
        let key = icon_key(url);
        if key.is_empty() || png.is_empty() || png.len() > 24 * 1024 {
            return;
        }
        let data = format!("data:image/png;base64,{}", base64::prelude::BASE64_STANDARD.encode(png));
        if self.icons.get(&key) != Some(&data) {
            self.icons.insert(key, data);
            self.dirty_icons = true;
        }
    }

    /// Ikon untuk daftar alamat (host yang punya ikon saja).
    pub fn icons_for<'a>(&self, urls: impl Iterator<Item = &'a str>) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        for u in urls {
            let k = icon_key(u);
            if let Some(d) = self.icons.get(&k) {
                m.insert(k, serde_json::Value::String(d.clone()));
            }
        }
        m
    }

    /// Simpan riwayat dan ikon bila berubah (dipanggil berkala dan saat keluar).
    pub fn flush(&mut self) {
        if self.dirty_history {
            write(&self.history_path, &self.history);
            self.dirty_history = false;
        }
        if self.dirty_icons {
            write(&self.icons_path, &self.icons);
            self.dirty_icons = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_and_html_roundtrip() {
        let d = std::env::temp_dir().join(format!("ovner-store-html-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let mut s = Store::load(&d);
        s.add_bookmark("Google", "https://google.com/", "");
        s.add_bookmark("HR", "https://hr.kantor.id/", "Kerja");
        s.add_bookmark("Absen", "https://absen.kantor.id/", "Kerja/Harian");
        s.add_folder("Kosong");
        assert_eq!(s.all_folders(), vec!["Kerja", "Kerja/Harian", "Kosong"]);
        s.rename_folder("Kerja", "Kantor & HR");
        assert_eq!(s.bookmarks[2].folder, "Kantor & HR/Harian");
        let html = s.export_html();
        assert!(html.contains("<H3>Kantor &amp; HR</H3>") && html.contains("HREF=\"https://absen.kantor.id/\""));
        let parsed = parse_bookmarks_html(&html);
        assert_eq!(parsed.len(), 3);
        assert!(parsed.contains(&("Absen".into(), "https://absen.kantor.id/".into(), "Kantor & HR/Harian".into())));
        assert!(parsed.contains(&("Google".into(), "https://google.com/".into(), String::new())));
        s.edit_bookmark("https://hr.kantor.id/", "HR baru", "https://hr2.kantor.id/", "Kosong");
        assert_eq!((s.bookmarks[1].url.as_str(), s.bookmarks[1].folder.as_str()), ("https://hr2.kantor.id/", "Kosong"));
        s.remove_folder("Kantor & HR");
        assert_eq!(s.bookmarks.len(), 2);
        std::fs::remove_dir_all(&d).unwrap();
    }

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("ovner-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn history_and_bookmarks_roundtrip() {
        let d = tmp();
        let mut s = Store::load(&d);
        s.visit("https://a.id/", "A");
        s.visit("https://a.id/", "A lagi"); // digabung
        s.visit("https://b.id/", "");
        s.set_title("https://b.id/", "B");
        assert_eq!(s.history.len(), 2);
        assert_eq!(s.history[0].title, "A lagi");
        assert_eq!(s.search_history("b", 10).len(), 1);
        s.visit("ovner://settings/", "x");
        assert_eq!(s.history.len(), 2);
        s.flush();

        assert!(s.toggle_bookmark("https://a.id/", "A"));
        assert!(s.is_bookmarked("https://a.id/"));
        s.rename_bookmark("https://a.id/", "Situs A");

        assert_eq!(s.add_bookmarks(&[("B".into(), "https://b.id/".into(), "Kerja".into()), ("A".into(), "https://a.id/".into(), String::new())]), 1);
        let sug = s.suggest("b.i", 5);
        assert_eq!(sug[0].1, "https://b.id/");
        assert!(sug[0].2, "bookmark diutamakan");
        assert!(s.suggest("", 5).is_empty());
        s.remove_bookmark("https://b.id/");

        let again = Store::load(&d);
        assert_eq!(again.history, s.history);
        assert_eq!(again.bookmarks[0].title, "Situs A");
        let mut again = again;
        assert!(!again.toggle_bookmark("https://a.id/", ""));
        again.remove_visits("https://a.id/");
        assert_eq!(again.history.len(), 1);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn drag_and_drop_moves() {
        let d = tmp();
        let mut s = Store::load(&d);
        for (t, f) in [("a", ""), ("b", ""), ("k1", "Kerja"), ("k2", "Kerja/HR"), ("c", "")] {
            s.add_bookmark(t, &format!("https://{t}.id/"), f);
        }
        let order = |s: &Store| s.bookmarks.iter().map(|b| format!("{}@{}", b.title, b.folder)).collect::<Vec<_>>().join(",");
        // c ke depan bilah.
        s.move_item(Some("https://c.id/"), None, "", Some("https://a.id/"), None);
        assert_eq!(order(&s), "c@,a@,b@,k1@Kerja,k2@Kerja/HR");
        // a masuk folder Kerja (di akhir isinya).
        s.move_item(Some("https://a.id/"), None, "Kerja", None, None);
        assert_eq!(order(&s), "c@,b@,k1@Kerja,k2@Kerja/HR,a@Kerja");
        // Folder Kerja/HR keluar ke bilah, sebelum b.
        s.move_item(None, Some("Kerja/HR"), "", Some("https://b.id/"), None);
        assert_eq!(order(&s), "c@,k2@HR,b@,k1@Kerja,a@Kerja");
        // Folder tidak bisa masuk ke dirinya sendiri.
        s.move_item(None, Some("Kerja"), "Kerja", None, None);
        assert_eq!(order(&s), "c@,k2@HR,b@,k1@Kerja,a@Kerja");
        // Folder Kerja ke depan, sebelum folder HR.
        s.move_item(None, Some("Kerja"), "", None, Some("HR"));
        assert_eq!(order(&s), "c@,k1@Kerja,a@Kerja,k2@HR,b@");
        // Salin (bukan pindah) a ke HR, lalu hapus hanya salinan di Kerja.
        s.relocate(Some("https://a.id/"), Some("Kerja"), None, "HR", None, None, true);
        assert_eq!(order(&s), "c@,k1@Kerja,a@Kerja,k2@HR,a@HR,b@");
        s.remove_bookmark_in("https://a.id/", "Kerja");
        assert_eq!(order(&s), "c@,k1@Kerja,k2@HR,a@HR,b@");
        // Salin folder HR ke dalam Kerja.
        s.relocate(None, None, Some("HR"), "Kerja", None, None, true);
        assert_eq!(order(&s), "c@,k1@Kerja,k2@Kerja/HR,a@Kerja/HR,k2@HR,a@HR,b@");
        let _ = std::fs::remove_dir_all(&d);
    }
}
