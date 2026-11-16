//! Pemakaian RAM, CPU, dan GPU Ovner beserta proses WebView2-nya.
//!
//! Proses WebView2 tidak selalu tercatat sebagai anak ovner.exe (di sebagian PC induknya
//! proses lain), jadi proses milik Ovner dikenali dari baris perintahnya: semua proses
//! msedgewebview2.exe Ovner memuat `--user-data-dir=<folder data Ovner>\WebView2`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// Satu proses Ovner/WebView2 beserta pemakaiannya.
#[derive(Clone, Debug, Default)]
pub struct ProcStat {
    pub pid: u32,
    /// "ovner", "browser", "renderer", "gpu", "network", "storage", "audio", "utility", "crashpad", "lain".
    pub kind: String,
    /// Private working set (kolom Memory di Task Manager), byte.
    pub working: u64,
    /// Private bytes / commit, byte.
    pub commit: u64,
    /// Persen CPU dari seluruh komputer (seperti Task Manager), sejak sampel sebelumnya.
    pub cpu: f64,
    /// Persen GPU (mesin GPU paling sibuk untuk proses ini).
    pub gpu: f64,
    /// Memori GPU (dedicated + shared), byte.
    pub gpu_mem: u64,
}

/// Jumlah semua proses.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Total {
    pub working: u64,
    pub cpu: f64,
    pub gpu: f64,
    pub gpu_mem: u64,
    pub procs: usize,
}

pub fn total(stats: &[ProcStat]) -> Total {
    Total {
        working: stats.iter().map(|p| p.working).sum(),
        cpu: stats.iter().map(|p| p.cpu).sum::<f64>().min(100.0),
        gpu: stats.iter().map(|p| p.gpu).sum::<f64>().min(100.0),
        gpu_mem: stats.iter().map(|p| p.gpu_mem).sum(),
        procs: stats.len(),
    }
}

static USER_DATA: OnceLock<String> = OnceLock::new();

/// Folder data WebView2 Ovner (dipanggil sekali saat mulai).
pub fn set_user_data_dir(dir: &Path) {
    let _ = USER_DATA.set(dir.display().to_string().to_ascii_lowercase().trim_end_matches(['\\', '/']).to_string());
}

/// Jenis proses Chromium dari baris perintahnya; `None` bila bukan milik Ovner.
pub fn classify(cmdline: &str, user_data: &str) -> Option<String> {
    let lower = cmdline.to_ascii_lowercase();
    let ours = (!user_data.is_empty() && lower.contains(user_data)) || lower.contains("--webview-exe-name=ovner.exe");
    if !ours {
        return None;
    }
    let arg = |name: &str| {
        lower
            .split_whitespace()
            .find_map(|a| a.trim_matches('"').strip_prefix(name).map(|v| v.trim_matches('"').to_string()))
    };
    let kind = match arg("--type=").as_deref() {
        None => "browser".to_string(),
        Some("renderer") => "renderer".into(),
        Some("gpu-process") => "gpu".into(),
        Some("crashpad-handler") => "crashpad".into(),
        Some("utility") => match arg("--utility-sub-type=").as_deref() {
            Some(s) if s.contains("network") => "network".into(),
            Some(s) if s.contains("storage") => "storage".into(),
            Some(s) if s.contains("audio") => "audio".into(),
            _ => "utility".into(),
        },
        Some(_) => "lain".into(),
    };
    Some(kind)
}

