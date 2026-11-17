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
