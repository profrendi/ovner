//! Pengelola sandi Ovner (seperti Password Manager Brave): login per situs disimpan di
//! `passwords.dat`, dienkripsi DPAPI Windows sehingga hanya bisa dibuka akun Windows ini.
//! Situs hanya pernah menerima sandi untuk alamatnya sendiri, dan hanya saat diisi.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Login {
    pub id: u64,
    /// Nama situs (host tanpa "www."), mis. "shopee.co.id".
    pub site: String,
    /// Alamat lengkap tempat login disimpan.
    pub url: String,
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub note: String,
    pub created: u64,
    #[serde(default)]
    pub used: u64,
}

#[derive(Serialize, Deserialize, Default)]
struct Data {
    logins: Vec<Login>,
    /// Situs yang tidak pernah ditawari simpan sandi.
    never: Vec<String>,
    next_id: u64,
}

pub struct Vault {
    path: PathBuf,
    data: Data,
}

/// Host tanpa "www." dari alamat.
pub fn site_of(url: &str) -> String {
    url::Url::parse(url).ok().and_then(|u| u.host_str().map(|h| h.trim_start_matches("www.").to_ascii_lowercase())).unwrap_or_default()
}

/// Boleh mengisi login yang disimpan di `saved` ke halaman `current`? Menurunkan https ke http
/// tidak boleh; alamat lokal (localhost, 127.x, ::1) dianggap aman.
fn scheme_allows(saved: &str, current: &str) -> bool {
    let (Ok(s), Ok(c)) = (url::Url::parse(saved), url::Url::parse(current)) else { return true };
    if s.scheme() != "https" || c.scheme() == "https" {
        return true;
    }
    matches!(c.host_str(), Some("localhost" | "127.0.0.1" | "[::1]" | "::1"))
}

impl Vault {
    pub fn load(data_dir: &Path) -> Vault {
        let path = data_dir.join("passwords.dat");
        let raw = std::fs::read(&path).ok().filter(|b| !b.is_empty());
        let data: Option<Data> = raw.as_deref().and_then(|b| crate::win::protect(b, false)).and_then(|b| serde_json::from_slice(&b).ok());
        if let (Some(raw), None) = (&raw, &data) {
            // Berkas ada tetapi tidak bisa dibuka (dipindah ke PC/akun Windows lain, profil
            // Windows di-reset, berkas rusak). Tanpa ini, simpan berikutnya menimpanya dengan
            // brankas kosong dan sandi lama hilang selamanya. Simpan salinan sekali; salinan
            // yang sudah ada tidak ditimpa (itu yang asli).
            let bak = data_dir.join("passwords.dat.rusak");
            if !bak.exists() && std::fs::write(&bak, raw).is_ok() {
                crate::log("[sandi] PERINGATAN: passwords.dat tidak bisa dibuka (akun/PC Windows berbeda atau berkas rusak); salinan asli disimpan di passwords.dat.rusak");
            }
        }
        Vault { path, data: data.unwrap_or_default() }
    }

    fn save(&self) {
        use std::io::Write;
        if let Ok(json) = serde_json::to_vec(&self.data) {
            if let Some(enc) = crate::win::protect(&json, true) {
                let tmp = self.path.with_extension("tmp");
                // Tulis, paksa ke disk, baru ganti berkas lama (mati listrik di tengah tidak
                // meninggalkan berkas setengah jadi).
                let ok = std::fs::File::create(&tmp).and_then(|mut f| f.write_all(&enc).and_then(|_| f.sync_all())).is_ok();
                if ok {
                    let _ = std::fs::rename(&tmp, &self.path);
                }
            }
        }
    }

    pub fn logins(&self) -> &[Login] {
        &self.data.logins
    }

    pub fn get(&self, id: u64) -> Option<&Login> {
        self.data.logins.iter().find(|l| l.id == id)
    }

    /// Login untuk halaman ini: host sama (abaikan "www."). Login yang disimpan dari halaman
    /// https tidak pernah ditawarkan di halaman http (kecuali alamat lokal): halaman http bisa
    /// disadap atau disisipi skrip yang membaca sandi yang diisikan.
    pub fn for_url(&self, url: &str) -> Vec<&Login> {
        let site = site_of(url);
        self.data.logins.iter().filter(|l| !site.is_empty() && l.site == site && scheme_allows(&l.url, url)).collect()
    }

    pub fn is_never(&self, url: &str) -> bool {
        self.data.never.contains(&site_of(url))
    }

