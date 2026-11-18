//! Ovner sebagai browser default Windows (http/https, .htm/.html, .pdf), tanpa hak admin.
//!
//! Semua pendaftaran di HKCU (khusus pengguna ini), seperti Chrome/Brave per-user:
//! - `Software\Classes\OvnerURL|OvnerHTML|OvnerPDF`: perintah buka
//!   `"ovner.exe" --single-argument %1`. `--single-argument` (seperti Chrome) membuat seluruh
//!   sisa baris perintah dianggap satu alamat, jadi tautan berbahaya tidak bisa menyelipkan
//!   opsi lain (mis. `--uninstall`).
//! - `Software\Clients\StartMenuInternet\Ovner\Capabilities` + `Software\RegisteredApplications`:
//!   Ovner muncul di Pengaturan Windows → Aplikasi default.
//!
//! Windows tidak mengizinkan aplikasi menjadikan dirinya default sendiri (pilihan pengguna
//! dilindungi hash); Ovner membuka halaman Aplikasi default dengan Ovner terpilih dan pengguna
//! menekan "Set default" sekali.

use std::path::Path;

pub const APP_NAME: &str = "Ovner";
pub const PROG_URL: &str = "OvnerURL";
pub const PROG_HTML: &str = "OvnerHTML";
pub const PROG_PDF: &str = "OvnerPDF";
pub const CLIENT_KEY: &str = r"Software\Clients\StartMenuInternet\Ovner";
pub const HTML_EXTS: [&str; 5] = [".htm", ".html", ".shtml", ".xht", ".xhtml"];

/// Status pendaftaran dan pilihan default pengguna saat ini.
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct Status {
    pub registered: bool,
    pub http: bool,
    pub https: bool,
    pub html: bool,
    pub pdf: bool,
}

/// Perintah buka yang didaftarkan.
pub fn open_command(exe: &Path) -> String {
    format!("\"{}\" --single-argument %1", exe.display())
}

/// Ambil satu-satunya alamat dari baris perintah mentah setelah `--single-argument`.
/// Tanda kutip pembungkus dibuang; sisanya diambil apa adanya (spasi di nama berkas aman).
pub fn single_argument(cmdline: &str) -> Option<String> {
    let i = cmdline.find(" --single-argument ")?;
    let rest = cmdline[i + " --single-argument ".len()..].trim();
    let rest = rest.strip_prefix('"').and_then(|r| r.strip_suffix('"')).unwrap_or(rest);
    (!rest.is_empty()).then(|| rest.to_string())
}

/// Alamat yang boleh dibuka dari luar: http(s), file://, atau jalur berkas yang ada
/// (diubah ke file:///…). Lainnya ditolak (mis. javascript:, data:, opsi baris perintah).
pub fn external_target(arg: &str) -> Option<String> {
    let a = arg.trim();
    let low = a.to_ascii_lowercase();
    if low.starts_with("http://") || low.starts_with("https://") {
        return Some(a.to_string());
    }
    if low.starts_with("file:///") {
        return Some(a.to_string());
    }
    if a.starts_with('-') {
        return None;
    }
    let p = Path::new(a);
    if p.is_absolute() && p.exists() {
        return url::Url::from_file_path(p).ok().map(|u| u.to_string());
    }
    None
}

#[cfg(windows)]
mod imp {
    use super::*;
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    fn set_default(key: &RegKey, v: &str) {
        let _ = key.set_value("", &v.to_string());
    }

    fn prog_id(hk: &RegKey, id: &str, name: &str, exe: &Path, url_protocol: bool) -> std::io::Result<()> {
        let (k, _) = hk.create_subkey(format!(r"Software\Classes\{id}"))?;
        set_default(&k, name);
        k.set_value("FriendlyTypeName", &name.to_string())?;
        if url_protocol {
            k.set_value("URL Protocol", &String::new())?;
        }
        let (icon, _) = k.create_subkey("DefaultIcon")?;
        set_default(&icon, &format!("\"{}\",0", exe.display()));
        let (app, _) = k.create_subkey("Application")?;
        app.set_value("ApplicationName", &APP_NAME.to_string())?;
        app.set_value("ApplicationIcon", &format!("\"{}\",0", exe.display()))?;
        let (cmd, _) = k.create_subkey(r"shell\open\command")?;
        set_default(&cmd, &open_command(exe));
        Ok(())
    }

