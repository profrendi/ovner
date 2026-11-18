//! Unduh video dari halaman (YouTube dan situs video lain), seperti modul `mdm-tools` di
//! Mister Download Manager: Ovner menjalankan **yt-dlp** (Unlicense) sebagai program terpisah,
//! dan **ffmpeg** untuk menggabungkan video+audio resolusi tinggi. Keduanya diunduh sekali ke
//! `<data>/tools` saat pertama dipakai.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

#[cfg(windows)]
pub const YTDLP_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
#[cfg(not(windows))]
pub const YTDLP_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux";
/// ffmpeg + ffprobe (build "shared": ±87 MB, setengah dari build statis). Varian LGPL yang dulu
/// dipakai sudah tidak diterbitkan yt-dlp/FFmpeg-Builds (unduhan 404 → galat 0x800C0006 di
/// 0.8.15), jadi ada beberapa sumber cadangan.
pub const FFMPEG_URLS: &[&str] = &[
    "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl-shared.zip",
    "https://github.com/FFmpeg-Builds/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl-shared.zip",
    "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip",
];
/// Deno: runtime JavaScript yang sejak yt-dlp 2025.11.12 dibutuhkan untuk YouTube (memecahkan
/// tantangan JavaScript YouTube; tanpa ini format bisa kurang atau unduhan ditolak 403).
pub const DENO_URL: &str = "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip";

/// Situs video yang didukung yt-dlp dan sering dipakai.
const VIDEO_HOSTS: &[&str] = &[
    "youtube.com", "youtu.be", "vimeo.com", "dailymotion.com", "tiktok.com", "facebook.com", "fb.watch",
    "instagram.com", "x.com", "twitter.com", "twitch.tv", "bilibili.com", "vidio.com", "reddit.com",
];

/// Halaman ini kemungkinan berisi video yang bisa diunduh?
pub fn is_video_page(url: &str) -> bool {
    let Ok(u) = url::Url::parse(url) else { return false };
    let host = u.host_str().unwrap_or("").trim_start_matches("www.").trim_start_matches("m.").to_ascii_lowercase();
    let path = u.path();
    if host == "youtube.com" || host == "music.youtube.com" {
        return path.starts_with("/watch") || path.starts_with("/shorts/") || path.starts_with("/live/");
    }
    if host == "youtu.be" {
        return path.len() > 1;
    }
    VIDEO_HOSTS.iter().any(|h| host == *h || host.ends_with(&format!(".{h}"))) && path.len() > 1
}

pub fn tools_dir(data: &Path) -> PathBuf {
    data.join("tools")
}

pub fn ytdlp(data: &Path) -> PathBuf {
    tools_dir(data).join(if cfg!(windows) { "yt-dlp.exe" } else { "yt-dlp" })
}

/// Folder ffmpeg (ffmpeg, ffprobe, dan DLL-nya); diberikan ke yt-dlp lewat --ffmpeg-location.
pub fn ffmpeg_dir(data: &Path) -> PathBuf {
    tools_dir(data).join("ffmpeg")
}

pub fn ffmpeg(data: &Path) -> PathBuf {
    ffmpeg_dir(data).join(if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" })
}

pub fn deno(data: &Path) -> PathBuf {
    tools_dir(data).join(if cfg!(windows) { "deno.exe" } else { "deno" })
}

/// Halaman YouTube (butuh Deno untuk yt-dlp).
pub fn is_youtube(url: &str) -> bool {
    let host = url::Url::parse(url).ok().and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase())).unwrap_or_default();
    host == "youtu.be" || host == "youtube.com" || host.ends_with(".youtube.com") || host.ends_with(".youtube-nocookie.com")
}

/// Komponen yang dibutuhkan sebuah pekerjaan.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Needs {
    pub ffmpeg: bool,
    pub deno: bool,
}

/// Satu pemasangan komponen pada satu waktu: dua penelusuran/unduhan bersamaan dulu menulis ke
/// berkas sementara yang sama sehingga salah satunya gagal "Operation aborted (0x80004004)".
static TOOLS_LOCK: Mutex<()> = Mutex::new(());

/// Unduh `url` ke `dest` sambil melaporkan MB yang diterima. Windows: mesin WinHTTP Ovner
/// (mengikuti pengalihan GitHub, tanpa cache IE); Linux: curl.
fn fetch(url: &str, dest: &Path, what: &str, note: &dyn Fn(&str)) -> Result<(), String> {
    let _ = std::fs::remove_file(dest);
    #[cfg(windows)]
    {
        let req = crate::dlengine::Request {
            url: url.to_string(),
            user_agent: format!("Ovner/{}", env!("CARGO_PKG_VERSION")),
            headers: Vec::new(),
            if_range: None,
            wait_secs: 60,
        };
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let r = crate::dlengine::fetch_whole(&req, dest, false, &cancel, |got| note(&format!("{what} ({} MB)", got / 1_048_576)));
        if let Err(e) = r {
            let _ = std::fs::remove_file(dest);
            return Err(e);
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        note(what);
        crate::win::download(url, dest).inspect_err(|_| {
            let _ = std::fs::remove_file(dest);
        })
    }
}

/// Coba sumber satu per satu; galat terakhir dilaporkan bila semuanya gagal.
fn fetch_any(urls: &[&str], dest: &Path, what: &str, note: &dyn Fn(&str)) -> Result<(), String> {
    let mut last = String::from("tidak ada sumber");
    for u in urls {
        match fetch(u, dest, what, note) {
            Ok(()) => return Ok(()),
            Err(e) => {
                crate::log(&format!("unduh {what} dari {u} gagal: {e}"));
                last = e;
            }
        }
    }
    Err(format!("{what} tidak bisa diunduh: {last}"))
}

/// Buka zip `zip` ke folder sementara (tar bawaan Windows 10/11 bisa membuka zip).
fn unzip(zip: &Path, out: &Path) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(out);
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    // tar bawaan Windows (System32, bsdtar: bisa membuka zip). tar lain di PATH (mis. dari Git)
    // tidak bisa membuka zip.
    #[cfg(windows)]
    let tar = std::env::var_os("SystemRoot").map(|r| PathBuf::from(r).join("System32").join("tar.exe")).filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from("tar"));
    #[cfg(not(windows))]
    let tar = PathBuf::from("tar");
    let mut cmd = Command::new(tar);
    cmd.arg("-xf").arg(zip).arg("-C").arg(out);
    hide_window(&mut cmd);
    let st = cmd.status().map_err(|e| format!("tar: {e}"))?;
    if !st.success() {
        return Err("arsip zip tidak bisa dibuka".into());
    }
    Ok(())
}

/// Jalankan `exe arg` (mis. `ffmpeg -version`): Ok(baris pertama keluarannya) bila berhasil.
fn runs(exe: &Path, arg: &str) -> Result<String, String> {
    let mut cmd = Command::new(exe);
    cmd.arg(arg).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    hide_window(&mut cmd);
    let o = cmd.output().map_err(|e| format!("{} tidak bisa dijalankan: {e}", exe.display()))?;
    if !o.status.success() {
        return Err(format!("{} gagal dijalankan (kode {:?})", exe.display(), o.status.code()));
    }
    Ok(String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or("").chars().take(120).collect())
}

