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

/// Satu proses WebView2 beserta halaman (bingkai utama) yang dilayaninya.
#[derive(Clone, Debug, Default)]
pub struct ProcExt {
    pub pid: u32,
    /// "browser", "renderer", "gpu", "utility", "lainnya".
    pub kind: &'static str,
    /// Alamat bingkai utama halaman yang memakai proses ini.
    pub mains: Vec<String>,
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

    pub fn info(text: &str) {
        use windows::Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_OK};
        message_box(text, MB_OK | MB_ICONINFORMATION);
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
    fn tree_pids() -> Vec<u32> {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
        };
        use windows::Win32::System::Threading::GetCurrentProcessId;
        let mut pairs: Vec<(u32, u32)> = Vec::new();
        unsafe {
            let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return Vec::new() };
            let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            if Process32FirstW(snap, &mut e).is_ok() {
                loop {
                    let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                    let name = String::from_utf16_lossy(&e.szExeFile[..len]);
                    if name.eq_ignore_ascii_case("msedgewebview2.exe") {
                        pairs.push((e.th32ProcessID, e.th32ParentProcessID));
                    }
                    if Process32NextW(snap, &mut e).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snap);
        }
        let me = unsafe { GetCurrentProcessId() };
        let mut tree = vec![me];
        let mut i = 0;
        while i < tree.len() {
            let parent = tree[i];
            for &(pid, ppid) in &pairs {
                if ppid == parent && pid != parent && !tree.contains(&pid) {
                    tree.push(pid);
                }
            }
            i += 1;
        }
        tree
    }

    /// Snapshot RAM proses ini + semua proses WebView2-nya.
    pub fn process_snapshot() -> Vec<Proc> {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2};
        use windows::Win32::System::Threading::{GetCurrentProcessId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        let me = unsafe { GetCurrentProcessId() };
        let mut out = Vec::new();
        for pid in tree_pids() {
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
        for pid in tree_pids() {
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
                let _ = st.SetIsPasswordAutosaveEnabled(s.password_manager);
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
    pub fn on_process_failed(webview: &wry::WebView, cb: impl Fn(ProcFail) + 'static) {
        use webview2_com::Microsoft::Web::WebView2::Win32::*;
        use webview2_com::ProcessFailedEventHandler;
        use wry::WebViewExtWindows;
        let handler = ProcessFailedEventHandler::create(Box::new(move |_wv, args| {
            if let Some(a) = args {
                let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
                unsafe {
                    a.ProcessFailedKind(&mut kind)?;
                }
                cb(if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED {
                    ProcFail::Browser
                } else if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED {
                    ProcFail::Render
                } else if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE {
                    ProcFail::Unresponsive
                } else {
                    ProcFail::Other
                });
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
    pub fn request_process_infos(
        env: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment,
        cb: impl FnOnce(Vec<ProcExt>) + 'static,
    ) {
        use webview2_com::Microsoft::Web::WebView2::Win32::*;
        use webview2_com::{take_pwstr, GetProcessExtendedInfosCompletedHandler};
        use windows::core::{BOOL, PWSTR};
        let Ok(env13) = env.cast::<ICoreWebView2Environment13>() else { return };
        let cb = std::cell::RefCell::new(Some(cb));
        let handler = GetProcessExtendedInfosCompletedHandler::create(Box::new(move |res, coll| {
            let mut out = Vec::new();
            if let (Ok(()), Some(coll)) = (res, coll) {
                unsafe {
                    let mut n = 0u32;
                    let _ = coll.Count(&mut n);
                    for i in 0..n {
                        let Ok(info) = coll.GetValueAtIndex(i) else { continue };
                        let Ok(pi) = info.ProcessInfo() else { continue };
                        let (mut pid, mut kind) = (0i32, COREWEBVIEW2_PROCESS_KIND::default());
                        let _ = pi.ProcessId(&mut pid);
                        let _ = pi.Kind(&mut kind);
                        let kind = if kind == COREWEBVIEW2_PROCESS_KIND_BROWSER {
                            "browser"
                        } else if kind == COREWEBVIEW2_PROCESS_KIND_RENDERER {
                            "renderer"
                        } else if kind == COREWEBVIEW2_PROCESS_KIND_GPU {
                            "gpu"
                        } else if kind == COREWEBVIEW2_PROCESS_KIND_UTILITY {
                            "utility"
                        } else {
                            "lainnya"
                        };
                        let mut mains = Vec::new();
                        if let Ok(it) = info.AssociatedFrameInfos().and_then(|f| f.GetIterator()) {
                            let mut has = BOOL::default();
                            while it.HasCurrent(&mut has).is_ok() && has.as_bool() {
                                if let Ok(mut fr) = it.GetCurrent() {
                                    // Naik ke bingkai utama (halaman tab).
                                    for _ in 0..16 {
                                        match fr.cast::<ICoreWebView2FrameInfo2>().and_then(|f2| f2.ParentFrameInfo()) {
                                            Ok(p) => fr = p,
                                            Err(_) => break,
                                        }
                                    }
                                    let mut src = PWSTR::null();
                                    if fr.Source(&mut src).is_ok() {
                                        let s = take_pwstr(src);
                                        if !s.is_empty() && !mains.contains(&s) {
                                            mains.push(s);
                                        }
                                    }
                                }
                                let mut more = BOOL::default();
                                if it.MoveNext(&mut more).is_err() || !more.as_bool() {
                                    break;
                                }
                            }
                        }
                        out.push(ProcExt { pid: pid as u32, kind, mains });
                    }
                }
            }
            if let Some(cb) = cb.borrow_mut().take() {
                cb(out);
            }
            Ok(())
        }));
        unsafe {
            let _ = env13.GetProcessExtendedInfos(&handler);
        }
    }

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
    pub fn process_snapshot() -> Vec<Proc> {
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
    pub fn tune_webview(_webview: &wry::WebView, _s: &crate::settings::Settings) {}
    pub fn try_suspend(_webview: &wry::WebView) {}
    pub fn resume(_webview: &wry::WebView) {}
    pub fn set_script_enabled(_webview: &wry::WebView, _enabled: bool) {}
    pub fn on_process_failed(_webview: &wry::WebView, _cb: impl Fn(ProcFail) + 'static) {}
    pub fn load_extensions(_webview: &wry::WebView, _dirs: &[String], _log: fn(&str)) {}
    pub fn cname_of(_host: &str) -> Option<String> {
        None
    }
}

pub use imp::*;
