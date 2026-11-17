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
    /// Nama berkas akhir (untuk menghapus sisa .part bila dibatalkan).
    Path(String),
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
    if let Some(t) = line.split("[ovner-path] ").nth(1) {
        return Some(Event::Path(t.trim().to_string()));
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
}

impl Spec {
    pub fn is_audio(&self) -> bool {
        self.key.starts_with("a:")
    }
    /// Butuh ffmpeg: menggabungkan video+audio, mengubah format audio, atau memasukkan subtitle.
    pub fn needs_ffmpeg(&self) -> bool {
        self.key != "a:m4a"
    }
}

/// Ekstensi berkas hasil untuk sebuah pilihan.
pub fn ext_for(key: &str) -> &'static str {
    match key {
        "a:m4a" => "m4a",
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
    let mut f = |fmt: &str| {
        a.push("-f".into());
        a.push(fmt.into());
    };
    let key = spec.key.as_str();
    if let Some(h) = key.strip_prefix("v:").and_then(|h| h.parse::<u32>().ok()) {
        f(&format!("bv*[height={h}][ext=mp4]+ba[ext=m4a]/bv*[height={h}]+ba[ext=m4a]/bv*[height={h}]+ba/b[height={h}]/bv*[height<={h}]+ba/b"));
        a.extend(["--merge-output-format".into(), "mp4".into()]);
    } else if key == "a:m4a" {
        f("ba[ext=m4a]/ba");
    } else if key == "a:opus" {
        f("ba[acodec=opus]/ba");
        a.extend(["-x".into(), "--audio-format".into(), "opus".into()]);
    } else if key == "a:flac" {
        f("ba");
        a.extend(["-x".into(), "--audio-format".into(), "flac".into()]);
    } else if let Some(q) = key.strip_prefix("a:mp3:") {
        f("ba");
        let q = if q == "0" { "0".to_string() } else { format!("{}K", q.parse::<u32>().unwrap_or(192)) };
        a.extend(["-x".into(), "--audio-format".into(), "mp3".into(), "--audio-quality".into(), q]);
    } else {
        f("bv*[ext=mp4]+ba[ext=m4a]/b[ext=mp4]/b");
        a.extend(["--merge-output-format".into(), "mp4".into()]);
    }
    if !spec.sub.is_empty() && !spec.is_audio() {
        a.extend(["--write-subs".into(), "--write-auto-subs".into(), "--sub-langs".into(), spec.sub.clone(), "--convert-subs".into(), "srt".into()]);
        if spec.embed {
            a.push("--embed-subs".into());
        }
    }
    a
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
    // Video per tinggi: MP4 diutamakan, yang paling tajam (tbr terbesar).
    let mut by_h: std::collections::BTreeMap<u64, (u64, bool, bool)> = std::collections::BTreeMap::new();
    for f in fmts.iter().filter(|f| !is_none(&f["vcodec"])) {
        let Some(h) = f["height"].as_u64().filter(|h| *h >= 100) else { continue };
        if f["protocol"].as_str().is_some_and(|p| p.contains("m3u8") || p.contains("dash") && false) && f["ext"] != "mp4" {
            continue;
        }
        let mp4 = f["ext"] == "mp4";
        let with_audio = !is_none(&f["acodec"]);
        let sz = size(f) + if with_audio { 0 } else { a_size };
        let e = by_h.entry(h).or_insert((0, false, false));
        // Ganti bila lebih baik: MP4 mengalahkan non-MP4, lalu ukuran terbesar.
        if (mp4 && !e.1) || (mp4 == e.1 && sz > e.0) {
            *e = (sz, mp4, with_audio);
        }
    }
    let mut list: Vec<serde_json::Value> = by_h
        .iter()
        .rev()
        .map(|(h, (sz, _, _))| json!({"key": format!("v:{h}"), "label": format!("{h}p MP4"), "size": sz, "ext": "mp4", "kind": "video"}))
        .collect();
    if list.is_empty() && !fmts.is_empty() {
        list.push(json!({"key": "v:best", "label": "MP4", "size": fmts.last().map(size).unwrap_or(0), "ext": "mp4", "kind": "video"}));
    }
    let est = |kbps: f64| (kbps * 1000.0 / 8.0 * dur) as u64;
    list.push(json!({"key": "a:m4a", "label": "Audio, M4A asli", "en": "Audio only, original M4A", "size": a_size, "ext": "m4a", "kind": "audio"}));
    list.push(json!({"key": "a:mp3:0", "label": "Audio MP3 VBR", "en": "Audio only MP3 VBR", "size": est(245.0), "ext": "mp3", "kind": "audio", "approx": true}));
    for k in [320, 256, 192, 128] {
        list.push(json!({"key": format!("a:mp3:{k}"), "label": format!("Audio MP3 {k} kbps"), "en": format!("Audio only MP3 {k} kbps"), "size": est(k as f64), "ext": "mp3", "kind": "audio"}));
    }
    list.push(json!({"key": "a:opus", "label": "Audio OPUS", "en": "Audio only OPUS", "size": best_opus.map(|f| size(f)).unwrap_or(est(160.0)), "ext": "opus", "kind": "audio"}));
    list.push(json!({"key": "a:flac", "label": "Audio FLAC", "en": "Audio only FLAC", "size": est(900.0), "ext": "flac", "kind": "audio", "approx": true}));
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

/// Telusuri halaman dengan yt-dlp (`-J`): daftar format, judul, subtitle. Memblokir.
pub fn probe(data: &Path, page_url: &str) -> Result<serde_json::Value, String> {
    let mut cmd = Command::new(ytdlp(data));
    cmd.args(["-J", "--no-playlist", "--no-warnings", "--no-colors"]).arg(page_url).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
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
    let have_ff = ffmpeg(data).exists();
    let mut cmd = Command::new(ytdlp(data));
    cmd.args(["--newline", "--no-colors", "--no-playlist", "--no-simulate", "--progress", "--no-mtime"])
        .args(["--progress-template", "download:[ovner] %(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s"])
        .args(["--print", "before_dl:[ovner-title] %(title)s"])
        .args(["--print", "before_dl:[ovner-path] %(filename)s"])
        .args(["--print", "after_move:[ovner-file] %(filepath)s"])
        .args(args_for(spec))
        .arg("-o")
        .arg(Path::new(&spec.dir).join(format!("{}.%(ext)s", strip_ext(&spec.name).replace('%', "%%"))));
    if have_ff {
        cmd.arg("--ffmpeg-location").arg(ffmpeg(data));
    }
    cmd.arg(&spec.page).stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
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
        assert_eq!(list[1]["key"], "v:360");
        assert_eq!(list[1]["size"], 44000000, "progresif: sudah ada suara");
        assert!(list.iter().any(|x| x["key"] == "a:opus" && x["size"] == 15320000));
        assert!(list.iter().any(|x| x["key"] == "a:mp3:320" && x["size"] == 30640000));
        let subs = c["subs"].as_array().unwrap();
        assert_eq!(subs.len(), 2, "en asli + id otomatis: {subs:?}");
        let spec = Spec { page: "u".into(), key: "a:mp3:320".into(), dir: "d".into(), name: "a.mp4".into(), sub: "en".into(), embed: true };
        let a = args_for(&spec);
        assert!(a.windows(2).any(|w| w[0] == "--audio-quality" && w[1] == "320K"));
        assert!(!a.contains(&"--embed-subs".to_string()), "audio tanpa subtitle");
        let v = Spec { key: "v:1080".into(), ..spec.clone() };
        assert!(args_for(&v).contains(&"--embed-subs".to_string()));
        assert_eq!(strip_ext("Lagu.MP4"), "Lagu");
        assert_eq!(ext_for("a:mp3:0"), "mp3");
        assert!(!Spec { key: "a:m4a".into(), ..spec }.needs_ffmpeg());
    }
}
