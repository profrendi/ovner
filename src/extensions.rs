//! Ekstensi Chrome di Ovner.
//!
//! WebView2 bisa menjalankan ekstensi Chrome yang sudah diekstrak (folder), tetapi tidak punya
//! Chrome Web Store, ikon ekstensi di toolbar, popup, maupun panel samping. Bagian yang hilang
//! itu dibuat di sini:
//! - Pasang dari Chrome Web Store: berkas .crx diunduh dari server pembaruan Google (sama seperti
//!   Chrome), kepala CRX3 dibaca, isinya (zip) diekstrak dengan tar.exe bawaan Windows, lalu kunci
//!   publik ekstensi ditulis ke manifest supaya ID-nya sama persis dengan di Web Store.
//! - Info manifest (nama, versi, ikon, popup/panel samping) untuk tombol ekstensi di toolbar;
//!   popup/panel samping dibuka Ovner di panel kanan jendela.

use base64::Engine as _;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Folder ekstensi yang dipasang Ovner (dari Web Store).
pub fn root(data: &Path) -> PathBuf {
    data.join("extensions")
}

/// ID ekstensi Chrome: 32 huruf a–p.
pub fn is_id(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| (b'a'..=b'p').contains(&b))
}

/// ID ekstensi dari alamat halaman Chrome Web Store (kosong bila bukan halaman ekstensi).
pub fn store_id(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    let host = u.host_str()?;
    let store = host == "chromewebstore.google.com" || (host == "chrome.google.com" && u.path().starts_with("/webstore/"));
    if !store {
        return None;
    }
    let segs: Vec<&str> = u.path_segments()?.collect();
    let pos = segs.iter().position(|s| *s == "detail")?;
    segs[pos + 1..].iter().find(|s| is_id(s)).map(|s| s.to_string())
}

/// ID ekstensi dari kunci publiknya (cara Chrome: SHA-256, 16 byte pertama, tiap nibble → a–p).
pub fn id_from_key(key: &[u8]) -> String {
    let h = Sha256::digest(key);
    h[..16].iter().flat_map(|b| [b >> 4, b & 15]).map(|n| (b'a' + n) as char).collect()
}

/// ID ekstensi dari unduhan .crx yang dicegat: dari alamat server pembaruan
/// (`x=id%3D<id>…`) atau dari nama berkas ("<id>_12345.crx").
pub fn crx_id(url: &str, name: &str) -> Option<String> {
    let dec = url.replace("%3D", "=").replace("%3d", "=").replace("%26", "&");
    if let Some(i) = dec.find("id=") {
        let cand: String = dec[i + 3..].chars().take(32).collect();
        if is_id(&cand) {
            return Some(cand);
        }
    }
    let base: String = name.chars().take(32).collect();
    is_id(&base).then_some(base)
}

/// Alamat unduhan .crx dari server pembaruan Google.
pub fn crx_url(id: &str, chromium: &str) -> String {
    let ver = if chromium.is_empty() { "140.0.0.0" } else { chromium };
    format!("https://clients2.google.com/service/update2/crx?response=redirect&prodversion={ver}&acceptformat=crx2,crx3&x=id%3D{id}%26installsource%3Dondemand%26uc")
}

/// Halaman sebuah ekstensi.
pub fn ext_url(id: &str, path: &str) -> String {
    format!("chrome-extension://{id}/{}", path.trim_start_matches('/'))
}

// ---- Membaca berkas CRX ----

fn varint(b: &[u8], i: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let x = *b.get(*i)?;
        *i += 1;
        v |= ((x & 0x7f) as u64) << shift;
        if x & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

/// Medan protobuf (nomor, isi) yang bertipe bytes; medan lain dilewati.
fn proto_fields(b: &[u8]) -> Option<Vec<(u64, &[u8])>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let tag = varint(b, &mut i)?;
        match tag & 7 {
            0 => {
                varint(b, &mut i)?;
            }
            1 => i += 8,
            2 => {
                let n = varint(b, &mut i)? as usize;
                out.push((tag >> 3, b.get(i..i + n)?));
                i += n;
            }
            5 => i += 4,
            _ => return None,
        }
    }
    Some(out)
}