    /// Daftarkan (atau perbarui alamat exe). Dipanggil saat mulai bila sudah pernah didaftarkan,
    /// supaya Ovner yang dipindah/diperbarui tetap terbuka dari tautan.
    pub fn register(exe: &Path) -> Result<bool, String> {
        let hk = RegKey::predef(HKEY_CURRENT_USER);
        let want = open_command(exe);
        let current: Option<String> = hk.open_subkey(format!(r"Software\Classes\{PROG_URL}\shell\open\command")).ok().and_then(|k| k.get_value("").ok());
        let has_caps = hk.open_subkey(format!(r"{CLIENT_KEY}\Capabilities\FileAssociations")).ok().and_then(|k| k.get_value::<String, _>(".pdf").ok()).is_some();
        if current.as_deref() == Some(want.as_str()) && has_caps {
            return Ok(false);
        }
        let e = |x: std::io::Error| format!("registry: {x}");
        prog_id(&hk, PROG_URL, "Ovner URL", exe, true).map_err(e)?;
        prog_id(&hk, PROG_HTML, "Ovner HTML Document", exe, false).map_err(e)?;
        prog_id(&hk, PROG_PDF, "Ovner PDF Document", exe, false).map_err(e)?;
        let (client, _) = hk.create_subkey(CLIENT_KEY).map_err(e)?;
        set_default(&client, APP_NAME);
        let (ic, _) = client.create_subkey("DefaultIcon").map_err(e)?;
        set_default(&ic, &format!("\"{}\",0", exe.display()));
        let (oc, _) = client.create_subkey(r"shell\open\command").map_err(e)?;
        set_default(&oc, &format!("\"{}\"", exe.display()));
        let (caps, _) = client.create_subkey("Capabilities").map_err(e)?;
        caps.set_value("ApplicationName", &APP_NAME.to_string()).map_err(e)?;
        caps.set_value("ApplicationIcon", &format!("\"{}\",0", exe.display())).map_err(e)?;
        caps.set_value("ApplicationDescription", &"Browser ringan dengan pemblokir iklan".to_string()).map_err(e)?;
        let (sm, _) = caps.create_subkey("StartMenu").map_err(e)?;
        sm.set_value("StartMenuInternet", &APP_NAME.to_string()).map_err(e)?;
        let (urls, _) = caps.create_subkey("URLAssociations").map_err(e)?;
        for scheme in ["http", "https"] {
            urls.set_value(scheme, &PROG_URL.to_string()).map_err(e)?;
        }
        let (files, _) = caps.create_subkey("FileAssociations").map_err(e)?;
        for ext in HTML_EXTS {
            files.set_value(ext, &PROG_HTML.to_string()).map_err(e)?;
            // Muncul di menu "Buka dengan".
            if let Ok((o, _)) = hk.create_subkey(format!(r"Software\Classes\{ext}\OpenWithProgids")) {
                let _ = o.set_raw_value(PROG_HTML, &winreg::RegValue { bytes: std::borrow::Cow::Borrowed(&[]), vtype: winreg::enums::RegType::REG_NONE });
            }
        }
        files.set_value(".pdf", &PROG_PDF.to_string()).map_err(e)?;
        if let Ok((o, _)) = hk.create_subkey(r"Software\Classes\.pdf\OpenWithProgids") {
            let _ = o.set_raw_value(PROG_PDF, &winreg::RegValue { bytes: std::borrow::Cow::Borrowed(&[]), vtype: winreg::enums::RegType::REG_NONE });
        }
        let (reg, _) = hk.create_subkey(r"Software\RegisteredApplications").map_err(e)?;
        reg.set_value(APP_NAME, &format!(r"{CLIENT_KEY}\Capabilities")).map_err(e)?;
        notify_assoc_changed();
        Ok(true)
    }

    pub fn unregister() {
        let hk = RegKey::predef(HKEY_CURRENT_USER);
        for id in [PROG_URL, PROG_HTML, PROG_PDF] {
            let _ = hk.delete_subkey_all(format!(r"Software\Classes\{id}"));
        }
        for ext in HTML_EXTS {
            if let Ok(o) = hk.open_subkey_with_flags(format!(r"Software\Classes\{ext}\OpenWithProgids"), winreg::enums::KEY_ALL_ACCESS) {
                let _ = o.delete_value(PROG_HTML);
            }
        }
        if let Ok(o) = hk.open_subkey_with_flags(r"Software\Classes\.pdf\OpenWithProgids", winreg::enums::KEY_ALL_ACCESS) {
            let _ = o.delete_value(PROG_PDF);
        }
        let _ = hk.delete_subkey_all(CLIENT_KEY);
        if let Ok(r) = hk.open_subkey_with_flags(r"Software\RegisteredApplications", winreg::enums::KEY_ALL_ACCESS) {
            let _ = r.delete_value(APP_NAME);
        }
        notify_assoc_changed();
    }

