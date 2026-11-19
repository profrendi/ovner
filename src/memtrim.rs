//! Rapikan RAM cerdas: memutuskan proses mana yang layak dirapikan (working set dikosongkan),
//! lalu mengukur apakah hasilnya bertahan.
//!
//! Mengosongkan working set hanya memindahkan isi memori ke cadangan Windows; proses yang
//! langsung menyentuhnya lagi (tab yang sedang dilihat, GPU saat menggambar/memutar video)
//! membuatnya naik lagi, kadang lebih besar, dan memakan CPU. Karena itu:
//! - proses yang sedang dipakai tidak disentuh (keputusan "boleh" dibuat oleh App per jenis proses);
//! - proses hanya dirapikan bila memorinya bertambah cukup besar sejak terakhir dirapikan;
//! - 60 detik setelah dirapikan, hasilnya diukur: proses yang memantul kembali (≥80% dari yang
//!   dibuang) dilewati makin lama (jarak ×2, ×4, ×8); yang bertahan kembali ke jarak normal.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Proses baru dirapikan bila bertambah sebanyak ini sejak terakhir dirapikan.
pub const GROWTH: u64 = 20 * 1_048_576;
/// Proses yang lebih kecil dari ini tidak usah dirapikan.
pub const MIN_SIZE: u64 = 8 * 1_048_576;
/// Hasil dinilai sekian lama setelah dirapikan.
pub const CHECK_AFTER: Duration = Duration::from_secs(60);
/// Kelipatan jarak terbesar untuk proses yang selalu memantul.
pub const MAX_BACKOFF: u32 = 8;

#[derive(Clone, Debug)]
pub struct Candidate {
    pub pid: u32,
    pub kind: String,
    /// Private working set sekarang (byte).
    pub working: u64,
    /// Alasan proses ini tidak boleh dirapikan sekarang (None = boleh).
    pub blocked: Option<&'static str>,
}

#[derive(Clone, Copy, Debug)]
struct PidState {
    last: Instant,
    /// Working set tepat setelah dirapikan terakhir.
    after: u64,
    /// Kelipatan jarak (1 = normal).
    backoff: u32,
}

#[derive(Clone, Debug)]
pub struct Trimmed {
    pub pid: u32,
    pub kind: String,
    pub before: u64,
    pub after: u64,
}

#[derive(Debug, Default, PartialEq)]
pub struct Verdict {
    pub before: u64,
    pub after: u64,
    pub now: u64,
    /// Proses yang memantul (jenis, kelipatan jarak barunya).
    pub bounced: Vec<(String, u32)>,
}

impl Verdict {
    /// Persen hasil yang bertahan (100 = tetap serendah setelah dirapikan).
    pub fn held_pct(&self) -> u64 {
        let freed = self.before.saturating_sub(self.after);
        if freed == 0 {
            return 100;
        }
        let back = self.now.saturating_sub(self.after).min(freed);
        (freed - back) * 100 / freed
    }
}

#[derive(Default)]
pub struct Trimmer {
    state: HashMap<u32, PidState>,
    pending: Option<(Instant, Vec<Trimmed>)>,
}

impl Trimmer {
    /// Keadaan per proses untuk Diagnostik: (pid, menit sejak dirapikan, MB setelahnya, jeda ×).
    pub fn describe(&self) -> Vec<(u32, u64, u64, u32)> {
        let mut v: Vec<(u32, u64, u64, u32)> = self.state.iter().map(|(pid, st)| (*pid, st.last.elapsed().as_secs() / 60, st.after / 1_048_576, st.backoff)).collect();
        v.sort();
        v
    }

    pub fn new() -> Trimmer {
        Trimmer::default()
    }

