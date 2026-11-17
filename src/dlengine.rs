//! Mesin unduh multi-koneksi Ovner (terinspirasi Mister Download Manager dan IDM).
//!
//! Cara kerja:
//! 1. **Uji server**: minta `Range: bytes=0-0`. Hanya server yang menjawab `206` dengan ukuran
//!    lengkap yang diambil alih; selain itu unduhan tetap dikerjakan WebView2 seperti biasa.
//! 2. **Bagi berkas** menjadi beberapa bagian, tiap bagian diunduh koneksinya sendiri dan
//!    ditulis langsung ke posisinya di berkas `.ovnerpart`.
//! 3. **Seimbangkan sampai byte terakhir**: koneksi yang selesai lebih dulu membelah bagian
//!    terbesar yang masih berjalan dan mengambil separuh belakangnya, jadi tidak ada koneksi
//!    lambat yang menahan akhir unduhan.
//! 4. **Tahan putus**: bagian yang gagal diulang (sampai 6 kali, jeda makin lama) dari posisi
//!    terakhirnya; unduhan bisa dijeda dan dilanjutkan.
//!
//! Permintaan memakai WinHTTP (ikut setelan proxy Windows) dengan cookie, user agent, dan
//! referrer dari tab asal supaya situs yang perlu login tetap mengizinkan.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Bagian terkecil yang masih dibelah untuk koneksi lain.
const MIN_SPLIT: u64 = 512 * 1024;
/// Percobaan gagal berturut-turut per bagian sebelum menyerah (direset setiap kali ada data).
const MAX_FAILS: u32 = 20;

/// Permintaan HTTP: alamat dan tajuk tambahan (Cookie, Referer). User agent terpisah.
#[derive(Clone, Debug, Default)]
pub struct Request {
    pub url: String,
    pub user_agent: String,
    pub headers: Vec<(String, String)>,
    /// Penanda versi berkas (ETag kuat / Last-Modified) untuk tajuk `If-Range`, dikirim bila
    /// server meminta prasyarat (HTTP 428, mis. CDN Bunny untuk mirror Parrot).
    pub if_range: Option<String>,
}

/// Hasil uji server.
#[derive(Clone, Debug, PartialEq)]
pub struct Probe {
    pub status: u16,
    /// Ukuran lengkap berkas (dari Content-Range).
    pub total: u64,
    /// Alamat akhir setelah pengalihan: semua bagian memakai mirror yang sama.
    pub url: Option<String>,
    /// ETag kuat atau Last-Modified dari server (untuk If-Range).
    pub validator: Option<String>,
}

/// Kabar dari unduhan ke Ovner.
#[derive(Clone, Debug, PartialEq)]
pub enum Update {
    Progress { received: u64, total: u64, speed: u64, conns: usize, paused: bool },
    Done { ok: bool, path: String, received: u64, error: Option<String> },
}

/// Jawaban HTTP sebagian (tajuk penting saja).
#[derive(Debug, Default)]
struct Head {
    status: u16,
    /// Alamat akhir setelah pengalihan (mirror yang benar-benar melayani).
    final_url: Option<String>,
    etag: Option<String>,
    last_modified: Option<String>,
    /// Ukuran lengkap dari `Content-Range: bytes a-b/TOTAL`.
    range_total: Option<u64>,
    /// Awal rentang yang benar-benar dikirim server.
    range_start: Option<u64>,
}

/// Isi respons boleh ditulis: tanpa Range cukup 200/206; dengan Range harus 206 dari posisi yang diminta.
fn body_ok(h: &Head, range: Option<(u64, u64)>) -> bool {
    match range {
        Some((a, _)) => h.status == 206 && h.range_start == Some(a),
        None => h.status == 200 || h.status == 206,
    }
}

/// Baca tajuk mentah (CRLF) menjadi [`Head`].
fn parse_head(raw: &str) -> Head {
    let mut h = Head::default();
    for (n, line) in raw.split("\r\n").enumerate() {
        if n == 0 {
            h.status = line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            continue;
        }
        let Some((k, v)) = line.split_once(':') else { continue };
        if k.trim().eq_ignore_ascii_case("etag") {
            h.etag = Some(v.trim().to_string());
        } else if k.trim().eq_ignore_ascii_case("last-modified") {
            h.last_modified = Some(v.trim().to_string());
        }
        if k.trim().eq_ignore_ascii_case("content-range") {
            // bytes 0-0/12345
            let v = v.trim().trim_start_matches("bytes").trim();
            if let Some((range, total)) = v.split_once('/') {
                h.range_total = total.trim().parse().ok();
                h.range_start = range.split('-').next().and_then(|s| s.trim().parse().ok());
            }
        }
    }
    h
}

