//! Instalasi per-pengguna tanpa admin, seperti Outlook Lite dan ZFPlus.
//!
//! Saat `Ovner-Setup.exe` dijalankan dari tempat lain (misalnya Downloads), ia menyalin dirinya
//! ke `%LOCALAPPDATA%\Programs\Ovner\ovner.exe`, membuat ikon di Start Menu dan Desktop,
//! mendaftarkan entri di "Apps & features" (untuk uninstall), lalu menjalankan salinan itu.
//!
//! Mode portabel: ada file `portable.txt` (atau `portable`) di sebelah exe, atau argumen
//! `--portable`. Semua data lalu disimpan di folder `Data` di sebelah exe.

use std::path::{Path, PathBuf};

pub const EXE_NAME: &str = "ovner.exe";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Ovner";
const SHORTCUT_NAME: &str = "Ovner.lnk";
const SHORTCUT_DESC: &str = "Browser ringan dengan pemblokir iklan";

fn local_appdata() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

pub fn install_dir() -> PathBuf {
    local_appdata().join("Programs").join("Ovner")
}

pub fn installed_exe() -> PathBuf {
    install_dir().join(EXE_NAME)
}

fn same_dir(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().to_lowercase();
    norm(a) == norm(b)
}

pub fn is_installed_copy(exe: &Path) -> bool {
    exe.parent().is_some_and(|d| same_dir(d, &install_dir()))
}

pub fn is_portable(exe: &Path, flag: bool) -> bool {
    flag || exe.parent().is_some_and(|d| d.join("portable").exists() || d.join("portable.txt").exists())
}

pub fn should_install(exe: &Path, portable_flag: bool) -> bool {
    cfg!(windows) && !is_portable(exe, portable_flag) && !is_installed_copy(exe)
}

/// Folder data: `Data` di sebelah exe (portabel) atau `%LOCALAPPDATA%\Ovner` (terpasang).
pub fn data_dir(exe: &Path, portable: bool) -> PathBuf {
    if portable {
        exe.parent().map(|d| d.join("Data")).unwrap_or_else(|| PathBuf::from("Data"))
    } else if cfg!(windows) {
        local_appdata().join("Ovner")
    } else {
        // Linux (pengujian): ~/.local/share/ovner
        std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join(".local/share/ovner"))
            .unwrap_or_else(|| PathBuf::from("Data"))
    }
}

pub fn start_menu_shortcut() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join(r"Microsoft\Windows\Start Menu\Programs").join(SHORTCUT_NAME))
}

pub fn desktop_shortcut() -> Option<PathBuf> {
    crate::win::desktop_dir().map(|d| d.join(SHORTCUT_NAME))
}

/// Versi terpasang menurut entri uninstall (None bila belum terpasang).
pub fn installed_version() -> Option<String> {
    imp::installed_version()
}

/// Ikon Start Menu (dibuat ulang bila hilang) + entri uninstall. Ikon Desktop hanya dibuat
/// saat instalasi/upgrade, supaya kalau sengaja dihapus pengguna tidak muncul lagi.
pub fn ensure_integration(exe: &Path, with_desktop: bool) {
    let mut links = Vec::new();
    if let Some(l) = start_menu_shortcut() {
        links.push(l);
    }
    if with_desktop {
        if let Some(l) = desktop_shortcut() {
            links.push(l);
        }
    }
    for lnk in links {
        if with_desktop || !lnk.exists() {
            if let Err(e) = crate::win::create_shortcut(&lnk, exe, SHORTCUT_DESC) {
                crate::log(&format!("shortcut {} gagal: {e}", lnk.display()));
            }
        }
    }
    imp::write_uninstall_entry(exe);
}

pub fn remove_integration() {
    for l in [start_menu_shortcut(), desktop_shortcut()].into_iter().flatten() {
        let _ = std::fs::remove_file(l);
    }
    imp::remove_registry();
}

/// Salin exe ini ke folder instalasi (bila versinya lebih baru atau belum ada) dan buat ikon.
/// Mengembalikan exe terpasang yang harus dijalankan.
pub fn install_self(exe: &Path, stop_running: impl FnOnce()) -> Result<PathBuf, String> {
    let target = installed_exe();
    std::fs::create_dir_all(install_dir()).map_err(|e| format!("buat folder: {e}"))?;

    let mine = env!("CARGO_PKG_VERSION");
    let keep_existing =
        target.exists() && installed_version().is_some_and(|v| !crate::update::is_newer(mine, &v));
    if !keep_existing {
        stop_running();
        if target.exists() {
            let old = target.with_extension("exe.old");
            let _ = std::fs::remove_file(&old);
            // Windows mengizinkan mengganti nama exe yang sedang berjalan.
            let _ = std::fs::rename(&target, &old);
        }
        std::fs::copy(exe, &target).map_err(|e| format!("salin exe: {e}"))?;
    }
    ensure_integration(&target, true);
    Ok(target)
}

#[cfg(windows)]
mod imp {
    use super::*;
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};

    pub fn installed_version() -> Option<String> {
        RegKey::predef(HKEY_CURRENT_USER).open_subkey(UNINSTALL_KEY).ok()?.get_value("DisplayVersion").ok()
    }

    pub fn write_uninstall_entry(exe: &Path) {
        let Ok((k, _)) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(UNINSTALL_KEY) else { return };
        let exe_s = exe.display().to_string();
        let dir = exe.parent().map(|d| d.display().to_string()).unwrap_or_default();
        let size_kb = std::fs::metadata(exe).map(|m| (m.len() / 1024) as u32).unwrap_or(0);
        let _ = k.set_value("DisplayName", &"Ovner".to_string());
        let _ = k.set_value("DisplayVersion", &env!("CARGO_PKG_VERSION").to_string());
        let _ = k.set_value("Publisher", &"profrendi".to_string());
        let _ = k.set_value("DisplayIcon", &format!("\"{exe_s}\",0"));
        let _ = k.set_value("InstallLocation", &dir);
        let _ = k.set_value("UninstallString", &format!("\"{exe_s}\" --uninstall"));
        let _ = k.set_value("URLInfoAbout", &format!("https://github.com/{}", crate::update::REPO));
        let _ = k.set_value("NoModify", &1u32);
        let _ = k.set_value("NoRepair", &1u32);
        let _ = k.set_value("EstimatedSize", &size_kb);
    }

    pub fn remove_registry() {
        let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(UNINSTALL_KEY);
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    pub fn installed_version() -> Option<String> {
        let _ = UNINSTALL_KEY;
        None
    }
    pub fn write_uninstall_entry(_exe: &Path) {}
    pub fn remove_registry() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_marker_and_data_dir() {
        let d = std::env::temp_dir().join(format!("ovner-portable-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let exe = d.join("ovner.exe");
        assert!(!is_portable(&exe, false));
        assert!(is_portable(&exe, true));
        std::fs::write(d.join("portable.txt"), "").unwrap();
        assert!(is_portable(&exe, false));
        assert_eq!(data_dir(&exe, true), d.join("Data"));
        assert!(!should_install(&exe, false));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
