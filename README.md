# Ovner

Browser ringan untuk Windows 10/11, ditulis dengan Rust.

- **Mesin**: Microsoft Edge WebView2, mesin Chromium yang sudah ada di Windows 11. Semua situs
  (YouTube, Office online, Outlook web, situs bank) tampil seperti di Edge.
- **Pemblokir iklan**: [adblock-rust](https://github.com/brave/adblock-rust), mesin yang sama
  dengan Brave Shields, memakai daftar EasyList, EasyPrivacy, ABPindo (iklan situs Indonesia),
  dan uBlock. Permintaan iklan dibatalkan sebelum terkirim, dan kotak iklan yang kosong
  disembunyikan.
- **Hemat RAM**: tab yang tidak dibuka selama 15 menit ditidurkan (halamannya dibuang dari
  RAM dan dimuat ulang saat diklik). Tab yang memutar suara tidak ditidurkan. Tab di
  belakang memakai mode memori rendah WebView2, dan fitur Chromium yang tidak perlu
  (proses cadangan, terjemahan, cast) dimatikan.
- **Tanpa hak admin**: terpasang per pengguna, runtime Visual C++ sudah tertanam di exe.

## Pasang

1. Unduh **Ovner-Setup.exe** dari halaman [Releases](https://github.com/profrendi/ovner/releases/latest).
2. Jalankan. Ovner menyalin dirinya ke `%LOCALAPPDATA%\Programs\Ovner`, membuat ikon di
   **Start Menu** dan **Desktop**, lalu terbuka.
3. Kalau Windows SmartScreen muncul: **More info → Run anyway** (exe belum ditandatangani).

Ovner memeriksa versi baru saat dibuka. Kalau ada, tombol hijau **Update** muncul di
toolbar; klik untuk membuka ulang dengan versi baru.

Hapus lewat **Settings → Apps → Installed apps → Ovner → Uninstall**.

## Versi portabel

Unduh **Ovner-Portable.zip**, ekstrak ke folder mana saja (termasuk flashdisk), jalankan
`ovner.exe`. File `portable.txt` di sebelahnya membuat semua data (cookie, login, riwayat,
daftar filter) disimpan di folder `Data` di sebelah exe, dan tidak ada yang dipasang.

## Fitur

**Menu ☰** (seperti Brave): tab baru, tab privat (Ctrl+Shift+N), buka lagi tab yang
ditutup, bookmark (bilah bookmark, Ctrl+D), riwayat (Ctrl+H), unduhan (Ctrl+J), zoom,
cetak, cari di halaman (Ctrl+F), task manager (Shift+Esc), tidurkan semua tab, rapikan
RAM, perbarui filter iklan, hapus data penjelajahan, DevTools, pengaturan, sembunyikan
ke tray. Klik kanan tab: muat ulang, duplikat, tidurkan, tutup, tutup tab lain.

**Tray**: tombol X menyembunyikan Ovner ke tray (bisa dimatikan). Saat di tray atau
diminimalkan, Ovner langsung masuk mode hemat seperti Outlook Lite dan ZFPlus: mode memori
rendah WebView2, tab ditangguhkan, working set dirapikan, dan mode efisiensi Windows.

**Pengaturan** (menu ☰ → Pengaturan, atau ketik `ovner://settings`), dengan bagian yang
mengikuti Brave Origin:

| Bagian | Isi |
|---|---|
| Tampilan | Tema terang/gelap/ikuti Windows, bilah bookmark, tombol beranda, halaman tab baru (kotak cari atau kosong), zoom bawaan |
| Mesin pencari | Google, DuckDuckGo, Bing, Brave Search, atau alamat sendiri; tanpa saran pencarian |
| Shields | Blokir iklan dan pelacak, sembunyikan kotak iklan, buang parameter pelacak, naikkan ke HTTPS (kembali ke http bila situs tidak mendukung), Global Privacy Control + DNT, blokir telemetri Edge/Office, filter otomatis, situs pengecualian |
| Privasi dan keamanan | Simpan riwayat, hapus data saat keluar, tolak kamera/mikrofon/lokasi, WebRTC tanpa IP lokal, prediksi jaringan, putar media otomatis, pengelola sandi, isi otomatis, SmartScreen, tracking prevention Edge, laporan crash lokal |
| Unduhan | Folder unduhan |
| Saat mulai | Pulihkan tab terakhir (dalam keadaan tidur), mulai bersama Windows di tray |
| Hemat RAM | Tidurkan tab (menit), tangguhkan tab di belakang, X ke tray, memori rendah saat tersembunyi, rapikan RAM saat tersembunyi dan tiap N menit, mode efisiensi Windows, tekanan memori simulasi, mode perangkat RAM kecil, JavaScript hemat memori, isolasi situs, proses cadangan, matikan GPU, zero-copy, unduhan paralel, argumen WebView2 tambahan |
| Sistem | Periksa update, DevTools, catat pemakaian RAM ke log |

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
| Klik tengah pada tab | Tutup tab |

## Tameng (pemblokir iklan)

Angka di ikon tameng oranye adalah jumlah iklan dan pelacak yang diblokir di halaman itu.
Kalau ada situs yang rusak karena pemblokir, klik tameng untuk mematikannya **di situs itu
saja**; klik lagi untuk menyalakan.

Daftar filter diunduh saat Ovner pertama dibuka, lalu diperbarui seminggu sekali
(menu ☰ → Alat lain → *Perbarui daftar filter iklan* untuk memperbarui sekarang).

## Batasan

- RAM per tab yang **aktif** kurang lebih sama dengan Edge, karena mesinnya sama.
  Penghematan datang dari iklan yang tidak dimuat dan tab yang ditidurkan.
- Belum ada sinkronisasi antar perangkat, Brave Rewards, VPN, Wallet, atau Leo AI (Brave Origin juga membuang fitur-fitur ini).
- Setelan bertanda "berlaku setelah dibuka ulang" adalah argumen mesin WebView2, jadi baru aktif saat Ovner dibuka lagi.
- Ekstensi Chrome belum didukung.
- Ovner belum bisa dijadikan browser default Windows.

## Build sendiri

```
cargo build --release
```

Proyek ini juga bisa dikompilasi dan diuji di Linux (WebKitGTK) untuk pengembangan;
pemblokir jaringan, tidur tab, tray, dan menu ☰ hanya aktif penuh di Windows.

Browser eksperimen berbasis mesin Servo yang dulu ada di repo ini sekarang di
[profrendi/lumen](https://github.com/profrendi/lumen).