/// Uji apakah server mendukung unduhan bersegmen.
pub fn probe(req: &Request) -> Result<Probe, String> {
    let mut got = 0usize;
    let head = http_get(req, Some((0, 0)), &mut |d| {
        got += d.len();
        got < 4096
    })?;
    match (head.status, head.range_total, head.range_start) {
        (206, Some(total), Some(0)) if total > 0 => Ok(Probe {
            status: 206,
            total,
            url: head.final_url.clone().filter(|u| u.starts_with("http") && *u != req.url),
            // If-Range hanya menerima ETag kuat (bukan W/"...") atau tanggal.
            validator: head.etag.clone().filter(|e| !e.starts_with("W/")).or(head.last_modified.clone()),
        }),
        (s, _, _) => Err(format!("server tidak mendukung unduhan bersegmen (HTTP {s})")),
    }
}

struct Seg {
    /// Awal bagian (untuk peta bagian di kotak unduhan).
    start: u64,
    /// Batas akhir (tidak termasuk). Bisa mengecil saat bagian ini dibelah.
    end: u64,
    /// Posisi berikutnya yang akan ditulis.
    pos: u64,
    busy: bool,
    fails: u32,
}

const RUN: u8 = 0;
const PAUSED: u8 = 1;
const CANCELLED: u8 = 2;

struct Shared {
    req: Request,
    total: u64,
    /// Jumlah koneksi; dikurangi bila server mulai menolak (pembatasan).
    conns: AtomicUsize,
    /// Server meminta prasyarat (428): kirim If-Range mulai sekarang.
    precond: std::sync::atomic::AtomicBool,
    segs: Mutex<Vec<Seg>>,
    file: File,
    received: AtomicU64,
    state: AtomicU8,
    active: AtomicUsize,
    error: Mutex<Option<String>>,
}

/// Unduhan yang sedang berjalan; bisa dijeda, dilanjutkan, dan dibatalkan.
#[derive(Clone)]
pub struct Job {
    shared: Arc<Shared>,
}

impl Job {
    pub fn pause(&self) {
        let _ = self.shared.state.compare_exchange(RUN, PAUSED, Ordering::SeqCst, Ordering::SeqCst);
    }
    pub fn resume(&self) {
        let _ = self.shared.state.compare_exchange(PAUSED, RUN, Ordering::SeqCst, Ordering::SeqCst);
    }
    pub fn cancel(&self) {
        self.shared.state.store(CANCELLED, Ordering::SeqCst);
    }
    pub fn paused(&self) -> bool {
        self.shared.state.load(Ordering::SeqCst) == PAUSED
    }
    /// Peta bagian: (awal, posisi, akhir, sedang diunduh) — seperti kotak unduhan IDM.
    pub fn segments(&self) -> Vec<(u64, u64, u64, bool)> {
        let mut v: Vec<(u64, u64, u64, bool)> = self.shared.segs.lock().unwrap().iter().map(|s| (s.start, s.pos, s.end, s.busy)).collect();
        v.sort_by_key(|x| x.0);
        v
    }
}

/// Ambil isi teks sebuah alamat (mis. info lokasi server). Memblokir: panggil dari thread latar.
pub fn get_text(url: &str) -> Option<String> {
    let req = Request { url: url.to_string(), user_agent: "Ovner".into(), headers: vec![("Accept".into(), "application/json".into())], if_range: None };
    let mut body = Vec::new();
    let head = http_get(&req, None, &mut |d| {
        body.extend_from_slice(d);
        body.len() < 256 * 1024
    })
    .ok()?;
    (head.status == 200).then(|| String::from_utf8_lossy(&body).to_string())
}

/// Nama berkas yang belum dipakai: "laporan.pdf" → "laporan (1).pdf".
/// Folder kategori untuk berkas, seperti Internet Download Manager / Mister Download Manager.
pub fn category(name: &str) -> Option<&'static str> {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    Some(match ext.as_str() {
        "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "cab" | "arj" | "lz" | "lzh" | "iso" | "img" => "Compressed",
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "xlsm" | "csv" | "ppt" | "pptx" | "txt" | "rtf" | "odt" | "ods" | "odp" | "epub" | "md" | "xml" | "json" => "Documents",
        "mp3" | "m4a" | "wav" | "flac" | "aac" | "ogg" | "opus" | "wma" | "mid" | "midi" => "Music",
        "exe" | "msi" | "msix" | "msixbundle" | "appx" | "appxbundle" | "apk" | "xapk" | "bat" | "cmd" | "ps1" | "jar" | "dmg" | "deb" | "rpm" => "Programs",
        "mp4" | "mkv" | "avi" | "mov" | "wmv" | "flv" | "webm" | "m4v" | "3gp" | "mpeg" | "mpg" | "ts" => "Video",
        "torrent" => "Torrents",
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "svg" | "heic" | "avif" | "tif" | "tiff" | "ico" => "Pictures",
        _ => return None,
    })
}

