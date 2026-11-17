//! Bagian khusus WebView2 (Windows): pemblokir permintaan jaringan, tajuk privasi, dan
//! beberapa fitur yang belum disediakan wry (cek audio, dialog unduhan, kembali/maju).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    take_pwstr, AddScriptToExecuteOnDocumentCreatedCompletedHandler, NavigationStartingEventHandler, WebResourceRequestedEventHandler,
};
use windows::core::{Interface, BOOL, HSTRING, PWSTR};
use wry::WebViewExtWindows;

use crate::blocker::{self, Blocker};
use crate::settings::Settings;
use crate::shields::{decode_data_url, is_local_target};

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
    /// Cache CNAME (host → nama asli), dibagi semua tab. `None` = sedang/tidak ada.
    pub cname: Arc<Mutex<HashMap<String, Option<String>>>>,
}

/// Domain dasar perkiraan (eTLD+1): `a.b.kompas.com` → `kompas.com`, `x.co.id` → `x.co.id`.
pub fn base_domain(host: &str) -> String {
    let parts: Vec<&str> = host.trim_end_matches('.').split('.').collect();
    if parts.len() <= 2 {
        return host.to_string();
    }
    let n = parts.len();
    let take = if parts[n - 2].len() <= 3 && parts[n - 1].len() == 2 { 3 } else { 2 };
    parts[n.saturating_sub(take)..].join(".")
}

