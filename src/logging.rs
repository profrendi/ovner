//! Log Ovner: `<data>/ovner.log` dengan waktu setempat, tingkat (INFO/PERINGATAN/GALAT), dan
//! kategori fitur, mis. `2026-11-19 13:31:05.120 INFO  [pdf] PDF ditampilkan di tab …`.
//!
//! - Ditulis oleh thread sendiri (UI tidak pernah menunggu disk); `flush` dipanggil saat keluar
//!   dan saat panic supaya baris terakhir tidak hilang.
//! - Riwayat: 5 berkas × 1 MB (ovner.log, ovner.1.log … ovner.4.log), yang tertua dibuang.
//! - Kategori dan tingkat ditebak dari isi pesan (ratusan pemanggil `log()` lama tetap jalan);
//!   pesan baru boleh menulis kategori sendiri di depan: "[setelan] …", dan tingkat lewat
//!   "GALAT: …" / "PERINGATAN: …".

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::Duration;

pub const MAX_BYTES: u64 = 1024 * 1024;
pub const KEEP: usize = 5;

/// Semua kategori, untuk penyaring di halaman Diagnostik.
pub const CATEGORIES: &[&str] = &[
    "sistem", "setelan", "tab", "tidur", "ram", "ukur", "pdf", "unduh", "video", "subtitle", "codec", "shields", "update", "default", "sesi", "diag", "crash", "umum",
];

enum Msg {
    Line(String),
    Flush(mpsc::Sender<()>),
}

static WRITER: OnceLock<Mutex<mpsc::Sender<Msg>>> = OnceLock::new();
static DIR: OnceLock<PathBuf> = OnceLock::new();

/// Mulai penulis log di `dir`. Sebelum dipanggil (atau di uji), `log` hanya menulis ke stderr.
pub fn init(dir: &Path) {
    if DIR.set(dir.to_path_buf()).is_err() {
        return;
    }
    // Berkas lama (sampai 0.8.21) ikut riwayat.
    let old = dir.join("ovner.old.log");
    if old.exists() && !dir.join("ovner.1.log").exists() {
        let _ = std::fs::rename(&old, dir.join("ovner.1.log"));
    } else {
        let _ = std::fs::remove_file(&old);
    }
    let (tx, rx) = mpsc::channel::<Msg>();
    let d = dir.to_path_buf();
    let spawned = std::thread::Builder::new().name("ovner-log".into()).spawn(move || {
        while let Ok(first) = rx.recv() {
            let mut lines = Vec::new();
            let mut acks = Vec::new();
            let mut take = |m: Msg| match m {
                Msg::Line(l) => lines.push(l),
                Msg::Flush(a) => acks.push(a),
            };
            take(first);
            while let Ok(m) = rx.try_recv() {
                take(m);
            }
            if !lines.is_empty() {
                append(&d, &lines);
            }
            for a in acks {
                let _ = a.send(());
            }
        }
    });
    if spawned.is_ok() {
        let _ = WRITER.set(Mutex::new(tx));
    }
}

/// Tunggu semua baris tertulis (paling lama 1 detik).
pub fn flush() {
    let Some(w) = WRITER.get() else { return };
    let (tx, rx) = mpsc::channel();
    if w.lock().map(|w| w.send(Msg::Flush(tx)).is_ok()).unwrap_or(false) {
        let _ = rx.recv_timeout(Duration::from_secs(1));
    }
}

/// Berkas log, terbaru dulu.
pub fn files(dir: &Path) -> Vec<PathBuf> {
    let mut v = vec![dir.join("ovner.log")];
    v.extend((1..KEEP).map(|i| dir.join(format!("ovner.{i}.log"))));
    v.into_iter().filter(|p| p.exists()).collect()
}

fn rotate(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(format!("ovner.{}.log", KEEP - 1)));
    for i in (1..KEEP - 1).rev() {
        let _ = std::fs::rename(dir.join(format!("ovner.{i}.log")), dir.join(format!("ovner.{}.log", i + 1)));
    }
    let _ = std::fs::rename(dir.join("ovner.log"), dir.join("ovner.1.log"));
}