/// `path` dipindah ke subfolder kategorinya (dibuat bila perlu), mis. Downloads\Video\a.mp4.
pub fn categorized(path: &Path) -> PathBuf {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else { return path.to_path_buf() };
    let name = name.to_string_lossy();
    // Sudah di folder kategori (mis. pengguna memilih sendiri): biarkan.
    if dir.file_name().is_some_and(|d| ["Compressed", "Documents", "Music", "Programs", "Video", "Torrents", "Pictures"].iter().any(|c| d.eq_ignore_ascii_case(c))) {
        return path.to_path_buf();
    }
    match category(&name) {
        Some(c) => {
            let d = dir.join(c);
            if std::fs::create_dir_all(&d).is_ok() {
                d.join(name.as_ref())
            } else {
                path.to_path_buf()
            }
        }
        None => path.to_path_buf(),
    }
}

/// Nama berkas PDF dari `Content-Disposition` (filename*= UTF-8, filename=) atau dari alamat.
/// Karakter yang dilarang Windows diganti, dan selalu berakhiran `.pdf`.
pub fn pdf_name(disposition: &str, url: &str) -> String {
    fn pct(s: &str) -> String {
        let b = s.as_bytes();
        let mut out = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'%' && i + 2 < b.len() {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
            }
            out.push(b[i]);
            i += 1;
        }
        String::from_utf8_lossy(&out).to_string()
    }
    let mut name = String::new();
    for part in disposition.split(';').map(str::trim) {
        let Some((k, v)) = part.split_once('=') else { continue };
        let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
        if k == "filename*" {
            let v = v.rsplit("''").next().unwrap_or(v).trim_matches('"');
            name = pct(v);
            break;
        }
        if k == "filename" && name.is_empty() {
            name = v.trim_matches('"').to_string();
        }
    }
    if name.trim().is_empty() {
        let path = url.split(['?', '#']).next().unwrap_or("");
        let path = path.split_once("://").map(|(_, r)| r).unwrap_or(path);
        name = path.split_once('/').map(|(_, p)| p).unwrap_or("").rsplit('/').find(|s| !s.is_empty()).map(pct).unwrap_or_default();
    }
    let mut name: String = name.chars().map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { '_' } else { c }).collect();
    name = name.trim().trim_matches('.').to_string();
    if name.is_empty() {
        name = "dokumen".into();
    }
    if !name.to_ascii_lowercase().ends_with(".pdf") {
        name.push_str(".pdf");
    }
    name
}

pub fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    for n in 1..1000 {
        let p = path.with_file_name(format!("{stem} ({n}){ext}"));
        if !p.exists() {
            return p;
        }
    }
    path.to_path_buf()
}

/// Mulai unduhan ke `path` (ukuran `total` dari [`probe`]). `cb` dipanggil dari thread latar.
pub fn start(req: Request, path: &Path, total: u64, conns: usize, cb: impl Fn(Update) + Send + 'static) -> Result<Job, String> {
    let part = PathBuf::from(format!("{}.ovnerpart", path.display()));
    let file = OpenOptions::new().create(true).truncate(true).read(true).write(true).open(&part).map_err(|e| e.to_string())?;
    // Berkas jarang (sparse): menulis bagian di posisi jauh (mis. 5 GB) tidak membuat NTFS mengisi
    // nol semua isi sebelumnya lebih dulu. Tanpa ini awal unduhan besar macet ±1 menit.
    #[cfg(windows)]
    unsafe {
        use std::os::windows::io::AsRawHandle;
        const FSCTL_SET_SPARSE: u32 = 590_020;
        let h = windows::Win32::Foundation::HANDLE(file.as_raw_handle());
        let _ = windows::Win32::System::IO::DeviceIoControl(h, FSCTL_SET_SPARSE, None, 0, None, 0, None, None);
    }
    file.set_len(total).map_err(|e| e.to_string())?;
    let conns = conns.clamp(1, 32);
    // Bagian awal: sama besar, paling kecil 1 MB.
    let n = ((total / (1024 * 1024)).max(1) as usize).min(conns);
    let step = total / n as u64;
    let segs = (0..n)
        .map(|k| {
            let start = step * k as u64;
            let end = if k + 1 == n { total } else { step * (k as u64 + 1) };
            Seg { start, end, pos: start, busy: false, fails: 0 }
        })
        .collect();
    let shared = Arc::new(Shared {
        req,
        total,
        conns: AtomicUsize::new(conns),
        precond: std::sync::atomic::AtomicBool::new(false),
        segs: Mutex::new(segs),
        file,
        received: AtomicU64::new(0),
        state: AtomicU8::new(RUN),
        active: AtomicUsize::new(0),
        error: Mutex::new(None),
    });
    let job = Job { shared: shared.clone() };
    let final_path = path.to_path_buf();
    std::thread::Builder::new()
        .name("ovner-unduh".into())
        .spawn(move || controller(shared, part, final_path, cb))
        .map_err(|e| e.to_string())?;
    Ok(job)
}

