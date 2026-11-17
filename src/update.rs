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
    /// Tanda tangan Ed25519 (base64) atas [`signed_message`], dibuat CI dengan kunci privat
    /// yang hanya ada di secret GitHub `OVNER_SIGN_KEY`.
    #[serde(default)]
    sig: String,
}

/// Kunci publik penanda tangan update Ovner (Ed25519). Pasangannya (kunci privat) hanya ada di
/// secret GitHub `OVNER_SIGN_KEY`; berkas update yang tidak ditandatangani kunci itu ditolak,
/// walaupun rilis GitHub atau unduhannya diubah orang lain.
pub const UPDATE_PUBLIC_KEY: &str = "bxLf9EtUrIzzyww9yT9aOXceX0ygJwHXCTCIvgA0FXI=";

/// Pesan yang ditandatangani untuk satu update.
pub fn signed_message(version: &str, build: u64, sha256: &str) -> String {
    format!("ovner-update\n{version}\n{build}\n{}", sha256.trim().to_ascii_lowercase())
}

/// Periksa tanda tangan update. Ok(true) = sah, Ok(false) = tidak ada tanda tangan, Err = salah.
pub fn verify_signature(version: &str, build: u64, sha256: &str, sig_b64: &str) -> Result<bool, String> {
    use base64::Engine as _;
    let sig_b64 = sig_b64.trim();
    if sig_b64.is_empty() {
        return Ok(false);
    }
    let key: [u8; 32] = base64::prelude::BASE64_STANDARD.decode(UPDATE_PUBLIC_KEY).ok().and_then(|k| k.try_into().ok()).ok_or("kunci publik rusak")?;
    let key = ed25519_dalek::VerifyingKey::from_bytes(&key).map_err(|e| e.to_string())?;
    let sig = base64::prelude::BASE64_STANDARD.decode(sig_b64).map_err(|_| "tanda tangan bukan base64".to_string())?;
    let sig = ed25519_dalek::Signature::from_slice(&sig).map_err(|_| "tanda tangan rusak".to_string())?;
    key.verify_strict(signed_message(version, build, sha256).as_bytes(), &sig).map_err(|_| "tanda tangan update tidak sah".to_string())?;
    Ok(true)
}

/// Sejak update bertanda tangan pertama kali terlihat, update tanpa tanda tangan ditolak
/// (menghapus tanda tangan dari ovner-latest.json tidak bisa dipakai untuk menurunkan keamanan).
fn check_signed(data_dir: &Path, version: &str, build: u64, sha256: &str, sig: &str) -> Result<(), String> {
    let flag = data_dir.join("update-signed.flag");
    match verify_signature(version, build, sha256, sig)? {
        true => {
            let _ = std::fs::write(&flag, version);
            Ok(())
        }
        false if flag.exists() => Err("update tidak bertanda tangan, dibatalkan".into()),
        false => Ok(()),
    }
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
    // Tanda tangan diperiksa sebelum mengunduh: sha256 yang dicocokkan nanti ikut ditandatangani.
    check_signed(data_dir, &latest.version, latest.build, &latest.sha256, &latest.sig)?;
    // Instalasi Program Files: folder exe tidak bisa ditulis tanpa admin. Simpan update di
    // folder data; dipasang dengan izin admin saat Ovner dibuka ulang (apply_machine_update).
    let machine = crate::install::is_machine_copy(&exe);
    let stage_dir = data_dir.join("update");
    if machine && machine_staged(data_dir).is_some_and(|(_, v)| v == latest.version) {
        // Sudah diunduh, tinggal dipasang dengan izin admin.
        let label = if channel == "nightly" { format!("{}-nightly.{}", latest.version, latest.build) } else { latest.version.clone() };
        return Ok(Some(label));
    }
    let new = if machine {
        std::fs::create_dir_all(&stage_dir).map_err(|e| e.to_string())?;
        stage_dir.join(crate::install::EXE_NAME)
    } else {
        exe.with_extension("exe.new")
    };
    let old = exe.with_extension("exe.old");
    crate::win::download(&latest.url, &new)?;
    let bytes = std::fs::read(&new).map_err(|e| e.to_string())?;
    let got: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    if !got.eq_ignore_ascii_case(latest.sha256.trim()) {
        let _ = std::fs::remove_file(&new);
        return Err("checksum update tidak cocok, dibatalkan".into());
    }
    let label = if channel == "nightly" { format!("{}-nightly.{}", latest.version, latest.build) } else { latest.version.clone() };
    if machine {
        std::fs::write(stage_dir.join("version.txt"), &latest.version).map_err(|e| e.to_string())?;
        return Ok(Some(label));
    }
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| format!("ganti nama exe lama: {e}"))?;
    if let Err(e) = std::fs::rename(&new, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(format!("pasang exe baru: {e}"));
    }
    Ok(Some(label))
}

/// Update Program Files yang sudah diunduh dan lebih baru dari versi ini: (exe, versi).
pub fn machine_staged(data_dir: &Path) -> Option<(std::path::PathBuf, String)> {
    let dir = data_dir.join("update");
    let exe = dir.join(crate::install::EXE_NAME);
    let v = std::fs::read_to_string(dir.join("version.txt")).ok()?.trim().to_string();
    (exe.is_file() && is_newer(&v, env!("CARGO_PKG_VERSION"))).then_some((exe, v))
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
    fn update_signature() {
        // Dibuat dengan tools/sign_update.py dan kunci privat pasangan UPDATE_PUBLIC_KEY.
        let sig = "rmGmg/mpzZ48Xux4FuHtIWo0v+CZ065+dUYOkGSxriwUQrj6K7AIHcAoFmN/lPKknMpkWDY9GoJpXFYRYi5qBQ==";
        assert_eq!(verify_signature("0.8.13", 99, "ABCDEF0123", sig), Ok(true));
        assert!(verify_signature("0.8.14", 99, "abcdef0123", sig).is_err(), "versi lain");
        assert!(verify_signature("0.8.13", 99, "abcdef0124", sig).is_err(), "sha256 lain");
        assert_eq!(verify_signature("0.8.13", 99, "abcdef0123", ""), Ok(false));
        let dir = std::env::temp_dir().join(format!("ovner-sig-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(check_signed(&dir, "0.8.13", 99, "abcdef0123", "").is_ok(), "belum pernah bertanda tangan");
        assert!(check_signed(&dir, "0.8.13", 99, "abcdef0123", sig).is_ok());
        assert!(check_signed(&dir, "0.8.14", 100, "ff", "").is_err(), "setelah bertanda tangan, wajib");
        std::fs::remove_dir_all(&dir).unwrap();
    }

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
