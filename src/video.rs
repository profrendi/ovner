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
pub const FFMPEG_URL: &str = "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-lgpl.zip";

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

pub fn ffmpeg(data: &Path) -> PathBuf {
    tools_dir(data).join(if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" })
}

/// Format yt-dlp untuk pilihan kualitas. Tanpa ffmpeg hanya video "progresif" (satu berkas).
pub fn format_for(choice: &str, have_ffmpeg: bool) -> (&'static str, bool) {
    match (choice, have_ffmpeg) {
        ("audio", _) => ("ba[ext=m4a]/ba", false),
        ("720", true) => ("bv*[height<=720][ext=mp4]+ba[ext=m4a]/b[height<=720][ext=mp4]/b[height<=720]", true),
        ("720", false) => ("b[height<=720][ext=mp4]/b[ext=mp4]/b", false),
        (_, true) => ("bv*[height<=1080][ext=mp4]+ba[ext=m4a]/b[height<=1080][ext=mp4]/b", true),
        (_, false) => ("b[ext=mp4]/b", false),
    }
}

/// Pastikan yt-dlp (dan ffmpeg bila `need_ffmpeg`) ada. Memblokir: panggil dari thread latar.
pub fn ensure_tools(data: &Path, need_ffmpeg: bool, note: &dyn Fn(&str)) -> Result<(), String> {
    let dir = tools_dir(data);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let y = ytdlp(data);
    if !y.exists() {
        note("yt-dlp");
        let tmp = dir.join("yt-dlp.download");
        crate::win::download(YTDLP_URL, &tmp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
        }
        std::fs::rename(&tmp, &y).map_err(|e| e.to_string())?;
    }
    let f = ffmpeg(data);
    if need_ffmpeg && !f.exists() {
        note("ffmpeg");
        let zip = dir.join("ffmpeg.zip");
        crate::win::download(FFMPEG_URL, &zip)?;
        let out = dir.join("ffmpeg-extract");
        let _ = std::fs::remove_dir_all(&out);
        std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        // tar bawaan Windows 10/11 (bsdtar) bisa membuka zip.
        let mut cmd = Command::new("tar");
        cmd.arg("-xf").arg(&zip).arg("-C").arg(&out);
        hide_window(&mut cmd);
        let ok = cmd.status().map(|s| s.success()).unwrap_or(false);
        let found = ok.then(|| find_file(&out, if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" })).flatten();
        let res = match found {
            Some(p) => std::fs::rename(&p, &f).or_else(|_| std::fs::copy(&p, &f).map(|_| ())).map_err(|e| e.to_string()),
            None => Err("ffmpeg tidak ditemukan di arsip".into()),
        };
        let _ = std::fs::remove_dir_all(&out);
        let _ = std::fs::remove_file(&zip);
        res?;
    }
    Ok(())
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
    Progress { received: u64, total: u64, speed: u64, eta: u64 },
    Done { ok: bool, path: String, error: String },
}

/// Satu baris keluaran yt-dlp → kejadian (baris lain diabaikan).
pub fn parse_line(line: &str) -> Option<Event> {
    if let Some(rest) = line.split("[ovner] ").nth(1) {
        let f: Vec<&str> = rest.trim().split('|').collect();
        let num = |i: usize| f.get(i).and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(0.0) as u64;
        let total = if num(1) > 0 { num(1) } else { num(2) };
        return Some(Event::Progress { received: num(0), total, speed: num(3), eta: num(4) });
    }
    if let Some(t) = line.split("[ovner-title] ").nth(1) {
        return Some(Event::Title(t.trim().to_string()));
    }
    if let Some(p) = line.split("[ovner-file] ").nth(1) {
        return Some(Event::Done { ok: true, path: p.trim().to_string(), error: String::new() });
    }
    None
}

/// Proses yt-dlp yang berjalan (untuk dibatalkan).
#[derive(Clone)]
pub struct Job(Arc<Mutex<Option<Child>>>);

impl Job {
    pub fn cancel(&self) {
        if let Some(c) = self.0.lock().unwrap().as_mut() {
            let _ = c.kill();
        }
    }
}

/// Mulai mengunduh `page_url` ke `out_dir`. `cb` dipanggil dari thread latar.
pub fn start(data: &Path, page_url: &str, choice: &str, out_dir: &Path, cb: impl Fn(Event) + Send + Sync + 'static) -> Result<Job, String> {
    let have_ff = ffmpeg(data).exists();
    let (fmt, merge) = format_for(choice, have_ff);
    let mut cmd = Command::new(ytdlp(data));
    cmd.args(["--newline", "--no-colors", "--no-playlist", "--no-simulate", "--progress", "--no-mtime"])
        .args(["--progress-template", "download:[ovner] %(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s"])
        .args(["--print", "before_dl:[ovner-title] %(title)s"])
        .args(["--print", "after_move:[ovner-file] %(filepath)s"])
        .arg("-f")
        .arg(fmt)
        .arg("-o")
        .arg(out_dir.join("%(title).150B [%(id)s].%(ext)s"));
    if have_ff {
        cmd.arg("--ffmpeg-location").arg(ffmpeg(data));
    }
    if merge {
        cmd.args(["--merge-output-format", "mp4"]);
    }
    cmd.arg(page_url).stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
    hide_window(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| format!("yt-dlp tidak bisa dijalankan: {e}"))?;
    let (out, err) = (child.stdout.take(), child.stderr.take());
    let job = Job(Arc::new(Mutex::new(Some(child))));
    let cb = Arc::new(cb);
    let last_err = Arc::new(Mutex::new(String::new()));
    let done = Arc::new(Mutex::new(false));
    let mut readers = Vec::new();
    for stream in [out.map(|o| Box::new(o) as Box<dyn std::io::Read + Send>), err.map(|e| Box::new(e) as Box<dyn std::io::Read + Send>)].into_iter().flatten() {
        let (cb, last_err, done) = (cb.clone(), last_err.clone(), done.clone());
        readers.push(std::thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                match parse_line(&line) {
                    Some(ev @ Event::Done { .. }) => {
                        *done.lock().unwrap() = true;
                        cb(ev);
                    }
                    Some(ev) => cb(ev),
                    None if line.contains("ERROR") => *last_err.lock().unwrap() = line.trim().chars().take(300).collect(),
                    None => {}
                }
            }
        }));
    }
    let j2 = job.clone();
    std::thread::spawn(move || {
        for r in readers {
            let _ = r.join();
        }
        let status = j2.0.lock().unwrap().as_mut().and_then(|c| c.wait().ok());
        if !*done.lock().unwrap() {
            let ok = status.is_some_and(|s| s.success());
            let e = last_err.lock().unwrap().clone();
            cb(Event::Done { ok: false, path: String::new(), error: if ok { "berkas tidak ditemukan".into() } else if e.is_empty() { "dibatalkan".into() } else { e } });
        }
    });
    Ok(job)
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
        assert_eq!(format_for("best", false).0, "b[ext=mp4]/b");
        assert!(format_for("best", true).1);
    }
}