/// Berapa kali unduhan dipulihkan (semua bagian diberi kesempatan lagi dengan koneksi lebih
/// sedikit) sebelum benar-benar dinyatakan gagal.
const RECOVER_ROUNDS: u32 = 8;

fn controller(sh: Arc<Shared>, part: PathBuf, final_path: PathBuf, cb: impl Fn(Update)) {
    let mut last = (Instant::now(), 0u64);
    let mut speed = 0u64;
    let mut rounds = 0u32;
    loop {
        let state = sh.state.load(Ordering::SeqCst);
        if state == RUN {
            // Tambah koneksi selama masih ada pekerjaan dan kuota.
            while sh.active.load(Ordering::SeqCst) < sh.conns.load(Ordering::SeqCst) {
                let Some(idx) = claim(&sh) else { break };
                sh.active.fetch_add(1, Ordering::SeqCst);
                let s2 = sh.clone();
                let spawned = std::thread::Builder::new().name("ovner-unduh-bagian".into()).stack_size(256 * 1024).spawn(move || {
                    worker(&s2, idx);
                    s2.active.fetch_sub(1, Ordering::SeqCst);
                });
                if spawned.is_err() {
                    sh.active.fetch_sub(1, Ordering::SeqCst);
                    release(&sh, idx);
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(250));
        let received = sh.received.load(Ordering::SeqCst);
        let dt = last.0.elapsed().as_secs_f64();
        if dt >= 0.9 {
            let now_speed = ((received.saturating_sub(last.1)) as f64 / dt) as u64;
            speed = if speed == 0 { now_speed } else { (speed * 2 + now_speed) / 3 };
            last = (Instant::now(), received);
        }
        let active = sh.active.load(Ordering::SeqCst);
        let state = sh.state.load(Ordering::SeqCst);
        let (done, failed) = {
            let segs = sh.segs.lock().unwrap();
            (segs.iter().all(|s| s.pos >= s.end), segs.iter().any(|s| s.fails >= MAX_FAILS && s.pos < s.end))
        };
        if state == CANCELLED && active == 0 {
            let _ = std::fs::remove_file(&part);
            cb(Update::Done { ok: false, path: final_path.display().to_string(), received, error: Some("dibatalkan".into()) });
            return;
        }
        if done && active == 0 {
            let _ = sh.file.sync_all();
            let target = unique_path(&final_path);
            // Berkas harus ditutup dulu sebelum diganti nama (Windows): pegangan ada di `sh`,
            // jadi salin isi bila rename gagal karena berkas masih terbuka.
            let ok = std::fs::rename(&part, &target).is_ok() || (std::fs::copy(&part, &target).is_ok() && std::fs::remove_file(&part).is_ok());
            cb(Update::Done {
                ok,
                path: target.display().to_string(),
                received: sh.total,
                error: (!ok).then(|| "gagal memindahkan berkas".to_string()),
            });
            return;
        }
        if failed && active == 0 && rounds < RECOVER_ROUNDS && state == RUN {
            // Server menolak sementara (pembatasan, 428/429/503, mirror sibuk): jangan buang yang
            // sudah diunduh. Tunggu, kurangi koneksi separuh, lalu lanjutkan semua bagian.
            rounds += 1;
            let c = (sh.conns.load(Ordering::SeqCst) / 2).max(1);
            sh.conns.store(c, Ordering::SeqCst);
            for sg in sh.segs.lock().unwrap().iter_mut() {
                sg.fails = 0;
            }
            for _ in 0..(10 + 5 * rounds as u64) * 4 {
                if sh.state.load(Ordering::SeqCst) == CANCELLED {
                    break;
                }
                cb(Update::Progress { received, total: sh.total, speed: 0, conns: 0, paused: false });
                std::thread::sleep(Duration::from_millis(250));
            }
            continue;
        }
        if failed && active == 0 {
            let err = sh.error.lock().unwrap().clone().unwrap_or_else(|| "koneksi gagal".into());
            let _ = std::fs::remove_file(&part);
            cb(Update::Done { ok: false, path: final_path.display().to_string(), received, error: Some(err) });
            return;
        }
        cb(Update::Progress { received, total: sh.total, speed: if state == PAUSED { 0 } else { speed }, conns: active, paused: state == PAUSED });
    }
}

/// Ambil bagian yang belum dikerjakan, atau belah bagian terbesar yang sedang berjalan.
fn claim(sh: &Shared) -> Option<usize> {
    let mut segs = sh.segs.lock().unwrap();
    if let Some(i) = segs.iter().position(|s| !s.busy && s.pos < s.end && s.fails < MAX_FAILS) {
        segs[i].busy = true;
        return Some(i);
    }
    let (i, rest) = segs.iter().enumerate().filter(|(_, s)| s.busy).map(|(i, s)| (i, s.end.saturating_sub(s.pos))).max_by_key(|(_, r)| *r)?;
    if rest < MIN_SPLIT * 2 {
        return None;
    }
    let mid = segs[i].pos + rest / 2;
    let end = segs[i].end;
    segs[i].end = mid;
    segs.push(Seg { start: mid, end, pos: mid, busy: true, fails: 0 });
    Some(segs.len() - 1)
}

fn release(sh: &Shared, idx: usize) {
    if let Some(s) = sh.segs.lock().unwrap().get_mut(idx) {
        s.busy = false;
    }
}

fn write_at(file: &File, mut buf: &[u8], mut off: u64) -> std::io::Result<()> {
    while !buf.is_empty() {
        #[cfg(windows)]
        let n = std::os::windows::fs::FileExt::seek_write(file, buf, off)?;
        #[cfg(unix)]
        let n = std::os::unix::fs::FileExt::write_at(file, buf, off)?;
        if n == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::WriteZero, "tulis 0 byte"));
        }
        buf = &buf[n..];
        off += n as u64;
    }
    Ok(())
}