    pub fn add_never(&mut self, url: &str) {
        let s = site_of(url);
        if !s.is_empty() && !self.data.never.contains(&s) {
            self.data.never.push(s);
            self.save();
        }
    }

    /// Apa yang perlu ditawarkan setelah login dikirim: None (sudah sama), Some(false)
    /// (login baru), Some(true) (sandi berubah).
    pub fn offer_for(&self, url: &str, username: &str, password: &str) -> Option<bool> {
        if password.is_empty() || self.is_never(url) {
            return None;
        }
        let site = site_of(url);
        match self.data.logins.iter().find(|l| l.site == site && l.username == username) {
            Some(l) if l.password == password => None,
            Some(_) => Some(true),
            None => Some(false),
        }
    }

    /// Simpan atau perbarui login (kunci: situs + nama pengguna). Mengembalikan id.
    pub fn upsert(&mut self, url: &str, username: &str, password: &str) -> u64 {
        let site = site_of(url);
        let now = crate::store::now();
        if let Some(l) = self.data.logins.iter_mut().find(|l| l.site == site && l.username == username) {
            l.password = password.to_string();
            l.used = now;
            let id = l.id;
            self.save();
            return id;
        }
        self.data.next_id += 1;
        let id = self.data.next_id;
        self.data.logins.push(Login {
            id,
            site,
            url: url.to_string(),
            username: username.to_string(),
            password: password.to_string(),
            note: String::new(),
            created: now,
            used: now,
        });
        self.save();
        id
    }

    pub fn edit(&mut self, id: u64, username: &str, password: &str, note: &str) {
        if let Some(l) = self.data.logins.iter_mut().find(|l| l.id == id) {
            l.username = username.to_string();
            if !password.is_empty() {
                l.password = password.to_string();
            }
            l.note = note.to_string();
            self.save();
        }
    }

    pub fn touch(&mut self, id: u64) {
        if let Some(l) = self.data.logins.iter_mut().find(|l| l.id == id) {
            l.used = crate::store::now();
            self.save();
        }
    }

    pub fn remove(&mut self, id: u64) {
        self.data.logins.retain(|l| l.id != id);
        self.save();
    }

    pub fn clear(&mut self) {
        self.data = Data::default();
        self.save();
    }

    /// Ekspor CSV format Chrome/Brave: name,url,username,password,note.
    pub fn export_csv(&self) -> String {
        let q = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
        let mut out = String::from("name,url,username,password,note\n");
        for l in &self.data.logins {
            out.push_str(&format!("{},{},{},{},{}\n", q(&l.site), q(&l.url), q(&l.username), q(&l.password), q(&l.note)));
        }
        out
    }

    /// Impor CSV dari Brave/Chrome/Edge/Firefox (kolom url, username, password; urutan bebas).
    /// Mengembalikan jumlah login yang ditambahkan/diperbarui.
    pub fn import_csv(&mut self, csv: &str) -> usize {
        let rows = parse_csv(csv);
        let Some(head) = rows.first() else { return 0 };
        let col = |names: &[&str]| head.iter().position(|h| names.contains(&h.trim().to_ascii_lowercase().as_str()));
        let (Some(cu), Some(cn), Some(cp)) = (col(&["url", "origin", "login_uri"]), col(&["username", "login_username"]), col(&["password", "login_password"])) else {
            return 0;
        };
        let cnote = col(&["note", "notes"]);
        let mut n = 0;
        for r in rows.iter().skip(1) {
            let get = |i: usize| r.get(i).map(|s| s.as_str()).unwrap_or("");
            let url = get(cu);
            if site_of(url).is_empty() || get(cp).is_empty() {
                continue;
            }
            let id = self.upsert(url, get(cn), get(cp));
            if let Some(c) = cnote {
                let note = get(c).to_string();
                if let Some(l) = self.data.logins.iter_mut().find(|l| l.id == id) {
                    l.note = note;
                }
            }
            n += 1;
        }
        self.save();
        n
    }
}