fn append(dir: &Path, lines: &[String]) {
    let p = dir.join("ovner.log");
    if std::fs::metadata(&p).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        rotate(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let mut buf = String::new();
        for l in lines {
            buf.push_str(l);
            buf.push('\n');
        }
        let _ = f.write_all(buf.as_bytes());
    }
}

/// Baris terbaru di memori (untuk uji --self-test dan halaman Diagnostik): (nomor urut, baris).
static RING: Mutex<std::collections::VecDeque<(u64, String)>> = Mutex::new(std::collections::VecDeque::new());
static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
const RING_MAX: usize = 3000;

/// Nomor urut baris log terakhir.
pub fn seq() -> u64 {
    SEQ.load(std::sync::atomic::Ordering::SeqCst)
}

/// Baris log sesudah nomor urut `after` (dari memori, paling banyak 3000 terakhir).
pub fn since(after: u64) -> Vec<String> {
    RING.lock().map(|r| r.iter().filter(|(n, _)| *n > after).map(|(_, l)| l.clone()).collect()).unwrap_or_default()
}

/// Tulis satu pesan. Tanpa `init` (mis. saat uji) pesan tidak ditulis ke berkas.
pub fn log(msg: &str) {
    let line = format_line(&crate::win::local_time(), msg);
    if let Ok(mut r) = RING.lock() {
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        r.push_back((n, line.clone()));
        while r.len() > RING_MAX {
            r.pop_front();
        }
    }
    // Penghitung harian per kategori (dan galat per kategori) untuk pengukuran.
    let (cat, rest) = split_category(msg);
    crate::metrics::count(&format!("log_{cat}"), 1);
    match split_level(rest).0 {
        "GALAT" => crate::metrics::count(&format!("galat_{cat}"), 1),
        "PERINGATAN" => crate::metrics::count(&format!("peringatan_{cat}"), 1),
        _ => {}
    }
    if cfg!(debug_assertions) {
        eprintln!("[ovner] {line}");
    }
    let Some(w) = WRITER.get() else { return };
    if let Ok(w) = w.lock() {
        let _ = w.send(Msg::Line(line));
    }
}

/// Tulis langsung (panic: thread penulis mungkin tidak sempat jalan).
pub fn log_now(msg: &str) {
    flush();
    if let Some(d) = DIR.get() {
        append(d, &[format_line(&crate::win::local_time(), msg)]);
    }
}

/// "2026-11-19 13:31:05.120 INFO  [pdf] …"
pub fn format_line(time: &str, msg: &str) -> String {
    let (cat, rest) = split_category(msg);
    let (level, rest) = split_level(rest);
    format!("{time} {level:<10} [{cat}] {rest}")
}

/// Kategori tertulis "[nama] …" (harus salah satu CATEGORIES) atau tebakan dari isi pesan.
fn split_category(msg: &str) -> (&'static str, &str) {
    if let Some(rest) = msg.strip_prefix('[') {
        if let Some((name, tail)) = rest.split_once("] ") {
            if let Some(c) = CATEGORIES.iter().find(|c| **c == name) {
                return (c, tail);
            }
        }
    }
    (categorize(msg), msg)
}

fn split_level(msg: &str) -> (&'static str, &str) {
    for (p, l) in [("GALAT: ", "GALAT"), ("PERINGATAN: ", "PERINGATAN"), ("INFO: ", "INFO")] {
        if let Some(rest) = msg.strip_prefix(p) {
            return (l, rest);
        }
    }
    (level_of(msg), msg)
}

