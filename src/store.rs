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
    pub history: Vec<Visit>,
    pub bookmarks: Vec<Bookmark>,
    dirty_history: bool,
}

impl Store {
    pub fn load(data_dir: &Path) -> Store {
        let history_path = data_dir.join("history.json");
        let bookmarks_path = data_dir.join("bookmarks.json");
        Store {
            history: read(&history_path),
            bookmarks: read(&bookmarks_path),
            history_path,
            bookmarks_path,
            dirty_history: false,
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

    pub fn rename_bookmark(&mut self, url: &str, title: &str) {
        if let Some(b) = self.bookmarks.iter_mut().find(|b| b.url == url) {
            b.title = if title.trim().is_empty() { url.to_string() } else { title.trim().to_string() };
            write(&self.bookmarks_path, &self.bookmarks);
        }
    }

    pub fn remove_bookmark(&mut self, url: &str) {
        self.bookmarks.retain(|b| b.url != url);
        write(&self.bookmarks_path, &self.bookmarks);
    }

    /// Simpan riwayat bila berubah (dipanggil berkala dan saat keluar).
    pub fn flush(&mut self) {
        if self.dirty_history {
            write(&self.history_path, &self.history);
            self.dirty_history = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
