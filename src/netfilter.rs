//! Bagian khusus WebView2 (Windows): pemblokir permintaan jaringan, tajuk privasi, dan
//! beberapa fitur yang belum disediakan wry (cek audio, dialog unduhan, kembali/maju).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{take_pwstr, WebResourceRequestedEventHandler};
use windows::core::{Interface, BOOL, HSTRING, PWSTR};
use wry::WebViewExtWindows;

use crate::blocker::{self, Blocker};
use crate::settings::Settings;

/// Telemetri Microsoft Edge/WebView2 dan Office: tidak dibutuhkan halaman mana pun.
const TELEMETRY_HOSTS: &[&str] = &[
    "events.data.microsoft.com",
    "browser.pipe.aria.microsoft.com",
    "mobile.events.data.microsoft.com",
    "edge.microsoft.com",
    "config.edge.skype.com",
    "nav.smartscreen.microsoft.com",
];

/// Data yang dibagi antara tab dan penyaring jaringannya.
#[derive(Clone)]
pub struct FilterCtx {
    pub blocker: Rc<RefCell<Blocker>>,
    pub settings: Rc<RefCell<Settings>>,
    /// Jumlah permintaan yang diblokir di halaman ini.
    pub blocked: Rc<Cell<u32>>,
    /// Alamat navigasi halaman utama yang sedang berjalan (sumber untuk aturan $third-party).
    pub page: Rc<RefCell<String>>,
}

fn host_matches(host: &str, list: &[&str]) -> bool {
    list.iter().any(|h| host == *h || host.ends_with(&format!(".{h}")))
}

/// Pasang penyaring jaringan pada tab ini: pemblokir iklan, blokir telemetri, tajuk GPC/DNT.
pub fn attach(wv: &wry::WebView, ctx: FilterCtx) -> windows::core::Result<()> {
    let core = wv.webview();
    let env = wv.environment();
    unsafe {
        let filter = HSTRING::from("*");
        // API baru juga menangkap permintaan dari iframe dan worker.
        match core.cast::<ICoreWebView2_22>() {
            Ok(w22) => w22.AddWebResourceRequestedFilterWithRequestSourceKinds(
                &filter,
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
            )?,
            Err(_) => core.AddWebResourceRequestedFilter(&filter, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)?,
        }
        let mut token: i64 = 0;
        core.add_WebResourceRequested(
            &WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let req = args.Request()?;
                let mut uri = PWSTR::null();
                req.Uri(&mut uri)?;
                let uri = take_pwstr(uri);
                if !(uri.starts_with("http://") || uri.starts_with("https://")) {
                    return Ok(());
                }
                let (block_ads, gpc, block_telemetry, exception) = {
                    let s = ctx.settings.borrow();
                    let page = ctx.page.borrow();
                    let host = url::Url::parse(&page).ok().and_then(|u| u.host_str().map(str::to_string));
                    (s.block_ads, s.gpc, s.block_telemetry, host.is_some_and(|h| s.is_exception(&h)))
                };
                if gpc {
                    if let Ok(h) = req.Headers() {
                        let _ = h.SetHeader(&HSTRING::from("Sec-GPC"), &HSTRING::from("1"));
                        let _ = h.SetHeader(&HSTRING::from("DNT"), &HSTRING::from("1"));
                    }
                }
                let host = url::Url::parse(&uri).ok().and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase())).unwrap_or_default();
                let mut block = block_telemetry && host_matches(&host, TELEMETRY_HOSTS);
                if !block && block_ads && !exception {
                    let page = ctx.page.borrow().clone();
                    let mut kind = COREWEBVIEW2_WEB_RESOURCE_CONTEXT::default();
                    args.ResourceContext(&mut kind)?;
                    let is_main = kind == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT && (uri == page || page.is_empty());
                    let rtype = blocker::request_type_from_webview2(kind.0, is_main);
                    block = ctx.blocker.borrow().should_block(&uri, &page, rtype);
                }
                if block {
                    let resp = env.CreateWebResourceResponse(None, 403, &HSTRING::from("Diblokir Ovner"), &HSTRING::new())?;
                    args.SetResponse(&resp)?;
                    ctx.blocked.set(ctx.blocked.get() + 1);
                }
                Ok(())
            })),
            &mut token,
        )?;
    }
    Ok(())
}

/// Panggil `cb(berhasil, status_error)` setiap navigasi halaman utama selesai.
pub fn on_navigation_completed(wv: &wry::WebView, cb: impl Fn(bool, i32) + 'static) {
    use webview2_com::NavigationCompletedEventHandler;
    let handler = NavigationCompletedEventHandler::create(Box::new(move |_wv, args| {
        if let Some(a) = args {
            let mut ok = BOOL(0);
            let mut status = COREWEBVIEW2_WEB_ERROR_STATUS(0);
            unsafe {
                let _ = a.IsSuccess(&mut ok);
                let _ = a.WebErrorStatus(&mut status);
            }
            cb(ok.as_bool(), status.0);
        }
        Ok(())
    }));
    let mut token = 0i64;
    unsafe {
        let _ = wv.webview().add_NavigationCompleted(&handler, &mut token);
    }
}

/// Apakah tab ini sedang memutar suara (tab seperti ini tidak ditidurkan).
pub fn is_playing_audio(wv: &wry::WebView) -> bool {
    unsafe {
        let Ok(w8) = wv.webview().cast::<ICoreWebView2_8>() else { return false };
        let mut playing = BOOL::default();
        w8.IsDocumentPlayingAudio(&mut playing).is_ok() && playing.as_bool()
    }
}

/// Tampilkan panel unduhan bawaan WebView2.
pub fn open_downloads(wv: &wry::WebView) {
    unsafe {
        if let Ok(w9) = wv.webview().cast::<ICoreWebView2_9>() {
            let _ = w9.OpenDefaultDownloadDialog();
        }
    }
}

pub fn go_back(wv: &wry::WebView) {
    unsafe {
        let _ = wv.webview().GoBack();
    }
}

pub fn go_forward(wv: &wry::WebView) {
    unsafe {
        let _ = wv.webview().GoForward();
    }
}

pub fn stop(wv: &wry::WebView) {
    unsafe {
        let _ = wv.webview().Stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telemetry_hosts() {
        assert!(host_matches("v10.events.data.microsoft.com", TELEMETRY_HOSTS));
        assert!(!host_matches("www.microsoft.com", TELEMETRY_HOSTS));
    }
}
