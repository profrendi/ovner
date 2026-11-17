# Ovner

**Bahasa Indonesia** · [English](#english)

Browser ringan untuk Windows 10/11, ditulis dengan Rust. Tampilan tersedia dalam
**Bahasa Indonesia dan English** (Pengaturan → Bahasa; bawaan mengikuti bahasa Windows).

- **Mesin**: Microsoft Edge WebView2, mesin Chromium yang sudah ada di Windows 11. Semua situs
  (YouTube, Office online, Outlook web, situs bank) tampil seperti di Edge.
- **Shields ala Brave**: [adblock-rust](https://github.com/brave/adblock-rust) (mesin Brave
  Shields) dengan EasyList, EasyPrivacy, ABPindo, uBlock; blokir popup persetujuan cookie;
  acak sidik jari (*farbling*); blokir JavaScript per situs; pemilih elemen; daftar dan
  aturan filter sendiri; buang parameter pelacak, lewati link pengalih (debounce), de-AMP;
  ungkap pelacak yang menyamar lewat CNAME (eksperimen).
- **Hemat RAM**: tab tidur otomatis, mode hemat saat diminimalkan/di tray seperti Outlook
  Lite dan ZFPlus, batas jumlah proses halaman, task manager dengan RAM per tab.
- **Tanpa hak admin**: terpasang per pengguna, runtime Visual C++ sudah tertanam di exe.

## Pasang

1. Unduh **Ovner-Setup.exe** dari [Releases](https://github.com/profrendi/ovner/releases/latest).
2. Jalankan. Ovner menyalin dirinya ke `%LOCALAPPDATA%\Programs\Ovner`, membuat ikon di
   **Start Menu** dan **Desktop**, lalu membuka halaman selamat datang (bahasa, mesin pencari,
   Shields, impor bookmark dari Chrome/Edge/Brave).
3. Kalau Windows SmartScreen muncul: **More info → Run anyway** (exe belum ditandatangani).

Ovner memeriksa versi baru saat dibuka; tombol hijau **Update** muncul di toolbar bila ada.
Ada dua kanal (Pengaturan → Sistem → Kanal update):

- **Stabil**: rilis `ovner-vX.Y.Z`.
- **Nightly**: prarilis [`ovner-nightly`](https://github.com/profrendi/ovner/releases/tag/ovner-nightly),
  dibangun ulang setiap ada perubahan. Fitur baru datang lebih dulu, mungkin kurang stabil.

Hapus lewat **Settings → Apps → Installed apps → Ovner → Uninstall**.

## Versi portabel

Unduh **Ovner-Portable.zip**, ekstrak ke mana saja (termasuk flashdisk), jalankan `ovner.exe`.
File `portable.txt` membuat semua data disimpan di folder `Data` di sebelah exe.
Catatan: sandi dan cookie yang disimpan WebView2 dienkripsi dengan akun Windows, jadi login
situs tidak ikut pindah ke komputer lain; bookmark, riwayat, dan pengaturan ikut.

## Fitur baru di 0.5

- **Pengelola bookmark seperti Brave** (☰ → Bookmark → Kelola, atau `ovner://bookmarks`):
  pohon folder, daftar per folder, cari, urutkan berdasarkan nama, tambah bookmark/folder,
  edit/pindah folder/ganti nama/hapus, impor dari Chrome/Edge/Brave atau berkas HTML, ekspor
  HTML. Menu ☰ → Bookmark juga menampilkan semua bookmark dan foldernya.
- **Ikon situs** (favicon) di tab, bilah bookmark, pengelola bookmark, dan saran alamat.
- **Pengelola sandi** (☰ → Sandi dan isi otomatis, `ovner://passwords`): tawaran simpan dan
  perbarui sandi di kanan atas, isi otomatis dan masuk otomatis, daftar per situs, lihat/salin/
  edit/hapus, impor/ekspor CSV Brave/Chrome. Sandi dienkripsi DPAPI (terikat akun Windows).
  Isi otomatis alamat dan kartu memakai fitur WebView2.
- **Kolom alamat seperti Brave**: daftar saran di bawah kolom (pencarian, riwayat, bookmark),
  panah atas/bawah, dan isi otomatis alamat saat mengetik (`sho` → `shopee.co.id`).
- **Unduhan seperti Brave**: gelembung unduhan di kanan atas dengan progres, tombol unduhan di
  toolbar, dan halaman `ovner://downloads` (Ctrl+J) dengan riwayat, buka berkas, tampilkan di
  folder, salin tautan, batalkan.
- **Task manager**: RAM per tab dan semua proses WebView2 kini terbaca; kesalahan tak terduga
  dicatat ke `ovner.log`.
- **YouTube**: level keamanan tidak lagi mematikan JIT JavaScript (penyebab video hitam/lambat
  di preset Maksimal; kini setelan terpisah), referrer hanya dipangkas untuk navigasi, dan situs
  video tidak terkena trik sidik jari yang berat. Mode GPU *Hemat* kini hanya memindahkan gambar
  halaman ke CPU; untuk video tetap disarankan *Penuh*.

## Perbaikan 0.8.8

- **Akar masalah crash saat mengunduh kartu PDF SIPP diperbaiki.** Sejak 0.8.6, PDF diubah menjadi
  unduhan WebView2 dengan mengganti tajuk respons (`Content-Disposition: attachment`) lewat
  DevTools. Pada PDF buatan server yang dibuka dari tab pop-up, cara itu membuat mesin WebView2
  mati (semua tab ikut mati) atau unduhannya macet di 0 KB dan tidak bisa dibatalkan. Kini isi PDF
  dibaca Ovner langsung dari respons yang sudah ada (`Fetch.takeResponseBodyAsStream` + `IO.read`)
  dan ditulis sendiri ke folder Unduhan (Documents): tidak ada unduhan WebView2, server tidak
  membuat PDF dua kali. Tab pop-up menampilkan "PDF disimpan" lalu ditutup otomatis.
- **Unduhan yang macet selalu bisa dibatalkan**: bila WebView2 tidak menanggapi, Ovner tetap
  menandainya batal dan menghapus sisa berkasnya, sehingga bisa dihapus dari daftar.
- **Log crash lebih rinci**: saat proses WebView2 mati, log mencatat alasan, kode keluar, proses
  dan modul penyebab, serta nama laporan crash (`EBWebView\Crashpad\reports\*.dmp`).
- Kolom koneksi PDF tertulis "1 (Ovner, PDF)"; satuan waktu mengikuti bahasa (s/h untuk English).

## Perbaikan 0.8.7

- **Kartu PDF SIPP tidak lagi membuat mesin WebView2 mati / macet di 0 KB.** PDF yang diubah
  menjadi unduhan kini langsung diteruskan dari respons yang sudah ada, tanpa ditahan untuk diuji
  mesin unduh Ovner (pengujian itu membuat server membuat ulang PDF ±20–30 detik).
- **Saat mesin WebView2 mati, Ovner dibuka ulang sekali saja** (dulu tiap tab membuka ulang
  sendiri sehingga muncul 5 jendela).
- **Catatan unduhan disimpan sejak unduhan dimulai** (tidak hilang bila Ovner mati), dan sisa
  berkas setengah jadi (`.ovnerpart`, `.crdownload`) dari unduhan yang terputus dihapus saat
  Ovner dibuka lagi.

## Perbaikan 0.8.6

- **Unduhan Parrot tidak lagi batal di ±2,2 GB.** CDN mirror Parrot (bunny.deb.parrot.sh)
  mulai menjawab HTTP 428 (minta prasyarat) setelah ±6 menit. Ovner kini mengirim `If-Range`
  (ETag/Last-Modified berkas) saat diminta, dan bila semua koneksi ditolak ia menunggu lalu
  melanjutkan dengan separuh koneksi (sampai 8 putaran), tanpa membuang yang sudah diunduh.
- **PDF besar langsung diunduh, tidak dibuka di tab** (Setelan → Unduhan, bisa dimatikan; batas
  bawaan 5 MB). Contoh: "Download Kartu" SIPP BPJS Ketenagakerjaan (522 kartu, ±19 MB) yang di
  penampil PDF bawaan sering lama lalu kosong/hitam. Tab pop-up kosong yang memicu unduhan
  ditutup sendiri dan Ovner kembali ke tab sebelumnya.

## Perbaikan 0.8.5

- **Unduhan bersegmen tidak lagi gagal di tengah jalan lalu mulai dari nol.** Semua koneksi kini
  memakai mirror yang sama (hasil pengalihan pertama; server seperti Parrot mengirim tiap
  permintaan ke mirror berbeda), tiap bagian boleh gagal 20 kali dengan jeda makin lama, hitungan
  gagal direset setiap ada data, dan unduhan yang sudah berjalan tidak lagi diulang dari nol
  lewat WebView2.
- **Berkas tidak bisa rusak oleh mirror yang mengabaikan Range**: respons yang bukan rentang
  yang diminta tidak ditulis.
- **Pindah tab tidak lagi membatalkan unduhan WebView2**: tab yang sedang mengunduh tidak
  ditidurkan/ditangguhkan.
- Download box v2: baris **Resume capability** (Yes/No), **ping server** (hijau near, kuning
  medium, merah far), dan IP/host/penyedia tampil lengkap tanpa terpotong.

## Perbaikan 0.8.4

- **RAM tidak lagi membengkak saat/sesudah mengunduh.** Penyebabnya proses toolbar Ovner (di
  Task Manager Windows: "WebView2: about:blank", sampai ±1 GB): keadaan toolbar beserta semua
  ikon bookmark dikirim tiap detik sebagai skrip baru yang terus disimpan mesin JavaScript.
  Kini dikirim sebagai pesan WebView2 (tanpa kompilasi skrip), ikon hanya dikirim saat berubah,
  dan halaman Unduhan diperbarui paling sering sekali per detik.
- Task manager Ovner menamai proses toolbar "Ovner (toolbar)".
- Kotak edit bookmark punya kolom **URL**.

## Perbaikan 0.8.3

- **Ovner tidak lagi membeku ±1 menit saat unduhan besar dimulai.** Berkas unduhan dibuat
  sebagai berkas jarang (sparse), jadi NTFS tidak mengisi nol gigabyte sebelum bagian di posisi
  jauh bisa ditulis, dan penulisan ke disk tidak lagi memegang kunci yang dipakai jendela untuk
  menggambar kemajuan.
- **RAM dirapikan otomatis setelah unduhan selesai atau dibatalkan**, dan log diagnostik kini
  mencatat RAM tiap proses (mis. `ovner 40MB, renderer 120MB`).

## Perbaikan 0.8.2

- **Menu klik kanan bookmark** di bilah dan menu folder: Buka di tab baru, Edit…, Ganti nama
  folder, Potong, Salin, Tempel (juga ke folder lain), Hapus. Tombol **Delete**, Ctrl+X, dan
  Ctrl+C bekerja pada bookmark yang sedang ditunjuk. Salinan bookmark bisa ada di beberapa folder.
- **Login dengan pemeriksaan "bukan robot"** (Cloudflare Turnstile, mis. EDABU BPJS Kesehatan;
  hCaptcha; reCAPTCHA): frame pemeriksaannya tidak lagi diacak sidik jarinya, karena nilai acak
  dianggap bot sehingga login harus diulang berkali-kali.
- **Rekam masalah tidak berhenti setelah halaman pertama**: DevTools dihidupkan lagi tiap pindah
  halaman (Chromium bisa memindah halaman ke proses lain).

## Perbaikan 0.8.1

- **Rekam masalah lebih lengkap untuk login/logout macet**: mencatat setiap pindah halaman
  (`NAV`), permintaan halaman beserta metodenya dan pengalihannya (`DOC-REQ … dialihkan dari
  HTTP-302`), status halaman (`HTTP-200 Document`), cookie yang ditolak mesin
  (`COOKIE-BLOCKED`), dan navigasi yang dibatalkan Ovner sendiri (`OVNER-REWRITE`,
  `OVNER-SCRIPTNAV`).

## Perbaikan 0.8.0

- **Indikator RAM/CPU/GPU pindah ke bilah judul Windows**, tepat di kiri tombol minimize, jadi
  tidak lagi memakan tempat di baris tab. Teksnya tembus-klik (bilah judul tetap bisa diseret),
  mengikuti jendela saat dipindah/diubah ukurannya, dan hilang saat layar penuh atau di tray.

## Perbaikan 0.7.9

- **Batal unduhan = berkas terhapus**: sisa `.ovnerpart` mesin Ovner dan `.part`/`.ytdl`/potongan
  format yt-dlp dihapus saat dibatalkan. Membatalkan saat masih "Menghubungkan…" kini benar-benar
  membatalkan (dulu tetap lanjut sampai selesai).
- **Hapus unduhan dengan pilihan "Hapus berkasnya juga"** di halaman Unduhan (satu per satu atau
  "Hapus semua"); pilihan terakhir diingat.

## Perbaikan 0.7.8

- **Download box v2** (Setelan → Unduhan, bisa dimatikan): kotak unduhan terpisah dihapus dan
  digabung ke gelembung unduhan bawaan yang lebih besar: kecepatan, sisa waktu, lama berjalan,
  koneksi, peta bagian, server (negara, kota, **jarak**, penyedia, IP). Berkas otomatis masuk
  folder sesuai jenisnya: Compressed, Documents, Music, Programs, Video, Pictures, Torrents.
- **Unduhan mulai instan**: langsung tampil di gelembung (Menghubungkan…) dan satu sesi WinHTTP
  dipakai bersama, jadi deteksi proxy otomatis tidak diulang tiap koneksi.
- **"Tampilkan di folder" membuka folder yang benar** (dulu ke Documents karena tanda kutip
  perintah Explorer salah).
- **Pengelola sandi**: hanya satu daftar (daftar "Saved info" WebView2 tidak lagi menumpuk),
  daftar langsung tersaring saat mengetik, panah atas/bawah + Enter untuk memilih.
- **Task manager seperti Task Manager Windows**: nama proses yang sama (WebView2 Manager,
  WebView2: judul tab, GPU Process, Utility: Network/Storage/Audio Service, Crashpad), grup
  "WebView2 Manager (N)", kolom nama proses, memori satu desimal, dan warna sesuai beban.
- **Bookmark**: kotak "Bookmark ditambahkan" untuk ganti nama, pilih folder, atau hapus; bookmark
  dan folder di bilah, menu folder, dan halaman Bookmark bisa diseret untuk diurutkan dan
  dimasukkan/dikeluarkan dari folder.
- **Rekam masalah**: menu jelas "● Mulai rekam" / "■ Selesai rekam dan salin laporan", merekam
  terus sepanjang waktu (juga saat pindah halaman), dengan waktu tiap kejadian dan tanpa isu
  DevTools yang tidak penting.

## Perbaikan 0.7.7

- **Infor (LN) hitam — penyebab ditemukan lewat Rekam masalah halaman**: wry memasang
  `window.ipc` yang tidak bisa dihapus di setiap halaman dan iframe. Aplikasi Infor LN (GWT)
  punya variabel `ipc` sendiri, sehingga skripnya gagal ("Identifier 'ipc' has already been
  declared") dan layar tetap hitam. Di Windows, Ovner kini menerima pesan halamannya lewat
  `chrome.webview` langsung; `window.ipc` hanya ada di halaman internal Ovner.

## Perbaikan 0.7.6

- **Kotak unduhan** seperti Mister Download Manager/IDM: begitu unduhan dimulai, muncul jendela
  kecil berisi persentase, ukuran, kecepatan, **sisa waktu**, lama berjalan, **server (host, IP,
  kota/negara, penyedia)**, jumlah koneksi, peta bagian yang diunduh bersamaan, dan tombol
  jeda/lanjut, batal, buka, buka folder. Bisa dimatikan di Setelan → Unduhan.
- **Unduh video** (YouTube, Vimeo, TikTok, Facebook, Instagram, X, Dailymotion, Twitch, dll.):
  tombol video muncul di toolbar pada halaman video; pilih kualitas terbaik ≤1080p, 720p, atau
  audio saja. Memakai yt-dlp (dan ffmpeg untuk 1080p) yang diunduh sekali ke folder `tools`.
- **Startpage instan**: tantangan Anubis di Startpage langsung diselesaikan mesin Ovner
  (multi-thread) tanpa menunggu skrip halaman; skrip Anubis halaman tidak dimuat lagi.
- **Menu bookmark, panel, dan gelembung** tidak lagi menggelapkan seluruh halaman: hanya kotak
  menunya yang menutupi halaman, seperti Brave.
- **Pengelola sandi**: edit menampilkan sandi tersimpan (dengan tombol 👁), daftar isi otomatis
  bisa digulir dan pindah ke atas kolom bila ruang bawah sempit, kolom pengguna dikenali lebih
  baik (NIK/NPP/ID karyawan), dan isi otomatis diulang sampai halaman siap.
- **Rekam masalah halaman** (tameng): mencatat galat JavaScript, permintaan gagal/CORS/terblokir,
  dan masalah cookie, lalu salin laporan ke papan klip untuk diagnosis situs seperti Infor.

## Perbaikan 0.7.5

- **SIPP BPJS dan Infor berputar terus**: dua penyebab yang dibuang.
  - Samaran "Microsoft Edge" lewat DevTools hanya berlaku di halaman, tidak di service
    worker/worker, sehingga satu situs menerima dua user agent dan sesi yang terikat user agent
    rusak. Kini Ovner memakai identitas asli WebView2 (mesin Edge); samaran hanya di mode
    seragam/preset Maksimal.
  - Tajuk GPC/DNT tidak lagi ditambahkan ke permintaan API (fetch/XHR) lintas situs, yang bisa
    membuat preflight CORS ditolak; kini hanya pada navigasi halaman.
- **Hapus cookie dan data situs ini** di menu tameng: keluar dari sesi lama yang rusak lalu
  muat ulang (semua subdomain situs itu).
- **☰ → Periksa dan pasang update**, dan **Tentang Ovner** menawarkan periksa update: begitu
  versi baru terunduh, Ovner menawarkan pasang sekarang lalu dibuka ulang dengan tab pulih.

## Perbaikan 0.7.4

- **Startpage/Anubis "Something went wrong during verification"**: identitas tab (user agent
  dan merek Sec-CH-UA "Microsoft Edge") kini dipasang sebelum alamat pertama dimuat dan tidak
  pernah diganti per situs. Sebelumnya permintaan pertama memakai identitas asli WebView2 lalu
  permintaan berikutnya identitas samaran (dan untuk Startpage diganti balik lagi), sehingga
  aturan Anubis saat tantangan dibuat berbeda dengan saat dijawab, dan Startpage menganggapnya
  "user-agent switcher".

## Perbaikan 0.7.3

- **Tingkat pemblokiran Standar seperti Brave** (bawaan): iklan dan pelacak pihak ketiga
  diblokir, tetapi sumber daya dari situs itu sendiri dibiarkan. Sebelumnya Ovner bekerja
  seperti mode Agresif Brave, sehingga skrip/API milik aplikasi kantor yang namanya mirip
  pelacak ikut diblokir dan halaman berputar terus (SIPP BPJS Ketenagakerjaan "Processing...",
  Infor Ming.le). Mode Agresif tetap ada di Pengaturan → Shields dan di preset Ketat/Maksimal.
- **Menu folder bookmark seperti Brave**: ikon situs di tiap baris, baris lebih lega, subfolder
  terbuka ke samping saat disorot, klik tengah membuka tab baru.

## Perbaikan 0.7.2

- **Startpage sebagai mesin pencari**: pemecah Anubis Ovner tidak lagi berlomba dengan skrip
  Anubis milik halaman. Dua jawaban untuk satu tantangan ditolak server ("double spend"),
  itulah penyebab "Something went wrong during verification". Kini skrip halaman bekerja lebih
  dulu; pemecah native hanya membantu bila halaman masih di tantangan setelah 10 detik, dan
  verifikasi yang gagal dicoba ulang otomatis dengan tantangan baru. Pencarian memakai alamat
  formulir Startpage sendiri (`/sp/search?query=`).

## Perbaikan 0.7.1

- **Pop-up dengan opener** (Infor Ming.le, OAuth, pembayaran): `window.open` kini dibuka
  sebagai tab Ovner yang tetap tersambung ke halaman pembukanya, seperti Brave. Sebelumnya
  tab baru dibuat terpisah sehingga `window.open` mengembalikan `null` dan aplikasi seperti
  Infor LN berputar selamanya. `window.close()` dari pop-up menutup tabnya.
- **Kolom alamat**: isi otomatis dari bookmark tampil sebagai teks bayangan; isi kolom tetap
  persis yang diketik sehingga selalu bisa diedit. Tab/→/End menerima, Enter membuka alamat
  lengkap.
- **Pengelola sandi**: kotak saran tetap rapi di situs dengan CSP ketat (login Infor);
  login disimpan untuk alamat halaman login, bukan halaman tujuan setelah masuk; nama
  pengguna/email diisi ulang bila kolomnya muncul sedikit terlambat.
- **Daftar filter seperti Brave** (Pengaturan → Daftar filter): 50+ daftar opsional dari
  katalog Brave (AI suggestions, newsletter, promo aplikasi, media sosial, YouTube Shorts dan
  lainnya, aplikasi chat, paywall, pornografi, regional, eksperimental) bisa dinyalakan satu
  per satu. Daftar bawaan Brave yang dulu belum dipakai kini selalu aktif: uBlock filters
  2020–2026, filters-general, resource abuse, link shorteners, URLhaus malware, Brave social,
  first-party, sugarcoat, dan cookie-specific.

## Fitur baru di 0.7

- **RAM per tab seperti Task Manager Windows**: tiap tab dipasangkan dengan proses halamannya
  (baris "WebView2: judul" di Task Manager), termasuk tab yang ditangguhkan. Pemantau
  pemakaian kini berjalan di thread sendiri dan diperbarui tiap detik; sebelumnya tick 2 detik
  tertunda terus selama mouse bergerak, sehingga angka di toolbar tampak macet.
- **Startpage**: tantangan anti-bot Anubis dipecahkan Ovner secara native (Rust, semua inti
  CPU, biasanya beberapa milidetik) lalu halaman diarahkan ke alamat lolos tantangan, seperti
  SearXNG. Pencarian memakai format ekstensi resmi Startpage (`pl=ext-chrome`). Aset Anubis
  tidak pernah diblokir. Berlaku juga untuk situs lain yang memakai Anubis.
- **Kolom alamat**: teks tidak lagi ditimpa saat diedit; isi otomatis dari bookmark bisa
  diterima dengan Tab/→/End lalu diedit, Backspace menghapus isi otomatis saja.
- **Mesin unduh multi-koneksi** (seperti Mister Download Manager/IDM): berkas ≥ 2 MB dari
  server yang mendukung Range diunduh lewat 8 koneksi; bagian yang selesai lebih dulu
  membelah bagian terbesar ("rebalance sampai byte terakhir"); bagian yang putus diulang dari
  posisi terakhirnya; bisa dijeda/dilanjutkan. Cookie, user agent, dan referrer tab ikut
  dikirim. Bila server tidak mendukung atau gagal di tengah, unduhan diulang lewat WebView2.
- **Mode gelap seperti Dark Reader**: tombol bulan di toolbar membuka panel per situs
  (Ikut umum / Nyala / Mati), mode untuk semua situs (mati, nyala, ikut tema Windows), cara
  (Dinamis = Dark Reader, Filter, Chromium), serta kecerahan, kontras, sepia, dan abu-abu.
  Berlaku langsung tanpa muat ulang. Dark Reader (MIT) dibundel dan hanya dimuat di situs
  yang digelapkan. "Paksa mode gelap" lama otomatis menjadi mode Chromium untuk semua situs.
- **Penghemat tab seperti TabWars**: situs yang tidak pernah ditidurkan, tab dengan formulir
  yang sedang diisi tidak ditidurkan, tutup tab duplikat, **Sesi tab** (simpan semua tab
  dengan nama, buka lagi dalam keadaan tidur, ganti tab, ubah nama), daftar tab yang baru
  ditutup, dan statistik (tab ditidurkan, RAM dibebaskan, duplikat ditutup).
- **Izinkan salin seperti Allow Copy+**: per situs lewat tameng (atau semua situs): pilih teks,
  salin, tempel, seret, dan klik kanan tidak bisa dilarang situs. **Paksa tempel**: Alt+Shift+V
  di kolom yang melarang tempel (semua situs).

## Fitur baru di 0.6

- **Anti-blokir** (Pengaturan → Privasi dan keamanan): proxy di dalam Ovner yang mencari
  alamat situs lewat DNS aman (Cloudflare/Google) dan memecah salam TLS di tengah nama situs,
  sehingga situs yang diblokir lewat DNS maupun SNI oleh ISP bisa dibuka. Isi tetap
  terenkripsi. Matikan bila jaringan kantor mewajibkan proxy.
- **Pemakaian realtime**: RAM, CPU, dan GPU Ovner (total semua proses WebView2) serta tab yang
  sedang dibuka, di ujung kanan baris tab; bisa dinyalakan/dimatikan masing-masing.
- **Task manager** baru: proses WebView2 dikenali dari folder data Ovner (sebelumnya hanya
  ovner.exe yang terhitung, sehingga perapian RAM di tray juga hanya mengenai ovner.exe),
  dengan kolom RAM, CPU, GPU, dan memori GPU per proses.
- **Video/musik di tab latar**: tetap berputar, langsung berhenti, atau berhenti setelah N menit.
- **Diagnostik** (☰ → Alat → Diagnostik, atau Pengaturan): uji pemblokir, brankas sandi, DoH,
  unduhan, cek update, daftar proses, crash, dan log; tombol *Salin laporan*.
- Pengaturan → Sistem: buka folder aplikasi, folder data, log; **Uninstall Ovner**.
- Crash tingkat Windows dicatat ke `ovner.log` (kode galat dan modul penyebab).
- Ikon asli situs di kolom alamat; ikon bookmark yang belum pernah dikunjungi diunduh sendiri.
- Startpage dan situs yang dikecualikan memakai identitas asli WebView2 tanpa farbling.

## Fitur baru di 0.4

**Tingkat perlindungan**: tiga preset di atas halaman Pengaturan — **Seimbang** (bawaan),
**Ketat**, dan **Maksimal**. Setelan rinci disembunyikan di balik *Tampilkan semua setelan*,
tetap bisa diubah satu per satu (preset lalu menjadi *Kustom*).

**Pemblokir sekuat Brave Shields**:
- scriptlet uBlock Origin (`+js(...)`) disuntikkan sebelum skrip situs: iklan video YouTube
  dan popup anti-adblock;
- resource pengganti (`$redirect`): skrip iklan diganti versi kosong supaya pemutar video
  situs berita tetap jalan;
- filter procedural (`:has-text`, `:upward`, `:matches-css`, `:remove()`): iklan "Bersponsor";
- `$removeparam` dan daftar AdGuard URL Tracking; daftar uBO Quick fixes, Unbreak, Badware,
  Brave Unbreak; daftar gangguan (opsional);
- perbaikan cepat dan scriptlet diperbarui tiap 6 jam, daftar besar tiap 3 hari.

**Privasi ala LibreWolf, Waterfox, dan Mullvad**: blokir akses situs ke jaringan lokal,
pangkas referrer, Mullvad DNS dan DNS ketat tanpa cadangan, HTTPS-only ketat, letterboxing,
mode seragam, samarkan WebView2 sebagai Edge biasa, **Identitas baru**, selalu mode privat,
mesin pencari terpisah untuk tab privat (Qwant, Startpage, Mullvad Leta, Ecosia, Mojeek),
salin alamat semua tab, CSS sendiri untuk toolbar.

**Level keamanan** (seperti Tor/Mullvad): Standar, Lebih aman (JavaScript mati di situs http,
JIT mati), Paling aman (JavaScript mati kecuali situs yang diizinkan lewat tameng).

**Tab vertikal** di kiri, dan **bilah bookmark seperti Brave**: folder, ikon, tombol `»`,
dan *Bookmark lain*. Impor ulang bookmark sekali agar foldernya ikut.

**Mode GPU**: *Penuh*, *Hemat* (GPU tetap untuk video, gambar halaman di CPU, memori GPU
dibatasi), atau *Mati*. Saat GPU mati, YouTube otomatis memakai H.264 yang paling ringan
untuk CPU. Pengguna "Matikan GPU" di 0.3 otomatis dipindah ke mode *Mati*; coba *Hemat*
bila video terasa buram.

## Fitur baru di 0.3

**Setelan Brave Origin dikoreksi**: prediksi jaringan nyala, zero-copy nyala, pemeriksa ejaan
mati, tab baru kosong, DevTools mati secara bawaan. Pengguna 0.2 otomatis disesuaikan sekali.

**Kolom alamat**
- **!bangs** seperti Helium: `rust !w`, `!yt lagu`, `!gh servo`, `!tok`, `!shp`, `!kbbi`,
  `!tr`, `!claude`, dan lain-lain; bisa menambah bang sendiri.
- Saran dari riwayat dan bookmark (lokal saja).
- Mode **No Search** seperti ungoogled-chromium: teks yang bukan alamat tidak dicari.
- Alamat internasional ditampilkan dalam bentuk punycode (cegah situs palsu yang mirip).

**Shields** (klik ikon tameng): blokir iklan, blokir JavaScript, dan acak sidik jari bisa
dimatikan/dinyalakan per situs, ditambah *Sembunyikan elemen...* untuk memilih bagian halaman
yang ingin dihilangkan selamanya.

**Acak sidik jari (farbling)**: situs bisa mengenali komputer Anda dari cara kartu grafis
menggambar, suara diproses, jumlah inti CPU, dan sebagainya. Ovner menambahkan "noda"
kecil yang tidak terlihat dan berbeda untuk tiap situs dan tiap sesi, jadi sidik jari Anda
berubah-ubah dan tidak bisa dipakai melacak. *Standar* hampir tanpa efek samping, *Ketat*
lebih kuat tetapi bisa membuat beberapa situs rewel.

**Privasi**: Secure DNS (Cloudflare, Quad9, AdGuard), matikan API iklan Chromium (Topics,
FLEDGE, Attribution Reporting) dan eksperimen lapangan, matikan WebGL, abaikan dialog
"Tinggalkan situs?", semua pop-up jadi tab.

**Tampilan**: paksa mode gelap di semua situs, CSS sendiri, **bagi layar** (dua tab
berdampingan: menu ☰ → *Bagi layar*, atau klik kanan tab).

**Hemat RAM dan stabilitas** (ide dari Servo): batas jumlah proses halaman, satu proses per
situs, task manager dengan RAM per tab, pemulihan otomatis bila halaman crash, tawaran
memuat ulang bila halaman macet.

**Ekstensi (eksperimen)**: nyalakan di Pengaturan → Sistem, lalu isi folder ekstensi yang
sudah diekstrak (misalnya uBlock Origin Lite). Hanya ekstensi yang didukung WebView2.

## Pintasan

| Tombol | Fungsi |
|---|---|
| Ctrl+T / Ctrl+W | Tab baru / tutup tab |
| Ctrl+Shift+N | Tab privat |
| Ctrl+Shift+T | Buka lagi tab yang ditutup |
| Ctrl+Tab, Ctrl+Shift+Tab | Tab berikut / sebelumnya |
| Ctrl+1 … Ctrl+8, Ctrl+9 | Pindah ke tab ke-N / tab terakhir |
| Ctrl+L, Alt+D, F6 | Ke kolom alamat |
| Ctrl+D, Ctrl+H, Ctrl+J | Bookmark, riwayat, unduhan |
| Ctrl+F, Ctrl+P | Cari di halaman, cetak |
| Shift+Esc | Task manager |
| Alt+← / Alt+→, F5, Alt+Home | Kembali / maju, muat ulang, beranda |
| Ctrl+ +/−/0 | Perbesar / perkecil / normal |

## Batasan

- RAM tab yang **aktif** kurang lebih sama dengan Edge, karena mesinnya sama. Penghematan
  datang dari iklan yang tidak dimuat, tab yang ditidurkan, dan batas proses.
- Setelan bertanda ↻ adalah argumen mesin WebView2 dan baru berlaku setelah Ovner dibuka ulang.
  Secure DNS, paksa mode gelap, dan API iklan Chromium memakai flag Chromium yang bisa berubah
  antar versi WebView2.
- Pengungkap CNAME bekerja di latar: permintaan pertama ke subdomain baru tetap lewat.
- Belum ada sinkronisasi, Rewards, VPN, Wallet, atau Leo AI (Brave Origin juga membuangnya).

## Build sendiri

```
cargo build --release
```

Bisa dikompilasi dan diuji di Linux (WebKitGTK) untuk pengembangan; pemblokir jaringan, tidur
tab, tray, dan menu ☰ hanya aktif penuh di Windows. Browser eksperimen berbasis Servo ada di
[profrendi/lumen](https://github.com/profrendi/lumen).

---

## English

A lightweight browser for Windows 10/11, written in Rust, with an **Indonesian and English**
interface (Settings → Language; follows Windows by default).

- **Engine**: Microsoft Edge WebView2 (Chromium, built into Windows 11).
- **Brave-style Shields**: adblock-rust (the Brave Shields engine) with EasyList, EasyPrivacy,
  ABPindo and uBlock lists; cookie-notice blocking; fingerprint randomization (farbling);
  per-site JavaScript blocking; element picker; custom filter lists and rules; tracking
  parameter stripping, debouncing, de-AMP; experimental CNAME uncloaking.
- **Memory saving**: automatic tab sleeping, a low-memory mode when minimized or in the tray
  (like Outlook Lite and ZFPlus), renderer process limit, per-tab RAM in the task manager.
- **No admin rights**: per-user install, Visual C++ runtime built into the exe.

**Install**: download **Ovner-Setup.exe** from
[Releases](https://github.com/profrendi/ovner/releases/latest) and run it. It installs to
`%LOCALAPPDATA%\Programs\Ovner`, adds Start Menu and Desktop shortcuts, and opens a welcome page.
Choose the **Stable** or **Nightly** update channel in Settings → System.
**Portable**: extract **Ovner-Portable.zip** anywhere and run `ovner.exe`; data stays in the
`Data` folder next to it (saved site logins are tied to the Windows account).

**Address bar**: `!bangs` (`rust !w`, `!yt`, `!gh`, custom bangs), local suggestions from history
and bookmarks, optional *No Search* mode, punycode display for internationalized domains.

**Privacy**: Secure DNS (Cloudflare, Quad9, AdGuard, Mullvad) with an optional no-fallback mode,
Privacy Sandbox APIs and field trials disabled, local-network access blocking, referrer trimming,
strict HTTPS-only, letterboxing, Mullvad-style uniform mode, WebView2 disguised as regular Edge,
New identity, always-private mode, security levels (Standard / Safer / Safest).

**0.4**: protection presets (Balanced / Strict / Maximum, detailed settings still editable),
uBlock Origin scriptlets, redirect resources, procedural cosmetic filters, `$removeparam`,
quick-fix lists refreshed every 6 hours, Brave-style bookmarks bar with folders, vertical tabs,
GPU modes (Full / Lite / Off) with automatic H.264 on YouTube when the GPU is off.

**Also**: forced dark mode, custom CSS, split view, crash and hang recovery, experimental
WebView2 extensions, bookmark import from Chrome/Edge/Brave.

Settings marked ↻ are WebView2 engine flags and apply after a restart.
