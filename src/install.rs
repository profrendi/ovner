//! Pemasangan Ovner. `Ovner-Setup.exe` yang dijalankan dari tempat lain (misalnya Downloads)
//! menampilkan pilihan (lihat `setup_ui`):
//!
//! - **Install** (normal, seperti software lain): `C:\Program Files\Ovner\ovner.exe`, untuk
//!   semua pengguna PC, ikon Start Menu/Desktop bersama, entri "Apps & features" di HKLM.
//!   Butuh izin admin (UAC); update berikutnya juga meminta izin admin.
//! - **Portable** (tanpa admin): `ovner.exe` + `portable.txt` di folder pilihan; semua data di
//!   folder `Data` di sebelahnya.
//! - Instalasi per-pengguna lama (`%LOCALAPPDATA%\Programs\Ovner`, tanpa admin) tetap
//!   didukung dan bisa diperbarui.
//!
//! Runtime Visual C++ ditanam di exe (crt-static), jadi tidak perlu VC++ Redistributable.

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

/// Folder instalasi untuk semua pengguna: `C:\Program Files\Ovner`.
pub fn machine_dir() -> PathBuf {
    std::env::var_os("ProgramW6432")
        .or_else(|| std::env::var_os("ProgramFiles"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"))
        .join("Ovner")
}

pub fn machine_exe() -> PathBuf {
    machine_dir().join(EXE_NAME)
}

/// Exe ini adalah salinan di Program Files (instalasi normal, untuk semua pengguna).
pub fn is_machine_copy(exe: &Path) -> bool {
    exe.parent().is_some_and(|d| same_dir(d, &machine_dir()))
}

/// Folder portabel bawaan: "Ovner Portable" di sebelah Setup (mis. Downloads\Ovner Portable).
pub fn default_portable_dir(exe: &Path) -> PathBuf {
    exe.parent().map(Path::to_path_buf).unwrap_or_else(std::env::temp_dir).join("Ovner Portable")
}

/// Pasang versi portabel ke `dir`: ovner.exe + portable.txt (+ ikon Desktop bila diminta).
pub fn install_portable(exe: &Path, dir: &Path, desktop: bool) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("buat folder {}: {e}", dir.display()))?;
    let target = dir.join(EXE_NAME);
    if !same_file(exe, &target) {
        if target.exists() {
            let old = target.with_extension("exe.old");
            let _ = std::fs::remove_file(&old);
            let _ = std::fs::rename(&target, &old);
        }
        std::fs::copy(exe, &target).map_err(|e| format!("salin exe: {e}"))?;
    }
    let marker = dir.join("portable.txt");
    if !marker.exists() {
        std::fs::write(
            &marker,
            "File ini membuat Ovner berjalan portabel: semua data disimpan di folder Data di sebelah ovner.exe.\r\nThis file makes Ovner portable: all data is kept in the Data folder next to ovner.exe.\r\n",
        )
        .map_err(|e| format!("tulis portable.txt: {e}"))?;
    }
    if desktop {
        if let Some(lnk) = crate::win::desktop_dir().map(|d| d.join("Ovner Portable.lnk")) {
            let _ = crate::win::create_shortcut(&lnk, &target, SHORTCUT_DESC);
        }
    }
    Ok(target)
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// Ikon untuk semua pengguna (Start Menu bersama dan Desktop publik).
pub fn machine_shortcuts() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(pd) = std::env::var_os("ProgramData") {
        v.push(PathBuf::from(pd).join(r"Microsoft\Windows\Start Menu\Programs").join(SHORTCUT_NAME));
    }
    if let Some(p) = std::env::var_os("PUBLIC") {
        v.push(PathBuf::from(p).join("Desktop").join(SHORTCUT_NAME));
    }
    v
}

/// Pasang ke Program Files (proses ini harus berjalan sebagai admin). Mengembalikan exe terpasang.
pub fn install_machine(exe: &Path, desktop: bool) -> Result<PathBuf, String> {
    let dir = machine_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("buat folder {}: {e}", dir.display()))?;
    let target = machine_exe();
    if !same_file(exe, &target) {
        if target.exists() {
            let old = target.with_extension("exe.old");
            let _ = std::fs::remove_file(&old);
            // Windows mengizinkan mengganti nama exe yang sedang berjalan.
            let _ = std::fs::rename(&target, &old);
        }
        std::fs::copy(exe, &target).map_err(|e| format!("salin exe ke {}: {e}", target.display()))?;
    }
    let links = machine_shortcuts();
    for (i, lnk) in links.iter().enumerate() {
        // [0] Start Menu selalu; [1] Desktop publik bila diminta (saat update: hanya bila sudah ada).
        if i == 0 || desktop || lnk.exists() {
            if let Err(e) = crate::win::create_shortcut(lnk, &target, SHORTCUT_DESC) {
                crate::log(&format!("shortcut {} gagal: {e}", lnk.display()));
            }
        }
    }
    imp::write_machine_entry(&target);
    Ok(target)
}

/// Lepas instalasi Program Files (sebagai admin): ikon, entri HKLM; folder dihapus kemudian.
pub fn remove_machine_integration() {
    for l in machine_shortcuts() {
        let _ = std::fs::remove_file(l);
    }
    imp::remove_machine_entry();
}

/// Versi yang terpasang di Program Files (None bila belum).
pub fn machine_installed_version() -> Option<String> {
    imp::machine_installed_version()
}

/// Hapus instalasi per-pengguna lama (setelah pindah ke Program Files). Data tidak disentuh.
pub fn remove_user_install() {
    remove_integration();
    let _ = std::fs::remove_file(installed_exe());
    crate::win::delete_dirs_later(&[&install_dir()]);
}

pub fn is_portable(exe: &Path, flag: bool) -> bool {
    flag || exe.parent().is_some_and(|d| d.join("portable").exists() || d.join("portable.txt").exists())
}

pub fn should_install(exe: &Path, portable_flag: bool) -> bool {
    cfg!(windows) && !is_portable(exe, portable_flag) && !is_installed_copy(exe) && !is_machine_copy(exe)
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

    pub fn machine_installed_version() -> Option<String> {
        RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE).open_subkey(UNINSTALL_KEY).ok()?.get_value("DisplayVersion").ok()
    }

    pub fn write_machine_entry(exe: &Path) {
        let Ok((k, _)) = RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE).create_subkey(UNINSTALL_KEY) else { return };
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

    pub fn remove_machine_entry() {
        let _ = RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE).delete_subkey_all(UNINSTALL_KEY);
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
    pub fn machine_installed_version() -> Option<String> {
        None
    }
    pub fn write_machine_entry(_exe: &Path) {}
    pub fn remove_machine_entry() {}
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

    #[test]
    fn portable_install_copies_exe_and_marker() {
        let src = std::env::temp_dir().join(format!("ovner-src-{}", std::process::id()));
        std::fs::create_dir_all(&src).unwrap();
        let exe = src.join("Ovner-Setup.exe");
        std::fs::write(&exe, b"MZ test").unwrap();
        let dir = default_portable_dir(&exe);
        assert_eq!(dir, src.join("Ovner Portable"));
        let target = install_portable(&exe, &dir, false).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"MZ test");
        assert!(is_portable(&target, false));
        assert_eq!(data_dir(&target, true), dir.join("Data"));
        // Pasang ulang (update) menimpa exe lama.
        std::fs::write(&exe, b"MZ v2").unwrap();
        install_portable(&exe, &dir, false).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"MZ v2");
        assert!(!is_machine_copy(&target));
        std::fs::remove_dir_all(&src).unwrap();
    }
}