/// Pemeriksaan CNAME untuk subdomain situs sendiri (tempat pelacak biasa menyamar).
/// Pencarian DNS dilakukan di thread latar; permintaan pertama ke host baru dibiarkan lewat.
fn cname_blocked(ctx: &FilterCtx, uri: &str, host: &str, page: &str, rtype: &str) -> bool {
    let page_host = url::Url::parse(page).ok().and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase())).unwrap_or_default();
    if host == page_host || page_host.is_empty() || base_domain(host) != base_domain(&page_host) {
        return false;
    }
    let cached = ctx.cname.lock().ok().and_then(|m| m.get(host).cloned());
    match cached {
        Some(Some(cname)) => ctx.blocker.borrow().should_block_cname(uri, &cname, page, rtype),
        Some(None) => false,
        None => {
            if let Ok(mut m) = ctx.cname.lock() {
                m.insert(host.to_string(), None);
            }
            let (cache, h) = (ctx.cname.clone(), host.to_string());
            std::thread::spawn(move || {
                let c = crate::win::cname_of(&h).filter(|c| base_domain(c) != base_domain(&h));
                if let Ok(mut m) = cache.lock() {
                    m.insert(h, c);
                }
            });
            false
        }
    }
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
                // Aset tantangan anti-bot Anubis (Startpage, dsb.) tidak pernah diblokir.
                if uri.contains("/.within.website/") {
                    return Ok(());
                }
                let page = ctx.page.borrow().clone();
                let page_host = url::Url::parse(&page).ok().and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase())).unwrap_or_default();
                let (block_ads, aggressive, gpc, block_telemetry, exception, cname_on, local_block, referrer) = {
                    let s = ctx.settings.borrow();
                    (
                        s.block_ads,
                        s.blocking_mode == "aggressive",
                        s.gpc,
                        s.block_telemetry,
                        !page_host.is_empty() && s.is_exception(&page_host),
                        s.cname_uncloak,
                        s.block_local_network,
                        s.referrer_policy.clone(),
                    )
                };
                let host = url::Url::parse(&uri).ok().and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase())).unwrap_or_default();
                if let Ok(h) = req.Headers() {
                    // Referrer ke situs lain dipangkas (seperti LibreWolf/Waterfox), hanya untuk
                    // navigasi dan iframe: permintaan video/API (googlevideo.com dari YouTube)
                    // butuh referrer asal situs agar tetap diputar.
                    let mut kind = COREWEBVIEW2_WEB_RESOURCE_CONTEXT::default();
                    let _ = args.ResourceContext(&mut kind);
                    let navigation = kind == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT;
                    // GPC/DNT hanya pada navigasi halaman. Tajuk tambahan pada permintaan API
                    // lintas situs (fetch/XHR) membuat preflight CORS ditolak server, sehingga
                    // aplikasi seperti Infor dan SIPP BPJS menunggu jawaban selamanya.
                    if gpc && navigation {
                        let _ = h.SetHeader(&HSTRING::from("Sec-GPC"), &HSTRING::from("1"));
                        let _ = h.SetHeader(&HSTRING::from("DNT"), &HSTRING::from("1"));
                    }
                    if navigation && referrer != "default" && !page_host.is_empty() && base_domain(&host) != base_domain(&page_host) {
                        let name = HSTRING::from("Referer");
                        let mut has = BOOL(0);
                        if h.Contains(&name, &mut has).is_ok() && has.as_bool() {
                            if referrer == "none" {
                                let _ = h.RemoveHeader(&name);
                            } else if let Ok(origin) = url::Url::parse(&page).map(|u| u.origin().ascii_serialization()) {
                                let _ = h.SetHeader(&name, &HSTRING::from(format!("{origin}/")));
                            }
                        }
                    }
                }
                // Situs publik yang mencoba menghubungi jaringan lokal/komputer ini.
                let mut block = local_block && is_local_target(&host) && !page_host.is_empty() && !is_local_target(&page_host);
                block |= block_telemetry && host_matches(&host, TELEMETRY_HOSTS);
                let mut redirect = None;
                let mut rewritten = None;
                if !block && block_ads && !exception {
                    let mut kind = COREWEBVIEW2_WEB_RESOURCE_CONTEXT::default();
                    args.ResourceContext(&mut kind)?;
                    let is_main = kind == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT && (uri == page || page.is_empty());
                    let rtype = blocker::request_type_from_webview2(kind.0, is_main);
                    // Mode Standar (seperti Brave): sumber daya dari situs itu sendiri (pihak
                    // pertama) tidak diblokir. Aplikasi kantor (SIPP BPJS, Infor) memuat skrip dan
                    // API dari domainnya sendiri yang namanya mirip pelacak (/analytics, /track);
                    // memblokirnya membuat halaman berputar "Processing..." selamanya.
                    let first_party = !page_host.is_empty() && base_domain(&host) == base_domain(&page_host);
                    let document = kind == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT;
                    if aggressive || !first_party || document {
                        let v = ctx.blocker.borrow().check(&uri, &page, rtype);
                        block = v.block;
                        redirect = v.redirect;
                        rewritten = v.rewritten.filter(|_| !is_main);
                    }
                    if !block && cname_on {
                        block = cname_blocked(&ctx, &uri, &host, &page, rtype);
                    }
                }
                if let Some((mime, bytes)) = redirect.as_deref().filter(|_| block).and_then(decode_data_url) {
                    // Resource pengganti (mis. google-ima.js kosong): pemutar video tetap jalan.
                    let stream = windows::Win32::UI::Shell::SHCreateMemStream(Some(&bytes));
                    let headers = format!("Content-Type: {mime}\r\nAccess-Control-Allow-Origin: *");
                    let resp = env.CreateWebResourceResponse(stream.as_ref(), 200, &HSTRING::from("OK"), &HSTRING::from(headers))?;
                    args.SetResponse(&resp)?;
                    ctx.blocked.set(ctx.blocked.get() + 1);
                } else if block {
                    let resp = env.CreateWebResourceResponse(None, 403, &HSTRING::from("Diblokir Ovner"), &HSTRING::new())?;
                    args.SetResponse(&resp)?;
                    ctx.blocked.set(ctx.blocked.get() + 1);
                } else if let Some(to) = rewritten {
                    // $removeparam: alihkan ke alamat tanpa parameter pelacak.
                    let headers = format!("Location: {to}\r\nAccess-Control-Allow-Origin: *");
                    let resp = env.CreateWebResourceResponse(None, 307, &HSTRING::from("Temporary Redirect"), &HSTRING::from(headers))?;
                    args.SetResponse(&resp)?;
                }
                Ok(())
            })),
            &mut token,
        )?;
    }
    Ok(())
}

