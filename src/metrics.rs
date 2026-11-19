//! Pengukuran berkala Ovner: satu baris JSON per menit di `<data>/metrics/YYYY-MM-DD.jsonl`
//! (disimpan 7 hari), plus penghitung harian (iklan diblokir, tab ditidurkan, MB dibebaskan,
//! unduhan, crash, peristiwa dan galat per kategori log). Murah: sampel proses sekali semenit
//! dan satu baris ±1 KB (±1,5 MB per hari bila Ovner menyala terus).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const KEEP_DAYS: u64 = 7;

struct Day {
    date: String,
    counts: BTreeMap<String, u64>,
}

static COUNTS: Mutex<Option<Day>> = Mutex::new(None);

fn today() -> String {
    crate::win::local_time().chars().take(10).collect()
}

/// Tambah penghitung harian `name` sebanyak `n`.
pub fn count(name: &str, n: u64) {
    if n == 0 {
        return;
    }
    let date = today();
    let mut g = COUNTS.lock().unwrap_or_else(|e| e.into_inner());
    let day = g.get_or_insert_with(|| Day { date: date.clone(), counts: BTreeMap::new() });
    if day.date != date {
        *day = Day { date, counts: BTreeMap::new() };
    }
    *day.counts.entry(name.to_string()).or_insert(0) += n;
}

/// Penghitung hari ini.
pub fn counts() -> BTreeMap<String, u64> {
    let g = COUNTS.lock().unwrap_or_else(|e| e.into_inner());
    match g.as_ref() {
        Some(d) if d.date == today() => d.counts.clone(),
        _ => BTreeMap::new(),
    }
}

pub fn dir(data: &Path) -> PathBuf {
    data.join("metrics")
}

/// Tulis satu baris pengukuran (objek JSON) dan buang berkas yang lebih tua dari 7 hari.
pub fn write(data: &Path, mut line: serde_json::Value) {
    let d = dir(data);
    let _ = std::fs::create_dir_all(&d);
    if let Some(o) = line.as_object_mut() {
        o.insert("t".into(), crate::win::local_time().into());
        o.insert("hitung".into(), serde_json::to_value(counts()).unwrap_or_default());
    }
    let path = d.join(format!("{}.jsonl", today()));
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{line}");
    }
    prune(&d);
}

fn prune(d: &Path) {
    let Ok(rd) = std::fs::read_dir(d) else { return };
    for e in rd.flatten() {
        let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|a| a.as_secs() > KEEP_DAYS * 86_400);
        if old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Berkas pengukuran, terbaru dulu.
pub fn files(data: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir(data)).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "jsonl")).collect();
    v.sort();
    v.reverse();
    v
}

// ---- Deteksi kebocoran RAM ----
//
// Kebocoran = RAM yang terus naik padahal pemakaian tetap. Maka yang dibandingkan hanya potongan
// waktu dengan jumlah tab hidup yang sama (tidak ada tab dibuka/ditidurkan), dalam satu sesi
// (Ovner tidak dibuka ulang), setelah 10 menit pertama (masa pemanasan), minimal 90 menit.
// Kemiringan dihitung dengan regresi garis lurus (MB per jam).

/// Satu sampel pengukuran untuk analisis tren.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub min: f64,
    pub live: u64,
    /// Heap Rust Ovner (MB); tidak ada di pengukuran sebelum 0.8.26.
    pub heap: Option<f64>,
    /// Memori komit ovner.exe (MB).
    pub ovner: f64,
    /// Memori komit proses inti WebView2 (MB).
    pub browser: f64,
}

pub fn sample_of(v: &serde_json::Value) -> Option<Sample> {
    let commit = |k: &str| v["per_jenis"][k]["commit"].as_f64();
    Some(Sample { min: v["menit"].as_f64()?, live: v["tab_hidup"].as_u64()?, heap: v["heap_rust"].as_f64(), ovner: commit("ovner")?, browser: commit("browser").unwrap_or(0.0) })
}

