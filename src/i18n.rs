//! Dua bahasa untuk teks yang dibuat Rust (menu, tray, dialog, judul halaman internal).
//! Teks di halaman HTML (toolbar, pengaturan, dsb.) punya kamus sendiri di halamannya dan
//! memilih bahasa dari `lang` yang dikirim Ovner.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Id,
    En,
}

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::Id => "id",
            Lang::En => "en",
        }
    }
}

/// Bahasa dari setelan: "id", "en", atau "auto" (bahasa tampilan Windows).
pub fn resolve(setting: &str) -> Lang {
    match setting {
        "id" => Lang::Id,
        "en" => Lang::En,
        _ => system_lang(),
    }
}

/// Bahasa tampilan sistem: Indonesia bila Windows (atau LANG di Linux) berbahasa Indonesia.
pub fn system_lang() -> Lang {
    #[cfg(windows)]
    {
        // LANGID: 10 bit bawah = bahasa utama; 0x21 = Indonesia.
        let id = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() };
        if id & 0x3ff == 0x21 {
            Lang::Id
        } else {
            Lang::En
        }
    }
    #[cfg(not(windows))]
    {
        let l = std::env::var("LANG").unwrap_or_default().to_ascii_lowercase();
        if l.starts_with("id") || l.starts_with("in_") {
            Lang::Id
        } else {
            Lang::En
        }
    }
}