/// Pengambil sampel: menyimpan waktu CPU sebelumnya untuk menghitung persen.
pub struct Sampler {
    prev_cpu: HashMap<u32, u64>,
    prev_at: Option<Instant>,
    #[cfg(windows)]
    gpu: Option<imp::Gpu>,
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Sampler {
    pub fn new() -> Sampler {
        Sampler {
            prev_cpu: HashMap::new(),
            prev_at: None,
            #[cfg(windows)]
            gpu: None,
        }
    }

    pub fn sample(&mut self, with_gpu: bool) -> Vec<ProcStat> {
        let now = Instant::now();
        let mut out = imp::snapshot();
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f64;
        let wall = self.prev_at.map(|t| now.duration_since(t).as_secs_f64()).unwrap_or(0.0);
        let mut next = HashMap::new();
        for p in &mut out {
            let cpu_time = imp::cpu_time_100ns(p.pid);
            if let (Some(t), Some(prev)) = (cpu_time, self.prev_cpu.get(&p.pid)) {
                if wall > 0.0 {
                    let used = t.saturating_sub(*prev) as f64 / 10_000_000.0;
                    p.cpu = (used / wall / cores * 100.0).clamp(0.0, 100.0);
                }
            }
            if let Some(t) = cpu_time {
                next.insert(p.pid, t);
            }
        }
        self.prev_cpu = next;
        self.prev_at = Some(now);
        #[cfg(windows)]
        if with_gpu {
            if self.gpu.is_none() {
                self.gpu = imp::Gpu::open();
            }
            if let Some(g) = &mut self.gpu {
                let (util, mem) = g.read();
                for p in &mut out {
                    p.gpu = util.get(&p.pid).copied().unwrap_or(0.0).min(100.0);
                    p.gpu_mem = mem.get(&p.pid).copied().unwrap_or(0);
                }
            }
        }
        #[cfg(not(windows))]
        let _ = with_gpu;
        out
    }
}

/// PID semua proses Ovner (ovner.exe + WebView2), untuk perapian RAM dan mode hemat.
pub fn ovner_pids() -> Vec<u32> {
    imp::processes().into_iter().map(|(pid, _)| pid).collect()
}

/// Ambil `pid` dari nama instance PDH, mis. "pid_1234_luid_0x..._engtype_3D".
pub fn pdh_pid(name: &str) -> Option<u32> {
    name.strip_prefix("pid_")?.split('_').next()?.parse().ok()
}

static KINDS: Mutex<Option<HashMap<u32, Option<String>>>> = Mutex::new(None);

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
    use windows::Win32::System::Threading::{GetCurrentProcessId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    fn command_line(h: HANDLE) -> Option<String> {
        use windows::Wdk::System::Threading::{NtQueryInformationProcess, ProcessCommandLineInformation};
        use windows::Win32::Foundation::UNICODE_STRING;
        let mut size = 4096u32;
        for _ in 0..3 {
            // Penyangga u64 supaya sejajar 8 byte untuk UNICODE_STRING.
            let mut buf = vec![0u64; (size as usize).div_ceil(8)];
            let mut need = 0u32;
            let st = unsafe { NtQueryInformationProcess(h, ProcessCommandLineInformation, buf.as_mut_ptr() as *mut _, size, &mut need) };
            if st.0 == 0 {
                let us = unsafe { &*(buf.as_ptr() as *const UNICODE_STRING) };
                if us.Buffer.is_null() || us.Length == 0 {
                    return None;
                }
                let start = buf.as_ptr() as usize;
                let end = start + buf.len() * 8;
                let p = us.Buffer.0 as usize;
                let len = us.Length as usize;
                // Buffer harus berada di dalam penyangga kita.
                if p < start || p + len > end {
                    return None;
                }
                let s = unsafe { std::slice::from_raw_parts(us.Buffer.0, len / 2) };
                return Some(String::from_utf16_lossy(s));
            }
            // STATUS_INFO_LENGTH_MISMATCH / BUFFER_TOO_SMALL: coba lagi dengan ukuran yang diminta.
            if need > size && need < 1 << 20 {
                size = need + 64;
            } else {
                return None;
            }
        }
        None
    }

    /// (pid, jenis) semua proses Ovner.
    pub fn processes() -> Vec<(u32, String)> {
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
        };
        let me = unsafe { GetCurrentProcessId() };
        let mut candidates = Vec::new();
        unsafe {
            let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return vec![(me, "ovner".into())] };
            let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            if Process32FirstW(snap, &mut e).is_ok() {
                loop {
                    let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                    if String::from_utf16_lossy(&e.szExeFile[..len]).eq_ignore_ascii_case("msedgewebview2.exe") {
                        candidates.push(e.th32ProcessID);
                    }
                    if Process32NextW(snap, &mut e).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snap);
        }
        let user_data = USER_DATA.get().cloned().unwrap_or_default();
        let mut guard = KINDS.lock().unwrap_or_else(|e| e.into_inner());
        let cache = guard.get_or_insert_with(HashMap::new);
        cache.retain(|pid, _| candidates.contains(pid));
        let mut out = vec![(me, "ovner".to_string())];
        for pid in candidates {
            let kind = cache.entry(pid).or_insert_with(|| unsafe {
                let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return None };
                let k = command_line(h).and_then(|c| classify(&c, &user_data));
                let _ = CloseHandle(h);
                k
            });
            if let Some(k) = kind {
                out.push((pid, k.clone()));
            }
        }
        out
    }

    pub fn snapshot() -> Vec<ProcStat> {
        use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2};
        processes()
            .into_iter()
            .filter_map(|(pid, kind)| unsafe {
                let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
                let mut p = ProcStat { pid, kind, ..Default::default() };
                let mut pmc = PROCESS_MEMORY_COUNTERS_EX2 { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32, ..Default::default() };
                if GetProcessMemoryInfo(h, &mut pmc as *mut _ as *mut PROCESS_MEMORY_COUNTERS, pmc.cb).is_ok() {
                    // Private working set = kolom "Memory" Task Manager Windows.
                    p.working = (if pmc.PrivateWorkingSetSize > 0 { pmc.PrivateWorkingSetSize } else { pmc.WorkingSetSize }) as u64;
                    p.commit = pmc.PrivateUsage as u64;
                }
                let _ = CloseHandle(h);
                Some(p)
            })
            .collect()
    }

    pub fn cpu_time_100ns(pid: u32) -> Option<u64> {
        use windows::Win32::System::Threading::GetProcessTimes;
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let (mut c, mut x, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
            let r = GetProcessTimes(h, &mut c, &mut x, &mut k, &mut u);
            let _ = CloseHandle(h);
            r.ok()?;
            let ft = |f: FILETIME| ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64;
            Some(ft(k) + ft(u))
        }
    }

    /// Penghitung GPU Windows (sama dengan kolom GPU di Task Manager).
    pub struct Gpu {
        query: windows::Win32::System::Performance::PDH_HQUERY,
        util: windows::Win32::System::Performance::PDH_HCOUNTER,
        dedicated: windows::Win32::System::Performance::PDH_HCOUNTER,
        shared: windows::Win32::System::Performance::PDH_HCOUNTER,
    }

    impl Gpu {
        pub fn open() -> Option<Gpu> {
            use windows::core::w;
            use windows::Win32::System::Performance::*;
            unsafe {
                let mut query = PDH_HQUERY::default();
                if PdhOpenQueryW(windows::core::PCWSTR::null(), 0, &mut query) != 0 {
                    return None;
                }
                let mut util = PDH_HCOUNTER::default();
                let mut dedicated = PDH_HCOUNTER::default();
                let mut shared = PDH_HCOUNTER::default();
                if PdhAddEnglishCounterW(query, w!("\\GPU Engine(*)\\Utilization Percentage"), 0, &mut util) != 0 {
                    let _ = PdhCloseQuery(query);
                    return None;
                }
                let _ = PdhAddEnglishCounterW(query, w!("\\GPU Process Memory(*)\\Dedicated Usage"), 0, &mut dedicated);
                let _ = PdhAddEnglishCounterW(query, w!("\\GPU Process Memory(*)\\Shared Usage"), 0, &mut shared);
                let _ = PdhCollectQueryData(query);
                Some(Gpu { query, util, dedicated, shared })
            }
        }

        fn items(counter: windows::Win32::System::Performance::PDH_HCOUNTER, large: bool) -> Vec<(String, f64)> {
            use windows::Win32::System::Performance::*;
            if counter.0.is_null() {
                return Vec::new();
            }
            unsafe {
                let fmt = if large { PDH_FMT_LARGE } else { PDH_FMT_DOUBLE };
                let (mut size, mut count) = (0u32, 0u32);
                let r = PdhGetFormattedCounterArrayW(counter, fmt, &mut size, &mut count, None);
                if r != PDH_MORE_DATA || size == 0 {
                    return Vec::new();
                }
                // Penyangga u64 agar item (berisi pointer dan f64) sejajar.
                let mut buf = vec![0u64; (size as usize).div_ceil(8) + 1];
                let ptr = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
                if PdhGetFormattedCounterArrayW(counter, fmt, &mut size, &mut count, Some(ptr)) != 0 {
                    return Vec::new();
                }
                let items = std::slice::from_raw_parts(ptr, count as usize);
                items
                    .iter()
                    .filter(|it| it.FmtValue.CStatus == 0 && !it.szName.is_null())
                    .map(|it| {
                        let v = if large { it.FmtValue.Anonymous.largeValue as f64 } else { it.FmtValue.Anonymous.doubleValue };
                        (it.szName.to_string().unwrap_or_default(), v)
                    })
                    .collect()
            }
        }

        /// (persen GPU per PID, memori GPU per PID).
        pub fn read(&mut self) -> (HashMap<u32, f64>, HashMap<u32, u64>) {
            use windows::Win32::System::Performance::PdhCollectQueryData;
            unsafe {
                let _ = PdhCollectQueryData(self.query);
            }
            // Seperti Task Manager: per proses diambil jenis mesin GPU yang paling sibuk.
            let mut per: HashMap<(u32, String), f64> = HashMap::new();
            for (name, v) in Self::items(self.util, false) {
                if let Some(pid) = pdh_pid(&name) {
                    let engine = name.rsplit("engtype_").next().unwrap_or("").to_string();
                    *per.entry((pid, engine)).or_default() += v;
                }
            }
            let mut util: HashMap<u32, f64> = HashMap::new();
            for ((pid, _), v) in per {
                let e = util.entry(pid).or_default();
                *e = e.max(v);
            }
            let mut mem: HashMap<u32, u64> = HashMap::new();
            for c in [self.dedicated, self.shared] {
                for (name, v) in Self::items(c, true) {
                    if let Some(pid) = pdh_pid(&name) {
                        *mem.entry(pid).or_default() += v.max(0.0) as u64;
                    }
                }
            }
            (util, mem)
        }
    }

    impl Drop for Gpu {
        fn drop(&mut self) {
            unsafe {
                let _ = windows::Win32::System::Performance::PdhCloseQuery(self.query);
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    /// Linux (pengembangan): hanya proses ini sendiri.
    pub fn processes() -> Vec<(u32, String)> {
        let _ = &KINDS;
        let _ = USER_DATA.get();
        vec![(std::process::id(), "ovner".into())]
    }

    pub fn snapshot() -> Vec<ProcStat> {
        let rss = std::fs::read_to_string("/proc/self/statm")
            .ok()
            .and_then(|s| s.split_whitespace().nth(1).and_then(|v| v.parse::<u64>().ok()))
            .unwrap_or(0)
            * 4096;
        vec![ProcStat { pid: std::process::id(), kind: "ovner".into(), working: rss, commit: rss, ..Default::default() }]
    }

    pub fn cpu_time_100ns(_pid: u32) -> Option<u64> {
        // utime + stime (tick, biasanya 100 per detik) → satuan 100 ns.
        let s = std::fs::read_to_string("/proc/self/stat").ok()?;
        let rest = s.rsplit(") ").next()?;
        let f: Vec<&str> = rest.split_whitespace().collect();
        let ticks = f.get(11)?.parse::<u64>().ok()? + f.get(12)?.parse::<u64>().ok()?;
        Some(ticks * 100_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_command_lines() {
        let ud = r"c:\users\rendi\appdata\local\ovner\webview2";
        let b = r#""C:\Program Files (x86)\Microsoft\EdgeWebView\Application\142.0\msedgewebview2.exe" --embedded-browser-webview=1 --webview-exe-name=ovner.exe --user-data-dir="C:\Users\Rendi\AppData\Local\Ovner\WebView2""#;
        assert_eq!(classify(b, ud).as_deref(), Some("browser"));
        let r = r#"msedgewebview2.exe --type=renderer --user-data-dir="C:\Users\Rendi\AppData\Local\Ovner\WebView2" --renderer-client-id=7"#;
        assert_eq!(classify(r, ud).as_deref(), Some("renderer"));
        let n = r#"msedgewebview2.exe --type=utility --utility-sub-type=network.mojom.NetworkService --user-data-dir="C:\Users\Rendi\AppData\Local\Ovner\WebView2""#;
        assert_eq!(classify(n, ud).as_deref(), Some("network"));
        let g = r#"msedgewebview2.exe --type=gpu-process --user-data-dir="C:\Users\Rendi\AppData\Local\Ovner\WebView2""#;
        assert_eq!(classify(g, ud).as_deref(), Some("gpu"));
        // WebView2 milik aplikasi lain (Teams, Outlook) bukan milik Ovner.
        let other = r#"msedgewebview2.exe --type=renderer --webview-exe-name=ms-teams.exe --user-data-dir="C:\Users\Rendi\AppData\Local\Packages\MSTeams\EBWebView""#;
        assert_eq!(classify(other, ud), None);
        assert_eq!(pdh_pid("pid_1234_luid_0x00000000_0x0000D1B5_phys_0_eng_0_engtype_3D"), Some(1234));
        assert_eq!(pdh_pid("_Total"), None);
    }

    #[test]
    fn cpu_sampler_runs() {
        let mut s = Sampler::new();
        let a = s.sample(false);
        assert_eq!(a.len(), 1);
        let mut x = 0u64;
        for i in 0..3_000_000u64 {
            x = x.wrapping_add(i * i);
        }
        std::hint::black_box(x);
        let b = s.sample(false);
        assert!(b[0].working > 0 && b[0].cpu >= 0.0);
        assert_eq!(total(&b).procs, 1);
    }
}