fn u32le(b: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?) as usize)
}

/// Pecah CRX (versi 2 atau 3): kunci publik ekstensi dan isi zip. `want` = ID yang diharapkan;
/// kunci yang dipakai harus menghasilkan ID itu (berkas lain/rusak ditolak).
pub fn parse_crx<'a>(b: &'a [u8], want: Option<&str>) -> Result<(Vec<u8>, &'a [u8]), String> {
    if b.get(..4) != Some(b"Cr24") {
        return Err("bukan berkas ekstensi Chrome (.crx)".into());
    }
    let bad = || "berkas .crx rusak".to_string();
    let (keys, zip): (Vec<Vec<u8>>, &[u8]) = match u32le(b, 4).ok_or_else(bad)? {
        2 => {
            let (kl, sl) = (u32le(b, 8).ok_or_else(bad)?, u32le(b, 12).ok_or_else(bad)?);
            let key = b.get(16..16 + kl).ok_or_else(bad)?.to_vec();
            (vec![key], b.get(16 + kl + sl..).ok_or_else(bad)?)
        }
        3 => {
            let hl = u32le(b, 8).ok_or_else(bad)?;
            let header = b.get(12..12 + hl).ok_or_else(bad)?;
            let mut keys = Vec::new();
            let mut signed_id: Option<Vec<u8>> = None;
            for (f, v) in proto_fields(header).ok_or_else(bad)? {
                match f {
                    2 | 3 => {
                        if let Some((_, k)) = proto_fields(v).ok_or_else(bad)?.into_iter().find(|(n, _)| *n == 1) {
                            keys.push(k.to_vec());
                        }
                    }
                    10000 => signed_id = proto_fields(v).and_then(|x| x.into_iter().find(|(n, _)| *n == 1).map(|(_, id)| id.to_vec())),
                    _ => {}
                }
            }
            // Kunci milik ekstensi = kunci yang hash-nya sama dengan crx_id di bagian bertanda tangan.
            if let Some(sid) = signed_id {
                keys.sort_by_key(|k| Sha256::digest(k)[..16] != sid[..]);
            }
            (keys, b.get(12 + hl..).ok_or_else(bad)?)
        }
        v => return Err(format!("versi .crx {v} tidak dikenal")),
    };
    let key = match want {
        Some(w) => keys.into_iter().find(|k| id_from_key(k) == w).ok_or_else(|| format!("berkas .crx bukan milik ekstensi {w}"))?,
        None => keys.into_iter().next().ok_or_else(bad)?,
    };
    if zip.get(..4) != Some(b"PK\x03\x04") {
        return Err(bad());
    }
    Ok((key, zip))
}

// ---- Memasang ----