fn worker(sh: &Shared, idx: usize) {
    loop {
        if sh.state.load(Ordering::SeqCst) != RUN {
            break;
        }
        let (pos, end) = {
            let segs = sh.segs.lock().unwrap();
            (segs[idx].pos, segs[idx].end)
        };
        if pos >= end {
            break;
        }
        let mut write_err: Option<String> = None;
        let precond_req;
        let req = if sh.precond.load(Ordering::SeqCst) && sh.req.if_range.is_some() {
            let mut r = sh.req.clone();
            r.headers.push(("If-Range".into(), sh.req.if_range.clone().unwrap_or_default()));
            precond_req = r;
            &precond_req
        } else {
            &sh.req
        };
        let r = http_get(req, Some((pos, end - 1)), &mut |data| {
            // Kunci hanya untuk membaca/memperbarui posisi, tidak selama menulis ke disk: menulis
            // bisa lama (disk lambat) dan kunci yang sama dipakai jendela untuk menggambar peta
            // bagian, sehingga dulu Ovner ikut membeku. Hanya pekerja ini yang memajukan `pos`;
            // pembelahan bagian hanya memperkecil `end`.
            let (off, n) = {
                let segs = sh.segs.lock().unwrap();
                let seg = &segs[idx];
                (seg.pos, (seg.end.saturating_sub(seg.pos) as usize).min(data.len()))
            };
            if n > 0 {
                if let Err(e) = write_at(&sh.file, &data[..n], off) {
                    write_err = Some(e.to_string());
                    return false;
                }
            }
            let mut segs = sh.segs.lock().unwrap();
            let seg = &mut segs[idx];
            // Bila bagian ini dibelah saat menulis, byte sesudah `end` milik bagian baru (isinya
            // sama, jadi aman ditulis dua kali) dan tidak dihitung di sini.
            let counted = (off + n as u64).min(seg.end).saturating_sub(off);
            sh.received.fetch_add(counted, Ordering::SeqCst);
            seg.pos = (off + n as u64).max(seg.pos);
            // Berhenti bila bagian ini sudah penuh (mungkin karena dibelah) atau dijeda.
            seg.pos < seg.end && sh.state.load(Ordering::SeqCst) == RUN
        });
        let bad = match (&r, &write_err) {
            (_, Some(e)) => Some(e.clone()),
            (Err(e), _) => Some(e.clone()),
            (Ok(h), _) if h.status != 206 || h.range_start.is_some_and(|s| s != pos) => {
                if h.status == 428 {
                    sh.precond.store(true, Ordering::SeqCst);
                }
                Some(format!("HTTP {}", h.status))
            }
            _ => None,
        };
        let mut segs = sh.segs.lock().unwrap();
        let seg = &mut segs[idx];
        if seg.pos >= seg.end || sh.state.load(Ordering::SeqCst) != RUN {
            break;
        }
        // Ada kemajuan pada percobaan ini: gangguan sebelumnya sudah lewat.
        if seg.pos > pos {
            seg.fails = 0;
        }
        match bad {
            Some(e) => {
                seg.fails += 1;
                *sh.error.lock().unwrap() = Some(e);
                if seg.fails >= MAX_FAILS {
                    break;
                }
                // Jeda makin lama (0,5 s … 10 s) supaya gangguan jaringan/mirror sempat pulih.
                let wait = (500 * seg.fails as u64).min(10_000);
                drop(segs);
                std::thread::sleep(Duration::from_millis(wait));
            }
            // Koneksi selesai lebih awal tanpa galat: ulangi dari posisi terakhir.
            None => seg.fails += 1,
        }
    }
    release(sh, idx);
}

