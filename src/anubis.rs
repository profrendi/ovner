//! Pemecah tantangan Anubis ("Making sure you're not a bot!") secara native.
//!
//! Startpage dan makin banyak situs memasang Anubis: browser harus mencari `nonce` sehingga
//! SHA-256(`randomData` + nonce) diawali sejumlah angka 0 heksadesimal. Di halaman, JavaScript
//! mengerjakannya dengan Web Worker dan bisa makan belasan detik di PC kantor (lebih lama lagi
//! dengan "JavaScript hemat memori"). Ovner menghitungnya di Rust dengan semua inti CPU, jadi
//! biasanya selesai dalam hitungan milidetik, lalu halaman diarahkan ke alamat lolos
//! tantangan seperti yang dilakukan skrip Anubis sendiri (cara yang sama dengan SearXNG).

use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

/// Kesulitan terbesar yang mau dikerjakan (16^6 ≈ 16 juta hash rata-rata).
pub const MAX_DIFFICULTY: u32 = 6;

/// Apakah hash (biner) diawali `difficulty` angka 0 heksadesimal?
fn leading_hex_zeros(hash: &[u8], difficulty: u32) -> bool {
    let full = (difficulty / 2) as usize;
    if hash.len() < full + 1 {
        return false;
    }
    if hash[..full].iter().any(|b| *b != 0) {
        return false;
    }
    difficulty.is_multiple_of(2) || hash[full] >> 4 == 0
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Cari nonce: kembalikan (hash heksadesimal, nonce). `None` bila tidak ketemu dalam batas
/// (8× rata-rata) atau kesulitannya terlalu tinggi.
pub fn solve(random: &str, difficulty: u32) -> Option<(String, u64)> {
    if difficulty == 0 || difficulty > MAX_DIFFICULTY || random.is_empty() || random.len() > 1024 {
        return None;
    }
    let limit: u64 = 16u64.pow(difficulty) * 8;
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).clamp(1, 8) as u64;
    let found = Arc::new(AtomicBool::new(false));
    let best = Arc::new(AtomicU64::new(u64::MAX));
    let mut handles = Vec::new();
    for t in 0..threads {
        let (found, best) = (found.clone(), best.clone());
        let prefix = Sha256::new_with_prefix(random.as_bytes());
        handles.push(std::thread::spawn(move || {
            let mut nonce = t;
            let mut buf = itoa_buf();
            let mut step = 0u32;
            while nonce < limit {
                // Berhenti bila thread lain sudah menemukan nonce yang lebih kecil.
                step = step.wrapping_add(1);
                if step & 1023 == 0 && found.load(Ordering::Relaxed) && nonce > best.load(Ordering::Relaxed) {
                    break;
                }
                let digits = write_u64(&mut buf, nonce);
                let mut h = prefix.clone();
                h.update(digits);
                let out = h.finalize();
                if leading_hex_zeros(&out, difficulty) {
                    best.fetch_min(nonce, Ordering::SeqCst);
                    found.store(true, Ordering::SeqCst);
                    break;
                }
                nonce += threads;
            }
        }));
    }
    for h in handles {
        let _ = h.join();
    }
    let n = best.load(Ordering::SeqCst);
    if n == u64::MAX {
        return None;
    }
    let mut h = Sha256::new();
    h.update(random.as_bytes());
    h.update(n.to_string().as_bytes());
    Some((hex(&h.finalize()), n))
}

fn itoa_buf() -> [u8; 20] {
    [0u8; 20]
}

/// Tulis angka desimal tanpa alokasi; kembalikan potongan berisi digitnya.
fn write_u64(buf: &mut [u8; 20], mut v: u64) -> &[u8] {
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    &buf[i..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solves_like_anubis() {
        let (hash, nonce) = solve("4c1d6a9e2b", 4).expect("terpecahkan");
        assert!(hash.starts_with("0000"), "{hash}");
        let mut h = Sha256::new();
        h.update(format!("4c1d6a9e2b{nonce}").as_bytes());
        assert_eq!(hex(&h.finalize()), hash);
        // Nonce terkecil: tidak ada nonce lebih kecil yang juga memenuhi.
        for n in 0..nonce {
            let mut h = Sha256::new();
            h.update(format!("4c1d6a9e2b{n}").as_bytes());
            assert!(!hex(&h.finalize()).starts_with("0000"));
        }
        assert!(solve("x", 0).is_none());
        assert!(solve("x", 9).is_none());
        assert!(leading_hex_zeros(&[0, 0x0f, 0xff], 3));
        assert!(!leading_hex_zeros(&[0, 0x1f, 0xff], 3));
        let mut b = itoa_buf();
        assert_eq!(write_u64(&mut b, 0), b"0");
        assert_eq!(write_u64(&mut b, 1234567), b"1234567");
    }
}
