//! Update otomatis dari GitHub Releases (seperti Outlook Lite), dengan dua kanal:
//! - Stabil: rilis `ovner-vX.Y.Z` (dibuat saat versi di Cargo.toml naik);
//! - Nightly: rilis `ovner-nightly` (dibuat ulang setiap perubahan di main).
//!
//! CI menerbitkan `Ovner-Setup.exe` dan `ovner-latest.json` ({version, url, sha256, build}).
//! Ovner mengunduh exe baru ke sebelah exe lama, memeriksa SHA-256, lalu menukarnya. Exe lama
//! diganti nama menjadi `.old` (diizinkan Windows walau sedang berjalan) dan dihapus pada start
//! berikutnya. Versi baru aktif setelah Ovner dibuka ulang.

use std::path::Path;

use serde::Deserialize;

pub const REPO: &str = "profrendi/ovner";

/// Nomor build CI (0 untuk build lokal) dan kanal tempat exe ini dibangun.
pub fn build_number() -> u64 {
    option_env!("OVNER_BUILD").and_then(|b| b.parse().ok()).unwrap_or(0)
}

pub fn build_channel() -> &'static str {
    match option_env!("OVNER_CHANNEL") {
        Some("nightly") => "nightly",
        _ => "stable",
    }
}

/// Versi untuk ditampilkan: `0.3.0` atau `0.3.0-nightly.57`.
pub fn display_version() -> String {
    if build_channel() == "nightly" {
        format!("{}-nightly.{}", env!("CARGO_PKG_VERSION"), build_number())
    } else {
        env!("CARGO_PKG_VERSION").to_string()
    }
}

#[derive(Deserialize, Debug)]
struct Latest {
    version: String,
    url: String,
    sha256: String,
    #[serde(default)]
    build: u64,
}

pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim().trim_start_matches("ovner-").trim_start_matches('v');
    let mut it = s.split(['.', '-', '+']);
    Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next().unwrap_or("0").parse().ok()?))
}

pub fn is_newer(remote: &str, local: &str) -> bool {
    matches!((parse_version(remote), parse_version(local)), (Some(r), Some(l)) if r > l)
}

/// Apakah rilis `remote` (dari kanal `want`) harus dipasang menggantikan exe ini.
fn should_install(want: &str, remote_version: &str, remote_build: u64, my_version: &str, my_channel: &str, my_build: u64) -> bool {
    match (want, my_channel) {
        // Nightly → nightly: nomor build lebih besar.
        ("nightly", "nightly") => remote_build > my_build,
        // Stabil → nightly: pindah kanal bila nightly tidak lebih lama.
        ("nightly", _) => remote_build > 0 && !is_newer(my_version, remote_version),
        // Nightly → stabil: kembali ke stabil bila versinya sama atau lebih baru.
        (_, "nightly") => !is_newer(my_version, remote_version),
        _ => is_newer(remote_version, my_version),
    }
}

pub fn latest_url(channel: &str) -> String {
    if channel == "nightly" {
        format!("https://github.com/{REPO}/releases/download/ovner-nightly/ovner-latest.json")
    } else {
        format!("https://github.com/{REPO}/releases/latest/download/ovner-latest.json")
    }
}

/// Hapus exe lama sisa update sebelumnya.
pub fn cleanup_old() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(exe.with_extension("exe.old"));
        let _ = std::fs::remove_file(exe.with_extension("exe.new"));
    }
}

/// Periksa rilis terbaru di kanal `channel`; bila perlu, pasang exe-nya (aktif saat Ovner
/// dibuka ulang). Mengembalikan versi baru yang sudah siap, None bila sudah terbaru.
#[cfg(windows)]
pub fn check_and_stage(data_dir: &Path, channel: &str) -> Result<Option<String>, String> {
    use sha2::{Digest, Sha256};

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let meta = data_dir.join("ovner-latest.json");
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    crate::win::download(&format!("{}?t={t}", latest_url(channel)), &meta)?;
    let text = std::fs::read_to_string(&meta).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&meta);
    let latest: Latest = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| format!("ovner-latest.json: {e}"))?;

    if !should_install(channel, &latest.version, latest.build, env!("CARGO_PKG_VERSION"), build_channel(), build_number()) {
        return Ok(None);
    }
    if !latest.url.starts_with(&format!("https://github.com/{REPO}/releases/download/")) {
        return Err("URL update tidak dikenal, dibatalkan".into());
    }
    let new = exe.with_extension("exe.new");
    let old = exe.with_extension("exe.old");
    crate::win::download(&latest.url, &new)?;
    let bytes = std::fs::read(&new).map_err(|e| e.to_string())?;
    let got: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    if !got.eq_ignore_ascii_case(latest.sha256.trim()) {
        let _ = std::fs::remove_file(&new);
        return Err("checksum update tidak cocok, dibatalkan".into());
    }
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| format!("ganti nama exe lama: {e}"))?;
    if let Err(e) = std::fs::rename(&new, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(format!("pasang exe baru: {e}"));
    }
    let label = if channel == "nightly" { format!("{}-nightly.{}", latest.version, latest.build) } else { latest.version };
    Ok(Some(label))
}

#[cfg(not(windows))]
pub fn check_and_stage(_data_dir: &Path, channel: &str) -> Result<Option<String>, String> {
    let _ = (latest_url(channel), should_install as fn(&str, &str, u64, &str, &str, u64) -> bool);
    let _: Option<Latest> = None;
    Err("update hanya di Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(parse_version("ovner-v0.1.2"), Some((0, 1, 2)));
        assert_eq!(parse_version("0.2"), Some((0, 2, 0)));
        assert!(is_newer("0.1.1", "0.1.0"));
        assert!(is_newer("v0.10.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("abc", "0.1.0"));
    }

    #[test]
    fn channel_switching() {
        // stabil → stabil
        assert!(should_install("stable", "0.3.1", 0, "0.3.0", "stable", 0));
        assert!(!should_install("stable", "0.3.0", 0, "0.3.0", "stable", 0));
        // nightly → nightly
        assert!(should_install("nightly", "0.3.0", 58, "0.3.0", "nightly", 57));
        assert!(!should_install("nightly", "0.3.0", 57, "0.3.0", "nightly", 57));
        // stabil → nightly (pindah kanal), tapi tidak mundur versi
        assert!(should_install("nightly", "0.3.0", 60, "0.3.0", "stable", 0));
        assert!(!should_install("nightly", "0.2.0", 60, "0.3.0", "stable", 0));
        // nightly → stabil
        assert!(should_install("stable", "0.3.0", 0, "0.3.0", "nightly", 60));
        assert!(!should_install("stable", "0.2.0", 0, "0.3.0", "nightly", 60));
    }
}