// ---------------------------------------------------------------------------
// HTTP

/// GET (dengan rentang byte opsional). `on_data` mengembalikan false untuk berhenti.
#[cfg(windows)]
fn http_get(req: &Request, range: Option<(u64, u64)>, on_data: &mut dyn FnMut(&[u8]) -> bool) -> Result<Head, String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Networking::WinHttp::*;
    let u = url::Url::parse(&req.url).map_err(|e| e.to_string())?;
    let secure = u.scheme() == "https";
    let host = u.host_str().ok_or("alamat tanpa host")?.to_string();
    let port = u.port_or_known_default().unwrap_or(if secure { 443 } else { 80 });
    let mut object = u.path().to_string();
    if let Some(q) = u.query() {
        object.push('?');
        object.push_str(q);
    }
    let agent = if req.user_agent.is_empty() { "Ovner".to_string() } else { req.user_agent.clone() };
    unsafe {
        // Satu sesi WinHTTP dipakai bersama (per user agent): deteksi proxy otomatis (WPAD) yang
        // bisa makan beberapa detik di jaringan kantor hanya terjadi sekali, dan koneksi TLS ke
        // server yang sama dipakai ulang oleh semua bagian unduhan. Dulu tiap permintaan membuka
        // sesi baru sehingga unduhan besar lama baru mulai.
        static SESSIONS: std::sync::Mutex<Vec<(String, usize)>> = std::sync::Mutex::new(Vec::new());
        let session = {
            let mut list = SESSIONS.lock().unwrap();
            match list.iter().find(|(a, _)| *a == agent) {
                Some((_, h)) => *h as *mut core::ffi::c_void,
                None => {
                    let h = WinHttpOpen(&HSTRING::from(agent.as_str()), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0);
                    if h.is_null() {
                        return Err("WinHttpOpen gagal".into());
                    }
                    let _ = WinHttpSetTimeouts(h, 8_000, 8_000, 15_000, 30_000);
                    // Cookie diambil dari WebView2 dan dikirim sendiri; WinHTTP tidak perlu menyimpannya.
                    let _ = WinHttpSetOption(Some(h as *const _), WINHTTP_OPTION_DISABLE_FEATURE, Some(&WINHTTP_DISABLE_COOKIES.to_le_bytes()[..]));
                    let _ = WinHttpSetOption(Some(h as *const _), WINHTTP_OPTION_MAX_CONNS_PER_SERVER, Some(&32u32.to_le_bytes()[..]));
                    list.push((agent.clone(), h as usize));
                    h
                }
            }
        };
        let close = |h: *mut core::ffi::c_void| {
            let _ = WinHttpCloseHandle(h);
        };
        let conn = WinHttpConnect(session, &HSTRING::from(host), port, 0);
        if conn.is_null() {
            return Err("tidak bisa terhubung".into());
        }
        let flags = if secure { WINHTTP_FLAG_SECURE } else { WINHTTP_OPEN_REQUEST_FLAGS(0) };
        let hreq = WinHttpOpenRequest(conn, &HSTRING::from("GET"), &HSTRING::from(object), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), flags);
        if hreq.is_null() {
            close(conn);
            return Err("WinHttpOpenRequest gagal".into());
        }
        let mut headers = String::new();
        if let Some((a, b)) = range {
            headers.push_str(&format!("Range: bytes={a}-{b}\r\n"));
        }
        headers.push_str("Accept-Encoding: identity\r\n");
        for (k, v) in &req.headers {
            if !k.is_empty() && !v.is_empty() && !v.contains(['\r', '\n']) {
                headers.push_str(&format!("{k}: {v}\r\n"));
            }
        }
        let wide: Vec<u16> = headers.encode_utf16().collect();
        let _ = WinHttpAddRequestHeaders(hreq, &wide, WINHTTP_ADDREQ_FLAG_ADD | WINHTTP_ADDREQ_FLAG_REPLACE);
        let result = (|| {
            WinHttpSendRequest(hreq, None, None, 0, 0, 0).map_err(|e| e.to_string())?;
            WinHttpReceiveResponse(hreq, std::ptr::null_mut()).map_err(|e| e.to_string())?;
            let mut len = 0u32;
            let _ = WinHttpQueryHeaders(hreq, WINHTTP_QUERY_RAW_HEADERS_CRLF, PCWSTR::null(), None, &mut len, std::ptr::null_mut());
            let mut buf = vec![0u16; (len as usize / 2) + 1];
            let mut len2 = (buf.len() * 2) as u32;
            let raw = if WinHttpQueryHeaders(hreq, WINHTTP_QUERY_RAW_HEADERS_CRLF, PCWSTR::null(), Some(buf.as_mut_ptr() as *mut _), &mut len2, std::ptr::null_mut()).is_ok() {
                String::from_utf16_lossy(&buf[..(len2 as usize / 2).min(buf.len())])
            } else {
                String::new()
            };
            let head = parse_head(&raw);
            // Baca isi hanya bila server benar-benar mengirim rentang yang diminta. Server/mirror
            // yang mengabaikan Range mengirim berkas dari awal (HTTP 200); dulu isinya tetap
            // ditulis di posisi bagian sehingga berkas rusak.
            let mut head = head;
            let mut ulen = 0u32;
            let _ = WinHttpQueryOption(hreq, WINHTTP_OPTION_URL, None, &mut ulen);
            if ulen > 0 {
                let mut ub = vec![0u16; ulen as usize / 2 + 1];
                let mut ul = (ub.len() * 2) as u32;
                if WinHttpQueryOption(hreq, WINHTTP_OPTION_URL, Some(ub.as_mut_ptr() as *mut _), &mut ul).is_ok() {
                    head.final_url = Some(String::from_utf16_lossy(&ub[..(ul as usize / 2).min(ub.len())]).trim_end_matches('\0').to_string());
                }
            }
            if body_ok(&head, range) {
                let mut chunk = vec![0u8; 64 * 1024];
                loop {
                    let mut n = 0u32;
                    WinHttpReadData(hreq, chunk.as_mut_ptr() as *mut _, chunk.len() as u32, &mut n).map_err(|e| e.to_string())?;
                    if n == 0 || !on_data(&chunk[..n as usize]) {
                        break;
                    }
                }
            }
            Ok(head)
        })();
        close(hreq);
        close(conn);
        result
    }
}

