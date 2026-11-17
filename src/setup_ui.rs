//! Jendela pemasang `Ovner-Setup.exe`: pilih **Install** (Program Files, admin) atau
//! **Portable** (tanpa admin), plus "Perbarui instalasi saat ini" bila ada instalasi
//! per-pengguna lama. Tampilannya halaman HTML kecil (src/ui/installer.html) di WebView2.

use std::path::{Path, PathBuf};

/// Pilihan pengguna di jendela pemasang.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Program Files untuk semua pengguna (UAC).
    Machine { desktop: bool },
    /// Folder pilihan, tanpa admin.
    Portable { dir: PathBuf, desktop: bool },
    /// Perbarui instalasi per-pengguna yang sudah ada (AppData, tanpa admin).
    User,
}

/// Terjemahkan pesan halaman pemasang menjadi pilihan.
pub fn parse_action(msg: &serde_json::Value) -> Option<Action> {
    if msg["t"] != "install" {
        return None;
    }
    let desktop = msg["desktop"].as_bool().unwrap_or(true);
    match msg["mode"].as_str()? {
        "machine" => Some(Action::Machine { desktop }),
        "user" => Some(Action::User),
        "portable" => {
            let dir = msg["dir"].as_str()?.trim();
            (!dir.is_empty()).then(|| Action::Portable { dir: PathBuf::from(dir), desktop })
        }
        _ => None,
    }
}

/// Isi halaman: data awal disisipkan sebagai JSON.
pub fn page(lang: &str, portable_dir: &Path, machine: Option<&str>, user: Option<&str>, elevated: bool) -> String {
    const HTML: &str = include_str!("ui/installer.html");
    let cfg = serde_json::json!({
        "lang": lang,
        "version": env!("CARGO_PKG_VERSION"),
        "portableDir": portable_dir.display().to_string(),
        "machine": machine,
        "user": user,
        "elevated": elevated,
    });
    let (Some(a), Some(b)) = (HTML.find("/*CONFIG*/"), HTML.find("/*END*/")) else { return HTML.to_string() };
    // `</` di dalam JSON tidak boleh menutup tag <script>.
    let json = cfg.to_string().replace("</", "<\\/");
    format!("{}{}{}", &HTML[..a], json, &HTML[b + "/*END*/".len()..])
}

/// Tampilkan pemasang. `install(pilihan)` mengerjakan pemasangan dan mengembalikan exe yang
/// harus dijalankan; galatnya ditampilkan di jendela (pengguna bisa memilih lagi).
/// Hasil: exe terpasang, atau None bila dibatalkan / WebView2 tidak bisa dipakai (Err).
#[cfg(windows)]
pub fn run(page_html: String, mut install: impl FnMut(Action) -> Result<PathBuf, String>) -> Result<Option<PathBuf>, String> {
    use tao::dpi::LogicalSize;
    use tao::event::{Event, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoopBuilder};
    use tao::platform::run_return::EventLoopExtRunReturn;
    use tao::platform::windows::WindowExtWindows;
    use tao::window::{Icon, WindowBuilder};

    let mut event_loop = EventLoopBuilder::<String>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let window = WindowBuilder::new()
        .with_title("Ovner Setup")
        .with_inner_size(LogicalSize::new(600.0, 600.0))
        .with_min_inner_size(LogicalSize::new(520.0, 560.0))
        .with_window_icon(Icon::from_rgba(crate::icon::rgba(64), 64, 64).ok())
        .build(&event_loop)
        .map_err(|e| e.to_string())?;
    if let Some(m) = window.current_monitor() {
        let (ms, mp, ws) = (m.size(), m.position(), window.outer_size());
        let x = mp.x + (ms.width as i32 - ws.width as i32) / 2;
        let y = mp.y + (ms.height as i32 - ws.height as i32) / 2;
        window.set_outer_position(tao::dpi::PhysicalPosition::new(x.max(mp.x), y.max(mp.y)));
    }
    // Data WebView2 pemasang di folder sementara, bukan di sebelah Setup (Downloads).
    let tmp = std::env::temp_dir().join("ovner-setup-webview");
    let mut ctx = wry::WebContext::new(Some(tmp.clone()));
    let webview = wry::WebViewBuilder::new_with_web_context(&mut ctx)
        .with_html(page_html)
        .with_ipc_handler(move |req| {
            let _ = proxy.send_event(req.body().clone());
        })
        .build(&window)
        .map_err(|e| e.to_string())?;
    let hwnd = window.hwnd();
    let mut result: Option<PathBuf> = None;
    event_loop.run_return(|event, _, flow| {
        *flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => *flow = ControlFlow::Exit,
            Event::UserEvent(text) => {
                let msg: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
                match msg["t"].as_str() {
                    Some("cancel") => *flow = ControlFlow::Exit,
                    Some("browse") => {
                        let start = PathBuf::from(msg["dir"].as_str().unwrap_or(""));
                        if let Some(d) = crate::win::pick_folder(Some(hwnd), "Ovner Portable", &start) {
                            // Folder pilihan: pakai subfolder "Ovner Portable" kecuali sudah bernama Ovner.
                            let name = d.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
                            let d = if name.contains("ovner") { d } else { d.join("Ovner Portable") };
                            let _ = webview.evaluate_script(&format!("setDir({})", serde_json::json!(d.display().to_string())));
                        }
                    }
                    Some("install") => match parse_action(&msg) {
                        Some(a) => match install(a) {
                            Ok(exe) => {
                                result = Some(exe);
                                *flow = ControlFlow::Exit;
                            }
                            Err(e) => {
                                let _ = webview.evaluate_script(&format!("setStatus({}, true)", serde_json::json!(e)));
                            }
                        },
                        None => {
                            let _ = webview.evaluate_script("setStatus('?', true)");
                        }
                    },
                    _ => {}
                }
            }
            _ => {}
        }
    });
    drop(webview);
    drop(window);
    drop(ctx);
    // Hapus data WebView2 sementara setelah prosesnya berhenti.
    crate::win::delete_dirs_later(&[&tmp]);
    Ok(result)
}

#[cfg(not(windows))]
pub fn run(_page_html: String, _install: impl FnMut(Action) -> Result<PathBuf, String>) -> Result<Option<PathBuf>, String> {
    Err("pemasang hanya untuk Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_from_page() {
        let v = serde_json::json!({"t": "install", "mode": "portable", "dir": " D:\\Ovner ", "desktop": false});
        assert_eq!(parse_action(&v), Some(Action::Portable { dir: PathBuf::from("D:\\Ovner"), desktop: false }));
        let v = serde_json::json!({"t": "install", "mode": "portable", "dir": "  "});
        assert_eq!(parse_action(&v), None);
        let v = serde_json::json!({"t": "install", "mode": "machine", "desktop": true});
        assert_eq!(parse_action(&v), Some(Action::Machine { desktop: true }));
        assert_eq!(parse_action(&serde_json::json!({"t": "install", "mode": "user"})), Some(Action::User));
        assert_eq!(parse_action(&serde_json::json!({"t": "cancel"})), None);
    }

    #[test]
    fn page_has_config() {
        let p = page("en", Path::new("C:\\x</script>"), Some("0.8.8"), None, false);
        assert!(p.contains("\"machine\":\"0.8.8\""));
        assert!(p.contains("x<\\/script>"));
        assert!(!p.contains("/*CONFIG*/"));
    }
}