/// ffmpeg dan ffprobe milik Ovner sudah terbukti bisa dijalankan (sekali per sesi).
static FFMPEG_OK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn ffmpeg_works(data: &Path) -> Result<(), String> {
    if FFMPEG_OK.load(std::sync::atomic::Ordering::Relaxed) {
        return Ok(());
    }
    let v = runs(&ffmpeg(data), "-version")?;
    let probe = ffmpeg_dir(data).join(if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" });
    runs(&probe, "-version")?;
    crate::log(&format!("ffmpeg siap: {v}"));
    FFMPEG_OK.store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

/// Pastikan yt-dlp (selalu), ffmpeg dan Deno (bila diminta) ada; yt-dlp diperbarui bila sudah
/// lebih dari 3 hari (YouTube sering berubah). Memblokir: panggil dari thread latar.
pub fn ensure_tools(data: &Path, needs: Needs, note: &dyn Fn(&str)) -> Result<(), String> {
    let _guard = TOOLS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tools_dir(data);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let y = ytdlp(data);
    if !y.exists() {
        let tmp = dir.join("yt-dlp.download");
        fetch_any(&[YTDLP_URL], &tmp, "yt-dlp", note)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
        }
        std::fs::rename(&tmp, &y).map_err(|e| e.to_string())?;
        let _ = std::fs::write(dir.join("yt-dlp.checked"), b"1");
    } else {
        let mark = dir.join("yt-dlp.checked");
        let stale = std::fs::metadata(&mark).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_none_or(|d| d.as_secs() > 3 * 86_400);
        if stale {
            note("yt-dlp (memperbarui)");
            let mut cmd = Command::new(&y);
            cmd.arg("-U").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
            hide_window(&mut cmd);
            match cmd.output() {
                Ok(o) => {
                    let out = String::from_utf8_lossy(&o.stdout);
                    crate::log(&format!("yt-dlp -U: {}", out.lines().last().unwrap_or("").trim()));
                }
                Err(e) => crate::log(&format!("yt-dlp -U gagal: {e}")),
            }
            let _ = std::fs::write(&mark, b"1");
        }
    }
    if needs.deno && cfg!(windows) && !deno(data).exists() {
        let zip = dir.join("deno.zip.download");
        let out = dir.join("deno-extract");
        let r = fetch_any(&[DENO_URL], &zip, "Deno", note).and_then(|_| unzip(&zip, &out)).and_then(|_| {
            let f = find_file(&out, "deno.exe").ok_or("deno.exe tidak ditemukan di arsip")?;
            std::fs::rename(&f, deno(data)).or_else(|_| std::fs::copy(&f, deno(data)).map(|_| ())).map_err(|e| e.to_string())
        });
        let _ = std::fs::remove_dir_all(&out);
        let _ = std::fs::remove_file(&zip);
        let r = r.and_then(|_| runs(&deno(data), "--version").map(|v| crate::log(&format!("Deno siap: {v}"))));
        if r.is_err() {
            let _ = std::fs::remove_file(deno(data));
        }
        // Tanpa Deno yt-dlp masih bisa mencoba (format mungkin lebih sedikit): jangan gagalkan.
        if let Err(e) = r {
            crate::log(&format!("Deno tidak terpasang, yt-dlp jalan tanpa runtime JavaScript: {e}"));
        }
    }
    // ffmpeg yang ada tetapi tidak bisa dijalankan (berkas rusak/kurang DLL): pasang ulang.
    if needs.ffmpeg && cfg!(windows) && ffmpeg(data).exists() {
        if let Err(e) = ffmpeg_works(data) {
            crate::log(&format!("ffmpeg dipasang ulang: {e}"));
            let _ = std::fs::remove_dir_all(ffmpeg_dir(data));
        }
    }
    if needs.ffmpeg && cfg!(windows) && !ffmpeg(data).exists() {
        let zip = dir.join("ffmpeg.zip.download");
        fetch_any(FFMPEG_URLS, &zip, "ffmpeg", note)?;
        note("ffmpeg (membuka arsip)");
        let out = dir.join("ffmpeg-extract");
        let target = ffmpeg_dir(data);
        let r = unzip(&zip, &out).and_then(|_| {
            // Folder bin: ffmpeg.exe, ffprobe.exe, dan DLL build "shared". ffplay tidak perlu.
            let exe = find_file(&out, "ffmpeg.exe").ok_or("ffmpeg.exe tidak ditemukan di arsip")?;
            let bin = exe.parent().ok_or("arsip ffmpeg aneh")?.to_path_buf();
            let _ = std::fs::remove_dir_all(&target);
            std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            for e in std::fs::read_dir(&bin).map_err(|e| e.to_string())?.flatten() {
                let name = e.file_name().to_string_lossy().to_ascii_lowercase();
                if name == "ffplay.exe" || !(name.ends_with(".exe") || name.ends_with(".dll")) {
                    continue;
                }
                std::fs::rename(e.path(), target.join(e.file_name()))
                    .or_else(|_| std::fs::copy(e.path(), target.join(e.file_name())).map(|_| ()))
                    .map_err(|e| e.to_string())?;
            }
            if ffmpeg(data).exists() {
                Ok(())
            } else {
                Err("ffmpeg.exe tidak tersalin".into())
            }
        });
        let _ = std::fs::remove_dir_all(&out);
        let _ = std::fs::remove_file(&zip);
        let r = r.and_then(|_| ffmpeg_works(data));
        if r.is_err() {
            let _ = std::fs::remove_dir_all(&target);
        }
        r?;
    }
    if needs.ffmpeg && cfg!(windows) {
        ffmpeg_works(data)?;
    }
    Ok(())
}

/// Argumen yt-dlp untuk komponen yang tersedia (ffmpeg dan Deno milik Ovner).
pub fn tool_args(data: &Path) -> Vec<std::ffi::OsString> {
    let mut a: Vec<std::ffi::OsString> = Vec::new();
    if ffmpeg(data).exists() {
        a.push("--ffmpeg-location".into());
        a.push(ffmpeg_dir(data).into_os_string());
    }
    let d = deno(data);
    if d.exists() {
        let mut v = std::ffi::OsString::from("deno:");
        v.push(d.as_os_str());
        a.push("--js-runtimes".into());
        a.push(v);
    }
    a
}

fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(f) = find_file(&p, name) {
                return Some(f);
            }
        } else if p.file_name().is_some_and(|n| n.eq_ignore_ascii_case(name)) {
            return Some(p);
        }
    }
    None
}

fn hide_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Title(String),
    /// Kabar persiapan (mis. "ffmpeg (35 MB)"), bukan nama berkas.
    Note(String),
    /// Nama berkas akhir (untuk menghapus sisa .part bila dibatalkan).
    Path(String),
    Progress { received: u64, total: u64, speed: u64, eta: u64 },
    Done { ok: bool, path: String, error: String },
    /// Video tersimpan, tetapi subtitlenya gagal (mis. YouTube membatasi, HTTP 429); bisa dicoba
    /// lagi tanpa mengunduh ulang videonya.
    SubsFailed(String),
}

/// Satu baris keluaran yt-dlp → kejadian (baris lain diabaikan).
pub fn parse_line(line: &str) -> Option<Event> {
    if let Some(rest) = line.split("[ovner] ").nth(1) {
        let f: Vec<&str> = rest.trim().split('|').collect();
        let num = |i: usize| f.get(i).and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(0.0) as u64;
        let total = if num(1) > 0 { num(1) } else { num(2) };
        return Some(Event::Progress { received: num(0), total, speed: num(3), eta: num(4) });
    }
    if let Some(t) = line.split("[ovner-path] ").nth(1) {
        return Some(Event::Path(t.trim().to_string()));
    }
    if let Some(t) = line.split("[ovner-title] ").nth(1) {
        return Some(Event::Title(t.trim().to_string()));
    }
    if let Some(p) = line.split("[ovner-file] ").nth(1) {
        return Some(Event::Done { ok: true, path: p.trim().to_string(), error: String::new() });
    }
    // Berkas potongan yang sedang ditulis (untuk menghapus sisanya bila dibatalkan).
    if let Some(p) = line.trim().strip_prefix("[download] Destination: ") {
        return Some(Event::Path(p.trim().to_string()));
    }
    None
}

/// Proses yt-dlp yang berjalan (untuk dibatalkan).
#[derive(Clone)]
pub struct Job(Arc<Mutex<Option<Child>>>, Arc<std::sync::atomic::AtomicBool>);

impl Job {
    fn new(child: Option<Child>) -> Job {
        Job(Arc::new(Mutex::new(child)), Arc::new(std::sync::atomic::AtomicBool::new(false)))
    }