/// Linux (pengujian): HTTP biasa tanpa TLS lewat TcpStream.
#[cfg(not(windows))]
fn http_get(req: &Request, range: Option<(u64, u64)>, on_data: &mut dyn FnMut(&[u8]) -> bool) -> Result<Head, String> {
    use std::io::{Read, Write};
    let u = url::Url::parse(&req.url).map_err(|e| e.to_string())?;
    if u.scheme() != "http" {
        return Err("hanya http di Linux".into());
    }
    let host = u.host_str().ok_or("tanpa host")?;
    let port = u.port_or_known_default().unwrap_or(80);
    let mut s = std::net::TcpStream::connect((host, port)).map_err(|e| e.to_string())?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    let mut path = u.path().to_string();
    if let Some(q) = u.query() {
        path.push('?');
        path.push_str(q);
    }
    let mut head = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: {}\r\nConnection: close\r\n", req.user_agent);
    if let Some((a, b)) = range {
        head.push_str(&format!("Range: bytes={a}-{b}\r\n"));
    }
    for (k, v) in &req.headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes()).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 16 * 1024];
    let body_start = loop {
        let n = s.read(&mut tmp).map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("koneksi tertutup".into());
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break p + 4;
        }
    };
    let parsed = parse_head(&String::from_utf8_lossy(&buf[..body_start]));
    if body_ok(&parsed, range) {
        if body_start < buf.len() && !on_data(&buf[body_start..]) {
            return Ok(parsed);
        }
        loop {
            let n = s.read(&mut tmp).map_err(|e| e.to_string())?;
            if n == 0 || !on_data(&tmp[..n]) {
                break;
            }
        }
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdf_names() {
        assert_eq!(pdf_name("", "https://sipp.bpjsketenagakerjaan.go.id/kartu/download"), "download.pdf");
        assert_eq!(pdf_name("inline; filename=\"kartu peserta.pdf\"", "https://x/y"), "kartu peserta.pdf");
        assert_eq!(pdf_name("inline; filename=a.pdf; filename*=UTF-8''Kartu%20BPJS%3A1.pdf", "https://x/y"), "Kartu BPJS_1.pdf");
        assert_eq!(pdf_name("inline", "https://x/dok/laporan%202026?id=3"), "laporan 2026.pdf");
        assert_eq!(pdf_name("", "https://x/"), "dokumen.pdf");
    }
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// Server HTTP kecil yang mendukung Range (dan bisa memutus koneksi di tengah).
    fn server(data: Arc<Vec<u8>>, flaky: bool) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let count = Arc::new(AtomicUsize::new(0));
        std::thread::spawn(move || {
            for c in l.incoming() {
                let Ok(mut c) = c else { continue };
                let data = data.clone();
                let count = count.clone();
                std::thread::spawn(move || {
                    let mut buf = [0u8; 4096];
                    let n = c.read(&mut buf).unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let range = req.lines().find_map(|l| l.strip_prefix("Range: bytes=")).and_then(|r| {
                        let (a, b) = r.trim().split_once('-')?;
                        Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?))
                    });
                    let k = count.fetch_add(1, Ordering::SeqCst);
                    match range {
                        Some((a, b)) => {
                            let b = b.min(data.len() - 1);
                            let body = &data[a..=b];
                            let _ = write!(c, "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {a}-{b}/{}\r\nContent-Length: {}\r\n\r\n", data.len(), body.len());
                            // Sesekali putus di tengah supaya percobaan ulang ikut diuji.
                            let cut = if flaky && k % 3 == 2 { body.len() / 2 } else { body.len() };
                            for piece in body[..cut].chunks(7000) {
                                if c.write_all(piece).is_err() {
                                    return;
                                }
                            }
                        }
                        None => {
                            let _ = write!(c, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", data.len());
                            let _ = c.write_all(&data);
                        }
                    }
                });
            }
        });
        port
    }

    #[test]
    fn head_parsing() {
        let h = parse_head("HTTP/1.1 206 Partial Content\r\nContent-Type: application/pdf\r\nContent-Range: bytes 0-0/123456\r\n\r\n");
        assert_eq!((h.status, h.range_total, h.range_start), (206, Some(123456), Some(0)));
        let h = parse_head("HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n");
        assert_eq!((h.status, h.range_total), (200, None));
    }

    fn run(flaky: bool) {
        let data: Vec<u8> = (0..3_500_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
        let data = Arc::new(data);
        let port = server(data.clone(), flaky);
        let req = Request { url: format!("http://127.0.0.1:{port}/berkas.bin"), user_agent: "uji".into(), headers: vec![("Cookie".into(), "a=b".into())], if_range: None };
        let p = probe(&req).expect("probe");
        assert_eq!(p.total, data.len() as u64);
        let dir = std::env::temp_dir().join(format!("ovner-dl-{}-{flaky}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let target = dir.join("berkas.bin");
        let _ = std::fs::remove_file(&target);
        let (tx, rx) = std::sync::mpsc::channel();
        let _job = start(req, &target, p.total, 6, move |u| {
            let _ = tx.send(u);
        })
        .unwrap();
        let done = loop {
            match rx.recv_timeout(Duration::from_secs(30)).expect("kabar unduhan") {
                Update::Done { ok, path, received, error } => break (ok, path, received, error),
                Update::Progress { .. } => {}
            }
        };
        assert!(done.0, "gagal: {:?}", done.3);
        assert_eq!(done.2, data.len() as u64);
        let got = std::fs::read(&done.1).unwrap();
        assert!(got == *data, "isi berkas berbeda");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn segmented_download_matches() {
        run(false);
    }

    #[test]
    fn segmented_download_survives_drops() {
        run(true);
    }

    #[test]
    fn unique_names() {
        let dir = std::env::temp_dir().join(format!("ovner-uniq-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let a = dir.join("laporan.pdf");
        std::fs::write(&a, b"x").unwrap();
        assert_eq!(unique_path(&a).file_name().unwrap(), "laporan (1).pdf");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