/// Kemiringan (MB per jam) dari titik (menit, MB).
pub fn slope_per_hour(pts: &[(f64, f64)]) -> f64 {
    let n = pts.len() as f64;
    if n < 2.0 {
        return 0.0;
    }
    let (sx, sy) = pts.iter().fold((0.0, 0.0), |a, p| (a.0 + p.0, a.1 + p.1));
    let (mx, my) = (sx / n, sy / n);
    let (mut num, mut den) = (0.0, 0.0);
    for (x, y) in pts {
        num += (x - mx) * (y - my);
        den += (x - mx) * (x - mx);
    }
    if den == 0.0 {
        0.0
    } else {
        num / den * 60.0
    }
}

/// Tren satu potongan waktu yang sebanding.
#[derive(Clone, Debug)]
pub struct Trend {
    pub minutes: f64,
    pub live: u64,
    pub heap: Option<f64>,
    pub ovner: f64,
    pub browser: f64,
}

pub const WARMUP_MIN: f64 = 10.0;
pub const MIN_SPAN_MIN: f64 = 90.0;
pub const HEAP_LIMIT: f64 = 4.0;
pub const OVNER_LIMIT: f64 = 8.0;
pub const BROWSER_LIMIT: f64 = 30.0;

/// Potong sampel per sesi dan per jumlah tab hidup, lalu hitung tren tiap potongan yang cukup
/// panjang (`min_span` menit).
pub fn trends(samples: &[Sample], min_span: f64) -> Vec<Trend> {
    let mut out = Vec::new();
    let mut seg: Vec<Sample> = Vec::new();
    let mut flush = |seg: &mut Vec<Sample>| {
        if seg.len() >= 3 {
            let span = seg.last().unwrap().min - seg[0].min;
            if span >= min_span {
                let pts = |f: &dyn Fn(&Sample) -> f64| seg.iter().map(|s| (s.min, f(s))).collect::<Vec<_>>();
                let heap = seg.iter().all(|s| s.heap.is_some()).then(|| slope_per_hour(&pts(&|s| s.heap.unwrap_or(0.0))));
                out.push(Trend { minutes: span, live: seg[0].live, heap, ovner: slope_per_hour(&pts(&|s| s.ovner)), browser: slope_per_hour(&pts(&|s| s.browser)) });
            }
        }
        seg.clear();
    };
    for s in samples {
        if s.min < WARMUP_MIN {
            flush(&mut seg);
            continue;
        }
        if let Some(last) = seg.last() {
            if s.min < last.min || s.min - last.min > 3.0 || s.live != last.live {
                flush(&mut seg);
            }
        }
        seg.push(*s);
    }
    flush(&mut seg);
    out
}

/// Peringatan bila tren mirip kebocoran.
pub fn verdict(t: &Trend) -> Option<String> {
    let mut why = Vec::new();
    if let Some(h) = t.heap.filter(|h| *h > HEAP_LIMIT) {
        why.push(format!("heap Rust Ovner +{h:.1} MB/jam"));
    }
    if t.ovner > OVNER_LIMIT {
        why.push(format!("ovner.exe +{:.1} MB/jam", t.ovner));
    }
    if t.browser > BROWSER_LIMIT {
        why.push(format!("proses inti WebView2 +{:.1} MB/jam", t.browser));
    }
    (!why.is_empty()).then(|| format!("{} selama {:.0} menit dengan {} tab hidup tetap", why.join(", "), t.minutes, t.live))
}