    pub fn cancel(&self) {
        self.1.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(c) = self.0.lock().unwrap().as_mut() {
            let _ = c.kill();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.1.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// Pilihan unduhan dari dialog "Unduh video dari halaman" (seperti Mister Download Manager).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Spec {
    /// Alamat halaman video.
    pub page: String,
    /// Kunci pilihan: "v:<tinggi>", "a:m4a", "a:mp3:0" (VBR), "a:mp3:<kbps>", "a:opus", "a:flac".
    pub key: String,
    /// Folder simpan.
    pub dir: String,
    /// Nama berkas (ekstensi disesuaikan dengan pilihan).
    pub name: String,
    /// Bahasa subtitle ("" = tanpa subtitle).
    #[serde(default)]
    pub sub: String,
    /// Subtitle dimasukkan ke dalam video (bukan berkas .srt terpisah).
    #[serde(default)]
    pub embed: bool,
    /// Subtitle yang dipilih adalah terjemahan otomatis (YouTube).
    #[serde(default)]
    pub sub_auto: bool,
    /// Format persis hasil penelusuran (mis. "137+140"), supaya yang diunduh benar-benar
    /// kualitas yang dipilih, bukan jatuh diam-diam ke format lain yang lebih kecil.
    #[serde(default)]
    pub fmt: String,
    /// Subtitle dimasukkan ke video DAN disimpan sebagai .srt bernama sama di sampingnya
    /// (Media Player bawaan Windows tidak membaca subtitle di dalam MP4).
    #[serde(default)]
    pub keep_srt: bool,
}

impl Spec {
    pub fn is_audio(&self) -> bool {
        self.key.starts_with("a:")
    }
    /// Butuh ffmpeg: menggabungkan video+audio, mengubah format audio, atau memasukkan subtitle.
    pub fn needs_ffmpeg(&self) -> bool {
        self.key != "a:m4a"
    }
    pub fn needs(&self) -> Needs {
        Needs { ffmpeg: self.needs_ffmpeg(), deno: is_youtube(&self.page) }
    }
}

/// Ekstensi berkas hasil untuk sebuah pilihan.
pub fn ext_for(key: &str) -> &'static str {
    match key {
        "a:m4a" | "a:m4ax" => "m4a",
        "a:opus" => "opus",
        "a:flac" => "flac",
        k if k.starts_with("a:mp3") => "mp3",
        _ => "mp4",
    }
}

/// Nama berkas aman untuk Windows, tanpa ekstensi.
pub fn safe_name(title: &str) -> String {
    let s: String = title.chars().map(|c| if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control() { '_' } else { c }).collect();
    let s = s.trim().trim_end_matches(['.', ' ']).to_string();
    let s: String = s.chars().take(150).collect();
    if s.is_empty() { "video".into() } else { s }
}

/// Buang ekstensi yang dikenal dari nama berkas yang diketik pengguna.
pub fn strip_ext(name: &str) -> &str {
    for ext in [".mp4", ".m4a", ".mp3", ".opus", ".flac", ".webm", ".mkv"] {
        if name.len() > ext.len() && name[name.len() - ext.len()..].eq_ignore_ascii_case(ext) {
            return &name[..name.len() - ext.len()];
        }
    }
    name
}

/// Argumen yt-dlp untuk sebuah pilihan.
pub fn args_for(spec: &Spec) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();
    // Format persis dari penelusuran dulu, lalu cadangan dengan kualitas yang sama.
    let exact = |chain: &str| if spec.fmt.is_empty() { chain.to_string() } else { format!("{}/{chain}", spec.fmt) };
    let mut f = |fmt: String| {
        a.push("-f".into());
        a.push(fmt);
    };
    let key = spec.key.as_str();
    if let Some(h) = key.strip_prefix("v:").and_then(|h| h.parse::<u32>().ok()) {
        // Tanpa jatuh ke kualitas lebih rendah: lebih baik gagal dengan pesan jelas daripada
        // diam-diam mengunduh 360p (0.8.16: pilih 1080p dapat 28 MB).
        f(exact(&format!("bv*[height={h}][ext=mp4]+ba[ext=m4a]/bv*[height={h}]+ba/b[height={h}]")));
        a.extend(["--merge-output-format".into(), "mp4/mkv".into()]);
    } else if key == "a:m4a" {
        f(exact("ba[ext=m4a]/ba"));
    } else if key == "a:m4ax" {
        // Halaman tanpa format audio terpisah (pemutar video biasa): suara diambil dari videonya.
        f(exact("ba[ext=m4a]/ba/b"));
        a.extend(["-x".into(), "--audio-format".into(), "m4a".into()]);
    } else if key == "a:opus" {
        f(exact("ba[acodec^=opus]/ba/b"));
        a.extend(["-x".into(), "--audio-format".into(), "opus".into()]);
    } else if key == "a:flac" {
        f(exact("ba/b"));
        a.extend(["-x".into(), "--audio-format".into(), "flac".into()]);
    } else if let Some(q) = key.strip_prefix("a:mp3:") {
        f(exact("ba/b"));
        let q = if q == "0" { "0".to_string() } else { format!("{}K", q.parse::<u32>().unwrap_or(192)) };
        a.extend(["-x".into(), "--audio-format".into(), "mp3".into(), "--audio-quality".into(), q]);
    } else {
        f(exact("bv*[ext=mp4]+ba[ext=m4a]/bv*+ba/b"));
        a.extend(["--merge-output-format".into(), "mp4/mkv".into()]);
    }
    // Subtitle tidak lagi diunduh di sini: subtitle yang gagal (YouTube 429) membuat yt-dlp
    // membatalkan seluruh unduhan. Subtitle diambil terpisah setelah video tersimpan
    // (lihat `subs_args`, `fetch_subs`, `embed_subs`).
    a
}

/// Spesifikasi ini meminta subtitle (video saja).
pub fn wants_subs(spec: &Spec) -> bool {
    !spec.sub.is_empty() && !spec.is_audio()
}

/// Argumen yt-dlp untuk mengambil subtitle saja (tanpa video), dinamai sama dengan videonya:
/// "<stem>.<bahasa>.srt".
pub fn subs_args(spec: &Spec, stem: &str) -> Vec<String> {
    let mut a: Vec<String> = vec!["--skip-download".into(), "--sub-langs".into(), spec.sub.clone(), "--convert-subs".into(), "srt".into()];
    a.push(if spec.sub_auto { "--write-auto-subs".into() } else { "--write-subs".into() });
    // Jeda kecil sebelum mengambil subtitle mengurangi penolakan "Too Many Requests".
    a.extend(["--sleep-subtitles".into(), "2".into()]);
    a.extend(["-o".into(), Path::new(&spec.dir).join(format!("{}.%(ext)s", stem.replace('%', "%%"))).display().to_string()]);
    a
}

/// Kode bahasa 3 huruf untuk trek subtitle MP4/MKV (pemutar menampilkan nama bahasanya).
pub fn lang3(code: &str) -> Option<&'static str> {
    let base = code.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase();
    Some(match base.as_str() {
        "id" | "in" => "ind",
        "en" => "eng",
        "ms" => "msa",
        "jv" => "jav",
        "su" => "sun",
        "ja" => "jpn",
        "ko" => "kor",
        "zh" => "zho",
        "ar" => "ara",
        "es" => "spa",
        "fr" => "fra",
        "de" => "deu",
        "pt" => "por",
        "ru" => "rus",
        "hi" => "hin",
        "th" => "tha",
        "vi" => "vie",
        "tl" | "fil" => "fil",
        "it" => "ita",
        "nl" => "nld",
        "tr" => "tur",
        _ => return None,
    })
}

/// Subtitle hasil `fetch_subs` untuk video `stem` di `dir`: "<stem>.<bahasa>.srt" (atau
/// "<stem>.srt"). Mengembalikan berkas yang paling baru.
fn find_srt(dir: &Path, stem: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let n = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let low = n.to_ascii_lowercase();
            low.ends_with(".srt") && n.len() > stem.len() + 4 && n.starts_with(stem) && n[stem.len()..].starts_with('.') && !n[stem.len() + 1..n.len() - 4].contains('.')
        })
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
}

/// Jalankan satu perintah alat (yt-dlp/ffmpeg) sebagai bagian dari `job` (supaya bisa
/// dibatalkan); keluaran stdout+stderr dibaca sampai selesai. (berhasil, baris ERROR terakhir).
fn run_tool(mut cmd: Command, job: Option<&Job>) -> (bool, String) {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
    hide_window(&mut cmd);
    let Ok(mut child) = cmd.spawn() else { return (false, "alat tidak bisa dijalankan".into()) };
    let (out, err) = (child.stdout.take(), child.stderr.take());
    let slot = job.map(|j| j.0.clone()).unwrap_or_else(|| Arc::new(Mutex::new(None)));
    *slot.lock().unwrap() = Some(child);
    let last = Arc::new(Mutex::new(String::new()));
    let mut readers = Vec::new();
    for stream in [out.map(|o| Box::new(o) as Box<dyn std::io::Read + Send>), err.map(|e| Box::new(e) as Box<dyn std::io::Read + Send>)].into_iter().flatten() {
        let last = last.clone();
        readers.push(std::thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                let l = line.trim();
                if std::env::var_os("OVNER_DEBUG_YTDLP").is_some() {
                    eprintln!("tool> {l}");
                }
                if l.starts_with("ERROR") || l.starts_with("[Subtitle") || l.starts_with("[info] Writing video subtitles") || l.starts_with("WARNING") {
                    crate::log(&format!("subtitle: {}", l.chars().take(300).collect::<String>()));
                }
                if l.starts_with("ERROR") {
                    *last.lock().unwrap() = l.trim_start_matches("ERROR: ").chars().take(300).collect();
                }
            }
        }));
    }
    for r in readers {
        let _ = r.join();
    }
    let ok = slot.lock().unwrap().take().and_then(|mut c| c.wait().ok()).is_some_and(|s| s.success());
    let e = last.lock().unwrap().clone();
    (ok, e)
}

