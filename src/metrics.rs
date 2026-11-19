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
}