/// Analisis semua pengukuran 7 hari terakhir: (ringkasan, ada peringatan).
pub fn analyze(data: &Path) -> (String, bool) {
    let mut files = files(data);
    files.reverse();
    let mut samples = Vec::new();
    let mut sessions = 0;
    let mut last_min = f64::MAX;
    for f in &files {
        let Ok(text) = std::fs::read_to_string(f) else { continue };
        for l in text.lines() {
            let Some(s) = serde_json::from_str::<serde_json::Value>(l).ok().as_ref().and_then(sample_of) else { continue };
            if s.min < last_min {
                sessions += 1;
            }
            last_min = s.min;
            samples.push(s);
        }
    }
    if samples.is_empty() {
        return ("belum ada pengukuran (aktifkan Pengukuran per menit di Pengaturan)".into(), false);
    }
    let hours = samples.len() as f64 / 60.0;
    let tr = trends(&samples, MIN_SPAN_MIN);
    if tr.is_empty() {
        return (format!("{hours:.1} jam data dari {sessions} sesi, tetapi belum ada potongan ≥ {MIN_SPAN_MIN:.0} menit dengan jumlah tab hidup tetap; pakai Ovner lebih lama tanpa membuka/menidurkan tab"), false);
    }
    let warns: Vec<String> = tr.iter().filter_map(verdict).collect();
    let max = |f: &dyn Fn(&Trend) -> f64| tr.iter().map(f).fold(f64::MIN, f64::max);
    let heap = tr.iter().filter_map(|t| t.heap).fold(f64::MIN, f64::max);
    let detail = format!(
        "{hours:.1} jam data, {sessions} sesi, {} potongan sebanding; kenaikan terbesar: {}ovner.exe {:+.1} MB/jam, inti WebView2 {:+.1} MB/jam",
        tr.len(),
        if heap > f64::MIN { format!("heap Rust {heap:+.1} MB/jam, ") } else { String::new() },
        max(&|t| t.ovner),
        max(&|t| t.browser)
    );
    if warns.is_empty() {
        (format!("tidak ada tanda kebocoran — {detail}"), false)
    } else {
        (format!("kemungkinan kebocoran: {} — {detail}", warns.join("; ")), true)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn counts_add_up() {
        super::count("uji_a", 2);
        super::count("uji_a", 3);
        super::count("uji_b", 0);
        let c = super::counts();
        assert_eq!(c.get("uji_a"), Some(&5));
        assert!(!c.contains_key("uji_b"));
    }

    fn run(min0: f64, n: usize, live: u64, heap: f64, grow: f64) -> Vec<super::Sample> {
        (0..n).map(|i| super::Sample { min: min0 + i as f64, live, heap: Some(heap + grow * i as f64 / 60.0), ovner: 78.0, browser: 60.0 }).collect()
    }

    #[test]
    fn flat_usage_is_not_a_leak() {
        let t = super::trends(&run(10.0, 120, 2, 20.0, 0.0), 90.0);
        assert_eq!(t.len(), 1);
        assert!(super::verdict(&t[0]).is_none());
    }

    #[test]
    fn steady_growth_is_flagged() {
        let t = super::trends(&run(10.0, 120, 2, 20.0, 10.0), 90.0);
        let v = super::verdict(&t[0]).expect("harus diperingatkan");
        assert!(v.contains("heap Rust Ovner +10.0 MB/jam"), "{v}");
    }

    #[test]
    fn tab_changes_and_restarts_split_segments() {
        let mut s = run(10.0, 100, 2, 20.0, 50.0);
        s.extend(run(110.0, 100, 3, 20.0, 50.0)); // tab hidup berubah
        s.extend(run(0.0, 50, 3, 20.0, 50.0)); // Ovner dibuka ulang (menit mundur), terlalu pendek
        let t = super::trends(&s, 90.0);
        assert_eq!(t.len(), 2);
        // Pemanasan 10 menit pertama tidak dihitung.
        let warm = super::trends(&run(0.0, 95, 1, 20.0, 0.0), 90.0);
        assert!(warm.is_empty());
    }

    #[test]
    fn user_session_0824_has_no_leak() {
        // Potongan nyata dari PC pengguna (0.8.24, 14:56–15:26): ovner.exe datar ±78 MB.
        let s: Vec<super::Sample> = (16..46).map(|m| super::Sample { min: m as f64, live: 1, heap: None, ovner: 78.0 + if m % 7 == 0 { 1.0 } else { 0.0 }, browser: 69.0 }).collect();
        let t = super::trends(&s, 25.0);
        assert!(super::verdict(&t[0]).is_none(), "{:?}", t);
    }
}
