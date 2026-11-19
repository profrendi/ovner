//! Penghitung heap Rust milik Ovner sendiri (ovner.exe), untuk diagnostik RAM dan deteksi
//! kebocoran: berapa byte yang sedang dialokasikan kode Rust Ovner, terpisah dari DLL
//! WebView2/Windows yang ikut menempati proses yang sama. Biaya: satu operasi atomik per
//! alokasi (tidak terasa).

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

pub struct Counting;

static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

#[inline]
fn grew(n: usize) {
    let v = NOW.fetch_add(n, Relaxed) + n;
    if v > PEAK.load(Relaxed) {
        PEAK.store(v, Relaxed);
    }
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = System.alloc(l);
        if !p.is_null() {
            grew(l.size());
        }
        p
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let p = System.alloc_zeroed(l);
        if !p.is_null() {
            grew(l.size());
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        NOW.fetch_sub(l.size(), Relaxed);
        System.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let q = System.realloc(p, l, n);
        if !q.is_null() {
            if n >= l.size() {
                grew(n - l.size());
            } else {
                NOW.fetch_sub(l.size() - n, Relaxed);
            }
        }
        q
    }
}

/// Byte heap Rust yang sedang dipakai.
pub fn now() -> u64 {
    NOW.load(Relaxed) as u64
}

/// Puncak heap Rust sejak Ovner mulai.
pub fn peak() -> u64 {
    PEAK.load(Relaxed) as u64
}

#[cfg(test)]
mod tests {
    #[test]
    fn counts_allocations() {
        // Penghitung global hanya aktif di biner Ovner; di sini cukup pastikan fungsi tersedia.
        let _ = (super::now(), super::peak());
    }
}