/// Tebak kategori dari isi pesan (urutan penting: yang lebih khusus dulu).
pub fn categorize(msg: &str) -> &'static str {
    let m = msg.to_lowercase();
    let has = |w: &[&str]| w.iter().any(|x| m.contains(x));
    if has(&["panic", "crash"]) {
        "crash"
    } else if has(&["subtitle"]) {
        "subtitle"
    } else if has(&["pdf"]) {
        "pdf"
    } else if has(&["codec", "dekoder"]) {
        "codec"
    } else if has(&["unduh video", "telusuri video", "hasil video", "yt-dlp", "ffmpeg", "deno"]) {
        "video"
    } else if has(&["browser default"]) {
        "default"
    } else if has(&["cek update", "update ", "webview2 terbaru"]) {
        "update"
    } else if has(&["tab ditidurkan", "tidurkan", "tidur ", "ditidurkan"]) {
        "tidur"
    } else if has(&["ram", "rapikan", "efisiensi", "penjaga tab", "memori", "mode hemat", "membengkak"]) {
        "ram"
    } else if has(&["unduh", "mesin unduh", "menunggu server", "server unduhan"]) {
        "unduh"
    } else if has(&["pemblokir", "filter", "anubis", "shields", "cookie", "doh", "dns"]) {
        "shields"
    } else if has(&["pengaturan", "setelan", "preset"]) {
        "setelan"
    } else if has(&["daftar", "sesi"]) {
        "sesi"
    } else if has(&["diagnostik", "uji "]) {
        "diag"
    } else if has(&["jendela", "tab ", "pop-up", "toolbar", "proses tab"]) {
        "tab"
    } else {
        "umum"
    }
}

/// Tebak tingkat dari isi pesan.
pub fn level_of(msg: &str) -> &'static str {
    let m = msg.to_lowercase();
    let has = |w: &[&str]| w.iter().any(|x| m.contains(x));
    if has(&["panic", "galat", "gagal", "error", "tidak bisa", "crash"]) && !has(&["tidak ada yang baru"]) {
        "GALAT"
    } else if has(&["peringatan", "ditolak", "429", "memantul", "lambat", "tidak menjawab", "hampir habis", "membengkak", "dibatalkan", "dihentikan"]) {
        "PERINGATAN"
    } else {
        "INFO"
    }
}

/// Tingkat sebuah baris log berformat baru ("" untuk baris lama).
pub fn line_level(line: &str) -> &str {
    line.split_whitespace().nth(2).filter(|l| matches!(*l, "INFO" | "PERINGATAN" | "GALAT")).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_get_category_and_level() {
        let t = "2026-11-19 13:31:05.120";
        assert_eq!(format_line(t, "PDF ditampilkan di tab: 40 KB"), "2026-11-19 13:31:05.120 INFO       [pdf] PDF ditampilkan di tab: 40 KB");
        assert_eq!(format_line(t, "[setelan] diubah: bloat_mb: 700 → 300"), "2026-11-19 13:31:05.120 INFO       [setelan] diubah: bloat_mb: 700 → 300");
        assert!(format_line(t, "subtitle gagal: HTTP 429").contains("GALAT      [subtitle]"));
        assert!(format_line(t, "hasil rapikan 1 menit kemudian: memantul: gpu").contains("PERINGATAN [ram]"));
        assert!(format_line(t, "GALAT: [x] apa").contains("GALAT      [umum] [x] apa"));
        assert!(format_line(t, "tab ditidurkan: youtube.com").contains("[tidur]"));
        assert!(format_line(t, "unduhan selesai: a.zip").contains("[unduh]"));
        assert!(format_line(t, "PANIC: oops").contains("GALAT      [crash]"));
        assert_eq!(line_level(&format_line(t, "x gagal")), "GALAT");
        assert_eq!(line_level("1795053184 lama"), "");
    }

    #[test]
    fn rotation_keeps_five() {
        let d = std::env::temp_dir().join(format!("ovner-logrot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for _ in 0..7 {
            std::fs::write(d.join("ovner.log"), vec![b'x'; (MAX_BYTES + 1) as usize]).unwrap();
            append(&d, &["baru".into()]);
        }
        assert_eq!(files(&d).len(), 5);
        assert_eq!(std::fs::read_to_string(d.join("ovner.log")).unwrap(), "baru\n");
        let _ = std::fs::remove_dir_all(&d);
    }
}