/// Ambil subtitle untuk video yang sudah tersimpan, dengan percobaan ulang bila YouTube
/// membatasi (HTTP 429) atau servernya sedang bermasalah: maks. 3 kali, jeda 5 lalu 15 detik.
pub fn fetch_subs(data: &Path, spec: &Spec, video: &Path, job: Option<&Job>, cancelled: &dyn Fn() -> bool) -> Result<PathBuf, String> {
    let stem = video.file_stem().map(|s| s.to_string_lossy().to_string()).ok_or("nama video tidak terbaca")?;
    let dir = video.parent().ok_or("folder video tidak terbaca")?;
    let mut last = String::new();
    for attempt in 0..3 {
        if attempt > 0 {
            let wait = if attempt == 1 { 5 } else { 15 };
            crate::log(&format!("subtitle: dicoba lagi dalam {wait} detik (percobaan {})", attempt + 1));
            for _ in 0..wait * 4 {
                if cancelled() {
                    return Err("dibatalkan".into());
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        }
        let mut cmd = Command::new(ytdlp(data));
        cmd.args(["--newline", "--no-colors", "--no-playlist", "--no-mtime"]).args(subs_args(spec, &stem)).args(tool_args(data)).arg(&spec.page);
        let (ok, err) = run_tool(cmd, job);
        if let Some(srt) = find_srt(dir, &stem) {
            return Ok(srt);
        }
        if cancelled() {
            return Err("dibatalkan".into());
        }
        last = if err.is_empty() && ok { "subtitle bahasa itu tidak tersedia".into() } else if err.is_empty() { "gagal".into() } else { err };
        let low = last.to_ascii_lowercase();
        let retry = low.contains("429") || low.contains("too many requests") || low.contains("http error 5") || low.contains("timed out") || low.contains("connection");
        if !retry {
            break;
        }
    }
    Err(friendly_sub_error(&last))
}

fn friendly_sub_error(e: &str) -> String {
    let low = e.to_ascii_lowercase();
    if low.contains("429") || low.contains("too many requests") {
        "YouTube sedang membatasi unduhan subtitle (HTTP 429); coba lagi beberapa menit lagi".into()
    } else {
        e.to_string()
    }
}

/// Masukkan `srt` ke video sebagai trek subtitle default (MP4: mov_text, MKV: srt,
/// WebM: webvtt). Trek subtitle lama diganti. Berkas video diganti setelah berhasil.
pub fn embed_subs(data: &Path, video: &Path, srt: &Path, lang: &str, job: Option<&Job>) -> Result<(), String> {
    let ext = video.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let codec = match ext.as_str() {
        "mkv" => "srt",
        "webm" => "webvtt",
        _ => "mov_text",
    };
    let tmp = video.with_extension(format!("ovner-sub.{ext}"));
    let _ = std::fs::remove_file(&tmp);
    let exe = if ffmpeg(data).exists() { ffmpeg(data) } else { PathBuf::from("ffmpeg") };
    let mut cmd = Command::new(exe);
    cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-i"]).arg(video).arg("-i").arg(srt);
    cmd.args(["-map", "0", "-map", "-0:s", "-map", "1:0", "-c", "copy", "-c:s", codec, "-disposition:s:0", "default"]);
    if let Some(l) = lang3(lang) {
        cmd.arg("-metadata:s:s:0").arg(format!("language={l}"));
    }
    if ext == "mp4" || ext == "m4v" || ext == "mov" {
        cmd.args(["-movflags", "+faststart"]);
    }
    cmd.arg(&tmp);
    let (ok, err) = run_tool(cmd, job);
    if !ok || std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0) == 0 {
        let _ = std::fs::remove_file(&tmp);
        return Err(if err.is_empty() { "ffmpeg gagal memasukkan subtitle".into() } else { format!("ffmpeg: {err}") });
    }
    std::fs::rename(&tmp, video).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("video tidak bisa diganti: {e}")
    })
}

/// Langkah subtitle setelah video tersimpan: ambil (dengan percobaan ulang), masukkan ke video
/// bila diminta, lalu .srt disimpan bernama sama dengan video (keep_srt / tanpa embed) atau
/// dihapus. Ok = pesan singkat untuk log.
pub fn subs_step(data: &Path, spec: &Spec, video: &Path, job: Option<&Job>, cancelled: &dyn Fn() -> bool) -> Result<String, String> {
    let srt = fetch_subs(data, spec, video, job, cancelled)?;
    let lang = spec.sub.split(',').next().unwrap_or("").trim().to_string();
    if spec.embed {
        embed_subs(data, video, &srt, &lang, job)?;
        if spec.keep_srt {
            sidecar_srt(video);
        } else {
            let _ = std::fs::remove_file(&srt);
        }
        // Sisa .vtt yang tidak dikonversi.
        cleanup_vtt(video);
        Ok(format!("subtitle {lang} dimasukkan ke video"))
    } else {
        sidecar_srt(video);
        cleanup_vtt(video);
        Ok(format!("subtitle {lang} disimpan sebagai .srt"))
    }
}