/// Pengurai CSV sederhana (tanda kutip ganda, koma dan baris baru di dalam kutip).
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
        } else {
            match c {
                '"' => quoted = true,
                ',' => row.push(std::mem::take(&mut field)),
                '\r' => {}
                '\n' => {
                    row.push(std::mem::take(&mut field));
                    if row.iter().any(|f| !f.is_empty()) {
                        rows.push(std::mem::take(&mut row));
                    }
                    row.clear();
                }
                _ => field.push(c),
            }
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// Uji enkripsi brankas (DPAPI Windows): sandi bisa dienkripsi lalu dibuka lagi.
pub fn self_test() -> bool {
    let sample = b"ovner-uji-brankas";
    crate::win::protect(sample, true).and_then(|e| crate::win::protect(&e, false)).as_deref() == Some(&sample[..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_roundtrip_and_csv() {
        let d = std::env::temp_dir().join(format!("ovner-vault-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let mut v = Vault::load(&d);
        assert_eq!(v.offer_for("https://www.shopee.co.id/login", "rendi", "rahasia"), Some(false));
        let id = v.upsert("https://www.shopee.co.id/login", "rendi", "rahasia");
        assert_eq!(v.offer_for("https://shopee.co.id/", "rendi", "rahasia"), None);
        assert_eq!(v.offer_for("https://shopee.co.id/", "rendi", "baru"), Some(true));
        assert_eq!(v.for_url("https://shopee.co.id/buyer/login").len(), 1);
        assert!(v.for_url("https://evil.example/").is_empty());
        v.add_never("https://bank.co.id/");
        assert_eq!(v.offer_for("https://bank.co.id/x", "a", "b"), None);
        let csv = v.export_csv();
        assert!(csv.contains("\"rendi\",\"rahasia\""));
        // Muat ulang dari disk.
        let v2 = Vault::load(&d);
        assert_eq!(v2.get(id).map(|l| l.password.as_str()), Some("rahasia"));
        let mut v3 = Vault::load(&d);
        v3.clear();
        let n = v3.import_csv("name,url,username,password,note\ngoogle.com,https://accounts.google.com/,\"a@b.c\",\"p,\"\"q\",\"catatan\"\ncause.id,https://cause.id/,x,y,\n");
        assert_eq!(n, 2);
        assert_eq!(v3.for_url("https://accounts.google.com/signin")[0].password, "p,\"q");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn berkas_tak_terbuka_tidak_ditimpa_kosong() {
        let d = std::env::temp_dir().join(format!("ovner-vault-rusak-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        // Berkas asli yang tidak bisa dibuka (di Windows: DPAPI akun lain; di sini: bukan JSON).
        std::fs::write(d.join("passwords.dat"), b"\x01\x02 bukan json, dienkripsi akun lain").unwrap();
        let mut v = Vault::load(&d);
        assert!(v.logins().is_empty());
        let bak = std::fs::read(d.join("passwords.dat.rusak")).unwrap();
        assert_eq!(bak, b"\x01\x02 bukan json, dienkripsi akun lain");
        // Menambah login lalu menyimpan boleh; salinan asli tetap utuh.
        v.upsert("https://a.example/", "u", "p");
        assert_eq!(std::fs::read(d.join("passwords.dat.rusak")).unwrap(), bak);
        assert_eq!(Vault::load(&d).logins().len(), 1);
        // Muat ulang yang sah tidak membuat salinan baru / menimpa salinan lama.
        std::fs::write(d.join("passwords.dat.rusak"), b"asli").unwrap();
        let _ = Vault::load(&d);
        assert_eq!(std::fs::read(d.join("passwords.dat.rusak")).unwrap(), b"asli");
        // Berkas kosong atau tidak ada bukan "rusak".
        let d2 = d.join("kosong");
        std::fs::create_dir_all(&d2).unwrap();
        let _ = Vault::load(&d2);
        assert!(!d2.join("passwords.dat.rusak").exists());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn login_https_tidak_diisi_ke_halaman_http() {
        let d = std::env::temp_dir().join(format!("ovner-vault-skema-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let mut v = Vault::load(&d);
        v.upsert("https://bank.example/login", "rendi", "rahasia");
        v.upsert("http://lama.example/login", "rendi", "x");
        v.upsert("https://localhost/login", "dev", "y");
        assert_eq!(v.for_url("https://bank.example/akun").len(), 1);
        assert!(v.for_url("http://bank.example/akun").is_empty(), "turun ke http tidak boleh");
        // Login yang memang disimpan dari http boleh dipakai di http dan di https.
        assert_eq!(v.for_url("http://lama.example/").len(), 1);
        assert_eq!(v.for_url("https://lama.example/").len(), 1);
        // Alamat lokal dianggap aman.
        assert_eq!(v.for_url("http://localhost:3000/").len(), 1);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