/// Teks untuk `key` dalam bahasa `lang`. Kunci yang tidak dikenal dikembalikan apa adanya.
pub fn t(lang: Lang, key: &str) -> &'static str {
    let (id, en) = match key {
        // Menu utama
        "m.newtab" => ("Tab baru\tCtrl+T", "New tab\tCtrl+T"),
        "m.private" => ("Tab privat baru\tCtrl+Shift+N", "New private tab\tCtrl+Shift+N"),
        "m.reopen" => ("Buka lagi tab yang ditutup\tCtrl+Shift+T", "Reopen closed tab\tCtrl+Shift+T"),
        "m.bookmarks_menu" => ("Bookmark", "Bookmarks"),
        "m.bookmark" => ("Bookmark halaman ini\tCtrl+D", "Bookmark this page\tCtrl+D"),
        "m.bookmark_bar" => ("Tampilkan/sembunyikan bilah bookmark", "Show/hide bookmarks bar"),
        "m.bookmarks" => ("Semua bookmark", "All bookmarks"),
        "m.import" => ("Impor bookmark dari browser lain", "Import bookmarks from another browser"),
        "m.history" => ("Riwayat\tCtrl+H", "History\tCtrl+H"),
        "m.downloads" => ("Unduhan\tCtrl+J", "Downloads\tCtrl+J"),
        "m.zoom" => ("Zoom", "Zoom"),
        "m.zoom_out" => ("Perkecil\tCtrl+−", "Zoom out\tCtrl+−"),
        "m.zoom_reset" => ("Normal (100%)\tCtrl+0", "Reset (100%)\tCtrl+0"),
        "m.zoom_in" => ("Perbesar\tCtrl++", "Zoom in\tCtrl++"),
        "m.print" => ("Cetak...\tCtrl+P", "Print...\tCtrl+P"),
        "m.find" => ("Cari di halaman...\tCtrl+F", "Find in page...\tCtrl+F"),
        "m.split" => ("Bagi layar dengan tab lain", "Split view with another tab"),
        "m.unsplit" => ("Tutup bagi layar", "Close split view"),
        "m.passwords" => ("Sandi dan isi otomatis", "Passwords and autofill"),
        "p.passwords" => ("Sandi", "Passwords"),
        "p.downloads" => ("Unduhan", "Downloads"),
        "d.pw_imported" => ("{n} sandi diimpor.", "{n} passwords imported."),
        "d.pw_export" => (
            "Ekspor semua sandi ke berkas CSV?\n\nBerkas ini TIDAK terenkripsi: siapa pun yang membukanya bisa membaca sandi Anda. Hapus berkasnya setelah dipakai.",
            "Export all passwords to a CSV file?\n\nThe file is NOT encrypted: anyone who opens it can read your passwords. Delete it after use.",
        ),
        "d.pw_clear" => ("Hapus semua sandi yang tersimpan di Ovner?", "Delete all passwords saved in Ovner?"),
        "m.diagnostics" => ("Diagnostik", "Diagnostics"),
        "m.sessions" => ("Sesi tab (simpan/buka)", "Tab sessions (save/open)"),
        "m.close_duplicates" => ("Tutup tab duplikat", "Close duplicate tabs"),
        "p.sessions" => ("Sesi tab", "Tab sessions"),
        "sh.dark" => ("Mode gelap di situs ini", "Dark mode on this site"),
        "sh.copy" => ("Izinkan salin dan klik kanan", "Allow copy and right-click"),
        "sh.nosleep" => ("Jangan tidurkan situs ini", "Never sleep this site"),
        "d.dl_engine_fail" => ("Mesin unduh Ovner gagal ({e}); mengunduh dengan cara biasa.", "Ovner's download engine failed ({e}); downloading the normal way."),
        "p.diagnostics" => ("Diagnostik", "Diagnostics"),
        "d.report_copied" => (
            "Laporan diagnostik sudah disalin. Tempelkan (Ctrl+V) ke percakapan untuk dianalisis.",
            "The diagnostics report has been copied. Paste it (Ctrl+V) into the conversation for analysis.",
        ),
        "m.new_identity" => ("Identitas baru", "New identity"),
        "tm.copy_urls" => ("Salin alamat semua tab", "Copy all tab addresses"),
        "sh.allow_js" => ("Izinkan JavaScript (level keamanan)", "Allow JavaScript (security level)"),
        "d.new_identity" => (
            "Mulai identitas baru?\n\nSemua tab ditutup, lalu cookie, login situs, cache, dan riwayat dihapus. Situs tidak bisa lagi mengenali Anda dari kunjungan sebelumnya.",
            "Start a new identity?\n\nAll tabs are closed, then cookies, site logins, cache and history are cleared. Sites can no longer recognize you from earlier visits.",
        ),
        "m.other_bookmarks" => ("Bookmark lain", "Other bookmarks"),
        "sh.title" => ("Shields untuk {h}", "Shields for {h}"),
        "sh.blocked" => ("{n} iklan/pelacak diblokir di halaman ini", "{n} ads/trackers blocked on this page"),
        "sh.ads" => ("Blokir iklan dan pelacak", "Block ads and trackers"),
        "sh.scripts" => ("Blokir JavaScript", "Block JavaScript"),
        "sh.fp" => ("Acak sidik jari", "Randomize fingerprint"),
        "sh.pick" => ("Sembunyikan elemen...", "Hide an element..."),
        "sh.settings" => ("Pengaturan Shields", "Shields settings"),
        "m.tools" => ("Alat lain", "More tools"),
        "m.tasks" => ("Task manager\tShift+Esc", "Task manager\tShift+Esc"),
        "m.sleep_all" => ("Tidurkan semua tab lain", "Sleep all other tabs"),
        "m.trim" => ("Rapikan RAM sekarang", "Trim memory now"),
        "m.pick" => ("Pilih elemen untuk disembunyikan", "Pick an element to hide"),
        "m.filters" => ("Perbarui daftar filter iklan", "Update ad filter lists"),
        "m.clear" => ("Hapus data penjelajahan...", "Clear browsing data..."),
        "m.data" => ("Buka folder data", "Open data folder"),
        "m.devtools" => ("DevTools\tF12", "DevTools\tF12"),
        "m.settings" => ("Pengaturan", "Settings"),
        "m.about" => ("Tentang Ovner", "About Ovner"),
        "m.check_update" => ("Periksa dan pasang update", "Check for updates and install"),
        "d.update_now" => (
            "Ovner {v} sudah diunduh. Pasang sekarang? Ovner dibuka ulang dan tab dipulihkan.",
            "Ovner {v} has been downloaded. Install now? Ovner restarts and your tabs are restored."
        ),
        "d.about_check" => ("\n\nPeriksa update sekarang?", "\n\nCheck for updates now?"),
        "d.checking" => ("Memeriksa update…", "Checking for updates…"),
        "d.video_tools" => ("Menyiapkan komponen {t} (sekali saja)…", "Preparing component {t} (one time)…"),
        "sh.record" => ("● Mulai rekam masalah halaman ini", "● Start recording problems on this page"),
        "sh.record_stop" => ("■ Selesai rekam dan salin laporan ({n} kejadian)", "■ Finish recording and copy report ({n} events)"),
        "sh.record_copy" => ("Salin laporan masalah halaman", "Copy page problem report"),
        "d.record_started" => (
            "Perekaman dimulai dan halaman dimuat ulang. Ovner merekam terus (juga saat pindah halaman) sampai Anda membuka menu tameng → Selesai rekam dan salin laporan, lalu tempel hasilnya ke chat.",
            "Recording started and the page reloaded. Ovner keeps recording (also across page changes) until you open the shield menu → Finish recording and copy report, then paste it."
        ),
        "d.record_copied" => ("Laporan masalah halaman ({n} catatan) disalin ke clipboard.", "Page problem report ({n} entries) copied to the clipboard."),
        "sh.clear_site" => ("Hapus cookie dan data situs ini (keluar dari situs)", "Clear cookies and data for this site (signs you out)"),
        "d.clear_site" => (
            "Hapus cookie, sesi login, dan data tersimpan {h}? Anda perlu masuk lagi ke situs ini.",
            "Clear cookies, login session and stored data for {h}? You will need to sign in again."
        ),
        "m.hide" => ("Sembunyikan ke tray", "Hide to tray"),
        "m.quit" => ("Keluar", "Exit"),
        // Menu tab
        "tm.reload" => ("Muat ulang", "Reload"),
        "tm.duplicate" => ("Duplikat tab", "Duplicate tab"),
        "tm.sleep" => ("Tidurkan tab", "Sleep tab"),
        "tm.split" => ("Bagi layar dengan tab ini", "Split view with this tab"),
        "tm.close" => ("Tutup tab", "Close tab"),
        "tm.close_others" => ("Tutup tab lain", "Close other tabs"),
        // Tray
        "tray.show" => ("Buka Ovner", "Open Ovner"),
        "tray.newtab" => ("Tab baru", "New tab"),
        // Judul halaman
        "p.newtab" => ("Tab baru", "New tab"),
        "p.settings" => ("Pengaturan", "Settings"),
        "p.history" => ("Riwayat", "History"),
        "p.bookmarks" => ("Bookmark", "Bookmarks"),
        "p.tasks" => ("Task manager", "Task manager"),
        "p.setup" => ("Selamat datang", "Welcome"),
        // Dialog
        "d.clear" => (
            "Hapus semua data penjelajahan (riwayat, cookie, login situs, cache)?\n\nAnda akan keluar dari semua situs.",
            "Clear all browsing data (history, cookies, site logins, cache)?\n\nYou will be signed out of all sites.",
        ),
        "d.filters" => (
            "Daftar filter iklan sedang diperbarui di latar. Halaman yang dibuka berikutnya memakai daftar terbaru.",
            "Ad filter lists are updating in the background. Pages opened next will use the new lists.",
        ),
        "d.uninstall" => (
            "Hapus Ovner dari komputer ini?\n\nRiwayat, bookmark, cookie, dan pengaturan Ovner juga akan dihapus.",
            "Remove Ovner from this computer?\n\nOvner's history, bookmarks, cookies and settings will be deleted too.",
        ),
        "d.uninstalled" => ("Ovner sudah dihapus.", "Ovner has been removed."),
        "d.uninstalled_portable" => (
            "Data Ovner sudah dihapus. File exe portabel bisa Anda hapus sendiri.",
            "Ovner's data has been deleted. You can delete the portable exe yourself.",
        ),
        "d.install_failed" => (
            "Ovner tidak bisa dipasang ({e}).\n\nOvner dijalankan langsung dari file ini.",
            "Ovner could not be installed ({e}).\n\nRunning Ovner directly from this file.",
        ),
        "d.need_webview2" => (
            "Ovner butuh Microsoft Edge WebView2 Runtime (sudah ada di Windows 11).\n\nGalat: {e}",
            "Ovner needs the Microsoft Edge WebView2 Runtime (included in Windows 11).\n\nError: {e}",
        ),
        "d.up_to_date" => ("Ovner sudah versi terbaru.", "Ovner is up to date."),
        "d.update_failed" => ("Tidak bisa memeriksa update: {e}", "Could not check for updates: {e}"),
        "d.unresponsive" => (
            "Halaman \"{t}\" tidak merespons.\n\nMuat ulang halaman ini?",
            "The page \"{t}\" is not responding.\n\nReload this page?",
        ),
        "d.imported" => ("{n} bookmark diimpor dari {b}.", "Imported {n} bookmarks from {b}."),
        "d.import_none" => (
            "Tidak ditemukan bookmark Chrome, Edge, atau Brave di komputer ini.",
            "No Chrome, Edge or Brave bookmarks were found on this computer.",
        ),
        "d.devtools_off" => (
            "DevTools dimatikan. Nyalakan di Pengaturan → Sistem.",
            "DevTools are turned off. Turn them on in Settings → System.",
        ),
        "d.about" => (
            "Ovner {v}\n\nMesin: Microsoft Edge WebView2 {e}\nPemblokir iklan: adblock-rust (Brave), {l}\nData: {d}\n\nhttps://github.com/{r}",
            "Ovner {v}\n\nEngine: Microsoft Edge WebView2 {e}\nAd blocker: adblock-rust (Brave), {l}\nData: {d}\n\nhttps://github.com/{r}",
        ),
        "d.lists_full" => ("daftar lengkap", "full lists"),
        "d.lists_builtin" => ("daftar bawaan (daftar lengkap sedang diunduh)", "built-in list (full lists downloading)"),
        _ => return Box::leak(key.to_string().into_boxed_str()),
    };
    match lang {
        Lang::Id => id,
        Lang::En => en,
    }
}

/// Seperti `t`, lalu mengganti `{nama}` dengan nilai.
pub fn tf(lang: Lang, key: &str, args: &[(&str, &str)]) -> String {
    let mut s = t(lang, key).to_string();
    for (k, v) in args {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_languages_and_format() {
        assert_eq!(t(Lang::Id, "m.quit"), "Keluar");
        assert_eq!(t(Lang::En, "m.quit"), "Exit");
        assert_eq!(tf(Lang::En, "d.imported", &[("n", "3"), ("b", "Edge")]), "Imported 3 bookmarks from Edge.");
        assert_eq!(resolve("id"), Lang::Id);
        assert_eq!(resolve("en"), Lang::En);
    }
}