/// Suntikkan scriptlet (+js) sedini mungkin: saat navigasi halaman atau iframe dimulai,
/// skrip untuk alamat tujuannya didaftarkan sebagai skrip awal dokumen (tidak terkena CSP
/// situs). Skrip dijaga nama host-nya, dan semua dibuang lagi saat halaman utama berganti.
pub fn attach_scriptlets(wv: &wry::WebView, ctx: FilterCtx) {
    let core = wv.webview();
    let ids: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let add = {
        let ctx = ctx.clone();
        let ids = ids.clone();
        Rc::new(move |core: &ICoreWebView2, uri: &str, main: bool| {
            let page_host = url::Url::parse(uri).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
            let (on, features) = {
                let s = ctx.settings.borrow();
                let on = s.block_ads && !page_host.is_empty() && !s.is_exception(&page_host);
                // Fitur per situs (mode gelap, izinkan salin): ditentukan oleh situs halaman utama.
                let features = if main {
                    crate::shields::site_feature_script(&s, &crate::settings::norm_host(&page_host))
                } else {
                    let top = url::Url::parse(&ctx.page.borrow()).ok().and_then(|u| u.host_str().map(crate::settings::norm_host)).unwrap_or_default();
                    if s.allow_copy_for(&top) { crate::shields::allow_copy_script().to_string() } else { String::new() }
                };
                (on, features)
            };
            let mut script = if on { ctx.blocker.borrow().scriptlets(uri) } else { String::new() };
            if !features.is_empty() {
                script.push('\n');
                script.push_str(&features);
            }
            if script.trim().is_empty() {
                return;
            }
            let host = serde_json::to_string(&page_host).unwrap_or_default();
            let js = format!("(function(){{if(location.hostname!=={host})return;\n{script}\n}})();");
            let ids = ids.clone();
            unsafe {
                let _ = core.AddScriptToExecuteOnDocumentCreated(
                    &HSTRING::from(js),
                    &AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(move |r, id| {
                        if r.is_ok() {
                            ids.borrow_mut().push(id);
                        }
                        Ok(())
                    })),
                );
            }
        })
    };
    let uri_of = |args: &ICoreWebView2NavigationStartingEventArgs| -> String {
        let mut p = PWSTR::null();
        unsafe {
            if args.Uri(&mut p).is_ok() {
                return take_pwstr(p);
            }
        }
        String::new()
    };
    let main = {
        let (add, ids) = (add.clone(), ids.clone());
        let ctx2 = ctx.clone();
        NavigationStartingEventHandler::create(Box::new(move |sender, args| {
            let (Some(core), Some(args)) = (sender, args) else { return Ok(()) };
            let uri = uri_of(&args);
            if !(uri.starts_with("http://") || uri.starts_with("https://")) {
                return Ok(());
            }
            // Identitas tab tidak pernah diganti di tengah jalan: situs seperti Startpage (Anubis)
            // menolak browser yang user agent-nya berubah antarpermintaan.
            let host = url::Url::parse(&uri).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
            let s = ctx2.settings.borrow().clone();
            // Mode gelap Chromium per situs (tanpa skrip, tanpa buka ulang Ovner).
            let (m, p) = crate::shields::dark_cdp(&s, &crate::settings::norm_host(&host));
            crate::win::cdp_core(&core, m, p);
            for id in ids.borrow_mut().drain(..) {
                unsafe {
                    let _ = core.RemoveScriptToExecuteOnDocumentCreated(&HSTRING::from(id));
                }
            }
            (*add)(&core, &uri, true);
            Ok(())
        }))
    };
    // ICoreWebView2::add_FrameNavigationStarting memakai jenis handler yang sama.
    let frame = NavigationStartingEventHandler::create(Box::new(move |sender, args| {
        let (Some(core), Some(args)) = (sender, args) else { return Ok(()) };
        let uri = uri_of(&args);
        if uri.starts_with("http://") || uri.starts_with("https://") {
            (*add)(&core, &uri, false);
        }
        Ok(())
    }));
    let mut token = 0i64;
    unsafe {
        let _ = core.add_NavigationStarting(&main, &mut token);
        let _ = core.add_FrameNavigationStarting(&frame, &mut token);
    }
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
    fn base_domains() {
        assert_eq!(base_domain("metrics.news.kompas.com"), "kompas.com");
        assert_eq!(base_domain("www.bank.co.id"), "bank.co.id");
        assert_eq!(base_domain("detik.com"), "detik.com");
    }

    #[test]
    fn telemetry_hosts() {
        assert!(host_matches("v10.events.data.microsoft.com", TELEMETRY_HOSTS));
        assert!(!host_matches("www.microsoft.com", TELEMETRY_HOSTS));
    }
}
