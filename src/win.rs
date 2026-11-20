//! Fungsi khusus Windows, semuanya di level pengguna (HKCU, tanpa hak admin).
//! Di platform lain berupa stub supaya proyek tetap bisa dicek dan diuji di Linux.

use std::path::Path;

pub const AUMID: &str = "Ovner.Browser";
pub const APP_REG_NAME: &str = "Ovner";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Satu proses Ovner/WebView2 untuk task manager.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Proc {
    pub pid: u32,
    /// Private working set (kolom Memory di Task Manager), byte.
    pub working: u64,
    /// Private bytes / commit, byte.
    pub commit: u64,
    pub is_self: bool,
}

/// Kejadian unduhan (seperti panel unduhan Brave).
#[derive(Debug, Clone)]
pub enum DlEvent {
    Started { id: u64, url: String, path: String, total: i64 },
    /// Unduhan ditahan: mesin unduh Ovner boleh menguji server lalu memutuskan lewat
    /// `resolve_download`. `cookie` = tajuk Cookie situs untuk alamat ini.
    Offer { id: u64, url: String, path: String, total: i64, cookie: String },
    Progress { id: u64, received: i64, total: i64 },
    /// Selesai (`ok`), gagal/dibatalkan (`!ok`).
    Done { id: u64, ok: bool, path: String, received: i64 },
    /// Halaman yang tampaknya unduhan (alamat berisi "download", "cetak", ".pdf", ...) sedang
    /// menunggu server: ditampilkan langsung di daftar unduhan (server SIPP butuh ±1 menit
    /// membuat PDF sebelum mengirim byte pertama).
    Waiting { id: u64, url: String },
    /// Ternyata bukan unduhan (halaman biasa) atau gagal: hapus tampilan menunggu.
    WaitEnd { id: u64 },
    /// Baris untuk ovner.log.
    Log(String),
    /// Halaman mencoba mengunduh ekstensi Chrome (.crx), mis. tombol "Tambahkan ke Chrome" di
    /// Chrome Web Store. Chromium memblokir unduhan ini; Ovner membatalkannya dan memasang
    /// ekstensinya sendiri. (alamat, nama berkas)
    Crx { url: String, name: String },
    /// PDF yang sedang dibaca dari respons boleh diunduh juga lewat koneksi Ovner sendiri
    /// (WinHTTP, seperti download manager): mana yang selesai lebih dulu dipakai.
    Race { id: u64, url: String, cookie: String, referer: String, user_agent: String, total: i64 },
}

/// Jenis kegagalan proses WebView2.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ProcFail {
    /// Seluruh mesin WebView2 berhenti: semua tab mati.
    Browser,
    /// Proses halaman tab ini berhenti (crash).
    Render,
    /// Halaman tidak merespons.
    Unresponsive,
    Other,
}

/// Item menu klik kanan Ovner ("-" = pemisah; `children` = submenu).
#[derive(Clone, Debug, Default)]
pub struct CtxItem {
    pub label: String,
    pub cmd: String,
    pub shortcut: String,
    /// Jangan tampilkan bila menu bawaan sudah punya item bernama ini (mis. "createQrCode").
    pub unless: &'static str,
    pub children: Vec<CtxItem>,
}

impl CtxItem {
    pub fn cmd(label: &str, cmd: &str) -> CtxItem {
        CtxItem { label: label.into(), cmd: cmd.into(), ..Default::default() }
    }
    pub fn key(mut self, k: &str) -> CtxItem {
        self.shortcut = k.into();
        self
    }
    pub fn sep() -> CtxItem {
        CtxItem { label: "-".into(), ..Default::default() }
    }
    pub fn sub(label: &str, children: Vec<CtxItem>) -> CtxItem {
        CtxItem { label: label.into(), children, ..Default::default() }
    }
}

/// Video/audio yang dimuat halaman (penangkap media, seperti IDM).
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MediaHit {
    pub url: String,
    /// "hls", "dash", atau "file".
    pub kind: &'static str,
    pub mime: String,
    /// Ukuran dari server (0 = tidak disebut).
    pub len: u64,
    /// Header permintaan yang dibutuhkan untuk mengunduhnya lagi (Referer, Origin, Cookie, User-Agent).
    #[serde(skip)]
    pub headers: Vec<(String, String)>,
}

/// Jenis media dari alamat + Content-Type, atau None bila bukan media yang layak diunduh
/// (potongan HLS/DASH, gambar, situs yang sudah ditangani yt-dlp seperti YouTube).
pub fn classify_media(url: &str, mime: &str, status: i32) -> Option<&'static str> {
    if !(url.starts_with("https://") || url.starts_with("http://")) || !(status == 200 || status == 206) {
        return None;
    }
    let host = url.split("://").nth(1).unwrap_or("").split(['/', '?', ':']).next().unwrap_or("").to_ascii_lowercase();
    // YouTube/Google Video: potongan DASH; diunduh lewat yt-dlp dengan alamat halaman.
    if host.ends_with("googlevideo.com") || host.ends_with("youtube.com") || host.ends_with("ytimg.com") {
        return None;
    }
    let path = url.split(['?', '#']).next().unwrap_or("").to_ascii_lowercase();
    let mime = mime.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    if path.ends_with(".m3u8") || mime == "application/vnd.apple.mpegurl" || mime == "application/x-mpegurl" || mime == "audio/mpegurl" {
        return Some("hls");
    }
    if path.ends_with(".mpd") || mime == "application/dash+xml" {
        return Some("dash");
    }
    // Potongan stream (bukan video utuh).
    if [".ts", ".m4s", ".m4f", ".cmfv", ".cmfa", ".aac", ".vtt", ".webvtt", ".key"].iter().any(|e| path.ends_with(e)) || mime == "video/mp2t" || mime == "video/iso.segment" {
        return None;
    }
    let file_ext = [".mp4", ".m4v", ".webm", ".mov", ".mkv", ".flv", ".m4a", ".mp3", ".ogg", ".opus", ".wav"].iter().any(|e| path.ends_with(e));
    if file_ext || mime.starts_with("video/") || (mime.starts_with("audio/") && mime != "audio/mpegurl") {
        return Some("file");
    }
    None
}

#[cfg(test)]
mod media_tests {
    use super::classify_media as c;
    #[test]
    fn classifies_media() {
        assert_eq!(c("https://cdn.detik.net.id/video/abc/index.m3u8?t=1", "", 200), Some("hls"));
        assert_eq!(c("https://x.com/play", "application/vnd.apple.mpegURL", 200), Some("hls"));
        assert_eq!(c("https://x.com/a.mpd", "", 200), Some("dash"));
        assert_eq!(c("https://x.com/seg-12.ts", "video/mp2t", 200), None);
        assert_eq!(c("https://x.com/v.mp4", "video/mp4", 206), Some("file"));
        assert_eq!(c("https://rr1---sn.googlevideo.com/videoplayback?x", "video/mp4", 200), None);
        assert_eq!(c("https://x.com/a.png", "image/png", 200), None);
        assert_eq!(c("https://x.com/v.mp4", "video/mp4", 404), None);
        assert_eq!(c("blob:https://x.com/1", "video/mp4", 200), None);
    }
}

/// Buka folder di Explorer.
pub fn open_folder(path: &Path) {
    let _ = std::process::Command::new(if cfg!(windows) { "explorer" } else { "xdg-open" }).arg(path).spawn();
}