    fn user_choice(path: &str) -> Option<String> {
        let hk = RegKey::predef(HKEY_CURRENT_USER);
        // Windows 11 24H2+ menyimpan pilihan baru di UserChoiceLatest.
        for sub in ["UserChoiceLatest", "UserChoice"] {
            if let Ok(k) = hk.open_subkey(format!(r"{path}\{sub}")) {
                if let Ok(p) = k.get_value::<String, _>("ProgId") {
                    return Some(p);
                }
            }
        }
        None
    }

    pub fn status() -> Status {
        let hk = RegKey::predef(HKEY_CURRENT_USER);
        let registered = hk.open_subkey(format!(r"{CLIENT_KEY}\Capabilities")).is_ok();
        let url = |s: &str| user_choice(&format!(r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\{s}")).is_some_and(|p| p == PROG_URL);
        let file = |e: &str, id: &str| user_choice(&format!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\{e}")).is_some_and(|p| p == id);
        Status { registered, http: url("http"), https: url("https"), html: file(".html", PROG_HTML) || file(".htm", PROG_HTML), pdf: file(".pdf", PROG_PDF) }
    }

    fn notify_assoc_changed() {
        use windows::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
        unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
    }

    /// Buka Pengaturan Windows → Aplikasi default, langsung ke halaman Ovner bila didukung.
    pub fn open_settings() {
        crate::win::open_uri("ms-settings:defaultapps?registeredAppUser=Ovner");
    }

    /// Baris perintah mentah proses ini (untuk `--single-argument`).
    pub fn raw_command_line() -> String {
        unsafe { windows::Win32::System::Environment::GetCommandLineW().to_string().unwrap_or_default() }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    pub fn register(_exe: &Path) -> Result<bool, String> {
        Err("hanya untuk Windows".into())
    }
    pub fn unregister() {}
    pub fn status() -> Status {
        Status::default()
    }
    pub fn open_settings() {}
    pub fn raw_command_line() -> String {
        std::env::args().collect::<Vec<_>>().join(" ")
    }
}

pub use imp::{open_settings, raw_command_line, register, status, unregister};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_argument_cannot_inject_flags() {
        let exe = Path::new(r"C:\Users\x\AppData\Local\Ovner\ovner.exe");
        assert_eq!(open_command(exe), r#""C:\Users\x\AppData\Local\Ovner\ovner.exe" --single-argument %1"#);
        // Seluruh sisa baris = satu alamat, termasuk yang tampak seperti opsi.
        let cl = r#""C:\o\ovner.exe" --single-argument https://x.test/a b" --uninstall"#;
        assert_eq!(single_argument(cl).as_deref(), Some(r#"https://x.test/a b" --uninstall"#));
        let cl = r#""C:\o\ovner.exe" --single-argument C:\Users\Rendi\Downloads\Laporan BPJS.pdf"#;
        assert_eq!(single_argument(cl).as_deref(), Some(r"C:\Users\Rendi\Downloads\Laporan BPJS.pdf"));
        let cl = r#""C:\o\ovner.exe" --single-argument "C:\a b\c.html""#;
        assert_eq!(single_argument(cl).as_deref(), Some(r"C:\a b\c.html"));
        assert_eq!(single_argument(r#""C:\o\ovner.exe" --tray"#), None);
    }

    #[test]
    fn external_targets() {
        assert_eq!(external_target("https://example.com/x").as_deref(), Some("https://example.com/x"));
        assert_eq!(external_target("HTTP://EXAMPLE.COM").as_deref(), Some("HTTP://EXAMPLE.COM"));
        assert_eq!(external_target("file:///C:/a/b.pdf").as_deref(), Some("file:///C:/a/b.pdf"));
        assert_eq!(external_target("javascript:alert(1)"), None);
        assert_eq!(external_target("--uninstall"), None);
        assert_eq!(external_target("relative.html"), None);
        let dir = std::env::temp_dir().join(format!("ovner def {}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("Laporan BPJS.html");
        std::fs::write(&f, b"<p>x</p>").unwrap();
        let u = external_target(&f.display().to_string()).unwrap();
        assert!(u.starts_with("file:///") && u.ends_with("Laporan%20BPJS.html"), "{u}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