fn cleanup_vtt(video: &Path) {
    let (Some(dir), Some(stem)) = (video.parent(), video.file_stem().map(|s| s.to_string_lossy().to_string())) else { return };
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n.starts_with(&stem) && n[stem.len()..].starts_with('.') && n.to_ascii_lowercase().ends_with(".vtt") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// Coba ambil subtitle lagi untuk video yang sudah ada (tombol "Coba unduh subtitle lagi").
pub fn retry_subs(data: &Path, spec: &Spec, video: &Path) -> Result<String, String> {
    if !video.exists() {
        return Err("berkas video sudah tidak ada".into());
    }
    ensure_tools(data, Needs { ffmpeg: spec.embed, deno: is_youtube(&spec.page) }, &|_| {})?;
    subs_step(data, spec, video, None, &|| false)
}

/// Hasil penelusuran halaman: judul, durasi, daftar pilihan (dengan perkiraan ukuran), subtitle.
pub fn choices(j: &serde_json::Value) -> serde_json::Value {
    use serde_json::json;
    let dur = j["duration"].as_f64().unwrap_or(0.0);
    let empty = Vec::new();
    let fmts = j["formats"].as_array().unwrap_or(&empty);
    let size = |f: &serde_json::Value| -> u64 {
        f["filesize"].as_u64().or_else(|| f["filesize_approx"].as_u64()).or_else(|| f["filesize"].as_f64().map(|v| v as u64)).unwrap_or_else(|| {
            let tbr = f["tbr"].as_f64().unwrap_or(0.0);
            (tbr * 1000.0 / 8.0 * dur) as u64
        })
    };
    let is_none = |v: &serde_json::Value| v.as_str().is_none_or(|c| c == "none");
    // Audio terbaik M4A (untuk digabung dengan video tanpa suara).
    let audio: Vec<&serde_json::Value> = fmts.iter().filter(|f| is_none(&f["vcodec"]) && !is_none(&f["acodec"])).collect();
    let best_m4a = audio.iter().filter(|f| f["ext"] == "m4a").max_by(|a, b| a["abr"].as_f64().unwrap_or(0.0).total_cmp(&b["abr"].as_f64().unwrap_or(0.0)));
    let best_opus = audio.iter().filter(|f| f["acodec"].as_str().is_some_and(|c| c.starts_with("opus"))).max_by(|a, b| a["abr"].as_f64().unwrap_or(0.0).total_cmp(&b["abr"].as_f64().unwrap_or(0.0)));
    let a_size = best_m4a.map(|f| size(f)).unwrap_or((128_000.0 / 8.0 * dur) as u64);
    let id_of = |f: &serde_json::Value| f["format_id"].as_str().unwrap_or("").to_string();
    // Audio pasangan video tanpa suara: M4A (AAC) agar hasil MP4 diputar di mana saja.
    let pair_audio = best_m4a.or_else(|| audio.iter().max_by(|a, b| a["abr"].as_f64().unwrap_or(0.0).total_cmp(&b["abr"].as_f64().unwrap_or(0.0))));
    // Video per tinggi. Urutan pilihan: unduhan langsung (https, bukan HLS), H.264 (avc1, paling
    // kompatibel), MP4, lalu yang paling tajam (ukuran terbesar).
    let rank = |f: &serde_json::Value| -> (bool, bool, bool, u64) {
        let proto = f["protocol"].as_str().unwrap_or("https");
        (
            !proto.contains("m3u8"),
            f["vcodec"].as_str().is_some_and(|c| c.starts_with("avc1") || c.starts_with("h264")),
            f["ext"] == "mp4",
            size(f),
        )
    };
    let mut by_h: std::collections::BTreeMap<u64, &serde_json::Value> = std::collections::BTreeMap::new();
    for f in fmts.iter().filter(|f| !is_none(&f["vcodec"]) && f["format_id"].is_string()) {
        let Some(h) = f["height"].as_u64().filter(|h| *h >= 100) else { continue };
        if f["ext"] == "mhtml" {
            continue;
        }
        let e = by_h.entry(h).or_insert(f);
        if rank(f) > rank(e) {
            *e = f;
        }
    }
    let mut list: Vec<serde_json::Value> = by_h
        .iter()
        .rev()
        .map(|(h, f)| {
            let with_audio = !is_none(&f["acodec"]);
            let (fmt, sz) = match (with_audio, pair_audio) {
                (false, Some(a)) => (format!("{}+{}", id_of(f), id_of(a)), size(f) + size(a)),
                _ => (id_of(f), size(f)),
            };
            let codec = f["vcodec"].as_str().unwrap_or("").split('.').next().unwrap_or("").to_string();
            json!({"key": format!("v:{h}"), "label": format!("{h}p MP4"), "size": sz, "ext": "mp4", "kind": "video", "fmt": fmt, "codec": codec, "suffix": format!("{h}p")})
        })
        .collect();
    if list.is_empty() && !fmts.is_empty() {
        list.push(json!({"key": "v:best", "label": "MP4", "size": fmts.last().map(size).unwrap_or(0), "ext": "mp4", "kind": "video"}));
    }
    let est = |kbps: f64| (kbps * 1000.0 / 8.0 * dur) as u64;
    let m4a_key = if audio.is_empty() { "a:m4ax" } else { "a:m4a" };
    let m4a_fmt = best_m4a.map(|f| id_of(f)).unwrap_or_default();
    // Akhiran nama berkas (huruf besar): "M4A 128 KBPS", "MP3 VBR", "OPUS 160 KBPS", "FLAC".
    let kbps = |f: Option<&&serde_json::Value>, fallback: u32| f.and_then(|f| f["abr"].as_f64()).map(snap_kbps).unwrap_or(fallback);
    let m4a_sfx = if best_m4a.is_some() || audio.is_empty() { format!("M4A {} KBPS", kbps(best_m4a, 128)) } else { "M4A".to_string() };
    list.push(json!({"key": m4a_key, "label": "Audio, M4A asli", "en": "Audio only, original M4A", "size": if dur > 0.0 || best_m4a.is_some() { a_size } else { 0 }, "ext": "m4a", "kind": "audio", "fmt": m4a_fmt, "suffix": m4a_sfx}));
    list.push(json!({"key": "a:mp3:0", "label": "Audio MP3 VBR", "en": "Audio only MP3 VBR", "size": est(245.0), "ext": "mp3", "kind": "audio", "approx": true, "suffix": "MP3 VBR"}));
    for k in [320, 256, 192, 128] {
        list.push(json!({"key": format!("a:mp3:{k}"), "label": format!("Audio MP3 {k} kbps"), "en": format!("Audio only MP3 {k} kbps"), "size": est(k as f64), "ext": "mp3", "kind": "audio", "suffix": format!("MP3 {k} KBPS")}));
    }
    list.push(json!({"key": "a:opus", "label": "Audio OPUS", "en": "Audio only OPUS", "size": best_opus.map(|f| size(f)).unwrap_or(est(160.0)), "ext": "opus", "kind": "audio", "fmt": best_opus.map(|f| id_of(f)).unwrap_or_default(), "suffix": format!("OPUS {} KBPS", kbps(best_opus, 160))}));
    list.push(json!({"key": "a:flac", "label": "Audio FLAC", "en": "Audio only FLAC", "size": est(900.0), "ext": "flac", "kind": "audio", "approx": true, "suffix": "FLAC"}));
    // Subtitle: buatan pemilik video dulu, lalu terjemahan otomatis bahasa Indonesia/Inggris.
    let mut subs: Vec<serde_json::Value> = Vec::new();
    if let Some(m) = j["subtitles"].as_object() {
        for (lang, v) in m.iter().filter(|(l, _)| *l != "live_chat") {
            let name = v.as_array().and_then(|a| a.first()).and_then(|x| x["name"].as_str()).unwrap_or(lang);
            subs.push(json!({"lang": lang, "name": name, "auto": false}));
        }
    }
    if let Some(m) = j["automatic_captions"].as_object() {
        for lang in ["id", "en"] {
            if m.contains_key(lang) && !subs.iter().any(|s| s["lang"] == lang) {
                subs.push(json!({"lang": lang, "name": if lang == "id" { "Indonesia (otomatis)" } else { "English (auto)" }, "auto": true}));
            }
        }
    }
    json!({
        "title": j["title"].as_str().unwrap_or(""),
        "name": safe_name(j["title"].as_str().unwrap_or("video")),
        "duration": dur,
        "choices": list,
        "subs": subs,
    })
}

/// Bitrate audio dibulatkan ke angka umum (YouTube M4A 129,5 kbps → 128, Opus 135 → 128/160).
pub fn snap_kbps(abr: f64) -> u32 {
    const STD: [u32; 11] = [32, 48, 64, 96, 128, 160, 192, 224, 256, 320, 384];
    STD.iter().copied().min_by(|a, b| (*a as f64 - abr).abs().total_cmp(&(*b as f64 - abr).abs())).unwrap_or(128)
}

/// Nama berkas yang belum dipakai di `dir` untuk unduhan `key`: "judul.mp4" → "judul (1).mp4"
/// bila sudah ada. Tanpa ini yt-dlp menganggap video "sudah diunduh" dan diam-diam memakai
/// berkas lama (0.8.17: pilih 720p, dapat berkas 480p yang belum diganti namanya).
pub fn unique_name(dir: &Path, name: &str, key: &str) -> String {
    let base = strip_ext(name).trim().to_string();
    let ext = ext_for(key);
    // Video bisa berakhir .mkv (format tidak muat di MP4); subtitle terpisah ikut diperiksa.
    let alts: &[&str] = if key.starts_with("a:") { &[ext] } else { &["mp4", "mkv", "webm"] };
    let taken = |b: &str| {
        alts.iter().any(|e| dir.join(format!("{b}.{e}")).exists() || dir.join(format!("{b}.{e}.part")).exists())
    };
    if !taken(&base) {
        return format!("{base}.{ext}");
    }
    for n in 1..1000 {
        let b = format!("{base} ({n})");
        if !taken(&b) {
            return format!("{b}.{ext}");
        }
    }
    format!("{base}.{ext}")
}

/// Telusuri halaman dengan yt-dlp (`-J`): daftar format, judul, subtitle. Memblokir.
pub fn probe(data: &Path, page_url: &str) -> Result<serde_json::Value, String> {
    let mut cmd = Command::new(ytdlp(data));
    cmd.args(["-J", "--no-playlist", "--no-warnings", "--no-colors"]).args(tool_args(data)).arg(page_url).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    hide_window(&mut cmd);
    let out = cmd.output().map_err(|e| format!("yt-dlp tidak bisa dijalankan: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err.lines().rev().find(|l| l.contains("ERROR")).unwrap_or("halaman ini tidak berisi video yang dikenali").trim();
        return Err(line.trim_start_matches("ERROR: ").chars().take(300).collect());
    }
    let j: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("jawaban yt-dlp tidak terbaca: {e}"))?;
    Ok(choices(&j))
}

/// Mulai mengunduh sesuai `spec`. `cb` dipanggil dari thread latar.
pub fn start(data: &Path, spec: &Spec, cb: impl Fn(Event) + Send + Sync + 'static) -> Result<Job, String> {
    // Nama berkas akhir ditulis ke berkas sementara (--print-to-file), bukan --print: --print
    // membuat yt-dlp diam sehingga langkah [Merger]/[ExtractAudio]/[EmbedSubtitle] dan format
    // yang benar-benar dipilih tidak terlihat di log.
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let result_file = std::env::temp_dir().join(format!("ovner-yt-{}-{}.txt", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let _ = std::fs::remove_file(&result_file);
    let mut cmd = Command::new(ytdlp(data));
    cmd.args(["--newline", "--no-colors", "--no-playlist", "--progress", "--no-mtime"])
        .args(["--progress-template", "download:[ovner] %(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s"])
        .args(["--print-to-file", "after_move:%(filepath)s"])
        .arg(&result_file)
        .args(args_for(spec))
        .arg("-o")
        .arg(Path::new(&spec.dir).join(format!("{}.%(ext)s", strip_ext(&spec.name).replace('%', "%%"))));
    cmd.args(tool_args(data));
    cmd.arg(&spec.page).stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
    hide_window(&mut cmd);
    crate::log(&format!("yt-dlp {} → {}", args_for(spec).join(" "), spec.page));
    let mut child = cmd.spawn().map_err(|e| format!("yt-dlp tidak bisa dijalankan: {e}"))?;
    let (out, err) = (child.stdout.take(), child.stderr.take());
    let job = Job::new(Some(child));
    let cb = Arc::new(cb);
    #[derive(Default)]
    struct Outcome {
        file: Option<String>,
        last_err: String,
        /// Peringatan yang berarti hasilnya tidak sesuai pilihan (ffmpeg tidak ada → video dan
        /// audio tidak digabung, subtitle/konversi audio tidak dikerjakan).
        fatal: Option<String>,
        logged: usize,
    }
    let state = Arc::new(Mutex::new(Outcome::default()));
    let mut readers = Vec::new();
    for stream in [out.map(|o| Box::new(o) as Box<dyn std::io::Read + Send>), err.map(|e| Box::new(e) as Box<dyn std::io::Read + Send>)].into_iter().flatten() {
        let (cb, state) = (cb.clone(), state.clone());
        readers.push(std::thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                match parse_line(&line) {
                    // Selesai baru dilaporkan setelah yt-dlp keluar dan hasilnya diperiksa.
                    Some(Event::Done { path, .. }) => state.lock().unwrap().file = Some(path),
                    Some(ev) => cb(ev),
                    None => {
                        let l = line.trim();
                        if std::env::var_os("OVNER_DEBUG_YTDLP").is_some() {
                            eprintln!("yt-dlp> {l}");
                        }
                        let mut st = state.lock().unwrap();
                        // Catat langkah penting ke ovner.log untuk laporan masalah.
                        let important = l.starts_with("WARNING") || l.starts_with("ERROR") || l.starts_with("[Merger]") || l.starts_with("[ExtractAudio]")
                            || l.starts_with("[EmbedSubtitle]") || l.starts_with("[SubtitlesConvertor]") || l.starts_with("[VideoRemuxer]")
                            || (l.starts_with("[info]") && (l.contains("format(s)") || l.contains("subtitle")));
                        if important && st.logged < 40 {
                            st.logged += 1;
                            crate::log(&format!("yt-dlp: {}", l.chars().take(300).collect::<String>()));
                        }
                        let low = l.to_ascii_lowercase();
                        if low.contains("ffmpeg is not installed") || low.contains("won't be merged") || low.contains("ffprobe and ffmpeg not found") || low.contains("ffmpeg not found") {
                            st.fatal = Some("ffmpeg tidak terpakai: video dan audio/subtitle tidak bisa digabung".into());
                        }
                        if l.starts_with("ERROR") {
                            st.last_err = l.trim_start_matches("ERROR: ").chars().take(300).collect();
                        }
                    }
                }
            }
        }));
    }
    let j2 = job.clone();
    let (spec2, data2) = (spec.clone(), data.to_path_buf());
    std::thread::spawn(move || {
        for r in readers {
            let _ = r.join();
        }
        let status = j2.0.lock().unwrap().as_mut().and_then(|c| c.wait().ok());
        let ok = status.is_some_and(|s| s.success());
        #[allow(unused_mut)]
        let mut st = state.lock().unwrap();
        if let Ok(t) = std::fs::read_to_string(&result_file) {
            if let Some(p) = t.lines().rev().map(str::trim).find(|l| !l.is_empty()) {
                st.file = Some(p.to_string());
            }
        }
        let _ = std::fs::remove_file(&result_file);
        // Video tersimpan: subtitle diambil terpisah. Gagal subtitle tidak membuang videonya.
        let mut sub_err = String::new();
        let video_ok = ok && st.fatal.is_none() && st.file.as_deref().is_some_and(|p| Path::new(p).exists());
        if video_ok && wants_subs(&spec2) {
            let file = PathBuf::from(st.file.clone().unwrap_or_default());
            drop(st);
            cb(Event::Note("subtitle…".into()));
            let jc = j2.clone();
            match subs_step(&data2, &spec2, &file, Some(&j2), &move || jc.is_cancelled()) {
                Ok(m) => crate::log(&format!("subtitle: {m}")),
                Err(e) => {
                    crate::log(&format!("subtitle gagal (video tetap tersimpan): {e}"));
                    sub_err = e;
                }
            }
            st = state.lock().unwrap();
        }
        if !sub_err.is_empty() {
            cb(Event::SubsFailed(sub_err.clone()));
        }
        let ev = match (&st.file, &st.fatal) {
            (_, Some(f)) => Event::Done { ok: false, path: st.file.clone().unwrap_or_default(), error: f.clone() },
            (Some(p), None) if ok && Path::new(p).exists() => Event::Done { ok: true, path: p.clone(), error: if sub_err.is_empty() { String::new() } else { format!("subtitle gagal: {sub_err}") } },
            (Some(p), None) if ok => Event::Done { ok: false, path: p.clone(), error: "berkas hasil tidak ditemukan".into() },
            _ => Event::Done {
                ok: false,
                path: st.file.clone().unwrap_or_default(),
                error: if !st.last_err.is_empty() { st.last_err.clone() } else if ok { "berkas tidak ditemukan".into() } else { "dibatalkan".into() },
            },
        };
        cb(ev);
    });
    Ok(job)
}