/// Hapus folder-folder ini beberapa detik setelah proses ini keluar.
pub fn delete_dirs_later(dirs: &[&Path]) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut script = String::from("/c ping 127.0.0.1 -n 4 >nul");
        for d in dirs {
            script.push_str(&format!(" & rmdir /s /q \"{}\"", d.display()));
        }
        let _ = std::process::Command::new("cmd").raw_arg(script).creation_flags(0x0800_0000).spawn();
    }
    #[cfg(not(windows))]
    let _ = dirs;
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::core::{Interface, HSTRING};
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};

    fn hkcu() -> RegKey {
        RegKey::predef(HKEY_CURRENT_USER)
    }

    fn message_box(text: &str, flags: windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE) -> i32 {
        use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_SETFOREGROUND, MB_TOPMOST};
        unsafe { MessageBoxW(None, &HSTRING::from(text), &HSTRING::from("Ovner"), flags | MB_TOPMOST | MB_SETFOREGROUND).0 }
    }

    pub fn confirm(text: &str) -> bool {
        use windows::Win32::UI::WindowsAndMessaging::{IDYES, MB_ICONQUESTION, MB_YESNO};
        message_box(text, MB_YESNO | MB_ICONQUESTION) == IDYES.0
    }

    /// Ya = Some(true), Tidak = Some(false), Batal = None.
    pub fn ask_yes_no_cancel(text: &str) -> Option<bool> {
        use windows::Win32::UI::WindowsAndMessaging::{IDNO, IDYES, MB_ICONQUESTION, MB_YESNOCANCEL};
        match message_box(text, MB_YESNOCANCEL | MB_ICONQUESTION) {
            r if r == IDYES.0 => Some(true),
            r if r == IDNO.0 => Some(false),
            _ => None,
        }
    }

    pub fn info(text: &str) {
        use windows::Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_OK};
        message_box(text, MB_OK | MB_ICONINFORMATION);
    }

    /// Jalankan `exe args` sebagai admin (dialog UAC) dan tunggu sampai selesai. Hasil: kode
    /// keluar proses itu; Err bila UAC ditolak atau gagal dijalankan.
    pub fn run_elevated(exe: &Path, args: &str) -> Result<u32, String> {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
        use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let verb = HSTRING::from("runas");
        let file = HSTRING::from(exe.as_os_str());
        let params = HSTRING::from(args);
        let parent = exe.parent().map(Path::to_path_buf).unwrap_or_default();
        let dir = HSTRING::from(parent.as_os_str());
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
            lpVerb: windows::core::PCWSTR(verb.as_ptr()),
            lpFile: windows::core::PCWSTR(file.as_ptr()),
            lpParameters: windows::core::PCWSTR(params.as_ptr()),
            lpDirectory: windows::core::PCWSTR(dir.as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        unsafe {
            ShellExecuteExW(&mut info).map_err(|e| e.message().to_string())?;
            if info.hProcess.is_invalid() {
                return Err("proses admin tidak bisa dipantau".into());
            }
            WaitForSingleObject(info.hProcess, INFINITE);
            let mut code = 1u32;
            let _ = GetExitCodeProcess(info.hProcess, &mut code);
            let _ = CloseHandle(info.hProcess);
            Ok(code)
        }
    }

    /// Proses ini berjalan sebagai admin (token elevated).
    pub fn is_elevated() -> bool {
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
        use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        unsafe {
            let mut token = HANDLE::default();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
                return false;
            }
            let mut e = TOKEN_ELEVATION::default();
            let mut len = 0u32;
            let ok = GetTokenInformation(token, TokenElevation, Some(&mut e as *mut _ as *mut core::ffi::c_void), std::mem::size_of::<TOKEN_ELEVATION>() as u32, &mut len).is_ok();
            let _ = CloseHandle(token);
            ok && e.TokenIsElevated != 0
        }
    }

    /// Dialog pilih folder Windows. `start` = folder awal.
    pub fn pick_folder(owner: Option<isize>, title: &str, start: &Path) -> Option<std::path::PathBuf> {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
        use windows::Win32::UI::Shell::{FileOpenDialog, IFileOpenDialog, IShellItem, SHCreateItemFromParsingName, FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, SIGDN_FILESYSPATH};
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let dlg: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
            let opts = dlg.GetOptions().unwrap_or_default();
            let _ = dlg.SetOptions(opts | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM);
            let _ = dlg.SetTitle(&HSTRING::from(title));
            let mut dir = start.to_path_buf();
            while !dir.is_dir() {
                match dir.parent() {
                    Some(p) => dir = p.to_path_buf(),
                    None => break,
                }
            }
            if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(dir.as_os_str()), None::<&windows::Win32::System::Com::IBindCtx>) {
                let _ = dlg.SetFolder(&item);
            }
            dlg.Show(owner.map(|h| HWND(h as *mut core::ffi::c_void))).ok()?;
            let item = dlg.GetResult().ok()?;
            let p = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
            let s = p.to_string().ok();
            CoTaskMemFree(Some(p.0 as *const core::ffi::c_void));
            s.map(std::path::PathBuf::from)
        }
    }

    /// Dialog pilih berkas Windows. `kinds` = nama jenis dan pola, mis. ("Video", "*.mp4;*.mkv").
    pub fn pick_file(owner: Option<isize>, title: &str, start: &Path, kinds: (&str, &str)) -> Option<std::path::PathBuf> {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::HWND;
        use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
        use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
        use windows::Win32::UI::Shell::{FileOpenDialog, IFileOpenDialog, IShellItem, SHCreateItemFromParsingName, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, SIGDN_FILESYSPATH};
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let dlg: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
            let opts = dlg.GetOptions().unwrap_or_default();
            let _ = dlg.SetOptions(opts | FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST);
            let _ = dlg.SetTitle(&HSTRING::from(title));
            let (name, spec) = (HSTRING::from(kinds.0), HSTRING::from(kinds.1));
            let specs = [COMDLG_FILTERSPEC { pszName: PCWSTR(name.as_ptr()), pszSpec: PCWSTR(spec.as_ptr()) }];
            let _ = dlg.SetFileTypes(&specs);
            if start.is_dir() {
                if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(start.as_os_str()), None::<&windows::Win32::System::Com::IBindCtx>) {
                    let _ = dlg.SetFolder(&item);
                }
            }
            dlg.Show(owner.map(|h| HWND(h as *mut core::ffi::c_void))).ok()?;
            let item = dlg.GetResult().ok()?;
            let p = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
            let s = p.to_string().ok();
            CoTaskMemFree(Some(p.0 as *const core::ffi::c_void));
            s.map(std::path::PathBuf::from)
        }
    }

    /// Ruang kosong (byte) di drive tempat `path` berada.
    pub fn disk_free(path: &Path) -> Option<u64> {
        use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        let mut dir = path.to_path_buf();
        while !dir.exists() {
            dir = dir.parent()?.to_path_buf();
        }
        let mut free = 0u64;
        unsafe { GetDiskFreeSpaceExW(&HSTRING::from(dir.as_os_str()), Some(&mut free as *mut u64), None, None).ok()? };
        Some(free)
    }

    /// Nama prosesor (registri), untuk baris pembuka log.
    pub fn cpu_name() -> String {
        use winreg::enums::HKEY_LOCAL_MACHINE;
        RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0")
            .and_then(|k| k.get_value::<String, _>("ProcessorNameString"))
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    }

    /// Identitas aplikasi untuk taskbar (ikon & pengelompokan jendela).
    pub fn init_identity() {
        use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
        let _ = unsafe { SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(AUMID)) };
    }

    /// Shortcut .lnk dengan AppUserModelID yang sama dengan jendela, supaya ikon taskbar menyatu.
    pub fn create_shortcut(lnk: &Path, exe: &Path, description: &str) -> Result<(), String> {
        use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, IPersistFile, StructuredStorage::PROPVARIANT, CLSCTX_INPROC_SERVER,
            COINIT_APARTMENTTHREADED,
        };
        use windows::Win32::UI::Shell::{IShellLinkW, PropertiesSystem::IPropertyStore, ShellLink};

        if let Some(dir) = lnk.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(|e| e.to_string())?;
            link.SetPath(&HSTRING::from(exe.as_os_str())).map_err(|e| e.to_string())?;
            if let Some(d) = exe.parent() {
                let _ = link.SetWorkingDirectory(&HSTRING::from(d.as_os_str()));
            }
            let _ = link.SetDescription(&HSTRING::from(description));
            let _ = link.SetIconLocation(&HSTRING::from(exe.as_os_str()), 0);
            let store: IPropertyStore = link.cast().map_err(|e| e.to_string())?;
            let pv = PROPVARIANT::from(AUMID);
            store.SetValue(&PKEY_AppUserModel_ID, &pv).map_err(|e| e.to_string())?;
            store.Commit().map_err(|e| e.to_string())?;
            let file: IPersistFile = link.cast().map_err(|e| e.to_string())?;
            file.Save(&HSTRING::from(lnk.as_os_str()), true).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Folder Desktop pengguna yang sebenarnya (bisa dialihkan ke OneDrive oleh kebijakan kantor).
    /// Folder Downloads Windows yang sebenarnya (bisa dipindah pengguna, mis. ke D:\ atau OneDrive).
    pub fn downloads_dir() -> Option<std::path::PathBuf> {
        use windows::Win32::UI::Shell::{FOLDERID_Downloads, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
        unsafe {
            let p = SHGetKnownFolderPath(&FOLDERID_Downloads, KF_FLAG_DEFAULT, None).ok()?;
            let s = p.to_string().ok();
            windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
            s.map(std::path::PathBuf::from)
        }
    }

    pub fn desktop_dir() -> Option<std::path::PathBuf> {
        use windows::Win32::System::Com::CoTaskMemFree;
        use windows::Win32::UI::Shell::{FOLDERID_Desktop, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
        unsafe {
            let p = SHGetKnownFolderPath(&FOLDERID_Desktop, KF_FLAG_DEFAULT, None).ok()?;
            let s = p.to_string().ok();
            CoTaskMemFree(Some(p.0 as *const _));
            s.map(std::path::PathBuf::from)
        }
    }

    /// Unduh URL ke file (memakai pengaturan proxy Windows, seperti Edge).
    pub fn download(url: &str, dest: &Path) -> Result<(), String> {
        use windows::Win32::System::Com::Urlmon::URLDownloadToFileW;
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            URLDownloadToFileW(None, &HSTRING::from(url), &HSTRING::from(dest.as_os_str()), 0, None)
                .map_err(|e| format!("unduh gagal: {e}"))
        }
    }

    // ---------- mulai bersama Windows ----------

    /// Perintah autostart Ovner yang terdaftar di registri (None = tidak ada).
    pub fn autostart_command() -> Option<String> {
        hkcu().open_subkey(RUN_KEY).ok()?.get_value::<String, _>(APP_REG_NAME).ok()
    }

    pub fn set_autostart(enabled: bool, exe: &Path) -> std::io::Result<()> {
        let (run, _) = hkcu().create_subkey(RUN_KEY)?;
        if enabled {
            run.set_value(APP_REG_NAME, &format!("\"{}\" --tray", exe.display()))
        } else {
            match run.delete_value(APP_REG_NAME) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                r => r,
            }
        }
    }

    // ---------- proses WebView2 ----------

    /// Proses ini + semua proses msedgewebview2.exe turunannya.
    /// Ovner + semua proses WebView2-nya (dikenali dari folder data di baris perintah).
    fn tree_pids() -> Vec<u32> {
        crate::usage::ovner_pids()
    }

    /// Snapshot RAM proses ini + semua proses WebView2-nya.
    /// RAM proses Ovner dan WebView2. `extra`: PID dari API WebView2 (proses WebView2 tidak
    /// selalu tercatat sebagai anak Ovner, jadi pohon proses saja tidak cukup).
    pub fn process_snapshot(extra: &[u32]) -> Vec<Proc> {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2};
        use windows::Win32::System::Threading::{GetCurrentProcessId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        let me = unsafe { GetCurrentProcessId() };
        let mut out = Vec::new();
        let mut pids = tree_pids();
        for &p in extra {
            if !pids.contains(&p) {
                pids.push(p);
            }
        }
        for pid in pids {
            unsafe {
                let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { continue };
                let mut p = Proc { pid, is_self: pid == me, ..Default::default() };
                let mut pmc = PROCESS_MEMORY_COUNTERS_EX2 { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32, ..Default::default() };
                if GetProcessMemoryInfo(h, &mut pmc as *mut _ as *mut PROCESS_MEMORY_COUNTERS, pmc.cb).is_ok() {
                    p.working = (if pmc.PrivateWorkingSetSize > 0 { pmc.PrivateWorkingSetSize } else { pmc.WorkingSetSize }) as u64;
                    p.commit = pmc.PrivateUsage as u64;
                }
                let _ = CloseHandle(h);
                out.push(p);
            }
        }
        out
    }

    /// Private working set satu proses (byte), 0 bila tidak terbaca.
    fn private_ws(h: windows::Win32::Foundation::HANDLE) -> u64 {
        use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2};
        let mut pmc = PROCESS_MEMORY_COUNTERS_EX2 { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32, ..Default::default() };
        unsafe {
            if GetProcessMemoryInfo(h, &mut pmc as *mut _ as *mut PROCESS_MEMORY_COUNTERS, pmc.cb).is_ok() {
                return (if pmc.PrivateWorkingSetSize > 0 { pmc.PrivateWorkingSetSize } else { pmc.WorkingSetSize }) as u64;
            }
        }
        0
    }

    /// Rapikan working set proses tertentu saja; hasil (pid, sebelum, sesudah) dalam byte.
    pub fn trim_pids(pids: &[u32]) -> Vec<(u32, u64, u64)> {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::ProcessStatus::EmptyWorkingSet;
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA, PROCESS_VM_READ};
        let mut out = Vec::new();
        for &pid in pids {
            unsafe {
                let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_QUOTA | PROCESS_VM_READ, false, pid)
                    .or_else(|_| OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_QUOTA, false, pid))
                else {
                    continue;
                };
                let before = private_ws(h);
                if EmptyWorkingSet(h).is_ok() {
                    out.push((pid, before, private_ws(h)));
                }
                let _ = CloseHandle(h);
            }
        }
        out
    }

    /// RAM fisik Windows: (total, tersedia) dalam byte.
    pub fn mem_status() -> Option<(u64, u64)> {
        use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut m = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
        unsafe { GlobalMemoryStatusEx(&mut m).ok()? };
        Some((m.ullTotalPhys, m.ullAvailPhys))
    }

    /// Mode efisiensi Windows (EcoQoS), prioritas CPU di bawah normal, dan prioritas memori
    /// rendah untuk semua proses saat Ovner tersembunyi; dikembalikan saat tampil lagi.
    pub fn set_background_mode(background: bool) -> u32 {
        set_pids_eco(&tree_pids(), background)
    }

    /// Mode efisiensi (EcoQoS, prioritas dan memori rendah) untuk proses tertentu saja, mis.
    /// proses halaman yang semua tabnya berada di jendela yang diminimalkan.
    /// Detik sejak masukan terakhir (mouse/keyboard) di Windows.
    pub fn idle_secs() -> u64 {
        use windows::Win32::System::SystemInformation::GetTickCount;
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
        let mut li = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
        unsafe {
            if !GetLastInputInfo(&mut li).as_bool() {
                return 0;
            }
            (GetTickCount().wrapping_sub(li.dwTime) / 1000) as u64
        }
    }

    pub fn set_pids_eco(pids: &[u32], background: bool) -> u32 {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{
            OpenProcess, ProcessMemoryPriority, ProcessPowerThrottling, SetPriorityClass, SetProcessInformation,
            BELOW_NORMAL_PRIORITY_CLASS, MEMORY_PRIORITY_INFORMATION, MEMORY_PRIORITY_LOW, MEMORY_PRIORITY_NORMAL,
            NORMAL_PRIORITY_CLASS, PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            PROCESS_POWER_THROTTLING_STATE, PROCESS_SET_INFORMATION,
        };
        let throttle = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: if background { PROCESS_POWER_THROTTLING_EXECUTION_SPEED } else { 0 },
            StateMask: if background { PROCESS_POWER_THROTTLING_EXECUTION_SPEED } else { 0 },
        };
        let mem = MEMORY_PRIORITY_INFORMATION { MemoryPriority: if background { MEMORY_PRIORITY_LOW } else { MEMORY_PRIORITY_NORMAL } };
        let class = if background { BELOW_NORMAL_PRIORITY_CLASS } else { NORMAL_PRIORITY_CLASS };
        let mut n = 0;
        for &pid in pids {
            unsafe {
                let Ok(h) = OpenProcess(PROCESS_SET_INFORMATION, false, pid) else { continue };
                let _ = SetPriorityClass(h, class);
                let _ = SetProcessInformation(h, ProcessPowerThrottling, &throttle as *const _ as *const core::ffi::c_void, std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32);
                if SetProcessInformation(h, ProcessMemoryPriority, &mem as *const _ as *const core::ffi::c_void, std::mem::size_of::<MEMORY_PRIORITY_INFORMATION>() as u32).is_ok() {
                    n += 1;
                }
                let _ = CloseHandle(h);
            }
        }
        n
    }

    // ---------- WebView2 ----------

    /// Environment WebView2 sendiri supaya bisa memakai opsi resmi yang tidak ada di wry:
    /// tracking prevention Edge (mati = lebih ringan, pemblokir Ovner yang bekerja) dan
    /// laporan crash lokal (tidak dikirim ke Microsoft).
    pub fn create_environment(
        data_dir: &Path,
        browser_args: &str,
        tracking_prevention: bool,
        local_crash_reports: bool,
        extensions: bool,
    ) -> Result<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment, String> {
        use webview2_com::Microsoft::Web::WebView2::Win32::{CreateCoreWebView2EnvironmentWithOptions, ICoreWebView2EnvironmentOptions};
        use webview2_com::{CoreWebView2EnvironmentOptions, CreateCoreWebView2EnvironmentCompletedHandler};
        use windows::core::PCWSTR;
        let (tx, rx) = std::sync::mpsc::channel();
        let options = CoreWebView2EnvironmentOptions::default();
        unsafe {
            options.set_additional_browser_arguments(browser_args.to_string());
            options.set_are_browser_extensions_enabled(extensions);
            options.set_enable_tracking_prevention(tracking_prevention);
            options.set_is_custom_crash_reporting_enabled(local_crash_reports);
            options.set_allow_single_sign_on_using_os_primary_account(false);
            CreateCoreWebView2EnvironmentWithOptions(
                PCWSTR::null(),
                &HSTRING::from(data_dir.as_os_str()),
                &ICoreWebView2EnvironmentOptions::from(options),
                &CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(move |error_code, environment| {
                    let r = error_code.and_then(|_| environment.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_POINTER)));
                    let _ = tx.send(r);
                    Ok(())
                })),
            )
            .map_err(|e| e.to_string())?;
        }
        webview2_com::wait_with_pump(rx).map_err(|e| format!("{e:?}"))?.map_err(|e| e.to_string())
    }

    /// Pengaturan WebView2 per tab yang tidak disediakan wry: pengelola kata sandi, isi
    /// otomatis, SmartScreen, folder unduhan, dan penolakan izin sensitif.
    thread_local! {
        static DOWNLOADS: std::cell::RefCell<std::collections::HashMap<u64, webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2DownloadOperation>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
    }
    static NEXT_DL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

    thread_local! {
        /// Unduhan yang ditahan (deferral) sambil Ovner menguji apakah mesin unduhnya bisa
        /// mengambil alih: id → (args, deferral, operasi, alamat, path, total, callback).
        static PENDING: std::cell::RefCell<std::collections::HashMap<u64, PendingDl>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
    }

    struct PendingDl {
        args: webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2DownloadStartingEventArgs,
        deferral: webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Deferral,
        op: webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2DownloadOperation,
        url: String,
        path: String,
        total: i64,
        cb: std::rc::Rc<dyn Fn(DlEvent)>,
    }

    /// Nomor unduhan berikutnya (dipakai juga oleh mesin unduh Ovner).
    pub fn next_download_id() -> u64 {
        NEXT_DL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// Ikuti kemajuan unduhan WebView2 `op` (panel bawaan disembunyikan).
    unsafe fn track_download(
        op: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2DownloadOperation,
        id: u64,
        url: String,
        path: String,
        total: i64,
        cb: std::rc::Rc<dyn Fn(DlEvent)>,
    ) {
        use webview2_com::Microsoft::Web::WebView2::Win32::*;
        use webview2_com::{take_pwstr, BytesReceivedChangedEventHandler, StateChangedEventHandler};
        cb(DlEvent::Started { id, url, path, total });
        DOWNLOADS.with(|d| d.borrow_mut().insert(id, op.clone()));
        let (cb1, cb2) = (cb.clone(), cb.clone());
        let last = std::cell::Cell::new(std::time::Instant::now());
        let progress = BytesReceivedChangedEventHandler::create(Box::new(move |op, _| {
            if let Some(op) = op {
                if last.get().elapsed() >= std::time::Duration::from_millis(300) {
                    last.set(std::time::Instant::now());
                    let (mut r, mut t) = (0i64, 0i64);
                    let _ = op.BytesReceived(&mut r);
                    let _ = op.TotalBytesToReceive(&mut t);
                    cb1(DlEvent::Progress { id, received: r, total: t });
                }
            }
            Ok(())
        }));
        let state = StateChangedEventHandler::create(Box::new(move |op, _| {
            if let Some(op) = op {
                let mut st = COREWEBVIEW2_DOWNLOAD_STATE::default();
                let _ = op.State(&mut st);
                if st != COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS {
                    let mut p = windows::core::PWSTR::null();
                    let path = if op.ResultFilePath(&mut p).is_ok() { take_pwstr(p) } else { String::new() };
                    let mut r = 0i64;
                    let _ = op.BytesReceived(&mut r);
                    DOWNLOADS.with(|d| d.borrow_mut().remove(&id));
                    cb2(DlEvent::Done { id, ok: st == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED, path, received: r });
                }
            }
            Ok(())
        }));
        let mut token = 0i64;
        let _ = op.add_BytesReceivedChanged(&progress, &mut token);
        let _ = op.add_StateChanged(&state, &mut token);
    }

    /// Lacak unduhan tab ini sendiri; panel unduhan bawaan WebView2 disembunyikan.
    /// `offer(alamat, total)` = true bila mesin unduh Ovner ingin mencoba mengambil alih:
    /// unduhan ditahan, cookie situs diambil, lalu `DlEvent::Offer` dikirim; Ovner menjawab
    /// lewat [`resolve_download`].
    pub fn on_download(
        webview: &wry::WebView,
        cb: impl Fn(DlEvent) + 'static,
        offer: impl Fn(&str, i64) -> bool + 'static,
        place: impl Fn(&str) -> String + 'static,
    ) {
        use webview2_com::Microsoft::Web::WebView2::Win32::*;
        use webview2_com::{take_pwstr, DownloadStartingEventHandler, GetCookiesCompletedHandler};
        use wry::WebViewExtWindows;
        let Ok(w4) = webview.webview().cast::<ICoreWebView2_4>() else { return };
        let cb: std::rc::Rc<dyn Fn(DlEvent)> = std::rc::Rc::new(cb);
        let handler = DownloadStartingEventHandler::create(Box::new(move |sender, args| {
            let Some(args) = args else { return Ok(()) };
            unsafe {
                let _ = args.SetHandled(true);
                let op = args.DownloadOperation()?;
                let id = next_download_id();
                let mut p = windows::core::PWSTR::null();
                let url = if op.Uri(&mut p).is_ok() { take_pwstr(p) } else { String::new() };
                let mut p = windows::core::PWSTR::null();
                let mut path = if args.ResultFilePath(&mut p).is_ok() { take_pwstr(p) } else { String::new() };
                let fname = std::path::Path::new(&path).file_name().map(|f| f.to_string_lossy().to_lowercase()).unwrap_or_default();
                let lurl = url.to_lowercase();
                if fname.ends_with(".crx") || lurl.contains("/service/update2/crx") || lurl.contains("googleusercontent.com/crx/") {
                    let _ = args.SetCancel(true);
                    cb(DlEvent::Crx { url: url.clone(), name: fname });
                    return Ok(());
                }
                // Folder kategori (Download box v2): mis. Downloads\Video\a.mp4.
                if !path.is_empty() {
                    let placed = place(&path);
                    if placed != path && args.SetResultFilePath(&HSTRING::from(placed.as_str())).is_ok() {
                        path = placed;
                    }
                }
                let mut total = 0i64;
                let _ = op.TotalBytesToReceive(&mut total);
                let cookies = sender.as_ref().and_then(|s| s.cast::<ICoreWebView2_2>().ok()).and_then(|w2| w2.CookieManager().ok());
                if offer(&url, total) {
                    if let (Some(cm), Ok(deferral)) = (cookies, args.GetDeferral()) {
                        PENDING.with(|m| {
                            m.borrow_mut().insert(
                                id,
                                PendingDl { args: args.clone(), deferral, op: op.clone(), url: url.clone(), path: path.clone(), total, cb: cb.clone() },
                            )
                        });
                        let cb3 = cb.clone();
                        let (u2, p2) = (url.clone(), path.clone());
                        let once = std::cell::RefCell::new(Some(()));
                        let h = GetCookiesCompletedHandler::create(Box::new(move |_r, list| {
                            if once.borrow_mut().take().is_none() {
                                return Ok(());
                            }
                            let mut cookie = String::new();
                            if let Some(list) = list {
                                let mut n = 0u32;
                                let _ = list.Count(&mut n);
                                for i in 0..n.min(300) {
                                    if let Ok(c) = list.GetValueAtIndex(i) {
                                        let (mut a, mut b) = (windows::core::PWSTR::null(), windows::core::PWSTR::null());
                                        if c.Name(&mut a).is_ok() && c.Value(&mut b).is_ok() {
                                            if !cookie.is_empty() {
                                                cookie.push_str("; ");
                                            }
                                            cookie.push_str(&take_pwstr(a));
                                            cookie.push('=');
                                            cookie.push_str(&take_pwstr(b));
                                        }
                                    }
                                }
                            }
                            cb3(DlEvent::Offer { id, url: u2.clone(), path: p2.clone(), total, cookie });
                            Ok(())
                        }));
                        if cm.GetCookies(&HSTRING::from(url.as_str()), &h).is_ok() {
                            return Ok(());
                        }
                        // Cookie tidak bisa diambil: unduh cara biasa.
                        if let Some(pd) = PENDING.with(|m| m.borrow_mut().remove(&id)) {
                            let _ = pd.deferral.Complete();
                        }
                    }
                }
                track_download(&op, id, url, path, total, cb.clone());
            }
            Ok(())
        }));
        let mut token = 0i64;
        unsafe {
            let _ = w4.add_DownloadStarting(&handler, &mut token);
        }
    }

    /// Jawaban untuk unduhan yang ditahan: `take_over` = mesin unduh Ovner yang mengerjakan
    /// (unduhan WebView2 dibatalkan), selain itu WebView2 melanjutkan seperti biasa.
    pub fn resolve_download(id: u64, take_over: bool) {
        let Some(pd) = PENDING.with(|m| m.borrow_mut().remove(&id)) else { return };
        unsafe {
            if take_over {
                let _ = pd.args.SetCancel(true);
                let _ = pd.deferral.Complete();
            } else {
                let _ = pd.deferral.Complete();
                track_download(&pd.op, id, pd.url, pd.path, pd.total, pd.cb);
            }
        }
    }

    /// Unduhan `id` adalah PDF yang dibaca Ovner sendiri (bukan unduhan WebView2).
    pub fn is_pdf_stream(id: u64) -> bool {
        STREAMS.with(|m| m.borrow().contains_key(&id))
    }

    /// Batalkan unduhan WebView2 / PDF `id`. Mengembalikan true bila unduhan itu dikenal.
    pub fn cancel_download(id: u64) -> bool {
        if STREAMS.with(|m| m.borrow().contains_key(&id)) {
            finish_stream(id, false, "dibatalkan");
            return true;
        }
        let waiting = WAITING.with(|m| {
            m.borrow_mut().values_mut().find(|(i, _)| *i == id).map(|w| {
                w.1 = true;
            })
        });
        if waiting.is_some() {
            return true;
        }
        DOWNLOADS.with(|d| {
            if let Some(op) = d.borrow().get(&id) {
                unsafe {
                    let _ = op.Cancel();
                }
                true
            } else {
                false
            }
        })
    }

    /// Buka URI Windows (mis. ms-settings:…) lewat Explorer.
    pub fn open_uri(uri: &str) {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("explorer.exe").raw_arg(format!("\"{uri}\"")).spawn();
    }

    /// Buka berkas dengan aplikasinya, atau tampilkan di Explorer.
    pub fn open_path(path: &str, reveal: bool) {
        use std::os::windows::process::CommandExt;
        let p = Path::new(path);
        let mut c = std::process::Command::new("explorer.exe");
        if reveal && p.is_file() {
            // Harus persis `/select,"C:\a b\x.zip"`. Dengan .arg() Rust membungkus seluruhnya
            // jadi `"/select,C:\a b\x.zip"`, yang tidak dikenali Explorer sehingga ia membuka
            // folder Documents.
            c.raw_arg(format!("/select,\"{path}\""));
        } else if reveal {
            // Berkas sudah dipindah/dihapus: buka foldernya saja.
            let dir = p.parent().filter(|d| d.is_dir()).map(Path::to_path_buf).or_else(|| std::env::var_os("USERPROFILE").map(|h| std::path::PathBuf::from(h).join("Downloads")));
            match dir {
                Some(d) => {
                    c.raw_arg(format!("\"{}\"", d.display()));
                }
                None => return,
            }
        } else {
            c.raw_arg(format!("\"{path}\""));
        }
        let _ = c.spawn();
    }

    /// Memori komit (private bytes) dan thread proses Ovner sendiri.
    pub fn self_memory() -> Option<(u64, u32)> {
        use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2};
        use windows::Win32::System::Threading::GetCurrentProcess;
        unsafe {
            let mut pmc = PROCESS_MEMORY_COUNTERS_EX2 { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32, ..Default::default() };
            GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc as *mut _ as *mut PROCESS_MEMORY_COUNTERS, pmc.cb).ok()?;
            Some((pmc.PrivateUsage as u64, thread_count()))
        }
    }

    /// Rincian memori komit proses Ovner sendiri.
    #[derive(Debug, Default, Clone)]
    pub struct SelfMem {
        /// Semua alokasi pribadi yang ter-komit (heap, stack thread, VirtualAlloc).
        pub private: u64,
        pub heaps: u32,
        /// Heap Windows (semua heap proses): dipakai dan dipesan.
        pub heap_used: u64,
        pub heap_commit: u64,
        /// DLL di luar Windows/WebView2/Ovner (jalur, ukuran image).
        pub third_party: Vec<(String, u64)>,
        pub modules: u32,
    }

    fn heap_totals() -> (u32, u64, u64) {
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::Memory::{GetProcessHeaps, HeapSummary, HEAP_SUMMARY};
        unsafe {
            let mut hs = vec![HANDLE::default(); 64];
            let n = GetProcessHeaps(&mut hs) as usize;
            if n > hs.len() {
                hs = vec![HANDLE::default(); n];
                GetProcessHeaps(&mut hs);
            }
            let (mut used, mut commit) = (0u64, 0u64);
            for h in hs.iter().take(n) {
                let mut sm = HEAP_SUMMARY { cb: std::mem::size_of::<HEAP_SUMMARY>() as u32, ..Default::default() };
                if HeapSummary(*h, 0, &mut sm).as_bool() {
                    used += sm.cbAllocated as u64;
                    commit += sm.cbCommitted as u64;
                }
            }
            (n.min(hs.len()) as u32, used, commit)
        }
    }

    pub fn self_breakdown() -> Option<SelfMem> {
        use windows::Win32::Foundation::HMODULE;
        use windows::Win32::System::Memory::{VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, MEM_PRIVATE};
        use windows::Win32::System::ProcessStatus::{EnumProcessModules, GetModuleFileNameExW, GetModuleInformation, MODULEINFO};
        use windows::Win32::System::Threading::GetCurrentProcess;
        let mut m = SelfMem::default();
        unsafe {
            // Jelajahi ruang alamat: jumlahkan halaman pribadi yang ter-komit.
            let mut addr: usize = 0;
            let mut mbi = MEMORY_BASIC_INFORMATION::default();
            while VirtualQuery(Some(addr as *const _), &mut mbi, std::mem::size_of::<MEMORY_BASIC_INFORMATION>()) != 0 {
                if mbi.State == MEM_COMMIT && mbi.Type == MEM_PRIVATE {
                    m.private += mbi.RegionSize as u64;
                }
                let next = (mbi.BaseAddress as usize).saturating_add(mbi.RegionSize);
                if next <= addr {
                    break;
                }
                addr = next;
            }
            let (n, used, commit) = heap_totals();
            m.heaps = n;
            m.heap_used = used;
            m.heap_commit = commit;
            let me = GetCurrentProcess();
            let mut mods = vec![HMODULE::default(); 1024];
            let mut need = 0u32;
            if EnumProcessModules(me, mods.as_mut_ptr(), (mods.len() * std::mem::size_of::<HMODULE>()) as u32, &mut need).is_ok() {
                let count = (need as usize / std::mem::size_of::<HMODULE>()).min(mods.len());
                m.modules = count as u32;
                let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_string_lossy().to_lowercase())).unwrap_or_default();
                for h in mods.iter().take(count) {
                    let mut buf = [0u16; 520];
                    let len = GetModuleFileNameExW(Some(me), Some(*h), &mut buf) as usize;
                    let path = String::from_utf16_lossy(&buf[..len]);
                    let low = path.to_lowercase();
                    let ours = low.contains("\\windows\\") || low.contains("webview2") || low.contains("\\microsoft\\edge") || (!exe_dir.is_empty() && low.starts_with(&exe_dir));
                    if !ours {
                        let mut mi = MODULEINFO::default();
                        let _ = GetModuleInformation(me, *h, &mut mi, std::mem::size_of::<MODULEINFO>() as u32);
                        m.third_party.push((path, mi.SizeOfImage as u64));
                    }
                }
            }
        }
        Some(m)
    }

    /// Kembalikan memori bebas di heap Windows ke sistem (HeapCompact semua heap proses).
    /// Mengembalikan byte komit yang dilepas.
    pub fn compact_heaps() -> u64 {
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::Memory::{GetProcessHeaps, HeapCompact, HEAP_NONE};
        let (_, _, before) = heap_totals();
        unsafe {
            let mut hs = vec![HANDLE::default(); 64];
            let n = (GetProcessHeaps(&mut hs) as usize).min(hs.len());
            for h in hs.iter().take(n) {
                let _ = HeapCompact(*h, HEAP_NONE);
            }
        }
        let (_, _, after) = heap_totals();
        before.saturating_sub(after)
    }

    fn thread_count() -> u32 {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32};
        use windows::Win32::System::Threading::GetCurrentProcessId;
        unsafe {
            let me = GetCurrentProcessId();
            let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) else { return 0 };
            let mut e = THREADENTRY32 { dwSize: std::mem::size_of::<THREADENTRY32>() as u32, ..Default::default() };
            let mut n = 0;
            if Thread32First(snap, &mut e).is_ok() {
                loop {
                    if e.th32OwnerProcessID == me {
                        n += 1;
                    }
                    if Thread32Next(snap, &mut e).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snap);
            n
        }
    }

    /// Bawa jendela anak WebView ini ke paling atas (overlay toolbar di atas halaman).
    pub fn raise(webview: &wry::WebView) {
        use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE};
        use wry::WebViewExtWindows;
        unsafe {
            let _ = SetWindowPos(webview.hwnd(), Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }

    /// Batasi bagian WebView yang terlihat/menerima klik ke kotak-kotak ini (piksel fisik,
    /// relatif ke WebView). Dipakai toolbar: saat menu/gelembung terbuka, hanya menu itu yang
    /// menutupi halaman, bukan selebar layar berwarna gelap. Kosong = tanpa batas.
    pub fn clip_webview(webview: &wry::WebView, rects: &[(i32, i32, i32, i32)]) {
        use windows::Win32::Graphics::Gdi::{CombineRgn, CreateRectRgn, DeleteObject, SetWindowRgn, HGDIOBJ, RGN_OR};
        use wry::WebViewExtWindows;
        unsafe {
            let hwnd = webview.hwnd();
            if rects.is_empty() {
                SetWindowRgn(hwnd, None, true);
                return;
            }
            let (x, y, w, h) = rects[0];
            let rgn = CreateRectRgn(x, y, x + w, y + h);
            for &(x, y, w, h) in &rects[1..] {
                let r = CreateRectRgn(x, y, x + w, y + h);
                let _ = CombineRgn(Some(rgn), Some(rgn), Some(r), RGN_OR);
                let _ = DeleteObject(HGDIOBJ(r.0));
            }
            // Setelah berhasil, region dimiliki Windows (jangan dihapus).
            if SetWindowRgn(hwnd, Some(rgn), true) == 0 {
                let _ = DeleteObject(HGDIOBJ(rgn.0));
            }
        }
    }

    /// Panggil `cb(png)` setiap ikon situs di tab ini berubah (FaviconChanged + GetFavicon).
    pub fn on_favicon(webview: &wry::WebView, cb: impl Fn(Vec<u8>) + 'static) {
        use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2_15, COREWEBVIEW2_FAVICON_IMAGE_FORMAT_PNG};
        use webview2_com::{FaviconChangedEventHandler, GetFaviconCompletedHandler};
        use wry::WebViewExtWindows;
        let cb = std::rc::Rc::new(cb);
        let Ok(w15) = webview.webview().cast::<ICoreWebView2_15>() else { return };
        let target = w15.clone();
        let handler = FaviconChangedEventHandler::create(Box::new(move |_, _| {
            let cb = cb.clone();
            let done = GetFaviconCompletedHandler::create(Box::new(move |res, stream| {
                if let (Ok(()), Some(stream)) = (res, stream) {
                    let mut out = Vec::new();
                    let mut buf = [0u8; 8192];
                    loop {
                        let mut n = 0u32;
                        let hr = unsafe { stream.Read(buf.as_mut_ptr() as *mut _, buf.len() as u32, Some(&mut n)) };
                        if hr.is_err() || n == 0 || out.len() > 64 * 1024 {
                            break;
                        }
                        out.extend_from_slice(&buf[..n as usize]);
                    }
                    if !out.is_empty() {
                        cb(out);
                    }
                }
                Ok(())
            }));
            unsafe {
                let _ = target.GetFavicon(COREWEBVIEW2_FAVICON_IMAGE_FORMAT_PNG, &done);
            }
            Ok(())
        }));
        let mut token = 0i64;
        unsafe {
            let _ = w15.add_FaviconChanged(&handler, &mut token);
        }
    }

    /// Enkripsi data dengan DPAPI (terikat akun Windows ini), seperti Chrome/Edge menyimpan sandi.
    pub fn protect(data: &[u8], encrypt: bool) -> Option<Vec<u8>> {
        use windows::Win32::Foundation::{LocalFree, HLOCAL};
        use windows::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB::default();
        unsafe {
            let ok = if encrypt {
                CryptProtectData(&input, &HSTRING::from("Ovner"), None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
            } else {
                CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
            };
            if ok.is_err() || out.pbData.is_null() {
                return None;
            }
            let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
            let _ = LocalFree(Some(HLOCAL(out.pbData as *mut _)));
            Some(v)
        }
    }

    /// Salin teks ke clipboard Windows.
    pub fn set_clipboard(text: &str) {
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
        use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
        const CF_UNICODETEXT: u32 = 13;
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            if OpenClipboard(None).is_err() {
                return;
            }
            let _ = EmptyClipboard();
            if let Ok(h) = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) {
                let p = GlobalLock(h) as *mut u16;
                if !p.is_null() {
                    std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
                    let _ = GlobalUnlock(h);
                    let _ = SetClipboardData(CF_UNICODETEXT, Some(HANDLE(h.0)));
                }
            }
            let _ = CloseClipboard();
        }
    }

    /// Baca teks dari clipboard Windows (untuk "Paksa tempel").
    pub fn read_clipboard() -> Option<String> {
        use windows::Win32::Foundation::HGLOBAL;
        use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
        use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
        const CF_UNICODETEXT: u32 = 13;
        unsafe {
            OpenClipboard(None).ok()?;
            let mut out = None;
            if let Ok(h) = GetClipboardData(CF_UNICODETEXT) {
                let g = HGLOBAL(h.0);
                let p = GlobalLock(g) as *const u16;
                if !p.is_null() {
                    let max = GlobalSize(g) / 2;
                    let mut n = 0usize;
                    while n < max && *p.add(n) != 0 {
                        n += 1;
                    }
                    out = Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, n)));
                    let _ = GlobalUnlock(g);
                }
            }
            let _ = CloseClipboard();
            out
        }
    }

    static CRASH_LOG: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

    /// Catat crash tingkat Windows (akses memori ilegal di COM/WebView2, dsb.) ke ovner.log:
    /// kode galat, alamat, dan modul (DLL) penyebabnya. Proses tetap berhenti seperti biasa.
    pub fn install_crash_logger(log_file: &Path) {
        use windows::Win32::System::Diagnostics::Debug::{SetUnhandledExceptionFilter, EXCEPTION_POINTERS};
        let _ = CRASH_LOG.set(log_file.to_path_buf());
        unsafe extern "system" fn filter(ep: *const EXCEPTION_POINTERS) -> i32 {
            use windows::Win32::Foundation::HMODULE;
            use windows::Win32::System::LibraryLoader::{
                GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            };
            unsafe {
                let Some(path) = CRASH_LOG.get() else { return 0 };
                let rec = if ep.is_null() { std::ptr::null_mut() } else { (*ep).ExceptionRecord };
                let (code, addr) = if rec.is_null() { (0u32, 0usize) } else { ((*rec).ExceptionCode.0 as u32, (*rec).ExceptionAddress as usize) };
                let mut module = String::from("?");
                let mut offset = 0usize;
                let mut hmod = HMODULE::default();
                if addr != 0
                    && GetModuleHandleExW(
                        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                        windows::core::PCWSTR(addr as *const u16),
                        &mut hmod,
                    )
                    .is_ok()
                {
                    let mut buf = [0u16; 260];
                    let n = GetModuleFileNameW(Some(hmod), &mut buf) as usize;
                    module = String::from_utf16_lossy(&buf[..n.min(buf.len())]);
                    offset = addr.wrapping_sub(hmod.0 as usize);
                }
                let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                let line = format!(
                    "{secs} CRASH kode=0x{code:08X} alamat=0x{addr:X} modul={module}+0x{offset:X} versi={}\n",
                    crate::update::display_version()
                );
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                    use std::io::Write;
                    let _ = f.write_all(line.as_bytes());
                }
            }
            0 // EXCEPTION_CONTINUE_SEARCH: biarkan Windows menutup proses seperti biasa.
        }
        unsafe {
            let _ = SetUnhandledExceptionFilter(Some(filter));
        }
    }

    /// ID bingkai utama tab ini (ICoreWebView2_20::FrameId), untuk mencocokkan proses halaman.
    pub fn main_frame_id(webview: &wry::WebView) -> Option<u32> {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_20;
        use wry::WebViewExtWindows;
        let w = webview.webview().cast::<ICoreWebView2_20>().ok()?;
        let mut id = 0u32;
        unsafe { w.FrameId(&mut id).ok()? };
        (id != 0).then_some(id)
    }

    /// Proses halaman (renderer) WebView2 beserta bingkai utama yang dilayaninya:
    /// `(pid, [frame id utama])`. Seperti nama "WebView2: <judul>" di Task Manager Windows.
    ///
    /// Untuk berjaga-jaga (Ovner 0.5 pernah tertutup sendiri di sekitar API ini), sebuah
    /// berkas penanda dibuat sebelum memanggil dan dihapus setelah hasilnya datang. Bila
    /// Ovner mati di tengahnya, penanda tertinggal dan fitur ini mati otomatis saat mulai.
    pub fn request_renderer_frames(
        env: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment,
        sentinel: &Path,
        cb: impl FnOnce(Vec<(u32, Vec<u32>)>) + 'static,
    ) -> bool {
        use webview2_com::GetProcessExtendedInfosCompletedHandler;
        use webview2_com::Microsoft::Web::WebView2::Win32::*;
        use windows::core::BOOL;
        let Ok(env13) = env.cast::<ICoreWebView2Environment13>() else { return false };
        let _ = std::fs::write(sentinel, b"1");
        let sentinel = sentinel.to_path_buf();
        let cb = std::cell::RefCell::new(Some(cb));
        let handler = GetProcessExtendedInfosCompletedHandler::create(Box::new(move |res, coll| {
            let mut out = Vec::new();
            if let (Ok(()), Some(coll)) = (res, coll) {
                unsafe {
                    let mut n = 0u32;
                    let _ = coll.Count(&mut n);
                    for i in 0..n.min(512) {
                        let Ok(info) = coll.GetValueAtIndex(i) else { continue };
                        let Ok(pi) = info.ProcessInfo() else { continue };
                        let (mut pid, mut kind) = (0i32, COREWEBVIEW2_PROCESS_KIND::default());
                        let _ = pi.ProcessId(&mut pid);
                        let _ = pi.Kind(&mut kind);
                        if kind != COREWEBVIEW2_PROCESS_KIND_RENDERER || pid <= 0 {
                            continue;
                        }
                        let mut mains: Vec<u32> = Vec::new();
                        let Ok(frames) = info.AssociatedFrameInfos() else { continue };
                        let Ok(it) = frames.GetIterator() else { continue };
                        let mut guard = 0;
                        loop {
                            guard += 1;
                            let mut has = BOOL::default();
                            if guard > 256 || it.HasCurrent(&mut has).is_err() || !has.as_bool() {
                                break;
                            }
                            if let Ok(fr) = it.GetCurrent() {
                                // Naik ke bingkai utama: iframe di proses lain tetap milik tab itu.
                                let mut cur = fr.cast::<ICoreWebView2FrameInfo2>().ok();
                                for _ in 0..16 {
                                    let Some(f2) = cur.clone() else { break };
                                    let mut k = COREWEBVIEW2_FRAME_KIND::default();
                                    let _ = f2.FrameKind(&mut k);
                                    if k == COREWEBVIEW2_FRAME_KIND_MAIN_FRAME {
                                        break;
                                    }
                                    match f2.ParentFrameInfo().and_then(|p| p.cast::<ICoreWebView2FrameInfo2>()) {
                                        Ok(p) => cur = Some(p),
                                        Err(_) => break,
                                    }
                                }
                                if let Some(f2) = cur {
                                    let mut id = 0u32;
                                    if f2.FrameId(&mut id).is_ok() && id != 0 && !mains.contains(&id) {
                                        mains.push(id);
                                    }
                                }
                            }
                            let mut more = BOOL::default();
                            if it.MoveNext(&mut more).is_err() || !more.as_bool() {
                                break;
                            }
                        }
                        out.push((pid as u32, mains));
                    }
                }
            }
            let _ = std::fs::remove_file(&sentinel);
            if let Some(cb) = cb.borrow_mut().take() {
                cb(out);
            }
            Ok(())
        }));
        unsafe { env13.GetProcessExtendedInfos(&handler).is_ok() }
    }

    /// Rekam masalah halaman lewat DevTools (galat JavaScript, pesan konsol, permintaan gagal
    /// beserta alasan CORS/blokir, masalah cookie dari Audits, jawaban HTTP ≥ 400) untuk
    /// laporan "Rekam masalah halaman". `cb(nama_kejadian, json)`.
    pub fn record_problems(webview: &wry::WebView, cb: impl Fn(&str, String) + 'static) {
        use webview2_com::DevToolsProtocolEventReceivedEventHandler;
        use wry::WebViewExtWindows;
        let core = webview.webview();
        let cb = std::rc::Rc::new(cb);
        for ev in ["Runtime.exceptionThrown", "Log.entryAdded", "Network.loadingFailed", "Audits.issueAdded", "Network.responseReceived", "Network.requestWillBeSent", "Network.responseReceivedExtraInfo", "Runtime.consoleAPICalled"] {
            let cb = cb.clone();
            let name = ev.to_string();
            let handler = DevToolsProtocolEventReceivedEventHandler::create(Box::new(move |_, args| {
                if let Some(a) = args {
                    let mut p = windows::core::PWSTR::null();
                    if unsafe { a.ParameterObjectAsJson(&mut p) }.is_ok() {
                        cb(&name, webview2_com::take_pwstr(p));
                    }
                }
                Ok(())
            }));
            unsafe {
                if let Ok(r) = core.GetDevToolsProtocolEventReceiver(&HSTRING::from(ev)) {
                    let mut token = 0i64;
                    let _ = r.add_DevToolsProtocolEventReceived(&handler, &mut token);
                }
            }
        }
        record_enable(webview);
    }

    /// Hidupkan lagi domain DevTools untuk Rekam masalah. Perlu di setiap pindah halaman: saat
    /// Chromium memindah halaman ke proses lain, domain yang dihidupkan sebelumnya hilang dan
    /// rekaman berhenti (laporan EDABU hanya berisi baris NAV setelah halaman pertama).
    pub fn record_enable(webview: &wry::WebView) {
        for m in ["Runtime.enable", "Log.enable", "Audits.enable"] {
            cdp(webview, m, "{}");
        }
        // Isi jawaban tidak dibutuhkan laporan: tanpa batas, Chromium menyimpan sampai ±100 MB
        // isi jawaban per tab di proses inti selama Network domain hidup.
        cdp(webview, "Network.enable", r#"{"maxTotalBufferSize":1048576,"maxResourceBufferSize":65536,"maxPostDataSize":0}"#);
    }

    /// Matikan lagi domain DevTools setelah rekaman selesai (dulu tetap hidup sampai tab
    /// ditutup: setiap permintaan tetap dikirim ke Ovner dan isinya disimpan proses inti).
    pub fn record_disable(webview: &wry::WebView) {
        for m in ["Network.disable", "Audits.disable", "Log.disable", "Runtime.disable"] {
            cdp(webview, m, "{}");
        }
    }

    /// Halaman meminta jendelanya ditutup (window.close dari pop-up).
    /// Terima pesan `chrome.webview.postMessage` dari skrip Ovner di halaman, tanpa membuat
    /// `window.ipc` di setiap frame seperti wry. Properti itu tidak bisa dihapus dan bentrok
    /// dengan situs yang punya variabel `ipc` sendiri (Infor LN/GWT: "Identifier 'ipc' has
    /// already been declared" → aplikasi hitam).
    pub fn on_message(webview: &wry::WebView, cb: impl Fn(String) + 'static) {
        use webview2_com::WebMessageReceivedEventHandler;
        use windows::core::PWSTR;
        use wry::WebViewExtWindows;
        let handler = WebMessageReceivedEventHandler::create(Box::new(move |_, args| {
            if let Some(args) = args {
                let mut js = PWSTR::null();
                if unsafe { args.TryGetWebMessageAsString(&mut js) }.is_ok() {
                    cb(webview2_com::take_pwstr(js));
                }
            }
            Ok(())
        }));
        let mut token = 0i64;
        unsafe {
            let _ = webview.webview().add_WebMessageReceived(&handler, &mut token);
        }
    }

    /// Teks pemakaian RAM/CPU di bilah judul Windows, tepat di kiri tombol minimize (tidak
    /// memakan tempat tab). Bilah judul digambar Windows (DWM), jadi teksnya digambar di jendela
    /// kecil tembus-klik milik jendela utama yang mengikuti posisinya. `parts` = (teks, tebal).
    /// `parts` kosong = sembunyikan.
    pub fn caption_text(owner: isize, parts: &[(String, bool)], dark: bool, scale: f64) {
        use windows::core::{w, PCWSTR};
        use windows::Win32::Foundation::*;
        use windows::Win32::Graphics::Dwm::*;
        use windows::Win32::Graphics::Gdi::*;
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::WindowsAndMessaging::*;
        // Satu jendela info per jendela Ovner (pemiliknya); dulu hanya satu untuk jendela pertama,
        // sehingga jendela Ovner kedua tidak menampilkan RAM/CPU/GPU.
        thread_local! {
            static CAP: std::cell::RefCell<std::collections::HashMap<isize, isize>> = std::cell::RefCell::new(std::collections::HashMap::new());
        }
        unsafe extern "system" fn proc_(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
            if m == WM_NCHITTEST {
                return LRESULT(HTTRANSPARENT as isize);
            }
            unsafe { DefWindowProcW(h, m, wp, lp) }
        }
        unsafe {
            let owner_h = HWND(owner as *mut _);
            // Buang catatan jendela Ovner yang sudah ditutup (jendela info ikut hancur bersamanya).
            CAP.with(|c| c.borrow_mut().retain(|o, _| IsWindow(Some(HWND(*o as *mut _))).as_bool()));
            let mut cap = CAP.with(|c| c.borrow().get(&owner).copied().unwrap_or(0));
            if cap != 0 && !IsWindow(Some(HWND(cap as *mut _))).as_bool() {
                cap = 0;
            }
            if parts.is_empty() || IsIconic(owner_h).as_bool() || !IsWindowVisible(owner_h).as_bool() {
                if cap != 0 {
                    let _ = ShowWindow(HWND(cap as *mut _), SW_HIDE);
                }
                return;
            }
            if cap == 0 {
                let inst = GetModuleHandleW(PCWSTR::null()).map(|m| HINSTANCE(m.0)).ok();
                // RegisterClassW gagal (sudah terdaftar) untuk jendela kedua dan seterusnya: tidak apa.
                let wc = WNDCLASSW { lpfnWndProc: Some(proc_), hInstance: inst.unwrap_or_default(), lpszClassName: w!("OvnerCaptionInfo"), ..Default::default() };
                RegisterClassW(&wc);
                let Ok(h) = CreateWindowExW(
                    WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                    w!("OvnerCaptionInfo"),
                    w!(""),
                    WS_POPUP,
                    0,
                    0,
                    1,
                    1,
                    Some(owner_h),
                    None,
                    inst,
                    None,
                ) else {
                    return;
                };
                cap = h.0 as isize;
                CAP.with(|c| c.borrow_mut().insert(owner, cap));
            }
            let hwnd = HWND(cap as *mut _);
            // Letak tombol judul (minimize/maximize/close) relatif ke jendela.
            let mut win = RECT::default();
            let _ = GetWindowRect(owner_h, &mut win);
            let dpi = ((scale.max(1.0)) * 96.0).round() as i32;
            let mut btn = RECT::default();
            let ok = DwmGetWindowAttribute(owner_h, DWMWA_CAPTION_BUTTON_BOUNDS, &mut btn as *mut _ as *mut _, std::mem::size_of::<RECT>() as u32).is_ok() && btn.right > btn.left;
            if !ok {
                let bw = 46 * dpi / 96;
                btn = RECT { left: win.right - win.left - 3 * bw - 8 * dpi / 96, top: 0, right: win.right - win.left, bottom: 32 * dpi / 96 };
            }
            let h = (btn.bottom - btn.top).max(20 * dpi / 96);
            // Gambar teks: putih di atas hitam, lalu kecerahan jadi alpha (teks halus tanpa kotak).
            let dc = CreateCompatibleDC(None);
            let size = -12 * dpi / 96;
            let mk = |weight: i32| CreateFontW(size, 0, 0, 0, weight, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, ANTIALIASED_QUALITY, 0, w!("Segoe UI"));
            let (f_reg, f_bold) = (mk(400), mk(600));
            let mut widths = Vec::new();
            let mut total = 0;
            for (t, b) in parts {
                SelectObject(dc, HGDIOBJ(if *b { f_bold.0 } else { f_reg.0 }));
                let wide: Vec<u16> = t.encode_utf16().collect();
                let mut sz = SIZE::default();
                let _ = GetTextExtentPoint32W(dc, &wide, &mut sz);
                widths.push(sz);
                total += sz.cx;
            }
            let pad = 6 * dpi / 96;
            let wpx = total + pad * 2;
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: wpx,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let Ok(bmp) = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else {
                let _ = DeleteDC(dc);
                return;
            };
            let old = SelectObject(dc, HGDIOBJ(bmp.0));
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, COLORREF(0x00FF_FFFF));
            let mut x = pad;
            for ((t, b), sz) in parts.iter().zip(&widths) {
                SelectObject(dc, HGDIOBJ(if *b { f_bold.0 } else { f_reg.0 }));
                let wide: Vec<u16> = t.encode_utf16().collect();
                let _ = TextOutW(dc, x, (h - sz.cy) / 2, &wide);
                x += sz.cx;
            }
            let _ = GdiFlush();
            let (r, g, b) = if dark { (232u32, 234, 237) } else { (32u32, 33, 36) };
            let px = std::slice::from_raw_parts_mut(bits as *mut u32, (wpx * h) as usize);
            for p in px.iter_mut() {
                let a = ((*p >> 16) & 0xff).max((*p >> 8) & 0xff).max(*p & 0xff);
                *p = (a << 24) | ((r * a / 255) << 16) | ((g * a / 255) << 8) | (b * a / 255);
            }
            let dst = POINT { x: win.left + btn.left - wpx - 4 * dpi / 96, y: win.top + btn.top };
            let sz = SIZE { cx: wpx, cy: h };
            let src = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
            let _ = UpdateLayeredWindow(hwnd, None, Some(&dst), Some(&sz), Some(dc), Some(&src), COLORREF(0), Some(&blend), ULW_ALPHA);
            SelectObject(dc, old);
            let _ = DeleteObject(HGDIOBJ(bmp.0));
            let _ = DeleteObject(HGDIOBJ(f_reg.0));
            let _ = DeleteObject(HGDIOBJ(f_bold.0));
            let _ = DeleteDC(dc);
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
    }

    /// Kirim data ke halaman Ovner sebagai pesan (`chrome.webview` "message"), bukan sebagai
    /// skrip. evaluate_script dengan JSON besar tiap detik membuat proses halaman (toolbar,
    /// halaman Unduhan) terus membengkak sampai lebih dari 1 GB selama mengunduh, karena tiap
    /// skrip dikompilasi dan disimpan mesin JavaScript. Mengembalikan false bila gagal.
    pub fn post_text(webview: &wry::WebView, text: &str) -> bool {
        use wry::WebViewExtWindows;
        unsafe { webview.webview().PostWebMessageAsString(&HSTRING::from(text)).is_ok() }
    }

    thread_local! {
        /// Unduhan PDF yang dibaca langsung dari respons lewat DevTools (IO.read).
        static STREAMS: std::cell::RefCell<std::collections::HashMap<u64, PdfStream>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
    }

    struct PdfStream {
        core: webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
        request: String,
        handle: String,
        file: Option<std::fs::File>,
        part: std::path::PathBuf,
        path: String,
        received: i64,
        total: i64,
        last: std::time::Instant,
        started: std::time::Instant,
        cb: std::rc::Rc<dyn Fn(DlEvent)>,
        /// Alamat PDF (untuk tombol "Buka di sini").
        url: String,
        /// Ukuran tidak disebut server: isi ditahan di memori dulu (lihat `Measure`).
        measure: Option<Measure>,
    }

    /// PDF tanpa Content-Length yang sedang diukur. Selesai di bawah `limit` = diserahkan ke
    /// penampil PDF (Fetch.fulfillRequest dengan isi dan tajuk aslinya), lewat = jadi unduhan
    /// dan `buf` menjadi awal berkasnya.
    struct Measure {
        buf: Vec<u8>,
        limit: u64,
        status: i64,
        headers: Vec<(String, String)>,
        url: String,
        referer: String,
        agent: String,
        waiting: Option<u64>,
    }

    /// Tombol "Buka di sini" pada halaman "PDF disimpan" membuka alamat PDF aslinya ditambah
    /// `ovner_saved=<token>`. Permintaan itu dilayani Ovner dari berkas di disk (tahap
    /// permintaan Fetch) dan tidak sampai ke server. Alamatnya tetap satu situs dengan halaman
    /// (bingkai Infor/SIPP), jadi tidak diblokir aturan keamanan situs (CSP).
    const SAVED_PARAM: &str = "ovner_saved=";

    fn saved_link(url: &str, token: &str) -> String {
        let base = url.split('#').next().unwrap_or(url);
        let sep = if base.contains('?') { '&' } else { '?' };
        format!("{base}{sep}{SAVED_PARAM}{token}")
    }

    fn saved_param(url: &str) -> Option<&str> {
        let query = url.split('#').next()?.split_once('?')?.1;
        query.split('&').find_map(|kv| kv.strip_prefix(SAVED_PARAM))
    }

    thread_local! {
        /// Token acak → path PDF yang disimpan dari respons (untuk "Buka di sini").
        static SAVED: std::cell::RefCell<std::collections::HashMap<String, String>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
    }

    fn saved_token(path: &str) -> String {
        use std::hash::{BuildHasher, Hash, Hasher};
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        path.hash(&mut h);
        std::time::SystemTime::now().hash(&mut h);
        let token = format!("{:016x}", h.finish());
        SAVED.with(|m| m.borrow_mut().insert(token.clone(), path.to_string()));
        token
    }

    /// Nama berkas untuk `filename*=UTF-8''…` dan alamat (persen-enkode, spasi = %20).
    fn pct(name: &str) -> String {
        url::form_urlencoded::byte_serialize(name.as_bytes()).collect::<String>().replace('+', "%20")
    }

    /// Layani alamat "Buka di sini" (`…ovner_saved=<token>`): PDF yang tadi disimpan, dibuka
    /// di penampil PDF bawaan (dengan tombol unduh dan cetak).
    fn serve_saved(core: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2, req: &str, token: &str) {
        use base64::Engine as _;
        let path = SAVED.with(|m| m.borrow().get(token).cloned());
        let params = match path.as_ref().map(|p| (p, std::fs::read(p))) {
            Some((p, Ok(bytes))) => {
                let name = Path::new(p).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                serde_json::json!({
                    "requestId": req,
                    "responseCode": 200,
                    "responseHeaders": [
                        {"name": "Content-Type", "value": "application/pdf"},
                        {"name": "Content-Disposition", "value": format!("inline; filename*=UTF-8''{}", pct(&name))},
                    ],
                    "body": base64::prelude::BASE64_STANDARD.encode(&bytes),
                })
            }
            _ => {
                let html = "<!doctype html><meta charset=utf-8><title>PDF</title><body style=\"font:15px Segoe UI,sans-serif;background:#202124;color:#e8eaed;padding:40px\">Berkas PDF ini sudah dipindah atau dihapus, atau Ovner sudah dimulai ulang. Buka dari daftar Unduhan (Ctrl+J).</body>";
                serde_json::json!({
                    "requestId": req,
                    "responseCode": 404,
                    "responseHeaders": [{"name": "Content-Type", "value": "text/html; charset=utf-8"}],
                    "body": base64::prelude::BASE64_STANDARD.encode(html.as_bytes()),
                })
            }
        };
        cdp_core(core, "Fetch.fulfillRequest", &params.to_string());
    }

    /// Pengukuran selesai di bawah batas: serahkan PDF ke penampil bawaan seperti aslinya.
    fn show_measured(id: u64) {
        use base64::Engine as _;
        let Some(mut st) = STREAMS.with(|m| m.borrow_mut().remove(&id)) else { return };
        let Some(ms) = st.measure.take() else { return };
        if !st.handle.is_empty() {
            cdp_core(&st.core, "IO.close", &serde_json::json!({"handle": st.handle}).to_string());
        }
        // Isi sudah dibuka kompresinya dan utuh: tajuk panjang/kompresi lama tidak berlaku lagi.
        let headers: Vec<serde_json::Value> = ms
            .headers
            .iter()
            .filter(|(n, _)| !["content-length", "content-encoding", "transfer-encoding"].contains(&n.to_ascii_lowercase().as_str()))
            .map(|(n, v)| serde_json::json!({"name": n, "value": v}))
            .collect();
        let code = if ms.status > 0 { ms.status } else { 200 };
        let params = serde_json::json!({
            "requestId": st.request,
            "responseCode": code,
            "responseHeaders": headers,
            "body": base64::prelude::BASE64_STANDARD.encode(&ms.buf),
        });
        cdp_core(&st.core, "Fetch.fulfillRequest", &params.to_string());
        if let Some(w) = ms.waiting {
            (st.cb)(DlEvent::WaitEnd { id: w });
        }
        (st.cb)(DlEvent::Log(format!(
            "PDF ditampilkan di tab: {} KB dalam {:.1} dtk (ukuran tidak disebut server, di bawah batas {} MB): {}",
            ms.buf.len() / 1024,
            st.started.elapsed().as_secs_f64(),
            ms.limit / 1_048_576,
            ms.url
        )));
    }

    /// Pengukuran melewati batas: lanjutkan sebagai unduhan. Yang sudah dibaca menjadi awal
    /// berkas, jadi tidak ada yang diunduh ulang. false = gagal (unduhan sudah diakhiri).
    fn switch_to_download(id: u64) -> bool {
        use std::io::Write as _;
        let info = STREAMS.with(|m| {
            let mut m = m.borrow_mut();
            let st = m.get_mut(&id)?;
            let ms = st.measure.take()?;
            Some((st.part.clone(), st.path.clone(), st.received, ms, st.cb.clone(), st.core.clone()))
        });
        let Some((part, path, received, ms, cb, core)) = info else { return false };
        cb(DlEvent::Log(format!("PDF tanpa ukuran melewati {} MB, dialihkan ke unduhan: {} → {path}", ms.limit / 1_048_576, ms.url)));
        cb(DlEvent::Started { id, url: ms.url.clone(), path: path.clone(), total: 0 });
        emit_race(&core, &cb, id, &ms.url, ms.referer, ms.agent, 0);
        let file = std::fs::File::create(&part).and_then(|mut f| f.write_all(&ms.buf).map(|_| f));
        match file {
            Ok(f) => {
                STREAMS.with(|m| {
                    if let Some(st) = m.borrow_mut().get_mut(&id) {
                        st.file = Some(f);
                        st.last = std::time::Instant::now();
                    }
                });
                cb(DlEvent::Progress { id, received, total: received });
                true
            }
            Err(e) => {
                finish_stream(id, false, &format!("tidak bisa menulis {}: {e}", part.display()));
                false
            }
        }
    }

    /// Minta aliran isi respons `req` lalu mulai membacanya.
    fn take_stream(sender: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2, req: &str, id: u64) {
        cdp_then(sender, "Fetch.takeResponseBodyAsStream", &serde_json::json!({"requestId": req}).to_string(), move |v| {
            let handle = v.as_ref().and_then(|v| v["stream"].as_str()).unwrap_or("").to_string();
            let known = STREAMS.with(|m| {
                m.borrow_mut().get_mut(&id).map(|s| {
                    s.handle = handle.clone();
                })
            });
            if known.is_none() {
                // Dibatalkan sebelum aliran didapat.
                return;
            }
            if handle.is_empty() {
                return finish_stream(id, false, "aliran respons tidak didapat");
            }
            read_stream(id);
        });
    }

    thread_local! {
        /// Permintaan dokumen yang sedang menunggu jawaban server: kunci tugas Fetch
        /// ("interception-job-N", tanpa akhiran pengalihan) → (id unduhan, dibatalkan).
        static WAITING: std::cell::RefCell<std::collections::HashMap<String, (u64, bool)>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
    }

    /// Alamat yang biasanya menghasilkan berkas (PDF kartu, laporan, ekspor).
    fn looks_like_download(url: &str) -> bool {
        crate::dlengine::looks_like_download(url)
    }

    /// Panggil metode DevTools pada `core` dan terima hasilnya (`None` = gagal).
    fn cdp_then(core: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2, method: &str, params: &str, cb: impl FnOnce(Option<serde_json::Value>) + 'static) {
        use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
        let cb: std::rc::Rc<std::cell::RefCell<Option<Box<dyn FnOnce(Option<serde_json::Value>)>>>> = std::rc::Rc::new(std::cell::RefCell::new(Some(Box::new(cb))));
        let cb2 = cb.clone();
        let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |r, json| {
            let f = cb2.borrow_mut().take();
            if let Some(f) = f {
                f(if r.is_ok() { serde_json::from_str(&json).ok() } else { None });
            }
            Ok(())
        }));
        let sent = unsafe { core.CallDevToolsProtocolMethod(&HSTRING::from(method), &HSTRING::from(params), &handler) };
        if sent.is_err() {
            // Tidak terkirim (mis. tab sudah ditutup): handler tidak akan dipanggil.
            let f = cb.borrow_mut().take();
            if let Some(f) = f {
                f(None);
            }
        }
    }

    /// Akhiri unduhan PDF `id`: `ok` = selesai utuh. Halaman (yang menunggu) diberi halaman
    /// kecil "tersimpan" atau dibatalkan, lalu `Done` dikirim.
    fn finish_stream(id: u64, ok: bool, reason: &str) {
        let Some(mut st) = STREAMS.with(|m| m.borrow_mut().remove(&id)) else { return };
        drop(st.file.take());
        if !st.handle.is_empty() {
            cdp_core(&st.core, "IO.close", &serde_json::json!({"handle": st.handle}).to_string());
        }
        if let Some(ms) = st.measure.take() {
            // Masih diukur (belum jadi unduhan): tidak ada berkas, cukup hentikan halamannya.
            cdp_core(&st.core, "Fetch.failRequest", &serde_json::json!({"requestId": st.request, "errorReason": "Aborted"}).to_string());
            if let Some(w) = ms.waiting {
                (st.cb)(DlEvent::WaitEnd { id: w });
            }
            (st.cb)(DlEvent::Log(format!("PDF: pengukuran dihentikan ({reason}; {} KB): {}", ms.buf.len() / 1024, ms.url)));
            return;
        }
        let mut reason = reason.to_string();
        // Aliran berakhir lebih pendek dari Content-Length: koneksi putus, berkas tidak utuh.
        let mut ok = ok;
        if ok && st.total > 0 && st.received != st.total {
            ok = false;
            reason = "koneksi server terputus sebelum selesai".into();
        }
        if ok {
            let _ = std::fs::remove_file(&st.path);
            if let Err(e) = std::fs::rename(&st.part, &st.path) {
                ok = false;
                reason = format!("ganti nama berkas gagal: {e}");
            }
        }
        let secs = st.started.elapsed().as_secs_f64();
        (st.cb)(DlEvent::Log(if ok {
            format!("PDF tersimpan: {} ({} KB dalam {:.1} dtk, {:.0} KB/dtk)", st.path, st.received / 1024, secs, st.received as f64 / 1024.0 / secs.max(0.1))
        } else {
            // "dibatalkan" = sengaja dihentikan (jalur Ovner lebih cepat / pengguna membatalkan),
            // bukan galat: jangan masuk daftar "Crash dan galat" di Diagnostik.
            if reason == "dibatalkan" {
                format!("PDF: pembacaan respons halaman dihentikan: {} ({} dari {} byte, {:.1} dtk)", st.path, st.received, st.total, secs)
            } else {
                format!("PDF gagal: {} ({reason}; {} dari {} byte, {:.1} dtk)", st.path, st.received, st.total, secs)
            }
        }));
        if ok {
            use base64::Engine as _;
            let name = Path::new(&st.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let link = saved_link(&st.url, &saved_token(&st.path));
            let big = if st.received >= 20 * 1_048_576 { "<br><small style=\"color:#9aa0a6\">PDF sebesar ini bisa lambat atau kosong di penampil; lebih aman dibuka dengan aplikasi PDF dari daftar Unduhan.</small>" } else { "" };
            let html = format!(
                "<!doctype html><meta charset=utf-8><title>{n}</title><body style=\"font:15px Segoe UI,sans-serif;background:#202124;color:#e8eaed;padding:40px\">&#10003; PDF disimpan: <b>{n}</b><br><small style=\"color:#9aa0a6\">{p}</small><p><a href=\"{l}\" target=\"_self\" style=\"display:inline-block;background:#8ab4f8;color:#202124;padding:8px 16px;border-radius:6px;text-decoration:none;font-weight:600\">Buka di sini (lihat &amp; cetak)</a>{big}</p></body>",
                n = html_escape(&name),
                p = html_escape(&st.path),
                l = html_escape(&link)
            );
            let params = serde_json::json!({
                "requestId": st.request,
                "responseCode": 200,
                "responseHeaders": [{"name": "Content-Type", "value": "text/html; charset=utf-8"}],
                "body": base64::prelude::BASE64_STANDARD.encode(html.as_bytes()),
            });
            cdp_core(&st.core, "Fetch.fulfillRequest", &params.to_string());
        } else {
            let _ = std::fs::remove_file(&st.part);
            cdp_core(&st.core, "Fetch.failRequest", &serde_json::json!({"requestId": st.request, "errorReason": "Aborted"}).to_string());
        }
        (st.cb)(DlEvent::Done { id, ok, path: st.path.clone(), received: st.received });
    }

    fn html_escape(s: &str) -> String {
        s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
    }

    /// Ambil cookie situs (termasuk HttpOnly) lalu kirim `DlEvent::Race` untuk jalur koneksi
    /// Ovner. `total` < 0: masih tahap permintaan (Ovner yang memutuskan mulai lebih awal).
    fn emit_race(sender: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2, cb: &std::rc::Rc<dyn Fn(DlEvent)>, id: u64, url: &str, referer: String, agent: String, total: i64) {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_2;
        let Some(cm) = sender.cast::<ICoreWebView2_2>().ok().and_then(|w| unsafe { w.CookieManager() }.ok()) else { return };
        let cb3 = cb.clone();
        let u2 = url.to_string();
        let once = std::cell::RefCell::new(Some((referer, agent)));
        let h = webview2_com::GetCookiesCompletedHandler::create(Box::new(move |_r, list| {
            let Some((referer, user_agent)) = once.borrow_mut().take() else { return Ok(()) };
            let mut cookie = String::new();
            if let Some(list) = list {
                let mut n = 0u32;
                unsafe {
                    let _ = list.Count(&mut n);
                    for i in 0..n.min(300) {
                        if let Ok(c) = list.GetValueAtIndex(i) {
                            let (mut a, mut b) = (windows::core::PWSTR::null(), windows::core::PWSTR::null());
                            if c.Name(&mut a).is_ok() && c.Value(&mut b).is_ok() {
                                if !cookie.is_empty() {
                                    cookie.push_str("; ");
                                }
                                cookie.push_str(&webview2_com::take_pwstr(a));
                                cookie.push('=');
                                cookie.push_str(&webview2_com::take_pwstr(b));
                            }
                        }
                    }
                }
            }
            cb3(DlEvent::Race { id, url: u2.clone(), cookie, referer, user_agent, total });
            Ok(())
        }));
        let _ = unsafe { cm.GetCookies(&HSTRING::from(url), &h) };
    }

    /// Baca potongan berikutnya dari aliran respons PDF `id` dan tulis ke berkas (atau tahan
    /// di memori selama diukur).
    fn read_stream(id: u64) {
        let Some((core, handle)) = STREAMS.with(|m| m.borrow().get(&id).map(|s| (s.core.clone(), s.handle.clone()))) else { return };
        // Satu permintaan baca menunggu sampai `size` terkumpul: 256 KB supaya kemajuan sering terlihat.
        let params = serde_json::json!({"handle": handle, "size": 256 * 1024}).to_string();
        cdp_then(&core, "IO.read", &params, move |v| {
            use base64::Engine as _;
            use std::io::Write as _;
            let Some(v) = v else { return finish_stream(id, false, "IO.read gagal (tab ditutup atau aliran putus)") };
            let data = v["data"].as_str().unwrap_or("");
            let bytes = if v["base64Encoded"].as_bool().unwrap_or(false) {
                match base64::prelude::BASE64_STANDARD.decode(data) {
                    Ok(b) => b,
                    Err(_) => return finish_stream(id, false, "data aliran rusak"),
                }
            } else {
                data.as_bytes().to_vec()
            };
            let eof = v["eof"].as_bool().unwrap_or(false);
            enum Step {
                Fail,
                Show,
                Switch,
                Measuring,
                Wrote(bool, i64, i64, std::rc::Rc<dyn Fn(DlEvent)>),
            }
            let step = STREAMS.with(|m| {
                let mut m = m.borrow_mut();
                let Some(st) = m.get_mut(&id) else { return Step::Fail };
                if let Some(ms) = st.measure.as_mut() {
                    ms.buf.extend_from_slice(&bytes);
                    st.received += bytes.len() as i64;
                    return if eof {
                        Step::Show
                    } else if ms.buf.len() as u64 > ms.limit {
                        Step::Switch
                    } else {
                        Step::Measuring
                    };
                }
                let Some(f) = st.file.as_mut() else { return Step::Fail };
                if !bytes.is_empty() && f.write_all(&bytes).is_err() {
                    return Step::Fail;
                }
                st.received += bytes.len() as i64;
                let due = st.last.elapsed() >= std::time::Duration::from_millis(300);
                if due {
                    st.last = std::time::Instant::now();
                }
                Step::Wrote(due, st.received, st.total, st.cb.clone())
            });
            match step {
                // Dibatalkan (sudah dihapus dari daftar) atau gagal menulis.
                Step::Fail => return finish_stream(id, false, "gagal menulis berkas"),
                Step::Show => return show_measured(id),
                Step::Switch => {
                    if !switch_to_download(id) {
                        return;
                    }
                }
                Step::Measuring => {}
                Step::Wrote(due, received, total, cb) => {
                    if eof {
                        return finish_stream(id, true, "");
                    }
                    if due {
                        cb(DlEvent::Progress { id, received, total: total.max(received) });
                    }
                }
            }
            read_stream(id);
        });
    }

    /// PDF diunduh, tidak dibuka di penampil PDF bawaan (seperti setelan Chrome "Unduh berkas
    /// PDF"). Respons dokumen PDF dicegat lewat DevTools (Fetch, tahap respons); isinya dibaca
    /// langsung dari respons yang sudah ada (Fetch.takeResponseBodyAsStream + IO.read) dan
    /// ditulis Ovner sendiri ke berkas.
    ///
    /// Dulu tajuknya diganti menjadi `Content-Disposition: attachment` agar WebView2 yang
    /// mengunduh. Pada kartu SIPP (PDF buatan server, dari tab pop-up) itu membuat mesin WebView2
    /// mati, atau unduhannya macet di 0 KB dan tidak bisa dibatalkan. Cara ini tidak membuat
    /// unduhan WebView2 sama sekali, dan server tidak perlu membuat PDF dua kali.
    ///
    /// `decide(alamat, ukuran, nama)` = None bila PDF ditampilkan seperti biasa; Some((path,
    /// None)) = diunduh; Some((path, Some(batas))) = ukuran tidak disebut server, diukur dulu
    /// (lihat `Measure`). `cb` menerima Started/Progress/Done seperti unduhan lain.
    /// `ready` dipanggil setelah pencegatan aktif (Fetch.enable selesai): alamat pertama tab baru
    /// baru dimuat sesudahnya, supaya PDF yang langsung dibuka di tab baru ikut dicegat.
    pub fn pdf_downloads(webview: &wry::WebView, decide: impl Fn(&str, Option<u64>, &str) -> Option<(String, Option<u64>)> + 'static, cb: impl Fn(DlEvent) + 'static, ready: impl FnOnce() + 'static) {
        use webview2_com::DevToolsProtocolEventReceivedEventHandler;
        use wry::WebViewExtWindows;
        let core = webview.webview();
        let cb: std::rc::Rc<dyn Fn(DlEvent)> = std::rc::Rc::new(cb);
        let handler = DevToolsProtocolEventReceivedEventHandler::create(Box::new(move |sender, args| {
            let (Some(sender), Some(args)) = (sender, args) else { return Ok(()) };
            let mut p = windows::core::PWSTR::null();
            if unsafe { args.ParameterObjectAsJson(&mut p) }.is_err() {
                return Ok(());
            }
            let v: serde_json::Value = serde_json::from_str(&webview2_com::take_pwstr(p)).unwrap_or_default();
            let req = v["requestId"].as_str().unwrap_or("").to_string();
            if req.is_empty() {
                return Ok(());
            }
            let url = v["request"]["url"].as_str().unwrap_or("").to_string();
            let job = req.rsplit_once('.').map(|(a, _)| a.to_string()).unwrap_or_else(|| req.clone());
            let continue_req = |sender: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2| cdp_core(sender, "Fetch.continueRequest", &serde_json::json!({"requestId": req}).to_string());
            // Berkas di komputer sendiri (file://) tidak perlu dicegat sama sekali.
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                continue_req(&sender);
                return Ok(());
            }
            // Tahap permintaan (hanya alamat yang mirip unduhan): tampilkan "menunggu server"
            // segera, jangan tunggu server selesai membuat berkasnya.
            if v.get("responseStatusCode").is_none() && v.get("responseErrorReason").is_none() {
                if let Some(token) = saved_param(&url) {
                    serve_saved(&sender, &req, token);
                    return Ok(());
                }
                if looks_like_download(&url) && !WAITING.with(|m| m.borrow().contains_key(&job)) {
                    let id = next_download_id();
                    WAITING.with(|m| m.borrow_mut().insert(job.clone(), (id, false)));
                    cb(DlEvent::Waiting { id, url: url.clone() });
                    // Alamat yang dulu menghasilkan PDF besar: jalur Ovner boleh mulai sekarang
                    // juga (server membuat berkas untuk tiap koneksi bersamaan).
                    let rh = &v["request"]["headers"];
                    let rhv = |name: &str| rh.as_object().and_then(|o| o.iter().find(|(k, _)| k.eq_ignore_ascii_case(name))).and_then(|(_, v)| v.as_str()).unwrap_or("").to_string();
                    emit_race(&sender, &cb, id, &url, rhv("referer"), rhv("user-agent"), -1);
                }
                continue_req(&sender);
                return Ok(());
            }
            let waiting = WAITING.with(|m| m.borrow_mut().remove(&job));
            if let Some((wid, true)) = waiting {
                // Dibatalkan pengguna saat menunggu server.
                cdp_core(&sender, "Fetch.failRequest", &serde_json::json!({"requestId": req, "errorReason": "Aborted"}).to_string());
                cb(DlEvent::WaitEnd { id: wid });
                return Ok(());
            }
            let headers = v["responseHeaders"].as_array().cloned().unwrap_or_default();
            let hv = |name: &str| headers.iter().find(|h| h["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name))).and_then(|h| h["value"].as_str()).map(str::to_string);
            let status = v["responseStatusCode"].as_i64().unwrap_or(0);
            if (300..400).contains(&status) {
                // Pengalihan: permintaan berikutnya masih tugas yang sama.
                if let Some(w) = waiting {
                    WAITING.with(|m| m.borrow_mut().insert(job.clone(), w));
                }
                continue_req(&sender);
                return Ok(());
            }
            let is_pdf = hv("content-type").is_some_and(|c| c.to_ascii_lowercase().contains("application/pdf"));
            let disp = hv("content-disposition").unwrap_or_default();
            let len = hv("content-length").and_then(|l| l.trim().parse::<u64>().ok());
            // Isi yang dibaca sudah dibuka kompresinya: Content-Length (ukuran terkompresi) tidak
            // bisa dipakai sebagai total.
            let encoded = hv("content-encoding").is_some_and(|e| !e.trim().is_empty() && !e.trim().eq_ignore_ascii_case("identity"));
            let total = if encoded { 0 } else { len.map_or(0, |l| l as i64) };
            let target = if status == 200 && is_pdf && !disp.to_ascii_lowercase().starts_with("attachment") {
                decide(&url, len, &crate::dlengine::pdf_name(&disp, &url))
            } else {
                None
            };
            let Some((path, measure)) = target else {
                if let Some((wid, _)) = waiting {
                    cb(DlEvent::WaitEnd { id: wid });
                }
                continue_req(&sender);
                return Ok(());
            };
            if let Some(limit) = measure {
                // Ukuran tidak disebut (PDF dibuat saat itu juga, mis. cetakan Infor LN): baca
                // dulu; kecil = tampilkan seperti aslinya, besar = jadi unduhan.
                let id = waiting.map(|(w, _)| w).unwrap_or_else(next_download_id);
                let rh = &v["request"]["headers"];
                let rhv = |name: &str| rh.as_object().and_then(|o| o.iter().find(|(k, _)| k.eq_ignore_ascii_case(name))).and_then(|(_, v)| v.as_str()).unwrap_or("").to_string();
                let all: Vec<(String, String)> = headers
                    .iter()
                    .filter_map(|h| Some((h["name"].as_str()?.to_string(), h["value"].as_str().unwrap_or("").to_string())))
                    .collect();
                STREAMS.with(|m| {
                    m.borrow_mut().insert(
                        id,
                        PdfStream {
                            core: sender.clone(),
                            request: req.clone(),
                            handle: String::new(),
                            file: None,
                            part: std::path::PathBuf::from(format!("{path}.ovnerpart")),
                            path: path.clone(),
                            received: 0,
                            total: 0,
                            last: std::time::Instant::now(),
                            started: std::time::Instant::now(),
                            cb: cb.clone(),
                            url: url.clone(),
                            measure: Some(Measure {
                                buf: Vec::new(),
                                limit,
                                status,
                                headers: all,
                                url: url.clone(),
                                referer: rhv("referer"),
                                agent: rhv("user-agent"),
                                waiting: waiting.map(|(w, _)| w),
                            }),
                        },
                    )
                });
                cb(DlEvent::Log(format!("PDF tanpa ukuran dari server, diukur dulu (batas {} MB): {url}", limit / 1_048_576)));
                take_stream(&sender, &req, id);
                return Ok(());
            }
            let part = std::path::PathBuf::from(format!("{path}.ovnerpart"));
            let file = match std::fs::File::create(&part) {
                Ok(f) => f,
                Err(e) => {
                    // Tidak bisa menulis: tampilkan PDF seperti biasa.
                    cb(DlEvent::Log(format!("PDF tidak bisa disimpan ke {path}: {e}")));
                    if let Some((wid, _)) = waiting {
                        cb(DlEvent::WaitEnd { id: wid });
                    }
                    continue_req(&sender);
                    return Ok(());
                }
            };
            let id = waiting.map(|(w, _)| w).unwrap_or_else(next_download_id);
            STREAMS.with(|m| {
                m.borrow_mut().insert(
                    id,
                    PdfStream {
                        core: sender.clone(),
                        request: req.clone(),
                        handle: String::new(),
                        file: Some(file),
                        part,
                        path: path.clone(),
                        received: 0,
                        total,
                        last: std::time::Instant::now(),
                        started: std::time::Instant::now(),
                        cb: cb.clone(),
                        url: url.clone(),
                        measure: None,
                    },
                )
            });
            let rh = &v["request"]["headers"];
            let rhv = |name: &str| rh.as_object().and_then(|o| o.iter().find(|(k, _)| k.eq_ignore_ascii_case(name))).and_then(|(_, v)| v.as_str()).unwrap_or("").to_string();
            let (referer, agent) = (rhv("referer"), rhv("user-agent"));
            cb(DlEvent::Log(format!(
                "PDF diunduh langsung dari respons: {url} → {path} ({} KB; Accept-Ranges={}, ETag={}, Last-Modified={}, Server={})",
                total / 1024,
                hv("accept-ranges").unwrap_or_default(),
                hv("etag").unwrap_or_default(),
                hv("last-modified").unwrap_or_default(),
                hv("server").unwrap_or_default()
            )));
            cb(DlEvent::Started { id, url: url.clone(), path, total });
            emit_race(&sender, &cb, id, &url, referer, agent, total);
            take_stream(&sender, &req, id);
            Ok(())
        }));
        unsafe {
            if let Ok(r) = core.GetDevToolsProtocolEventReceiver(&HSTRING::from("Fetch.requestPaused")) {
                let mut token = 0i64;
                let _ = r.add_DevToolsProtocolEventReceived(&handler, &mut token);
            }
        }
        // Tahap respons: semua dokumen (cek PDF). Tahap permintaan: hanya alamat yang mirip
        // unduhan, supaya bisa langsung ditampilkan "menunggu server".
        let mut patterns = vec![serde_json::json!({"urlPattern": "*", "resourceType": "Document", "requestStage": "Response"})];
        for k in ["*download*", "*Download*", "*cetak*", "*.pdf*", "*export*", "*unduh*", "*ovner_saved=*"] {
            patterns.push(serde_json::json!({"urlPattern": k, "resourceType": "Document", "requestStage": "Request"}));
        }
        cdp_then(&core, "Fetch.enable", &serde_json::json!({"patterns": patterns}).to_string(), move |_| ready());
    }

    pub fn on_window_close(webview: &wry::WebView, cb: impl Fn() + 'static) {
        use webview2_com::WindowCloseRequestedEventHandler;
        use wry::WebViewExtWindows;
        let handler = WindowCloseRequestedEventHandler::create(Box::new(move |_, _| {
            cb();
            Ok(())
        }));
        let mut token = 0i64;
        unsafe {
            let _ = webview.webview().add_WindowCloseRequested(&handler, &mut token);
        }
    }

    /// Panggil metode DevTools langsung pada ICoreWebView2 (dari handler navigasi).
    pub fn cdp_core(core: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2, method: &str, params: &str) {
        use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
        unsafe {
            let _ = core.CallDevToolsProtocolMethod(
                &HSTRING::from(method),
                &HSTRING::from(params),
                &CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|_, _| Ok(()))),
            );
        }
    }

    /// User agent bawaan WebView2 dari ICoreWebView2.
    pub fn core_user_agent(core: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2) -> String {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings2;
        unsafe {
            let Ok(st) = core.Settings().and_then(|s| s.cast::<ICoreWebView2Settings2>()) else { return String::new() };
            let mut p = windows::core::PWSTR::null();
            if st.UserAgent(&mut p).is_ok() {
                return webview2_com::take_pwstr(p);
            }
        }
        String::new()
    }

    /// Panggil metode DevTools dan terima hasil JSON-nya.
    pub fn cdp_call(webview: &wry::WebView, method: &str, params: &str, cb: impl FnOnce(String) + 'static) {
        use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
        use wry::WebViewExtWindows;
        let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |r, json| {
            if r.is_ok() {
                cb(json);
            }
            Ok(())
        }));
        unsafe {
            let _ = webview.webview().CallDevToolsProtocolMethod(&HSTRING::from(method), &HSTRING::from(params), &handler);
        }
    }

    /// Seperti `cdp_call`, tetapi `cb(None)` juga dipanggil saat perintah gagal.
    pub fn cdp_result(webview: &wry::WebView, method: &str, params: &str, cb: impl FnOnce(Option<String>) + 'static) {
        use wry::WebViewExtWindows;
        cdp_then(&webview.webview(), method, params, move |v| cb(v.map(|v| v.to_string())));
    }

    /// Tema aplikasi Windows sedang gelap? (Pengaturan → Personalisasi → Warna)
    pub fn system_dark() -> bool {
        winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
            .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize")
            .and_then(|k| k.get_value::<u32, _>("AppsUseLightTheme"))
            .map(|v| v == 0)
            .unwrap_or(false)
    }

    /// Versi Windows untuk laporan diagnostik, mis. "Windows 10.0.19045".
    pub fn os_version() -> String {
        let key = hkcu_local_machine("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion");
        let get = |n: &str| key.as_ref().and_then(|k| k.get_value::<String, _>(n).ok()).unwrap_or_default();
        let build = get("CurrentBuildNumber");
        let ubr = key.as_ref().and_then(|k| k.get_value::<u32, _>("UBR").ok()).map(|u| format!(".{u}")).unwrap_or_default();
        format!("{} {} (build {build}{ubr})", get("ProductName"), get("DisplayVersion"))
    }

    fn hkcu_local_machine(path: &str) -> Option<winreg::RegKey> {
        winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE).open_subkey(path).ok()
    }

    /// Panggil metode protokol DevTools pada tab ini (bekerja walau DevTools dimatikan).
    pub fn cdp(webview: &wry::WebView, method: &str, params: &str) {
        use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
        use wry::WebViewExtWindows;
        unsafe {
            let _ = webview.webview().CallDevToolsProtocolMethod(
                &HSTRING::from(method),
                &HSTRING::from(params),
                &CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|_, _| Ok(()))),
            );
        }
    }

    /// User agent bawaan WebView2 tab ini.
    pub fn user_agent(webview: &wry::WebView) -> String {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings2;
        use wry::WebViewExtWindows;
        unsafe {
            let Ok(st) = webview.webview().Settings().and_then(|s| s.cast::<ICoreWebView2Settings2>()) else { return String::new() };
            let mut p = windows::core::PWSTR::null();
            if st.UserAgent(&mut p).is_ok() {
                return webview2_com::take_pwstr(p);
            }
        }
        String::new()
    }

    pub fn tune_webview(webview: &wry::WebView, s: &crate::settings::Settings) {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            ICoreWebView2Settings4, ICoreWebView2Settings8, ICoreWebView2_13, COREWEBVIEW2_PERMISSION_KIND,
            COREWEBVIEW2_PERMISSION_KIND_CAMERA, COREWEBVIEW2_PERMISSION_KIND_GEOLOCATION,
            COREWEBVIEW2_PERMISSION_KIND_LOCAL_FONTS, COREWEBVIEW2_PERMISSION_KIND_MICROPHONE,
            COREWEBVIEW2_PERMISSION_KIND_MIDI_SYSTEM_EXCLUSIVE_MESSAGES, COREWEBVIEW2_PERMISSION_KIND_OTHER_SENSORS,
            COREWEBVIEW2_PERMISSION_KIND_WINDOW_MANAGEMENT, COREWEBVIEW2_PERMISSION_STATE_DENY,
        };
        use webview2_com::PermissionRequestedEventHandler;
        use wry::WebViewExtWindows;
        let wv = webview.webview();
        unsafe {
            if let Ok(st) = wv.Settings().and_then(|s| s.cast::<ICoreWebView2Settings4>()) {
                let _ = st.SetIsGeneralAutofillEnabled(s.autofill);
                // Sandi dikelola brankas Ovner sendiri (bisa dilihat dan diekspor), bukan WebView2.
                let _ = st.SetIsPasswordAutosaveEnabled(false);
            }
            if let Ok(st) = wv.Settings().and_then(|s| s.cast::<ICoreWebView2Settings8>()) {
                let _ = st.SetIsReputationCheckingRequired(s.smartscreen);
            }
            if !s.download_dir.trim().is_empty() {
                if let Ok(profile) = wv.cast::<ICoreWebView2_13>().and_then(|w| w.Profile()) {
                    let _ = profile.SetDefaultDownloadFolderPath(&HSTRING::from(s.download_dir.trim()));
                }
            }
            if s.deny_sensitive_permissions {
                let perm = PermissionRequestedEventHandler::create(Box::new(|_wv, args| {
                    let Some(a) = args else { return Ok(()) };
                    let mut kind = COREWEBVIEW2_PERMISSION_KIND::default();
                    a.PermissionKind(&mut kind)?;
                    let deny = [
                        COREWEBVIEW2_PERMISSION_KIND_CAMERA,
                        COREWEBVIEW2_PERMISSION_KIND_MICROPHONE,
                        COREWEBVIEW2_PERMISSION_KIND_GEOLOCATION,
                        COREWEBVIEW2_PERMISSION_KIND_OTHER_SENSORS,
                        COREWEBVIEW2_PERMISSION_KIND_MIDI_SYSTEM_EXCLUSIVE_MESSAGES,
                        COREWEBVIEW2_PERMISSION_KIND_WINDOW_MANAGEMENT,
                        COREWEBVIEW2_PERMISSION_KIND_LOCAL_FONTS,
                    ];
                    if deny.contains(&kind) {
                        a.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY)?;
                    }
                    Ok(())
                }));
                let mut token = 0i64;
                let _ = wv.add_PermissionRequested(&perm, &mut token);
            }
        }
    }

    /// Minta WebView2 menangguhkan halaman (hanya berhasil bila webview tidak terlihat).
    /// Simpan tampilan halaman sebagai PNG (untuk --self-test).
    pub fn capture_png(webview: &wry::WebView, path: &Path) {
        use webview2_com::{CapturePreviewCompletedHandler, Microsoft::Web::WebView2::Win32::COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG};
        use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
        use windows::Win32::System::Com::{STGM_CREATE, STGM_WRITE};
        use windows::Win32::UI::Shell::SHCreateStreamOnFileEx;
        use wry::WebViewExtWindows;
        unsafe {
            let Ok(stream) = SHCreateStreamOnFileEx(&HSTRING::from(path.as_os_str()), (STGM_CREATE | STGM_WRITE).0, FILE_ATTRIBUTE_NORMAL.0, true, None::<&windows::Win32::System::Com::IStream>) else { return };
            let keep = stream.clone();
            // Aliran dilepas setelah selesai supaya berkasnya tertutup.
            let handler = CapturePreviewCompletedHandler::create(Box::new(move |_| {
                drop(keep);
                Ok(())
            }));
            let _ = webview.webview().CapturePreview(COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG, &stream, &handler);
        }
    }

    pub fn try_suspend(webview: &wry::WebView) {
        use webview2_com::{Microsoft::Web::WebView2::Win32::ICoreWebView2_3, TrySuspendCompletedHandler};
        use wry::WebViewExtWindows;
        if let Ok(wv3) = webview.webview().cast::<ICoreWebView2_3>() {
            let handler = TrySuspendCompletedHandler::create(Box::new(|_, _| Ok(())));
            let _ = unsafe { wv3.TrySuspend(&handler) };
        }
    }

    /// Bangunkan halaman yang ditangguhkan.
    pub fn resume(webview: &wry::WebView) {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_3;
        use wry::WebViewExtWindows;
        if let Ok(wv3) = webview.webview().cast::<ICoreWebView2_3>() {
            let _ = unsafe { wv3.Resume() };
        }
    }

    /// Nyalakan/matikan JavaScript untuk navigasi berikutnya di tab ini.
    pub fn set_script_enabled(webview: &wry::WebView, enabled: bool) {
        use wry::WebViewExtWindows;
        unsafe {
            if let Ok(st) = webview.webview().Settings() {
                let _ = st.SetIsScriptEnabled(enabled);
            }
        }
    }

    /// Panggil `cb` bila proses WebView2 tab ini crash atau macet.
    /// `cb(jenis, rincian)`; rincian = alasan, kode keluar, proses dan modul penyebab dari
    /// WebView2 (untuk mencari akar masalah crash di log).
    pub fn on_process_failed(webview: &wry::WebView, cb: impl Fn(ProcFail, String) + 'static) {
        use webview2_com::Microsoft::Web::WebView2::Win32::*;
        use webview2_com::ProcessFailedEventHandler;
        use wry::WebViewExtWindows;
        let handler = ProcessFailedEventHandler::create(Box::new(move |_wv, args| {
            if let Some(a) = args {
                let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
                unsafe {
                    a.ProcessFailedKind(&mut kind)?;
                }
                let mut detail = format!("kind={}", kind.0);
                if let Ok(a2) = a.cast::<ICoreWebView2ProcessFailedEventArgs2>() {
                    unsafe {
                        let mut reason = COREWEBVIEW2_PROCESS_FAILED_REASON::default();
                        if a2.Reason(&mut reason).is_ok() {
                            detail.push_str(&format!(" reason={}", reason.0));
                        }
                        let mut code = 0i32;
                        if a2.ExitCode(&mut code).is_ok() {
                            detail.push_str(&format!(" exit=0x{:08X}", code as u32));
                        }
                        let mut p = windows::core::PWSTR::null();
                        if a2.ProcessDescription(&mut p).is_ok() {
                            let d = webview2_com::take_pwstr(p);
                            if !d.is_empty() {
                                detail.push_str(&format!(" proses=\"{d}\""));
                            }
                        }
                    }
                }
                if let Ok(a3) = a.cast::<ICoreWebView2ProcessFailedEventArgs3>() {
                    let mut p = windows::core::PWSTR::null();
                    if unsafe { a3.FailureSourceModulePath(&mut p) }.is_ok() {
                        let m = webview2_com::take_pwstr(p);
                        if !m.is_empty() {
                            detail.push_str(&format!(" modul={m}"));
                        }
                    }
                }
                cb(
                    if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED {
                        ProcFail::Browser
                    } else if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED {
                        ProcFail::Render
                    } else if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE {
                        ProcFail::Unresponsive
                    } else {
                        ProcFail::Other
                    },
                    detail,
                );
            }
            Ok(())
        }));
        let mut token = 0i64;
        unsafe {
            let _ = webview.webview().add_ProcessFailed(&handler, &mut token);
        }
    }

    /// Alamat halaman berubah, termasuk tanpa memuat ulang (YouTube dan situs satu halaman
    /// lain memakai history.pushState). Event resmi WebView2, jadi tidak bergantung pada skrip
    /// halaman (situs bisa memakai salinan pushState yang belum dipasangi pemberi tahu Ovner).
    /// `cb(alamat, dokumen_baru)`: dokumen_baru = false untuk perpindahan tanpa memuat ulang.
    pub fn on_source_changed(webview: &wry::WebView, cb: impl Fn(String, bool) + 'static) {
        use webview2_com::SourceChangedEventHandler;
        use wry::WebViewExtWindows;
        let handler = SourceChangedEventHandler::create(Box::new(move |wv, args| {
            if let Some(wv) = wv {
                let mut new_doc = windows::core::BOOL(1);
                if let Some(a) = &args {
                    let _ = unsafe { a.IsNewDocument(&mut new_doc) };
                }
                let mut p = windows::core::PWSTR::null();
                if unsafe { wv.Source(&mut p) }.is_ok() {
                    let url = webview2_com::take_pwstr(p);
                    if !url.is_empty() {
                        cb(url, new_doc.as_bool());
                    }
                }
            }
            Ok(())
        }));
        let mut token = 0i64;
        unsafe {
            let _ = webview.webview().add_SourceChanged(&handler, &mut token);
        }
    }

    /// Panel unduhan bawaan WebView2 ("Downloads" dengan See more) tidak pernah dipakai Ovner:
    /// begitu terbuka (Ctrl+J di PDF/iframe, unduhan yang lolos, dsb.) langsung ditutup dan
    /// `cb` dipanggil supaya Ovner menampilkan Download box v2 sendiri.
    pub fn on_download_dialog(webview: &wry::WebView, cb: impl Fn() + 'static) {
        use webview2_com::IsDefaultDownloadDialogOpenChangedEventHandler;
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_9;
        use wry::WebViewExtWindows;
        let Ok(w9) = webview.webview().cast::<ICoreWebView2_9>() else { return };
        let handler = IsDefaultDownloadDialogOpenChangedEventHandler::create(Box::new(move |sender, _| {
            if let Some(w9) = sender.and_then(|s| s.cast::<ICoreWebView2_9>().ok()) {
                let mut open = windows::core::BOOL(0);
                unsafe {
                    if w9.IsDefaultDownloadDialogOpen(&mut open).is_ok() && open.as_bool() {
                        let _ = w9.CloseDefaultDownloadDialog();
                        cb();
                    }
                }
            }
            Ok(())
        }));
        let mut token = 0i64;
        unsafe {
            let _ = w9.add_IsDefaultDownloadDialogOpenChanged(&handler, &mut token);
        }
    }

    /// Ctrl+J di mana pun dalam tab (termasuk penampil PDF, iframe, situs tanpa JavaScript):
    /// jangan buka panel unduhan bawaan WebView2, panggil `cb` (halaman Unduhan Ovner).
    pub fn on_ctrl_j(webview: &wry::WebView, cb: impl Fn() + 'static) {
        use webview2_com::AcceleratorKeyPressedEventHandler;
        use webview2_com::Microsoft::Web::WebView2::Win32::{COREWEBVIEW2_KEY_EVENT_KIND, COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN};
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
        use wry::WebViewExtWindows;
        let handler = AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            unsafe {
                let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
                let mut key = 0u32;
                let _ = args.KeyEventKind(&mut kind);
                let _ = args.VirtualKey(&mut key);
                let down = |vk: u16| GetKeyState(vk as i32) < 0;
                if kind == COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN && key == 0x4A && down(VK_CONTROL.0) && !down(VK_MENU.0) && !down(VK_SHIFT.0) {
                    let _ = args.SetHandled(true);
                    cb();
                }
            }
            Ok(())
        }));
        let mut token = 0i64;
        unsafe {
            let _ = webview.controller().add_AcceleratorKeyPressed(&handler, &mut token);
        }
    }

    /// Penangkap media: setiap respons video/audio/daftar putar (lihat `classify_media`)
    /// dilaporkan beserta header permintaannya. Hanya header yang dibaca, bukan isinya.
    pub fn on_media(webview: &wry::WebView, cb: impl Fn(super::MediaHit) + 'static) {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_2;
        use webview2_com::WebResourceResponseReceivedEventHandler;
        use wry::WebViewExtWindows;
        let Ok(w2) = webview.webview().cast::<ICoreWebView2_2>() else { return };
        let handler = WebResourceResponseReceivedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            unsafe {
                let (Ok(req), Ok(resp)) = (args.Request(), args.Response()) else { return Ok(()) };
                let mut p = windows::core::PWSTR::null();
                if req.Uri(&mut p).is_err() {
                    return Ok(());
                }
                let url = webview2_com::take_pwstr(p);
                // Saringan cepat: aset statis biasa tidak perlu dibaca header-nya (hemat CPU).
                {
                    let path = url.split(['?', '#']).next().unwrap_or("").to_ascii_lowercase();
                    if [".js", ".css", ".png", ".jpg", ".jpeg", ".gif", ".svg", ".webp", ".avif", ".ico", ".woff", ".woff2", ".ttf", ".json", ".html", ".htm", ".ts", ".m4s"].iter().any(|e| path.ends_with(e)) {
                        return Ok(());
                    }
                }
                let mut status = 0i32;
                let _ = resp.StatusCode(&mut status);
                let Ok(rh) = resp.Headers() else { return Ok(()) };
                let get_resp = |name: &str| {
                    let mut v = windows::core::PWSTR::null();
                    if rh.GetHeader(&HSTRING::from(name), &mut v).is_ok() {
                        webview2_com::take_pwstr(v)
                    } else {
                        String::new()
                    }
                };
                let mime = get_resp("Content-Type");
                let Some(kind) = super::classify_media(&url, &mime, status) else { return Ok(()) };
                let mut len: u64 = get_resp("Content-Length").trim().parse().unwrap_or(0);
                // 206: ukuran utuh ada di Content-Range ("bytes 0-1/12345").
                if status == 206 {
                    len = get_resp("Content-Range").rsplit('/').next().and_then(|t| t.trim().parse().ok()).unwrap_or(0);
                }
                // Video sangat kecil (ikon animasi, iklan pendek) tidak dicatat.
                if kind == "file" && len > 0 && len < 300 * 1024 {
                    return Ok(());
                }
                let mut headers = Vec::new();
                if let Ok(qh) = req.Headers() {
                    for name in ["Referer", "Origin", "Cookie", "User-Agent", "Authorization"] {
                        let mut v = windows::core::PWSTR::null();
                        if qh.GetHeader(&HSTRING::from(name), &mut v).is_ok() {
                            let val = webview2_com::take_pwstr(v);
                            if !val.is_empty() {
                                headers.push((name.to_string(), val));
                            }
                        }
                    }
                }
                cb(super::MediaHit { url, kind, mime: mime.split(';').next().unwrap_or("").trim().to_string(), len, headers });
            }
            Ok(())
        }));
        let mut token = 0i64;
        unsafe {
            let _ = w2.add_WebResourceResponseReceived(&handler, &mut token);
        }
    }

    /// Halaman (tab) mendapat fokus: dipakai untuk menutup panel toolbar saat halaman diklik.
    pub fn on_focus(webview: &wry::WebView, cb: impl Fn() + 'static) {
        use webview2_com::FocusChangedEventHandler;
        use wry::WebViewExtWindows;
        let handler = FocusChangedEventHandler::create(Box::new(move |_, _| {
            cb();
            Ok(())
        }));
        let mut token = 0i64;
        unsafe {
            let _ = webview.controller().add_GotFocus(&handler, &mut token);
        }
    }

    /// Menu klik kanan Ovner: item tambahan disisipkan ke menu bawaan WebView2 (gaya menu Edge).
    /// `items(info)` mengembalikan daftar item untuk sasaran klik ini; `cb(perintah, info)`
    /// dipanggil saat item dipilih. `info` berisi page, link, src, sel, kind ("page", "image",
    /// "video", "audio", "text"), editable.
    pub fn context_menu(webview: &wry::WebView, items: impl Fn(&serde_json::Value) -> Vec<super::CtxItem> + 'static, cb: impl Fn(&str, &serde_json::Value) + 'static) {
        use webview2_com::Microsoft::Web::WebView2::Win32::*;
        use webview2_com::{ContextMenuRequestedEventHandler, CustomItemSelectedEventHandler};
        use wry::WebViewExtWindows;
        let Ok(w11) = webview.webview().cast::<ICoreWebView2_11>() else { return };
        let Ok(env) = webview.environment().cast::<ICoreWebView2Environment9>() else { return };
        let cb = std::rc::Rc::new(cb);
        let handler = ContextMenuRequestedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            unsafe {
                let Ok(target) = args.ContextMenuTarget() else { return Ok(()) };
                let take = |f: &dyn Fn(*mut windows::core::PWSTR) -> windows::core::Result<()>| {
                    let mut p = windows::core::PWSTR::null();
                    if f(&mut p).is_ok() {
                        webview2_com::take_pwstr(p)
                    } else {
                        String::new()
                    }
                };
                let flag = |f: &dyn Fn(*mut windows::core::BOOL) -> windows::core::Result<()>| {
                    let mut b = windows::core::BOOL(0);
                    f(&mut b).is_ok() && b.as_bool()
                };
                let mut kind = COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND::default();
                let _ = target.Kind(&mut kind);
                let kind_s = match kind {
                    COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND_IMAGE => "image",
                    COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND_VIDEO => "video",
                    COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND_AUDIO => "audio",
                    COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND_SELECTED_TEXT => "text",
                    _ => "page",
                };
                let info = serde_json::json!({
                    "kind": kind_s,
                    "page": take(&|p| target.PageUri(p)),
                    "frame": take(&|p| target.FrameUri(p)),
                    "link": if flag(&|b| target.HasLinkUri(b)) { take(&|p| target.LinkUri(p)) } else { String::new() },
                    "src": if flag(&|b| target.HasSourceUri(b)) { take(&|p| target.SourceUri(p)) } else { String::new() },
                    "sel": if flag(&|b| target.HasSelection(b)) { take(&|p| target.SelectionText(p)) } else { String::new() },
                    "editable": flag(&|b| target.IsEditable(b)),
                });
                let list = items(&info);
                if list.is_empty() {
                    return Ok(());
                }
                let Ok(menu) = args.MenuItems() else { return Ok(()) };
                let mut count = 0u32;
                let _ = menu.Count(&mut count);
                // Sisipkan sebelum "Inspect" (paling bawah) supaya Inspect tetap terakhir, seperti Edge.
                let mut at = count;
                let mut names = std::collections::HashSet::new();
                for i in 0..count {
                    if let Ok(it) = menu.GetValueAtIndex(i) {
                        let mut p = windows::core::PWSTR::null();
                        if it.Name(&mut p).is_ok() {
                            let n = webview2_com::take_pwstr(p);
                            if n == "inspectElement" {
                                at = i;
                            }
                            names.insert(n);
                        }
                    }
                }
                let info = std::rc::Rc::new(info);
                fn make(
                    env: &ICoreWebView2Environment9,
                    it: &super::CtxItem,
                    names: &std::collections::HashSet<String>,
                    info: &std::rc::Rc<serde_json::Value>,
                    cb: &std::rc::Rc<dyn Fn(&str, &serde_json::Value)>,
                ) -> Option<ICoreWebView2ContextMenuItem> {
                    if !it.unless.is_empty() && names.contains(it.unless) {
                        return None;
                    }
                    unsafe {
                        if it.label == "-" {
                            return env.CreateContextMenuItem(&HSTRING::from(""), None, COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_SEPARATOR).ok();
                        }
                        let kind = if it.children.is_empty() { COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_COMMAND } else { COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_SUBMENU };
                        // WebView2 tidak menyediakan kolom pintasan untuk item buatan sendiri (tab di
                        // label bisa tampil sebagai karakter aneh): pintasan ditulis dalam kurung.
                        let label = if it.shortcut.is_empty() { it.label.clone() } else { format!("{} ({})", it.label, it.shortcut) };
                        let item = env.CreateContextMenuItem(&HSTRING::from(label.as_str()), None, kind).ok()?;
                        if it.children.is_empty() {
                            let (cmd, info, cb) = (it.cmd.clone(), info.clone(), cb.clone());
                            let h = CustomItemSelectedEventHandler::create(Box::new(move |_, _| {
                                cb(&cmd, &info);
                                Ok(())
                            }));
                            let mut tok = 0i64;
                            let _ = item.add_CustomItemSelected(&h, &mut tok);
                        } else if let Ok(kids) = item.Children() {
                            let mut n = 0u32;
                            for c in &it.children {
                                if let Some(ci) = make(env, c, names, info, cb) {
                                    let _ = kids.InsertValueAtIndex(n, &ci);
                                    n += 1;
                                }
                            }
                        }
                        Some(item)
                    }
                }
                let cbd: std::rc::Rc<dyn Fn(&str, &serde_json::Value)> = cb.clone();
                let mut pos = at;
                for it in &list {
                    if let Some(item) = make(&env, it, &names, &info, &cbd) {
                        let _ = menu.InsertValueAtIndex(pos, &item);
                        pos += 1;
                    }
                }
            }
            Ok(())
        }));
        let mut token = 0i64;
        unsafe {
            let _ = w11.add_ContextMenuRequested(&handler, &mut token);
        }
    }

    fn ext_profile(webview: &wry::WebView) -> Option<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Profile7> {
        use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2Profile7, ICoreWebView2_13};
        use wry::WebViewExtWindows;
        unsafe { webview.webview().cast::<ICoreWebView2_13>().and_then(|w| w.Profile()).and_then(|p| p.cast::<ICoreWebView2Profile7>()).ok() }
    }

    /// Muat ekstensi dari folder (unpacked). `cb(folder, Ok((id, nama)) / Err(alasan))`.
    pub fn load_extensions(webview: &wry::WebView, dirs: &[String], cb: impl Fn(String, Result<(String, String), String>) + Clone + 'static) {
        use webview2_com::ProfileAddBrowserExtensionCompletedHandler;
        let Some(profile) = ext_profile(webview) else {
            for d in dirs {
                cb(d.clone(), Err("WebView2 terlalu lama untuk ekstensi (butuh versi 118+)".into()));
            }
            return;
        };
        for dir in dirs {
            let d = dir.clone();
            let cb2 = cb.clone();
            let handler = ProfileAddBrowserExtensionCompletedHandler::create(Box::new(move |res, ext| {
                let r = match (res, ext) {
                    (Ok(()), Some(ext)) => unsafe {
                        let mut id = windows::core::PWSTR::null();
                        let mut name = windows::core::PWSTR::null();
                        let _ = ext.Id(&mut id);
                        let _ = ext.Name(&mut name);
                        Ok((webview2_com::take_pwstr(id), webview2_com::take_pwstr(name)))
                    },
                    (Ok(()), None) => Err("WebView2 tidak mengembalikan ekstensi".to_string()),
                    (Err(e), _) => Err(match e.code().0 as u32 {
                        0x8007_0032 => "ekstensi dimatikan di mesin WebView2 (perlu mulai ulang Ovner)".to_string(),
                        0x8007_0002 => "manifest.json tidak ada atau tidak sah".to_string(),
                        _ => format!("{e}"),
                    }),
                };
                cb2(d.clone(), r);
                Ok(())
            }));
            unsafe {
                if let Err(e) = profile.AddBrowserExtension(&HSTRING::from(dir.as_str()), &handler) {
                    cb(dir.clone(), Err(e.to_string()));
                }
            }
        }
    }

    /// Hapus ekstensi `id` dari profil WebView2.
    pub fn remove_extension(webview: &wry::WebView, id: &str) {
        use webview2_com::{BrowserExtensionRemoveCompletedHandler, ProfileGetBrowserExtensionsCompletedHandler};
        let Some(profile) = ext_profile(webview) else { return };
        let want = id.to_string();
        let handler = ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(move |_res, list| {
            let Some(list) = list else { return Ok(()) };
            unsafe {
                let mut n = 0u32;
                let _ = list.Count(&mut n);
                for i in 0..n {
                    let Ok(ext) = list.GetValueAtIndex(i) else { continue };
                    let mut id = windows::core::PWSTR::null();
                    let _ = ext.Id(&mut id);
                    if webview2_com::take_pwstr(id) == want {
                        let done = BrowserExtensionRemoveCompletedHandler::create(Box::new(|_| Ok(())));
                        let _ = ext.Remove(&done);
                    }
                }
            }
            Ok(())
        }));
        unsafe {
            let _ = profile.GetBrowserExtensions(&handler);
        }
    }

    /// Minta daftar proses WebView2 beserta halaman yang dilayani tiap proses (asinkron).

    /// Nama asli (CNAME terakhir) sebuah host, bila ada. Memblokir: panggil dari thread latar.
    pub fn cname_of(host: &str) -> Option<String> {
        use windows::Win32::NetworkManagement::Dns::{DnsFree, DnsFreeRecordList, DnsQuery_W, DNS_QUERY_STANDARD, DNS_RECORDA, DNS_RECORDW, DNS_TYPE_CNAME};
        let mut results: *mut DNS_RECORDA = std::ptr::null_mut();
        unsafe {
            let err = DnsQuery_W(&HSTRING::from(host), DNS_TYPE_CNAME, DNS_QUERY_STANDARD, None, &mut results, None);
            if err.0 != 0 || results.is_null() {
                return None;
            }
            let mut last = None;
            let mut rec = results as *mut DNS_RECORDW;
            while !rec.is_null() {
                if (*rec).wType == DNS_TYPE_CNAME.0 {
                    if let Ok(s) = (*rec).Data.CNAME.pNameHost.to_string() {
                        last = Some(s.to_ascii_lowercase());
                    }
                }
                rec = (*rec).pNext;
            }
            DnsFree(Some(results as *const core::ffi::c_void), DnsFreeRecordList);
            last
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    pub fn confirm(text: &str) -> bool {
        eprintln!("[konfirmasi] {text}");
        false
    }
    pub fn info(text: &str) {
        eprintln!("[info] {text}");
    }
    pub fn init_identity() {}
    pub fn ask_yes_no_cancel(_text: &str) -> Option<bool> {
        None
    }
    pub fn run_elevated(_exe: &Path, _args: &str) -> Result<u32, String> {
        Err("hanya di Windows".into())
    }
    pub fn is_elevated() -> bool {
        false
    }
    pub fn pick_folder(_owner: Option<isize>, _title: &str, _start: &Path) -> Option<std::path::PathBuf> {
        None
    }
    pub fn pick_file(_owner: Option<isize>, _title: &str, _start: &Path, _kinds: (&str, &str)) -> Option<std::path::PathBuf> {
        None
    }
    pub fn create_shortcut(_lnk: &Path, _exe: &Path, _description: &str) -> Result<(), String> {
        Ok(())
    }
    pub fn downloads_dir() -> Option<std::path::PathBuf> {
        None
    }
    pub fn desktop_dir() -> Option<std::path::PathBuf> {
        None
    }
    pub fn download(url: &str, dest: &Path) -> Result<(), String> {
        let ok = std::process::Command::new("curl").args(["-fsSL", "--retry", "2", "-o"]).arg(dest).arg(url).status().map_err(|e| e.to_string())?.success();
        if ok {
            Ok(())
        } else {
            Err("unduh gagal".into())
        }
    }
    pub fn autostart_command() -> Option<String> {
        None
    }
    pub fn set_autostart(_enabled: bool, _exe: &Path) -> std::io::Result<()> {
        let _ = RUN_KEY;
        Ok(())
    }
    /// Linux: hanya proses ini sendiri (dari /proc), cukup untuk menguji halaman task manager.
    pub fn process_snapshot(_extra: &[u32]) -> Vec<Proc> {
        let rss = std::fs::read_to_string("/proc/self/statm")
            .ok()
            .and_then(|s| s.split_whitespace().nth(1).and_then(|v| v.parse::<u64>().ok()))
            .unwrap_or(0)
            * 4096;
        vec![Proc { pid: std::process::id(), working: rss, commit: rss, is_self: true }]
    }
    pub fn trim_pids(_pids: &[u32]) -> Vec<(u32, u64, u64)> {
        Vec::new()
    }
    pub fn mem_status() -> Option<(u64, u64)> {
        None
    }
    pub fn set_background_mode(_background: bool) -> u32 {
        0
    }
    pub fn set_pids_eco(_pids: &[u32], _background: bool) -> u32 {
        0
    }
    pub fn idle_secs() -> u64 {
        0
    }
    pub fn tune_webview(_webview: &wry::WebView, _s: &crate::settings::Settings) {}
    pub fn cdp(_webview: &wry::WebView, _method: &str, _params: &str) {}
    pub fn cdp_call(_webview: &wry::WebView, _method: &str, _params: &str, _cb: impl FnOnce(String) + 'static) {}
    pub fn cdp_result(_webview: &wry::WebView, _method: &str, _params: &str, cb: impl FnOnce(Option<String>) + 'static) {
        cb(None)
    }
    pub fn install_crash_logger(_log_file: &Path) {}
    pub fn disk_free(_path: &Path) -> Option<u64> {
        None
    }
    pub fn cpu_name() -> String {
        std::fs::read_to_string("/proc/cpuinfo").ok().and_then(|c| c.lines().find_map(|l| l.strip_prefix("model name").map(|v| v.trim_start_matches([' ', '\t', ':']).to_string()))).unwrap_or_default()
    }
    pub fn os_version() -> String {
        std::fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|s| s.lines().find_map(|l| l.strip_prefix("PRETTY_NAME=").map(|v| v.trim_matches('"').to_string())))
            .unwrap_or_else(|| "Linux".into())
    }
    pub fn set_clipboard(_text: &str) {}
    pub fn on_download(
        _webview: &wry::WebView,
        _cb: impl Fn(DlEvent) + 'static,
        _offer: impl Fn(&str, i64) -> bool + 'static,
        _place: impl Fn(&str) -> String + 'static,
    ) {
    }
    pub fn resolve_download(_id: u64, _take_over: bool) {}
    pub fn caption_text(_owner: isize, _parts: &[(String, bool)], _dark: bool, _scale: f64) {}
    pub fn pdf_downloads(_webview: &wry::WebView, _decide: impl Fn(&str, Option<u64>, &str) -> Option<(String, Option<u64>)> + 'static, _cb: impl Fn(DlEvent) + 'static, ready: impl FnOnce() + 'static) {
        ready();
    }
    pub fn post_text(_webview: &wry::WebView, _text: &str) -> bool {
        false
    }
    pub fn next_download_id() -> u64 {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }
    pub fn is_pdf_stream(_id: u64) -> bool {
        false
    }
    pub fn cancel_download(_id: u64) -> bool {
        false
    }
    pub fn open_uri(uri: &str) {
        let _ = std::process::Command::new("xdg-open").arg(uri).spawn();
    }
    pub fn open_path(path: &str, reveal: bool) {
        let p = if reveal { std::path::Path::new(path).parent().map(|p| p.display().to_string()).unwrap_or_default() } else { path.to_string() };
        let _ = std::process::Command::new("xdg-open").arg(p).spawn();
    }
    pub fn raise(_webview: &wry::WebView) {}
    /// Linux (pengembangan): tanpa enkripsi.
    pub fn protect(data: &[u8], _encrypt: bool) -> Option<Vec<u8>> {
        Some(data.to_vec())
    }
    pub fn on_favicon(_webview: &wry::WebView, _cb: impl Fn(Vec<u8>) + 'static) {}
    pub fn user_agent(_webview: &wry::WebView) -> String {
        String::new()
    }
    pub fn capture_png(_webview: &wry::WebView, _path: &Path) {}
    pub fn try_suspend(_webview: &wry::WebView) {}
    pub fn resume(_webview: &wry::WebView) {}
    pub fn set_script_enabled(_webview: &wry::WebView, _enabled: bool) {}
    pub fn on_process_failed(_webview: &wry::WebView, _cb: impl Fn(ProcFail, String) + 'static) {}
    pub fn on_source_changed(_webview: &wry::WebView, _cb: impl Fn(String, bool) + 'static) {}
    pub fn on_download_dialog(_webview: &wry::WebView, _cb: impl Fn() + 'static) {}
    pub fn on_ctrl_j(_webview: &wry::WebView, _cb: impl Fn() + 'static) {}
    pub fn load_extensions(_webview: &wry::WebView, _dirs: &[String], _cb: impl Fn(String, Result<(String, String), String>) + Clone + 'static) {}
    pub fn remove_extension(_webview: &wry::WebView, _id: &str) {}
    pub fn self_memory() -> Option<(u64, u32)> {
        None
    }
    #[derive(Debug, Default, Clone)]
    pub struct SelfMem {
        pub private: u64,
        pub heaps: u32,
        pub heap_used: u64,
        pub heap_commit: u64,
        pub third_party: Vec<(String, u64)>,
        pub modules: u32,
    }
    pub fn self_breakdown() -> Option<SelfMem> {
        None
    }
    pub fn compact_heaps() -> u64 {
        0
    }
    pub fn on_media(_webview: &wry::WebView, _cb: impl Fn(super::MediaHit) + 'static) {}
    pub fn on_focus(_webview: &wry::WebView, _cb: impl Fn() + 'static) {}
    pub fn context_menu(_webview: &wry::WebView, _items: impl Fn(&serde_json::Value) -> Vec<super::CtxItem> + 'static, _cb: impl Fn(&str, &serde_json::Value) + 'static) {}
    pub fn cname_of(_host: &str) -> Option<String> {
        None
    }
    pub fn main_frame_id(_webview: &wry::WebView) -> Option<u32> {
        None
    }
    pub fn system_dark() -> bool {
        false
    }
    pub fn on_window_close(_webview: &wry::WebView, _cb: impl Fn() + 'static) {}
    pub fn clip_webview(_webview: &wry::WebView, _rects: &[(i32, i32, i32, i32)]) {}
    pub fn record_problems(_webview: &wry::WebView, _cb: impl Fn(&str, String) + 'static) {}
    pub fn record_enable(_webview: &wry::WebView) {}
    pub fn record_disable(_webview: &wry::WebView) {}
    pub fn read_clipboard() -> Option<String> {
        None
    }
}

pub use imp::*;

/// Waktu setempat "YYYY-MM-DD HH:MM:SS.mmm" untuk log.
pub fn local_time() -> String {
    #[cfg(windows)]
    {
        let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds)
    }
    #[cfg(not(windows))]
    {
        let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        utc_string(d.as_secs(), d.subsec_millis())
    }
}

/// Detik Unix (UTC) → "YYYY-MM-DD HH:MM:SS.mmm" (algoritme hari sipil Howard Hinnant).
pub fn utc_string(secs: u64, ms: u32) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}.{ms:03}", rem / 3600, rem / 60 % 60, rem % 60)
}

#[cfg(test)]
mod time_tests {
    #[test]
    fn utc() {
        assert_eq!(super::utc_string(0, 0), "1970-01-01 00:00:00.000");
        assert_eq!(super::utc_string(1_795_053_184, 7), "2026-11-19 01:53:04.007");
        assert_eq!(super::utc_string(951_782_400, 0), "2000-02-29 00:00:00.000");
    }
}