    /// Pilih proses yang dirapikan. `base` = jarak normal antar-rapikan per proses.
    /// `force` (diminta pengguna/diminimalkan): abaikan jarak dan syarat bertambah, tetapi
    /// proses yang sedang dipakai tetap tidak disentuh.
    pub fn choose(&self, now: Instant, procs: &[Candidate], base: Duration, force: bool) -> (Vec<u32>, Vec<(String, &'static str)>) {
        let mut pick = Vec::new();
        let mut skipped = Vec::new();
        for p in procs {
            if let Some(why) = p.blocked {
                skipped.push((p.kind.clone(), why));
                continue;
            }
            if p.working < MIN_SIZE {
                continue;
            }
            if !force {
                if let Some(st) = self.state.get(&p.pid) {
                    if now.duration_since(st.last) < base * st.backoff {
                        if st.backoff > 1 {
                            skipped.push((p.kind.clone(), "sering memantul"));
                        }
                        continue;
                    }
                    if p.working < st.after + GROWTH {
                        continue;
                    }
                } else if p.working < GROWTH {
                    continue;
                }
            }
            pick.push(p.pid);
        }
        (pick, skipped)
    }

    /// Catat hasil rapikan; penilaian dijadwalkan `CHECK_AFTER` kemudian.
    pub fn record(&mut self, now: Instant, done: Vec<Trimmed>) {
        for t in &done {
            let backoff = self.state.get(&t.pid).map(|s| s.backoff).unwrap_or(1);
            self.state.insert(t.pid, PidState { last: now, after: t.after, backoff });
        }
        if !done.is_empty() {
            self.pending = Some((now + CHECK_AFTER, done));
        }
    }

    pub fn check_due(&self, now: Instant) -> bool {
        self.pending.as_ref().is_some_and(|(at, _)| now >= *at)
    }

    /// Nilai hasil rapikan dengan working set sekarang (`current`: pid → byte).
    pub fn evaluate(&mut self, current: &HashMap<u32, u64>) -> Option<Verdict> {
        let (_, done) = self.pending.take()?;
        let mut v = Verdict::default();
        for t in done {
            let Some(&now) = current.get(&t.pid) else { continue };
            v.before += t.before;
            v.after += t.after;
            v.now += now;
            let freed = t.before.saturating_sub(t.after);
            let bounced = freed > 0 && now.saturating_sub(t.after) * 10 >= freed * 8;
            if let Some(st) = self.state.get_mut(&t.pid) {
                st.backoff = if bounced { (st.backoff * 2).min(MAX_BACKOFF) } else { 1 };
                if bounced {
                    v.bounced.push((t.kind.clone(), st.backoff));
                }
            }
        }
        Some(v)
    }

    /// Lupakan proses yang sudah tidak ada.
    pub fn retain(&mut self, alive: &[u32]) {
        self.state.retain(|pid, _| alive.contains(pid));
    }
}

/// Batas "RAM hampir habis": tersedia di bawah 6% RAM fisik, paling sedikit 512 MB.
/// Kembali normal setelah tersedia 1,5× batas itu (supaya tidak bolak-balik).
pub fn low_memory_threshold(total: u64) -> u64 {
    (total / 100 * 6).max(512 * 1_048_576)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1_048_576;

    fn c(pid: u32, kind: &str, mb: u64, blocked: Option<&'static str>) -> Candidate {
        Candidate { pid, kind: kind.into(), working: mb * MB, blocked }
    }

    #[test]
    fn chooses_only_idle_grown_processes_and_backs_off_on_bounce() {
        let mut t = Trimmer::new();
        let t0 = Instant::now();
        let base = Duration::from_secs(300);
        let procs = vec![
            c(1, "renderer", 300, Some("tab terlihat")),
            c(2, "gpu", 150, Some("GPU sedang dipakai")),
            c(3, "renderer", 120, None),
            c(4, "network", 12, None),
            c(5, "browser", 60, None),
        ];
        let (pick, skipped) = t.choose(t0, &procs, base, false);
        assert_eq!(pick, vec![3, 5], "yang dipakai dan yang terlalu kecil dilewati");
        assert_eq!(skipped.len(), 2);
        // Paksa (diminta pengguna): syarat bertambah diabaikan, yang dipakai tetap tidak.
        let (pick, _) = t.choose(t0, &procs, base, true);
        assert_eq!(pick, vec![3, 4, 5]);

        t.record(t0, vec![Trimmed { pid: 3, kind: "renderer".into(), before: 120 * MB, after: 20 * MB }, Trimmed { pid: 5, kind: "browser".into(), before: 60 * MB, after: 10 * MB }]);
        assert!(!t.check_due(t0 + Duration::from_secs(30)));
        assert!(t.check_due(t0 + CHECK_AFTER));
        // Renderer bertahan (naik 10 dari 100 yang dibuang); browser memantul ke 55 MB (90%).
        let cur = HashMap::from([(3, 30 * MB), (5, 55 * MB)]);
        let v = t.evaluate(&cur).unwrap();
        assert_eq!(v.bounced, vec![("browser".to_string(), 2)]);
        assert_eq!((v.before, v.after, v.now), (180 * MB, 30 * MB, 85 * MB));
        assert_eq!(v.held_pct(), 63);

        // 5 menit kemudian: renderer boleh lagi bila bertambah ≥20 MB; browser menunggu 10 menit.
        let t5 = t0 + base;
        let procs = vec![c(3, "renderer", 45, None), c(5, "browser", 70, None)];
        let (pick, skipped) = t.choose(t5, &procs, base, false);
        assert_eq!(pick, vec![3]);
        assert_eq!(skipped, vec![("browser".to_string(), "sering memantul")]);
        let procs = vec![c(3, "renderer", 35, None)];
        assert!(t.choose(t5, &procs, base, false).0.is_empty(), "baru bertambah 15 MB");
        let (pick, _) = t.choose(t0 + base * 2, &[c(5, "browser", 70, None)], base, false);
        assert_eq!(pick, vec![5]);

        // Memantul terus: jarak berlipat sampai 8×, lalu kembali 1× begitu bertahan.
        for _ in 0..5 {
            t.record(t0, vec![Trimmed { pid: 5, kind: "browser".into(), before: 60 * MB, after: 10 * MB }]);
            t.evaluate(&HashMap::from([(5, 60 * MB)]));
        }
        assert_eq!(t.state[&5].backoff, MAX_BACKOFF);
        t.record(t0, vec![Trimmed { pid: 5, kind: "browser".into(), before: 60 * MB, after: 10 * MB }]);
        t.evaluate(&HashMap::from([(5, 12 * MB)]));
        assert_eq!(t.state[&5].backoff, 1);
        t.retain(&[3]);
        assert!(!t.state.contains_key(&5));
    }

    #[test]
    fn low_memory_threshold_scales() {
        assert_eq!(low_memory_threshold(8 * 1024 * MB), 512 * MB);
        assert_eq!(low_memory_threshold(16 * 1024 * MB) / MB, 983);
    }
}