/// Subtitle terpisah yt-dlp bernama "judul.id.srt"; Media Player Windows dan banyak pemutar
/// lain mencari "judul.srt" di samping videonya. Bila hanya ada satu, namanya disamakan.
pub fn sidecar_srt(video: &Path) -> Option<PathBuf> {
    let (dir, stem) = (video.parent()?, video.file_stem()?.to_string_lossy().to_string());
    let want = dir.join(format!("{stem}.srt"));
    if want.exists() {
        return Some(want);
    }
    let found: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let n = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            n.len() > stem.len() + 5
                && n.starts_with(&stem)
                && n[stem.len()..].starts_with('.')
                && n.to_ascii_lowercase().ends_with(".srt")
                && !n[stem.len() + 1..n.len() - 4].contains('.')
        })
        .collect();
    if found.len() == 1 && std::fs::rename(&found[0], &want).is_ok() {
        crate::log(&format!("subtitle disimpan di samping video: {}", want.display()));
        return Some(want);
    }
    None
}

/// Resolusi dan codec video hasil unduhan (ffprobe), untuk dicatat dan dibandingkan dengan pilihan.
pub fn probe_result(data: &Path, file: &Path) -> Option<(u64, String)> {
    let exe = ffmpeg_dir(data).join(if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" });
    let exe = if exe.exists() { exe } else { PathBuf::from("ffprobe") };
    let mut cmd = Command::new(exe);
    cmd.args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=height,codec_name", "-of", "csv=p=0"]).arg(file);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    hide_window(&mut cmd);
    let o = cmd.output().ok()?;
    let line = String::from_utf8_lossy(&o.stdout).lines().next()?.trim().to_string();
    let mut it = line.split(',');
    let codec = it.next()?.to_string();
    let h = it.next()?.trim().parse().ok()?;
    Some((h, codec))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_and_lines() {
        assert!(is_video_page("https://www.youtube.com/watch?v=GRsoTIIZBNM"));
        assert!(is_video_page("https://youtu.be/abc"));
        assert!(!is_video_page("https://www.youtube.com/"));
        assert!(!is_video_page("https://sipp.bpjsketenagakerjaan.go.id/"));
        assert_eq!(
            parse_line("[ovner] 1048576|10485760|NA|524288.5|18"),
            Some(Event::Progress { received: 1048576, total: 10485760, speed: 524288, eta: 18 })
        );
        assert_eq!(parse_line("[ovner] 100|NA|2000|NA|NA"), Some(Event::Progress { received: 100, total: 2000, speed: 0, eta: 0 }));
        assert_eq!(parse_line("[ovner-title] Naruto OP 1"), Some(Event::Title("Naruto OP 1".into())));
        assert!(matches!(parse_line("[ovner-file] C:\\Users\\x\\Downloads\\a.mp4"), Some(Event::Done { ok: true, .. })));
        assert_eq!(parse_line("[youtube] abc: Downloading webpage"), None);
    }

    /// Uji ujung ke ujung (tanpa internet): yt-dlp dan ffmpeg di OVNER_VIDEO_E2E_DATA/tools.
    /// OVNER_VIDEO_E2E_DASH = manifest DASH lokal (video 1080p+360p dan audio terpisah, seperti
    /// YouTube); OVNER_VIDEO_E2E_SUBS = halaman lokal dengan <video> + <track> subtitle.
    fn run_spec(data: &Path, spec: &Spec) -> Event {
        let (tx, rx) = std::sync::mpsc::channel();
        let _job = start(data, spec, move |ev| {
            let _ = tx.send(ev);
        })
        .unwrap();
        loop {
            match rx.recv_timeout(std::time::Duration::from_secs(90)).expect("kejadian") {
                ev @ Event::Done { .. } => return ev,
                _ => {}
            }
        }
    }

    #[test]
    #[ignore]
    fn video_e2e() {
        let data = PathBuf::from(std::env::var("OVNER_VIDEO_E2E_DATA").unwrap());
        let url = std::env::var("OVNER_VIDEO_E2E_DASH").unwrap();
        let info = probe(&data, &url).expect("probe");
        eprintln!("{info}");
        let out = data.join("out");
        let _ = std::fs::remove_dir_all(&out);
        for c in info["choices"].as_array().unwrap() {
            let key = c["key"].as_str().unwrap().to_string();
            let spec = Spec {
                page: url.clone(),
                key: key.clone(),
                fmt: c["fmt"].as_str().unwrap_or("").to_string(),
                dir: out.display().to_string(),
                name: format!("tes {key}.mp4").replace(':', "_"),
                ..Default::default()
            };
            let done = run_spec(&data, &spec);
            eprintln!("{key} ({}): {done:?}", spec.fmt);
            let Event::Done { ok, path, .. } = done else { unreachable!() };
            assert!(ok, "{key}");
            assert!(path.ends_with(&format!(".{}", ext_for(&key))), "{key}: {path}");
            if let Some(h) = key.strip_prefix("v:").and_then(|h| h.parse::<u64>().ok()) {
                let (got, codec) = probe_result(&data, Path::new(&path)).expect("ffprobe");
                assert_eq!(got, h, "{key}: tinggi hasil");
                // Ada suara (video dan audio tergabung).
                let o = Command::new(ffmpeg_dir(&data).join("ffprobe")).args(["-v", "error", "-select_streams", "a", "-show_entries", "stream=codec_name", "-of", "csv=p=0"]).arg(&path).output().unwrap();
                assert!(String::from_utf8_lossy(&o.stdout).contains("aac"), "{key}: audio ({codec})");
            }
        }
        // Tidak ada sisa berkas potongan (.f0.mp4, .m4a sementara).
        let left: Vec<String> = std::fs::read_dir(&out).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.split('.').any(|p| p.len() > 1 && p.starts_with('f') && p[1..].chars().all(|c| c.is_ascii_digit()))).collect();
        assert!(left.is_empty(), "{left:?}");

        // Subtitle dimasukkan ke video, tanpa .srt tertinggal; dan versi berkas terpisah.
        let page = std::env::var("OVNER_VIDEO_E2E_SUBS").unwrap();
        let info = probe(&data, &page).expect("probe subs");
        eprintln!("subs: {}", info["subs"]);
        let lang = info["subs"][0]["lang"].as_str().expect("ada subtitle").to_string();
        let sub_out = data.join("out-subs");
        let _ = std::fs::remove_dir_all(&sub_out);
        let spec = Spec { page: page.clone(), key: "v:best".into(), dir: sub_out.display().to_string(), name: "sub".into(), sub: lang.clone(), embed: true, ..Default::default() };
        let Event::Done { ok, path, error } = run_spec(&data, &spec) else { unreachable!() };
        assert!(ok, "{error}");
        let o = Command::new(ffmpeg_dir(&data).join("ffprobe")).args(["-v", "error", "-select_streams", "s", "-show_entries", "stream=codec_name", "-of", "csv=p=0"]).arg(&path).output().unwrap();
        assert!(String::from_utf8_lossy(&o.stdout).contains("mov_text"), "subtitle tertanam: {:?}", String::from_utf8_lossy(&o.stdout));
        let srt: Vec<_> = std::fs::read_dir(&sub_out).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".srt")).collect();
        assert!(srt.is_empty(), "srt tertinggal");
        // Disematkan + .srt bernama sama untuk Media Player Windows.
        let keep = Spec { keep_srt: true, name: "sub3".into(), ..spec.clone() };
        let Event::Done { ok, path, error } = run_spec(&data, &keep) else { unreachable!() };
        assert!(ok, "{error}");
        assert!(sub_out.join("sub3.srt").exists(), "srt bernama sama: {path}");
        let o = Command::new(ffmpeg_dir(&data).join("ffprobe")).args(["-v", "error", "-select_streams", "s", "-show_entries", "stream_disposition=default", "-of", "csv=p=0"]).arg(&path).output().unwrap();
        assert!(String::from_utf8_lossy(&o.stdout).trim().starts_with('1'), "trek default: {:?}", String::from_utf8_lossy(&o.stdout));
        let spec2 = Spec { embed: false, name: "sub2".into(), ..spec };
        let Event::Done { ok, .. } = run_spec(&data, &spec2) else { unreachable!() };
        assert!(ok);
        assert!(sub_out.join("sub2.srt").exists(), "srt terpisah, bernama sama dengan videonya ({lang})");
    }

    /// Subtitle ditolak server (HTTP 429, seperti YouTube): video tetap tersimpan, subtitle bisa
    /// dicoba lagi; 429 sekali saja → percobaan ulang otomatis berhasil.
    /// OVNER_VIDEO_E2E_SUBS429 = halaman yang subtitlenya selalu 429;
    /// OVNER_VIDEO_E2E_SUBSFLAKY = halaman yang subtitlenya 429 pada permintaan pertama saja.
    #[test]
    #[ignore]
    fn video_subs_429() {
        let data = PathBuf::from(std::env::var("OVNER_VIDEO_E2E_DATA").unwrap());
        let out = data.join("out-429");
        let _ = std::fs::remove_dir_all(&out);
        let page = std::env::var("OVNER_VIDEO_E2E_SUBS429").unwrap();
        let spec = Spec { page: page.clone(), key: "v:best".into(), dir: out.display().to_string(), name: "v429.mp4".into(), sub: "id".into(), embed: true, keep_srt: true, ..Default::default() };
        let (tx, rx) = std::sync::mpsc::channel();
        let _job = start(&data, &spec, move |ev| {
            let _ = tx.send(ev);
        })
        .unwrap();
        let mut failed = None;
        let done = loop {
            match rx.recv_timeout(std::time::Duration::from_secs(120)).expect("selesai") {
                Event::SubsFailed(m) => failed = Some(m),
                ev @ Event::Done { .. } => break ev,
                _ => {}
            }
        };
        let Event::Done { ok, path, error } = done else { unreachable!() };
        assert!(ok, "video tetap tersimpan: {error}");
        assert!(Path::new(&path).exists());
        assert!(error.starts_with("subtitle gagal") && error.contains("429"), "{error}");
        assert!(failed.is_some_and(|m| m.contains("429")));
        // Coba lagi: masih 429 → pesan jelas, video tidak rusak.
        let e = retry_subs(&data, &spec, Path::new(&path)).unwrap_err();
        assert!(e.contains("429"), "{e}");
        assert!(probe_result(&data, Path::new(&path)).is_some(), "video masih utuh");

        // 429 sekali saja: percobaan ulang otomatis (jeda 5 detik) berhasil dan tersemat.
        let flaky = std::env::var("OVNER_VIDEO_E2E_SUBSFLAKY").unwrap();
        let spec = Spec { page: flaky, name: "vflaky.mp4".into(), ..spec };
        let Event::Done { ok, path, error } = run_spec(&data, &spec) else { unreachable!() };
        assert!(ok && error.is_empty(), "{error}");
        let o = Command::new(ffmpeg_dir(&data).join("ffprobe")).args(["-v", "error", "-select_streams", "s", "-show_entries", "stream=codec_name:stream_tags=language:stream_disposition=default", "-of", "csv=p=0"]).arg(&path).output().unwrap();
        let so = String::from_utf8_lossy(&o.stdout).to_string();
        assert!(so.contains("mov_text") && so.contains("ind"), "subtitle tertanam berbahasa ind: {so:?}");
        assert!(out.join("vflaky.srt").exists(), ".srt bernama sama");
        let vtt: Vec<_> = std::fs::read_dir(&out).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".vtt")).collect();
        assert!(vtt.is_empty(), "sisa .vtt");
    }

    /// Tanpa ffmpeg: penggabungan tidak boleh dilaporkan berhasil.
    #[test]
    #[ignore]
    fn video_without_ffmpeg_fails_clearly() {
        let data = PathBuf::from(std::env::var("OVNER_VIDEO_E2E_NOFF").unwrap());
        let url = std::env::var("OVNER_VIDEO_E2E_DASH").unwrap();
        let spec = Spec { page: url, key: "v:1080".into(), fmt: "0+2".into(), dir: data.join("out").display().to_string(), name: "x".into(), ..Default::default() };
        let Event::Done { ok, error, .. } = run_spec(&data, &spec) else { unreachable!() };
        assert!(!ok);
        assert!(error.contains("ffmpeg"), "{error}");
    }

    /// Dua penelusuran bersamaan saat yt-dlp belum ada (dulu: "Operation aborted 0x80004004").
    #[test]
    #[ignore]
    fn tools_parallel() {
        let data = std::env::temp_dir().join(format!("ovner-tools-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        let hs: Vec<_> = (0..3)
            .map(|_| {
                let d = data.clone();
                std::thread::spawn(move || ensure_tools(&d, Needs::default(), &|_| {}))
            })
            .collect();
        for h in hs {
            assert_eq!(h.join().unwrap(), Ok(()));
        }
        assert!(ytdlp(&data).exists());
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn choices_and_args() {
        let j: serde_json::Value = serde_json::from_str(r#"{"title":"Avengers: Doomsday | Credits?","duration":766,
          "formats":[
            {"format_id":"140","ext":"m4a","vcodec":"none","acodec":"mp4a.40.2","abr":129.5,"filesize":12400000},
            {"format_id":"251","ext":"webm","vcodec":"none","acodec":"opus","abr":160,"filesize":15320000},
            {"format_id":"137","ext":"mp4","vcodec":"avc1","acodec":"none","height":1080,"filesize":249000000},
            {"format_id":"248","ext":"webm","vcodec":"vp9","acodec":"none","height":1080,"filesize":300000000},
            {"format_id":"18","ext":"mp4","vcodec":"avc1","acodec":"mp4a","height":360,"filesize_approx":44000000},
            {"format_id":"sb0","ext":"mhtml","vcodec":"none","acodec":"none","height":90}
          ],
          "subtitles":{"en":[{"name":"English"}],"live_chat":[{}]},
          "automatic_captions":{"id":[{}],"en":[{}]}}"#).unwrap();
        let c = choices(&j);
        assert_eq!(c["name"], "Avengers_ Doomsday _ Credits_");
        let list = c["choices"].as_array().unwrap();
        assert_eq!(list[0]["key"], "v:1080");
        assert_eq!(list[0]["size"], 249000000 + 12400000, "MP4 1080p + audio M4A");
        assert_eq!(list[0]["fmt"], "137+140", "H.264 MP4 + M4A, bukan VP9");
        assert_eq!(list[1]["fmt"], "18");
        assert_eq!(list[1]["key"], "v:360");
        assert_eq!(list[1]["size"], 44000000, "progresif: sudah ada suara");
        assert!(list.iter().any(|x| x["key"] == "a:opus" && x["size"] == 15320000));
        assert!(list.iter().any(|x| x["key"] == "a:mp3:320" && x["size"] == 30640000));
        let subs = c["subs"].as_array().unwrap();
        assert_eq!(subs.len(), 2, "en asli + id otomatis: {subs:?}");
        let spec = Spec { page: "u".into(), key: "a:mp3:320".into(), dir: "d".into(), name: "a.mp4".into(), sub: "en".into(), embed: true, ..Default::default() };
        let a = args_for(&spec);
        assert!(a.windows(2).any(|w| w[0] == "--audio-quality" && w[1] == "320K"));
        assert!(!a.contains(&"--embed-subs".to_string()), "audio tanpa subtitle");
        let v = Spec { key: "v:1080".into(), fmt: "137+140".into(), ..spec.clone() };
        let va = args_for(&v);
        // Subtitle tidak lagi ikut unduhan video (gagal 429 dulu membatalkan videonya).
        assert!(!va.iter().any(|x| x.contains("sub")), "{va:?}");
        assert!(va[1].starts_with("137+140/"), "{va:?}");
        assert!(!va[1].contains("height<="), "tidak jatuh ke kualitas lebih rendah: {va:?}");
        assert!(wants_subs(&v) && !wants_subs(&spec), "audio tanpa subtitle");
        let sa = subs_args(&v, "Judul 1080p");
        assert!(sa.contains(&"--skip-download".to_string()) && sa.contains(&"--write-subs".to_string()) && !sa.contains(&"--write-auto-subs".to_string()), "{sa:?}");
        assert!(sa.last().unwrap().ends_with("Judul 1080p.%(ext)s"), "{sa:?}");
        let auto = subs_args(&Spec { sub_auto: true, ..v.clone() }, "x");
        assert!(auto.contains(&"--write-auto-subs".to_string()) && !auto.contains(&"--write-subs".to_string()));
        assert_eq!(lang3("id"), Some("ind"));
        assert_eq!(lang3("en-US"), Some("eng"));
        assert_eq!(lang3("xx"), None);
        assert_eq!(strip_ext("Lagu.MP4"), "Lagu");
        assert_eq!(ext_for("a:mp3:0"), "mp3");
        assert!(!Spec { key: "a:m4a".into(), ..spec }.needs_ffmpeg());
        // Akhiran nama berkas: resolusi untuk video, format + kualitas (huruf besar) untuk audio.
        let sfx = |key: &str| list.iter().find(|x| x["key"] == key).map(|x| x["suffix"].as_str().unwrap_or("").to_string()).unwrap();
        assert_eq!(sfx("v:1080"), "1080p");
        assert_eq!(sfx("v:360"), "360p");
        assert_eq!(sfx("a:m4a"), "M4A 128 KBPS", "129,5 kbps dibulatkan");
        assert_eq!(sfx("a:mp3:0"), "MP3 VBR");
        assert_eq!(sfx("a:mp3:320"), "MP3 320 KBPS");
        assert_eq!(sfx("a:opus"), "OPUS 160 KBPS");
        assert_eq!(sfx("a:flac"), "FLAC");
    }

    #[test]
    fn unique_names_and_sidecar_srt() {
        let dir = std::env::temp_dir().join(format!("ovner-uniq-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(unique_name(&dir, "Judul 720p.mp4", "v:720"), "Judul 720p.mp4");
        // Berkas lama dengan nama sama (mis. hasil 480p yang belum diganti namanya).
        std::fs::write(dir.join("Judul 720p.mp4"), b"x").unwrap();
        assert_eq!(unique_name(&dir, "Judul 720p.mp4", "v:720"), "Judul 720p (1).mp4");
        std::fs::write(dir.join("Judul 720p (1).mkv"), b"x").unwrap();
        assert_eq!(unique_name(&dir, "Judul 720p", "v:720"), "Judul 720p (2).mp4", "MKV ikut dihitung");
        assert_eq!(unique_name(&dir, "Judul 720p.mp3", "a:mp3:320"), "Judul 720p.mp3", "audio hanya melihat ekstensinya");
        // judul.id.srt → judul.srt (dibaca Media Player Windows), berkas lain tidak tersentuh.
        let v = dir.join("Judul 1080p.mp4");
        std::fs::write(&v, b"x").unwrap();
        std::fs::write(dir.join("Judul 1080p.id.srt"), b"1").unwrap();
        std::fs::write(dir.join("Judul 1080p (1).en.srt"), b"1").unwrap();
        assert_eq!(sidecar_srt(&v), Some(dir.join("Judul 1080p.srt")));
        assert!(!dir.join("Judul 1080p.id.srt").exists());
        assert!(dir.join("Judul 1080p (1).en.srt").exists());
        assert_eq!(snap_kbps(129.5), 128);
        assert_eq!(snap_kbps(150.0), 160);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
