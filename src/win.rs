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

    /// Rapikan working set semua proses (RAM di Task Manager langsung turun; halaman yang
    /// masih dipakai dimuat lagi dari page file saat dibutuhkan). Mengembalikan jumlah proses.
    pub fn trim_process_tree() -> u32 {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::ProcessStatus::EmptyWorkingSet;
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA};
        let mut n = 0;
        for pid in tree_pids() {
            unsafe {
                let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_QUOTA, false, pid) else { continue };
                if EmptyWorkingSet(h).is_ok() {
                    n += 1;
                }
                let _ = CloseHandle(h);
            }
        }
        n
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
        for m in ["Runtime.enable", "Log.enable", "Network.enable", "Audits.enable"] {
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
            format!("PDF gagal: {} ({reason}; {} dari {} byte, {:.1} dtk)", st.path, st.received, st.total, secs)
        }));
        if ok {
            use base64::Engine as _;
            let name = Path::new(&st.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let html = format!(
                "<!doctype html><meta charset=utf-8><title>{n}</title><body style=\"font:15px Segoe UI,sans-serif;background:#202124;color:#e8eaed;padding:40px\">&#10003; PDF disimpan: <b>{n}</b><br><small style=\"color:#9aa0a6\">{p}</small></body>",
                n = html_escape(&name),
                p = html_escape(&st.path)
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

    /// Baca potongan berikutnya dari aliran respons PDF `id` dan tulis ke berkas.
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
            let progress = STREAMS.with(|m| {
                let mut m = m.borrow_mut();
                let st = m.get_mut(&id)?;
                let f = st.file.as_mut()?;
                if !bytes.is_empty() && f.write_all(&bytes).is_err() {
                    return None;
                }
                st.received += bytes.len() as i64;
                let due = st.last.elapsed() >= std::time::Duration::from_millis(300);
                if due {
                    st.last = std::time::Instant::now();
                }
                Some((due, st.received, st.total, st.cb.clone()))
            });
            let Some((due, received, total, cb)) = progress else {
                // Dibatalkan (sudah dihapus dari daftar) atau gagal menulis.
                return finish_stream(id, false, "gagal menulis berkas");
            };
            if eof {
                return finish_stream(id, true, "");
            }
            if due {
                cb(DlEvent::Progress { id, received, total: total.max(received) });
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
    /// `decide(alamat, ukuran, nama)` = path tujuan bila PDF ini diunduh. `cb` menerima
    /// Started/Progress/Done seperti unduhan lain.
    pub fn pdf_downloads(webview: &wry::WebView, decide: impl Fn(&str, Option<u64>, &str) -> Option<String> + 'static, cb: impl Fn(DlEvent) + 'static) {
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
            // Tahap permintaan (hanya alamat yang mirip unduhan): tampilkan "menunggu server"
            // segera, jangan tunggu server selesai membuat berkasnya.
            if v.get("responseStatusCode").is_none() && v.get("responseErrorReason").is_none() {
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
            let Some(path) = target else {
                if let Some((wid, _)) = waiting {
                    cb(DlEvent::WaitEnd { id: wid });
                }
                continue_req(&sender);
                return Ok(());
            };
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
            cdp_then(&sender, "Fetch.takeResponseBodyAsStream", &serde_json::json!({"requestId": req}).to_string(), move |v| {
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
        for k in ["*download*", "*Download*", "*cetak*", "*.pdf*", "*export*", "*unduh*"] {
            patterns.push(serde_json::json!({"urlPattern": k, "resourceType": "Document", "requestStage": "Request"}));
        }
        cdp(webview, "Fetch.enable", &serde_json::json!({"patterns": patterns}).to_string());
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

    /// Muat ekstensi dari folder (unpacked). Hasilnya dicatat lewat `log`.
    pub fn load_extensions(webview: &wry::WebView, dirs: &[String], log: fn(&str)) {
        use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2Profile7, ICoreWebView2_13};
        use webview2_com::ProfileAddBrowserExtensionCompletedHandler;
        use wry::WebViewExtWindows;
        let profile = unsafe { webview.webview().cast::<ICoreWebView2_13>().and_then(|w| w.Profile()).and_then(|p| p.cast::<ICoreWebView2Profile7>()) };
        let Ok(profile) = profile else {
            log("ekstensi: WebView2 terlalu lama (butuh Profile7)");
            return;
        };
        for dir in dirs {
            let d = dir.clone();
            let handler = ProfileAddBrowserExtensionCompletedHandler::create(Box::new(move |res, _ext| {
                log(&format!("ekstensi {d}: {}", if res.is_ok() { "dimuat".to_string() } else { format!("gagal {res:?}") }));
                Ok(())
            }));
            unsafe {
                if let Err(e) = profile.AddBrowserExtension(&HSTRING::from(dir.as_str()), &handler) {
                    log(&format!("ekstensi {dir}: {e}"));
                }
            }
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
    pub fn create_shortcut(_lnk: &Path, _exe: &Path, _description: &str) -> Result<(), String> {
        Ok(())
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
    pub fn trim_process_tree() -> u32 {
        0
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
    pub fn install_crash_logger(_log_file: &Path) {}
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
    pub fn pdf_downloads(_webview: &wry::WebView, _decide: impl Fn(&str, Option<u64>, &str) -> Option<String> + 'static, _cb: impl Fn(DlEvent) + 'static) {}
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
    pub fn try_suspend(_webview: &wry::WebView) {}
    pub fn resume(_webview: &wry::WebView) {}
    pub fn set_script_enabled(_webview: &wry::WebView, _enabled: bool) {}
    pub fn on_process_failed(_webview: &wry::WebView, _cb: impl Fn(ProcFail, String) + 'static) {}
    pub fn load_extensions(_webview: &wry::WebView, _dirs: &[String], _log: fn(&str)) {}
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
    pub fn read_clipboard() -> Option<String> {
        None
    }
}

pub use imp::*;