fn unzip(zip: &Path, dest: &Path) -> Result<(), String> {
    // tar.exe bawaan Windows 10 (1803+) bisa mengekstrak zip; tidak perlu pustaka tambahan.
    let tar = std::env::var_os("SystemRoot").map(|r| PathBuf::from(r).join("System32").join("tar.exe")).filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from("tar"));
    let mut cmd = std::process::Command::new(tar);
    cmd.arg("-xf").arg(zip).arg("-C").arg(dest).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped());
    crate::video::hide_window(&mut cmd);
    let out = cmd.output().map_err(|e| format!("tar.exe tidak bisa dijalankan: {e}"))?;
    if !out.status.success() {
        return Err(format!("isi ekstensi gagal diekstrak: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(())
}

fn read_manifest(dir: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(dir.join("manifest.json")).ok()?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

/// Pasang isi .crx ke `<data>/extensions/<id>`. Mengembalikan foldernya.
pub fn install_crx(data: &Path, id: &str, crx: &[u8]) -> Result<PathBuf, String> {
    let (key, zip) = parse_crx(crx, Some(id))?;
    let base = root(data);
    std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let tmp = base.join(format!(".{id}-baru"));
    let zf = base.join(format!(".{id}.zip"));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    std::fs::write(&zf, zip).map_err(|e| e.to_string())?;
    let r = unzip(&zf, &tmp);
    let _ = std::fs::remove_file(&zf);
    if let Err(e) = r {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(e);
    }
    // _metadata = tanda tangan Web Store untuk Chrome; tidak dipakai WebView2.
    let _ = std::fs::remove_dir_all(tmp.join("_metadata"));
    let Some(mut m) = read_manifest(&tmp) else {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err("manifest.json ekstensi tidak terbaca".into());
    };
    if m.get("key").is_none() {
        m["key"] = base64::prelude::BASE64_STANDARD.encode(&key).into();
        let _ = std::fs::write(tmp.join("manifest.json"), serde_json::to_string_pretty(&m).unwrap_or_default());
    }
    let dest = base.join(id);
    swap_dir(&tmp, &dest)?;
    Ok(dest)
}

/// Ganti folder `dest` dengan `new` tanpa pernah meninggalkan `dest` setengah terhapus: yang lama
/// dipindah dulu (rename satu langkah, gagal utuh bila ada berkas yang sedang dipakai
/// WebView2), yang baru dipasang, baru yang lama dibuang. Gagal di mana pun: yang lama kembali.
/// (Dulu `remove_dir_all` lalu `rename`: berkas terkunci membuat ekstensi lama rusak sebagian.)
fn swap_dir(new: &Path, dest: &Path) -> Result<(), String> {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".lama");
    let old = dest.with_file_name(name);
    let _ = std::fs::remove_dir_all(&old);
    let had_old = dest.exists();
    if had_old {
        if let Err(e) = std::fs::rename(dest, &old) {
            let _ = std::fs::remove_dir_all(new);
            return Err(format!("ekstensi lama sedang dipakai, tidak bisa diganti (tutup tab yang memakainya lalu coba lagi): {e}"));
        }
    }
    if let Err(e) = std::fs::rename(new, dest) {
        if had_old {
            let _ = std::fs::rename(&old, dest);
        }
        let _ = std::fs::remove_dir_all(new);
        return Err(format!("folder ekstensi tidak bisa dipindah: {e}"));
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

/// Simpan berkas .crx asli (untuk dibagikan/diperiksa) ke `dir`; nama "<nama ekstensi> <versi>.crx".
pub fn save_crx(dir: &Path, name: &str, version: &str, crx: &[u8]) -> Result<PathBuf, String> {
    let clean: String = name.chars().map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { ' ' } else { c }).collect();
    let stem = format!("{} {}", clean.split_whitespace().collect::<Vec<_>>().join(" "), version).trim().to_string();
    let _ = std::fs::create_dir_all(dir);
    let mut p = dir.join(format!("{stem}.crx"));
    let mut n = 1;
    while p.exists() {
        p = dir.join(format!("{stem} ({n}).crx"));
        n += 1;
    }
    std::fs::write(&p, crx).map_err(|e| e.to_string())?;
    Ok(p)
}

/// Unduh .crx sebuah ekstensi dari Chrome Web Store (memblokir; panggil dari thread latar).
pub fn download(id: &str, chromium: &str) -> Result<Vec<u8>, String> {
    download_from(&crx_url(id, chromium))
}

pub fn download_from(url: &str) -> Result<Vec<u8>, String> {
    let r = crate::net::get(url, &[("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36")])?;
    if r.status != 200 {
        return Err(format!("Chrome Web Store menolak unduhan (HTTP {})", r.status));
    }
    if r.body.get(..4) != Some(b"Cr24") {
        return Err("Chrome Web Store tidak memberikan berkas ekstensi (mungkin ekstensi ini sudah dihapus atau berbayar)".into());
    }
    Ok(r.body)
}

// ---- Info untuk toolbar ----

#[derive(Serialize, Clone, Debug, Default)]
pub struct Info {
    pub dir: String,
    /// ID dari kunci di manifest (ekstensi Web Store); kosong = belum diketahui sampai dimuat.
    pub id: String,
    pub name: String,
    pub version: String,
    /// Halaman popup/panel samping (relatif), kosong bila ekstensi tidak punya.
    pub panel: String,
    /// "side" (panel samping), "popup", atau "".
    pub kind: String,
    pub icon: String,
    pub perms: Vec<String>,
    /// Dipasang Ovner dari Web Store (folder di dalam data Ovner).
    pub store: bool,
}

fn localize(dir: &Path, m: &serde_json::Value, s: &str) -> String {
    let Some(key) = s.strip_prefix("__MSG_").and_then(|k| k.strip_suffix("__")) else { return s.to_string() };
    let key = key.to_lowercase();
    let mut locales = vec!["id".to_string(), "en".to_string()];
    if let Some(d) = m["default_locale"].as_str() {
        locales.push(d.to_string());
    }
    for loc in locales {
        let Ok(t) = std::fs::read_to_string(dir.join("_locales").join(&loc).join("messages.json")) else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(t.trim_start_matches('\u{feff}')) else { continue };
        if let Some(obj) = v.as_object() {
            if let Some((_, msg)) = obj.iter().find(|(k, _)| k.to_lowercase() == key) {
                if let Some(text) = msg["message"].as_str() {
                    return text.to_string();
                }
            }
        }
    }
    s.to_string()
}

pub fn info(dir: &Path, data: &Path) -> Option<Info> {
    let m = read_manifest(dir)?;
    let name = localize(dir, &m, m["name"].as_str().unwrap_or("Ekstensi"));
    let id = m["key"].as_str().and_then(|k| base64::prelude::BASE64_STANDARD.decode(k.trim()).ok()).map(|k| id_from_key(&k)).unwrap_or_default();
    let (panel, kind) = if let Some(p) = m["side_panel"]["default_path"].as_str() {
        (p.to_string(), "side")
    } else if let Some(p) = ["action", "browser_action", "page_action"].iter().find_map(|k| m[*k]["default_popup"].as_str()) {
        (p.to_string(), "popup")
    } else {
        (String::new(), "")
    };
    // Ikon: ukuran terdekat ≥ 32 px.
    let mut icons: Vec<(u32, String)> = m["icons"].as_object().map(|o| o.iter().filter_map(|(k, v)| Some((k.parse().ok()?, v.as_str()?.to_string()))).collect()).unwrap_or_default();
    icons.sort();
    let pick = icons.iter().find(|(n, _)| *n >= 32).or(icons.last()).map(|(_, p)| p.clone());
    let icon = pick
        .and_then(|p| std::fs::read(dir.join(p.trim_start_matches('/'))).ok())
        .filter(|b| b.len() < 64 * 1024)
        .map(|b| {
            let mime = if b.starts_with(b"\x89PNG") { "image/png" } else if b.starts_with(b"<svg") || b.starts_with(b"<?xml") { "image/svg+xml" } else { "image/png" };
            format!("data:{mime};base64,{}", base64::prelude::BASE64_STANDARD.encode(&b))
        })
        .unwrap_or_default();
    let perms = m["permissions"].as_array().map(|a| a.iter().filter_map(|p| p.as_str().map(String::from)).collect()).unwrap_or_default();
    Some(Info {
        dir: dir.display().to_string(),
        id,
        name,
        version: m["version"].as_str().unwrap_or("").to_string(),
        panel,
        kind: kind.to_string(),
        icon,
        perms,
        store: dir.starts_with(root(data)),
    })
}

// ---- Membuat .crx (untuk uji GitHub: server uji menyajikan ekstensi buatan) ----

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

/// Zip tanpa kompresi (stored).
pub fn zip_stored(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, body) in files {
        let off = out.len() as u32;
        let crc = crc32(body);
        let n = name.as_bytes();
        let mut h = Vec::new();
        h.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        h.extend_from_slice(&crc.to_le_bytes());
        h.extend_from_slice(&(body.len() as u32).to_le_bytes());
        h.extend_from_slice(&(body.len() as u32).to_le_bytes());
        h.extend_from_slice(&(n.len() as u16).to_le_bytes());
        h.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&h);
        out.extend_from_slice(n);
        out.extend_from_slice(body);
        let mut c = vec![0x50, 0x4b, 0x01, 0x02, 20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        c.extend_from_slice(&crc.to_le_bytes());
        c.extend_from_slice(&(body.len() as u32).to_le_bytes());
        c.extend_from_slice(&(body.len() as u32).to_le_bytes());
        c.extend_from_slice(&(n.len() as u16).to_le_bytes());
        c.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        c.extend_from_slice(&off.to_le_bytes());
        c.extend_from_slice(n);
        central.extend_from_slice(&c);
    }
    let cd_off = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0]);
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_off.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn pb_bytes(field: u64, v: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut tag = (field << 3) | 2;
    loop {
        let b = (tag & 0x7f) as u8;
        tag >>= 7;
        out.push(if tag > 0 { b | 0x80 } else { b });
        if tag == 0 {
            break;
        }
    }
    let mut n = v.len() as u64;
    loop {
        let b = (n & 0x7f) as u8;
        n >>= 7;
        out.push(if n > 0 { b | 0x80 } else { b });
        if n == 0 {
            break;
        }
    }
    out.extend_from_slice(v);
    out
}

/// CRX3 dengan kunci publik `key` (tanpa tanda tangan sah: Ovner tidak memeriksanya, sama seperti
/// folder ekstensi biasa; keamanan unduhan Web Store dari HTTPS Google). Mengembalikan (id, crx).
pub fn build_crx(key: &[u8], zip: &[u8]) -> (String, Vec<u8>) {
    let id = id_from_key(key);
    let crx_id = &Sha256::digest(key)[..16];
    let mut header = pb_bytes(2, &[pb_bytes(1, key), pb_bytes(2, b"sig")].concat());
    header.extend(pb_bytes(10000, &pb_bytes(1, crx_id)));
    let mut out = b"Cr24".to_vec();
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(&(header.len() as u32).to_le_bytes());
    out.extend_from_slice(&header);
    out.extend_from_slice(zip);
    (id, out)
}

/// Kunci publik RSA uji (DER) untuk ekstensi buatan server uji.
pub const TEST_KEY_B64: &str = "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAltWAeegXaHt30jLpadvsTVHJk+4AIHBZe7zplsx3eJO4Anq0rUvSUto7hZArcUFgRU0GdrJuVcTRuj/MLYHUwCkwwSgX+X6a+zU+N4w+RKbrbhE4My/sEatG11Wdw9k4ah5EFZZf1pIYHY0WoqkOMyyN/aOlpgTwSz8PEHH1/1QwvLdSIJAyG8orCE4drvsF18Cf46ldu/YmeiViskqtxsNEVt9UdIkFqiecrhzo5rQlR0XUzQS6RYdngsKJkR/ikceGoOI/Gme1Rut6L+hMqI9SaNHW9uTzsjWH9Yo30hk3T/OZeEwUSA+K9L/E+a271DLau9lBFs24YWXJkhQExQIDAQAB";

/// Ekstensi uji: skrip konten menandai halaman, panel samping berjudul "Panel Uji Ovner".
pub fn test_crx() -> (String, Vec<u8>) {
    let manifest = br#"{"manifest_version":3,"name":"Ovner Uji Ekstensi","version":"1.0",
"content_scripts":[{"matches":["http://127.0.0.1/*"],"js":["cs.js"],"run_at":"document_end"}],
"side_panel":{"default_path":"panel.html"},"permissions":["storage"]}"#;
    let cs = b"document.documentElement.setAttribute('data-ovner-ext','1');";
    let panel = b"<!doctype html><title>Panel Uji Ovner</title><p id=p>panel ekstensi</p>";
    let zip = zip_stored(&[("manifest.json", manifest), ("cs.js", cs), ("panel.html", panel)]);
    let key = base64::prelude::BASE64_STANDARD.decode(TEST_KEY_B64).unwrap_or_default();
    build_crx(&key, &zip)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_urls() {
        let id = "pgjckoablokakigdlgbhdmeohbklphli";
        assert_eq!(store_id(&format!("https://chromewebstore.google.com/detail/copyclip-clipboard-histor/{id}?hl=en")).as_deref(), Some(id));
        assert_eq!(store_id(&format!("https://chromewebstore.google.com/detail/{id}")).as_deref(), Some(id));
        assert_eq!(store_id(&format!("https://chrome.google.com/webstore/detail/x/{id}")).as_deref(), Some(id));
        assert_eq!(store_id("https://chromewebstore.google.com/category/extensions"), None);
        assert_eq!(store_id(&format!("https://example.com/detail/{id}")), None);
    }

    #[test]
    fn crx_ids_from_intercepted_downloads() {
        let id = "pgjckoablokakigdlgbhdmeohbklphli";
        assert_eq!(crx_id(&crx_url(id, "157.0"), "").as_deref(), Some(id));
        assert_eq!(crx_id("https://clients2.googleusercontent.com/crx/blobs/AbC/x.crx", &format!("{id}_15820.crx")).as_deref(), Some(id));
        assert_eq!(crx_id("https://example.com/a.crx", "a.crx"), None);
    }

    #[test]
    fn crx_roundtrip_and_id_check() {
        let (id, crx) = test_crx();
        assert!(is_id(&id));
        let (key, zip) = parse_crx(&crx, Some(&id)).unwrap();
        assert_eq!(id_from_key(&key), id);
        assert!(zip.starts_with(b"PK\x03\x04"));
        assert!(parse_crx(&crx, Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")).is_err());
        assert!(parse_crx(b"PK\x03\x04", None).is_err());
    }

    #[test]
    fn known_extension_id() {
        // Kunci uji → ID dihitung seperti Chrome (16 byte SHA-256, nibble a–p).
        let key = base64::prelude::BASE64_STANDARD.decode(TEST_KEY_B64).unwrap();
        let id = id_from_key(&key);
        assert_eq!(id.len(), 32);
        assert!(id.bytes().all(|b| (b'a'..=b'p').contains(&b)));
    }

    #[test]
    fn manifest_info() {
        let d = std::env::temp_dir().join(format!("ovner-ext-uji-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("_locales/en")).unwrap();
        std::fs::write(d.join("manifest.json"), format!(r#"{{"manifest_version":3,"name":"__MSG_appName__","default_locale":"en","version":"2.1","key":"{TEST_KEY_B64}","side_panel":{{"default_path":"/side.html"}},"permissions":["sidePanel","storage"]}}"#)).unwrap();
        std::fs::write(d.join("_locales/en/messages.json"), r#"{"appName":{"message":"CopyClip Uji"}}"#).unwrap();
        let i = info(&d, Path::new("/tidak-ada")).unwrap();
        assert_eq!(i.name, "CopyClip Uji");
        assert_eq!(i.kind, "side");
        assert_eq!(i.panel, "/side.html");
        assert_eq!(ext_url(&i.id, &i.panel), format!("chrome-extension://{}/side.html", i.id));
        assert_eq!(i.perms, vec!["sidePanel", "storage"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn swap_dir_mengganti_dan_tidak_merusak_yang_lama() {
        let d = std::env::temp_dir().join(format!("ovner-swap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let (dest, new) = (d.join("ext"), d.join(".ext-baru"));
        // Pemasangan pertama.
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("v.txt"), "1").unwrap();
        swap_dir(&new, &dest).unwrap();
        assert_eq!(std::fs::read_to_string(dest.join("v.txt")).unwrap(), "1");
        assert!(!new.exists());
        // Pembaruan: yang lama diganti penuh, tanpa sisa ".lama".
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("v.txt"), "2").unwrap();
        std::fs::write(new.join("baru.txt"), "x").unwrap();
        swap_dir(&new, &dest).unwrap();
        assert_eq!(std::fs::read_to_string(dest.join("v.txt")).unwrap(), "2");
        assert!(dest.join("baru.txt").exists() && !d.join("ext.lama").exists());
        // Gagal memasang yang baru (folder baru tidak ada): yang lama kembali utuh.
        assert!(swap_dir(&d.join("tidak-ada"), &dest).is_err());
        assert_eq!(std::fs::read_to_string(dest.join("v.txt")).unwrap(), "2");
        assert!(dest.join("baru.txt").exists() && !d.join("ext.lama").exists());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
