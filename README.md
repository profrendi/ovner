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
2. Jalankan, lalu pilih cara memasang:
   - **Install**: seperti software lain, ke `C:\Program Files\Ovner` untuk semua pengguna PC,
     ikon di Start Menu (dan Desktop bila dicentang), entri di Apps & features. Perlu izin admin
     (UAC); update berikutnya juga meminta izin admin.
   - **Portable**: tanpa admin, ke folder pilihan (bawaan `Ovner Portable` di sebelah Setup).
     `ovner.exe` + `portable.txt`; semua data di folder `Data` di sebelahnya.
   - **Perbarui instalasi saat ini**: muncul bila sudah ada instalasi per-pengguna lama di
     `%LOCALAPPDATA%\Programs\Ovner` (tanpa admin).

   Tidak perlu Visual C++ Redistributable (runtime-nya sudah di dalam exe). Setelah terpasang,
   Ovner membuka halaman selamat datang (bahasa, mesin pencari, Shields, impor bookmark).
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

## Perbaikan 0.8.22

- **Ambil subtitle (.srt) dari dalam video**: tombol **SRT** di daftar Unduhan untuk video yang
  sudah selesai, dan menu Alat → "Ambil subtitle (.srt) dari video…" untuk video apa pun
  (MP4/MKV/WebM/…). Semua trek teks diambil (satu trek = `nama.srt`, lebih dari satu =
  `nama.bahasa.srt`) tanpa mengubah videonya. Subtitle gambar (Blu-ray PGS/DVD VobSub) dilewati
  dengan keterangan karena butuh OCR. Setelah subtitle dimasukkan ke video, Ovner kini
  memeriksa dengan ffprobe bahwa treknya benar-benar ada.
- **Log baru** (`ovner.log`): waktu setempat, tingkat (INFO/PERINGATAN/GALAT), dan kategori
  fitur `[pdf] [subtitle] [tidur] [ram] [unduh] [codec] [setelan] …`; ditulis di latar (UI
  tidak menunggu disk); riwayat 5 berkas × 1 MB. Baris pembuka setiap mulai (versi, Windows,
  WebView2, CPU, RAM, setelan yang berbeda dari bawaan, argumen mesin, versi alat video,
  perpindahan versi), riwayat setiap perubahan setelan (lama → baru), alasan setiap tab tidur,
  lama tab dimuat, setiap unduhan selesai/gagal (ukuran, lama, kecepatan, mesin), media di
  belakang yang dijeda, codec/resolusi/frame hilang yang benar-benar diputar YouTube, dan
  panic beserta jejaknya.
- **Pengukuran tiap menit** (bisa dimatikan): RAM/commit/CPU per jenis proses, GPU, RAM Windows
  tersisa, tab hidup/beku, unduhan, sisa disk, dan penghitung harian (iklan diblokir, tab
  ditidurkan, MB dibebaskan, unduhan, galat per kategori), disimpan 7 hari di `Data\metrics`.
- **Diagnostik → Uji semua fitur**: setiap setelan (138 + 12 yang tidak tampil) diperiksa
  nilainya dan dilacak lewat apa efeknya berlaku (argumen mesin, skrip halaman, skrip per situs,
  alamat/pencarian, identitas browser); fitur diuji: folder data, file setelan/unduhan/sesi,
  log, pengukuran, aturan PDF, subtitle json3→SRT, yt-dlp/ffmpeg/ffprobe/Deno, ffmpeg sungguhan
  (video uji → subtitle masuk → diperiksa → diambil lagi), keadaan setiap tab (kenapa hidup),
  RAM Windows, rapikan cerdas, folder unduhan, codec, browser default, autostart, Shields, DNS
  aman, unduhan, update + tanda tangan, WebView2. Yang hanya bisa dibuktikan di situs sungguhan
  ditandai Dilewati beserta alasannya. Log bisa disaring per kategori/tingkat.
- **Simpan paket diagnostik**: satu berkas teks berisi laporan, hasil uji, tabel setelan,
  setelan, pengukuran 7 hari, dan semua log; opsi menyembunyikan alamat situs dan nama pengguna
  Windows.
- **Uji Ovner sungguhan dengan WebView2 di GitHub** (`--self-test`): setiap build menjalankan
  Ovner di Windows dengan server uji lokal — PDF kecil/besar tanpa ukuran, PDF besar dengan
  ukuran, PDF dalam bingkai seperti Infor, PDF lokal, unduhan 20 MB dicek byte per byte, iklan
  diblokir, subtitle lewat pemutar YouTube tiruan, tidur N tab terakhir, formulir dilindungi,
  tidurkan semua, daftar tab, rapikan RAM, browser default, dan patokan RAM (1 tab, 10 tab,
  setelah ditidurkan). Hasil, tangkapan layar, log, dan pengukuran disimpan sebagai lampiran
  build. Tahap awal: belum membatalkan rilis sampai terbukti stabil.

## Perbaikan 0.8.21

- **Subtitle YouTube lewat halaman YouTube.** yt-dlp terus ditolak YouTube ("HTTP 429 Too Many
  Requests"), bahkan setelah 3 percobaan. Kini subtitle diambil oleh pemutar YouTube di tab
  Ovner sendiri (sesi browser dan token pemutar), lalu diubah ke .srt dan dimasukkan ke video.
  Bila tab videonya tidak terbuka, Ovner membukanya sebentar di belakang tanpa suara lalu
  menutupnya. yt-dlp tetap jadi cadangan. Tombol CC↻ memakai cara yang sama. Bisa dimatikan:
  Pengaturan → Unduhan → "Subtitle YouTube lewat halaman YouTube".
- **RAM tidak "nyangkut" setelah menutup YouTube.** Penyebabnya bukan "tetap hidupkan N tab":
  proses GPU masih memegang buffer dekoder video (100–300 MB) dan rapikan cerdas 0.8.19 tidak
  menyentuh GPU selama jendela terbuka. Kini begitu tab video/berat ditutup atau ditidurkan,
  4 detik kemudian GPU dan sisa proses tanpa tab dirapikan (kecuali ada video/suara lain yang
  diputar).
- Rapikan cerdas juga merapikan proses Ovner sendiri (±70 MB sisa saat mulai), dan proses
  browser/penyimpanan WebView2 setelah 10 detik tidak ada ketikan/klik (dulu 30 detik).
- **Penjaga tab membengkak** tidak memberi pemberitahuan selagi tab yang dilihat memutar video.
  Kualitas video tidak pernah diturunkan.
- PDF di komputer sendiri (file://) tidak lagi dicegat aturan "Unduh PDF besar": langsung
  dibuka di penampil.
- Log lebih bersih: "efisiensi per proses" hanya dicatat bila jumlahnya berubah; tab baru yang
  ditidurkan ditulis "(tab baru)".

## Perbaikan 0.8.20

- **Preview PDF Infor kembali.** Cetakan permintaan pembelian/pembayaran di Infor LN tampil lagi
  di halamannya sendiri (penampil PDF dengan tombol unduh dan cetak), tidak langsung terunduh
  sebagai `tmp….pdf`. Penyebab: server Infor tidak menyebut ukuran PDF-nya dan alamatnya
  berisi "download", sehingga aturan "Unduh PDF besar" menganggapnya PDF besar.
- **Aturan PDF baru** (Pengaturan → Unduhan):
  - Ukuran disebut server: sama seperti dulu (≥ batas = unduh, mis. kartu SIPP BPJS 17 MB;
    lebih kecil = tampil).
  - Ukuran tidak disebut: **Ukur dulu** (bawaan) — dibaca dulu; selesai di bawah batas =
    ditampilkan seperti aslinya, lewat batas = jadi unduhan tanpa mengunduh ulang. Pilihan
    lain: Selalu tampilkan, atau Selalu unduh (perilaku 0.8.19).
  - **Situs yang PDF-nya selalu ditampilkan** (bawaan: inforcloudsuite.com) dan **situs yang
    PDF-nya selalu diunduh** mengalahkan aturan ukuran.
- Halaman "✓ PDF disimpan" punya tombol **Buka di sini (lihat & cetak)**: PDF yang tadi
  disimpan dibuka di penampil bawaan di tab itu juga, langsung dari berkasnya (tidak diunduh
  ulang dari server).

## Perbaikan 0.8.19

- **Ovner bisa jadi browser default** untuk tautan http/https serta berkas .html/.htm dan .pdf,
  tanpa admin (didaftarkan khusus pengguna ini). Pengaturan → Saat mulai → Browser default →
  "Jadikan browser default" membuka halaman Aplikasi default Windows; tekan "Set default".
  Tautan dari Outlook/WhatsApp/Excel dibuka sebagai tab baru di jendela Ovner yang terbuka.
  Seperti Chrome, alamat dari Windows dibaca dengan `--single-argument`, jadi tautan berbahaya
  tidak bisa menyelipkan opsi baris perintah. Uninstall menghapus pendaftarannya.
- **Subtitle tidak lagi menggagalkan unduhan video.** Penyebab 0.8.18: YouTube menolak unduhan
  subtitle (HTTP 429 Too Many Requests) dan yt-dlp lalu membatalkan videonya juga. Kini video
  diunduh dulu, subtitle diambil terpisah dengan percobaan ulang (jeda 5 lalu 15 detik), lalu
  dimasukkan ke video oleh ffmpeg (trek default, bahasa tertulis, mis. "ind"). Bila tetap
  ditolak, video tetap tersimpan dengan catatan, dan tombol **CC↻ Coba unduh subtitle lagi**
  mengambil subtitlenya saja tanpa mengunduh ulang video.
- **Tidur tab lebih cerdas:**
  - **Tetap hidupkan N tab terakhir** (bawaan 3): tab lain langsung ditidurkan begitu kamu pindah
    tab. N tab itu tetap ikut "Tidurkan tab setelah … menit". Tab yang dibuka di belakang dan
    belum pernah dilihat hanya ikut aturan menit.
  - **Tidurkan tab lebih cepat saat diminimalkan/di tray** (on/off, bawaan 60 detik, paling
    cepat 30 detik). Halaman yang terakhir dilihat cukup dibekukan, jadi langsung tampil lagi.
    Bug lama: dengan setelan 1 menit, tab justru tidur lebih lambat saat diminimalkan.
  - Pemeriksaan tidur tiap 5 detik (dulu 30), supaya waktunya tepat.
  - Situs "tidak pernah ditidurkan" juga tidak dibekukan (notifikasi dan sesi login tetap jalan).
- **Tombol tidur/daftar tab di toolbar** (ikon zZ): Tidurkan semua tab lain · Pindahkan tab lain
  ke daftar · Pindahkan semua tab ke daftar · Buka daftar tab (seperti OneTab). Item daftar
  diklik untuk membuka tabnya; opsi **"Daftar tab: hapus item begitu dibuka"** bisa
  dinyalakan/dimatikan. "Tidurkan semua tab lain" kini memakai pengaman yang sama dengan tidur
  otomatis (tab yang memutar suara, mengunduh, atau formulirnya sedang diisi tidak ikut).
- **Rapikan RAM cerdas** (menggantikan "rapikan semua proses"): hanya proses yang diam dan sudah
  bertambah ≥20 MB yang dirapikan. Tab yang sedang dilihat, GPU saat jendela terbuka/video
  diputar, jaringan saat mengunduh, dan proses yang CPU-nya sibuk tidak disentuh. Hasilnya diukur
  1 menit kemudian; proses yang selalu memantul dilewati makin lama (5 → 10 → 20 → 40 menit).
  Berjalan berkala juga saat jendela terbuka. Log mencatat sebelum/sesudah/1 menit kemudian.
- **Penjaga tab membengkak** (bawaan 700 MB): tab di belakang yang melewati batas ditidurkan; tab
  yang sedang dilihat tidak dimuat ulang sendiri, hanya diberi pemberitahuan dengan tombol
  "Muat ulang" (proses baru) atau "Nanti".
- **Tidurkan tab saat RAM Windows hampir habis** (tersedia < 6% RAM, minimal 512 MB): tab di
  belakang yang paling lama tidak dipakai ditidurkan satu per satu sampai aman.
- **Task manager Ovner** menampilkan dua angka: RAM aktif (seperti Task Manager Windows) dan
  memori terpakai sebenarnya (commit, tidak turun hanya karena dirapikan).

## Perbaikan 0.8.18

- **Codec video YouTube seperti enhanced-h264ify** — tombol chip baru di kanan atas (di sebelah
  mode gelap) dengan pengaturannya sendiri. Tiga mode:
  - **Otomatis** (bawaan): Ovner memeriksa dekoder video GPU komputer ini sekali per sesi
    (WebCodecs, cadangannya MediaCapabilities). Codec yang tidak punya dekoder GPU diblokir: AV1 di
    GPU sebelum Intel Gen 11 / RTX 30 / RX 6000, VP9 di laptop lama. Tanpa dekoder GPU sama sekali
    atau GPU dimatikan di Ovner: hanya H.264, maksimal 30fps.
  - **Manual**: kotak centang Blokir AV1 / VP9 / H.264 / VP8 / 60fps / Opus / AAC, bawaan sama
    seperti enhanced-h264ify (semua kecuali H.264). AV1+VP9+H.264 tidak bisa diblokir bersamaan.
  - **Mati**: YouTube memilih sendiri.
  Setiap codec diberi keterangan singkat (kuota, ketajaman, beban dekode, syarat GPU), status
  dekoder GPU-nya, dan panel menampilkan codec yang sedang diputar (seperti Stats for nerds).
  Selain `isTypeSupported` dan `canPlayType` (cara enhanced-h264ify), Ovner juga menjawab
  `MediaCapabilities.decodingInfo` dan WebCodecs supaya YouTube tidak tetap memilih AV1 lewat
  jalur pemeriksaan yang lebih baru. Opsi "Matikan normalisasi volume" juga ada. Setelan lama
  "YouTube pakai H.264: Selalu" otomatis menjadi mode Manual.
- **Nama berkas video/audio otomatis diberi kualitas**: memilih 1080p → `judul 1080p.mp4`;
  audio → `judul MP3 320 KBPS.mp3`, `judul M4A 128 KBPS.m4a`, `judul OPUS 160 KBPS.opus`,
  `judul MP3 VBR.mp3`, `judul FLAC.flac`. Mengganti pilihan langsung mengganti akhiran nama.
- **720p tidak lagi berisi video 480p.** Akar masalahnya: bila berkas dengan nama sama sudah ada,
  yt-dlp menganggap video "sudah diunduh" dan memakai berkas lama tanpa mengunduh. Kini nama yang
  sudah terpakai otomatis diberi `(1)`, `(2)`, ... (MKV dan berkas .part ikut diperiksa).
- **Subtitle tampil di Media Player bawaan Windows.** Media Player Windows tidak membaca subtitle
  yang tertanam di MP4 (VLC bisa). Pilihan baru "Simpan juga berkas .srt" (nyala bawaan saat
  subtitle dimasukkan ke video): berkas `.srt` disimpan dengan nama sama persis dengan videonya
  (`judul 1080p.srt`, bukan `judul 1080p.id.srt`) sehingga bisa dipilih lewat Subtitle → Pilih
  berkas subtitle. Trek subtitle di dalam MP4 juga ditandai default.
- **Download box v2 selalu dipakai.** Pilihan "Download box v2" dihapus. Panel unduhan bawaan
  WebView2 ("Downloads" dengan See more) yang kadang muncul — misalnya Ctrl+J saat fokus di
  penampil PDF/iframe/situs tanpa JavaScript, atau unduhan yang lolos — kini langsung ditutup dan
  diganti gelembung Download box v2 Ovner; Ctrl+J di mana pun membuka halaman Unduhan Ovner.

## Perbaikan 0.8.17

- **Alamat YouTube selalu sesuai halaman yang dibuka.** YouTube berpindah video tanpa memuat
  ulang; pemberi tahu lewat skrip halaman bisa terlewat sehingga alamat (kolom alamat, dialog unduh
  video, tombol video di toolbar) tetap alamat pertama. Kini Ovner memakai event resmi WebView2
  (SourceChanged) ditambah cadangan pemeriksaan alamat di halaman. Perpindahan video juga masuk
  riwayat.
- **Unduh video sesuai kualitas yang dipilih.** Ovner memakai format persis hasil penelusuran
  (mis. `137+140` = 1080p H.264 + audio M4A) dan tidak lagi diam-diam jatuh ke 360p bila format
  itu gagal: lebih baik muncul pesan jelas. Pilihan per resolusi mengutamakan unduhan langsung
  (bukan HLS), H.264 (paling kompatibel), lalu MP4.
- **ffmpeg diperiksa benar-benar jalan** (`ffmpeg -version` dan `ffprobe -version`) setelah
  dipasang dan sebelum dipakai; bila rusak, dipasang ulang. Bila yt-dlp tetap melaporkan ffmpeg
  tidak ada ("formats won't be merged"), unduhan dinyatakan gagal dengan pesan jelas — dulu
  dilaporkan berhasil padahal video dan audio terpisah / audio tidak dikonversi / subtitle tidak
  tergabung. Arsip ffmpeg dibuka dengan `tar` bawaan Windows (bukan tar lain di PATH).
- **Subtitle "Masukkan ke video" benar-benar tergabung** dan berkas .srt tidak tertinggal
  (dulu `--write-subs` ikut dipasang). Memilih subtitle langsung mencentang *Masukkan ke video*.
  Subtitle otomatis YouTube hanya diminta bila yang dipilih memang otomatis.
- **Hasil diperiksa:** berkas akhir harus ada; resolusi hasil dibaca dengan ffprobe dan dicatat;
  bila lebih rendah dari pilihan, tertulis di daftar unduhan (mis. "360p (dipilih 1080p)").
- **Log lebih lengkap untuk laporan:** perintah yt-dlp, format yang dipilih, langkah
  Merger/ExtractAudio/EmbedSubtitle, dan semua peringatan/galat yt-dlp masuk `ovner.log`.
- Diuji ujung ke ujung dengan sumber DASH lokal (video 1080p/360p dan audio terpisah seperti
  YouTube): 1080p dan 360p tergabung dengan suara, M4A, MP3 VBR/320/256/192/128, OPUS, FLAC,
  subtitle tertanam (tanpa .srt tertinggal) dan subtitle terpisah; tanpa ffmpeg → gagal jelas.

## Perbaikan 0.8.16

- **Unduh video YouTube gagal "The system cannot locate the object specified (0x800C0006)".**
  Akar masalah: berkas ffmpeg yang diunduh Ovner (`ffmpeg-master-latest-win64-lgpl.zip`) sudah
  tidak diterbitkan lagi oleh yt-dlp/FFmpeg-Builds, jadi alamatnya 404. Kini Ovner memakai build
  `win64-gpl-shared` (±87 MB, setengah dari build statis) dengan dua sumber cadangan, dan memasang
  seluruh isi folder `bin` (ffmpeg, **ffprobe**, DLL) ke `tools\ffmpeg`. ffprobe juga dibutuhkan
  yt-dlp untuk mengubah ke MP3/OPUS/FLAC.
- **"Telusuri video gagal: Operation aborted (0x80004004)".** Akar masalah: dua penelusuran
  berjalan bersamaan saat yt-dlp belum terpasang, keduanya menulis ke berkas sementara yang sama.
  Kini pemasangan komponen dikunci (satu per satu) dan diunduh lewat mesin WinHTTP Ovner (tanpa
  cache IE) dengan tampilan MB yang sudah diterima.
- **YouTube butuh Deno.** Sejak yt-dlp 2025.11.12 YouTube memerlukan runtime JavaScript; Ovner
  memasang Deno (±43 MB, sekali) ke `tools` dan memberikannya ke yt-dlp (`--js-runtimes`).
  yt-dlp juga diperbarui otomatis (`yt-dlp -U`) bila sudah lebih dari 3 hari.
- **Daftar unduhan:** kabar "Menyiapkan komponen…" tidak lagi menimpa nama berkas; alasan gagal
  tampil di baris status ("Gagal / dibatalkan: …"). Entri lama yang rusak dirapikan otomatis.
- Halaman tanpa format audio terpisah (pemutar video biasa): pilihan audio diambil dari videonya.
- Penghentian respons PDF yang disengaja (jalur Ovner lebih cepat) tidak lagi tercatat sebagai
  galat di Diagnostik.

## Perbaikan 0.8.15

- **Tombol "Unduh video" langsung di atas video** (seperti panel unduh Mister Download Manager).
  Saat kursor di atas video (YouTube dan situs lain), tombol muncul di pojok kanan atas video.
  Menu ▾: *Unduh video*, *Sembunyikan di video ini*, *Jangan di situs ini*, *Pengaturan tombol…*.
  Klik tombol tidak menjeda/memutar video. Di YouTube hanya muncul di halaman video (bukan
  pratinjau di beranda). Pengaturan → Unduhan: *Tombol unduh di atas video* dan daftar situs
  yang dikecualikan.
- **Dialog "Unduh video dari halaman"** (juga dari tombol video di toolbar):
  - Alamat halaman + *Telusuri*, judul video, daftar semua kualitas yang tersedia (mis. 1080p,
    720p, 480p, 360p MP4) dengan ukurannya, plus *Audio saja*: M4A asli, MP3 VBR, MP3
    320/256/192/128 kbps, OPUS, FLAC (≈ = perkiraan).
  - *Simpan ke* (folder Video/Music otomatis, tombol … untuk memilih folder), *Nama berkas*
    (ekstensi mengikuti pilihan), *Subtitle* (buatan pemilik video, atau otomatis Indonesia/
    Inggris) dan *Masukkan ke video*.
  - *Unduh sekarang*, *Nanti* (masuk daftar unduhan sebagai dijeda, tetap ada setelah Ovner
    ditutup; klik ▶ untuk mulai), *Batal*. Klik dua kali pada pilihan = unduh sekarang.

## Perbaikan 0.8.14

- **Info RAM/CPU/GPU di judul muncul di semua jendela.** Dulu hanya jendela pertama yang punya
  tampilan info di bilah judul; kini tiap jendela punya sendiri.
- **Tab bisa diseret antar jendela.** Seret tab keluar lalu jatuhkan di baris tab (atau tab
  vertikal) jendela Ovner lain: tab pindah ke sana di posisi jatuhnya, halamannya tidak dimuat
  ulang. Jendela asal yang menjadi kosong ditutup. Dijatuhkan di tempat lain = jendela baru.
- **Jendela yang diminimalkan lebih hemat:**
  - Setelah 30 detik diminimalkan, tab-tabnya disembunyikan dan ditangguhkan (seperti di tray),
    kecuali yang memutar media, memuat, atau mengunduh. Dibangunkan lagi saat jendela dibuka.
  - Tab latar di jendela yang diminimalkan/di tray ditidurkan 3x lebih cepat (minimal 2 menit).
- **Mode efisiensi per jendela:** proses halaman yang semua tabnya tersembunyi (tab latar atau
  jendela diminimalkan) diberi mode efisiensi Windows (EcoQoS, prioritas dan memori rendah),
  sementara jendela lain yang terlihat tetap cepat. Begitu tabnya dibuka, prosesnya langsung normal.
- **Lebih sedikit pengecekan saat diam:** bila tidak ada gerakan mouse/keyboard selama 1 menit,
  pengukuran RAM/CPU berjalan tiap 3 detik (bukan tiap detik); di halaman yang tersembunyi,
  pemindai iklan kosmetik dan pengumpul kelas halaman bekerja tiap 2 detik (bukan 150–250 ms).

## Perbaikan 0.8.13

- **Banyak jendela (side by side).** Ovner kini bisa dibuka di beberapa jendela sekaligus:
  - ☰ → *Jendela baru* (Ctrl+N) atau *Jendela privat baru*; menu tray → *Jendela baru*.
  - Membuka Ovner lagi dari ikon/menu Start membuka jendela baru (bila Ovner di tray, jendelanya
    ditampilkan lagi). `ovner.exe --new-window` juga membuka jendela baru.
  - Klik kanan tab → *Pindahkan ke jendela baru*, atau seret tab keluar dari baris tab (ke bawah
    halaman) / dari tab vertikal (ke samping). Halaman tetap hidup, tidak dimuat ulang.
  - ☰ → *Gabungkan semua jendela ke sini* mengembalikan semua tab ke satu jendela.
  - Semua jendela memakai satu proses dan mesin WebView2 yang sama (hemat RAM), dan dipulihkan
    saat Ovner dibuka lagi. Task Manager Ovner menampilkan tab dari semua jendela. Menutup jendela
    (X) hanya menutup jendela itu; jendela terakhir mengikuti setelan tray seperti biasa.
  - Ctrl+N kini membuka jendela baru (dulu tab baru; tab baru tetap Ctrl+T).
- **Update bertanda tangan (Ed25519).** Berkas `ovner-latest.json` ditandatangani CI dengan kunci
  privat yang hanya ada di secret GitHub `OVNER_SIGN_KEY`; Ovner memeriksanya dengan kunci publik
  di dalam programnya sebelum mengunduh. Sejak update bertanda tangan pertama diterima, update
  tanpa tanda tangan atau dengan tanda tangan salah ditolak, walaupun rilis GitHub diubah orang lain.
- **Cek WebView2 terbaru dari Microsoft.** Ovner membaca versi Edge/WebView2 Stable terbaru dari
  API resmi Microsoft (`edgeupdates.microsoft.com`, paling sering sehari sekali) dan
  membandingkannya dengan WebView2 yang terpasang: terlihat di *Tentang Ovner*, Diagnostik (uji
  baru *Cek WebView2 terbaru*), dan log. Diagnostik dan *Tentang* juga menampilkan versi komponen
  (wry, tao, webview2-com, windows, adblock, tray-icon, ed25519-dalek).
- **Lebih hemat saat tidak dipakai:** bila semua jendela diminimalkan/di tray dan tidak ada unduhan,
  pekerjaan berkala Ovner berjalan tiap 5 detik (bukan tiap detik) dan pemantau proses berhenti;
  tab di jendela yang diminimalkan tidak diukur. Komponen diperbarui (base64 0.23, psl terbaru).

## Perbaikan 0.8.12

- **Tab bisa diseret** untuk memindah urutannya, di baris tab atas maupun di tab vertikal (bilah
  samping). Klik biasa tetap memindah ke tab itu; seret dimulai setelah bergeser sedikit.
- **Kartu PDF SIPP mulai lebih cepat.** Log 0.8.11: server menjawab halaman setelah 11 detik,
  tetapi jalur Ovner baru menerima data ±15 detik kemudian karena server membuat PDF lagi untuk
  jalur itu. Kini Ovner mengingat alamat yang pernah menghasilkan PDF besar (`pdf_endpoints.json`);
  saat alamat itu dibuka lagi, jalur Ovner langsung mulai bersamaan dengan halaman, sehingga
  semua koneksi menunggu server di waktu yang sama. Bila ternyata server tidak mengirim PDF
  (mis. sesi habis), jalur itu langsung dihentikan.

## Perbaikan 0.8.11

- **Kartu PDF SIPP lebih cepat selesai dan tidak mengulang dari nol.** Log 0.8.10: jalur halaman
  33 KB/dtk, koneksi Ovner 154 KB/dtk (jadi jalur halaman dihentikan, benar), tetapi koneksi Ovner
  diputus server di 99,8% lalu mengulang dari nol (±200 dtk lagi). Kini:
  - **Dua jalur koneksi Ovner** (jalur kedua 4 detik kemudian) mengunduh berkas yang sama; yang
    pertama selesai utuh dipakai, yang lain dihentikan. Bila satu jalur diputus, jalur lain sudah
    hampir selesai sehingga tidak mulai dari nol; jalur yang putus langsung diulang.
  - Jalur Ovner langsung dimulai begitu server mulai mengirim (dulu menunggu 6 detik), dan
    keputusan jalur mana yang lebih cepat diambil setelah 10 detik (dulu 15).
  - Bila jalur Ovner sempat dihentikan karena jalur halaman lebih cepat lalu jalur halaman putus,
    jalur Ovner dijalankan lagi.
- Catatan: server kartu SIPP tidak mendukung unduhan per bagian (Range), jadi satu berkas tidak
  bisa dibagi ke 8 koneksi seperti Google Drive; kecepatan per koneksi ditentukan server.

## Perbaikan 0.8.10

- **Kartu PDF SIPP: koneksi kedua seperti download manager.** Log 0.8.9 menunjukkan PDF gagal
  di 83% (15,1 dari 18,2 MB) setelah 423 detik, padahal tidak ada unduhan lain: server SIPP
  mengirim tiap koneksi pelan (±35 KB/dtk lewat mesin halaman, ±78 KB/dtk di Mister Download
  Manager), tidak mendukung Range/lanjut, dan memutus koneksi yang terlalu lama. Kini selain
  membaca respons halaman, Ovner membuka koneksi kedua sendiri (WinHTTP, dengan cookie dan
  Referer situs, menunggu server membuat PDF sampai 3 menit) dan mengulanginya sampai 3 kali bila
  putus. Setelah keduanya berjalan ±15 detik, yang jelas lebih lambat dihentikan agar yang lebih
  cepat dapat seluruh bandwidth; mana pun yang selesai utuh lebih dulu (dicek ukuran dan tanda
  `%PDF`) itulah yang disimpan. Bila respons halaman putus, unduhan tidak lagi gagal selama
  koneksi kedua masih berjalan.
- Kolom koneksi menunjukkan jumlah jalur PDF yang aktif; log mencatat kecepatan dan sisa waktu
  masing-masing, serta tajuk server (Accept-Ranges, ETag, Last-Modified).

## Perbaikan 0.8.9

- **Kartu PDF SIPP cepat dan tidak gagal di tengah jalan.** Penyebab lambat (±35 KB/dtk) dan
  "tiba-tiba gagal": PDF hanya satu koneksi, berebut bandwidth dengan ISO 8 koneksi yang sedang
  diunduh, dan saat Ovner diminimalkan mode efisiensi Windows (EcoQoS, prioritas rendah) juga
  memperlambat proses jaringan dan pembacaan PDF; transfer yang terlalu lama lalu diputus server.
  Kini selama PDF berjalan, unduhan mesin Ovner lain **dijeda sementara** dan dilanjutkan otomatis
  setelahnya, mode efisiensi dan perapian RAM **tidak dijalankan selama ada unduhan**.
- **Langsung tampil saat tab kartu terbuka.** Server SIPP butuh ±1 menit membuat PDF 522 kartu
  sebelum mengirim byte pertama (ukurannya sudah diketahui di awal, artinya PDF dibuat utuh dulu
  di server; Chrome dan download manager lain juga menunggu selama itu). Halaman yang alamatnya
  mirip unduhan (`download`, `cetak`, `.pdf`, `export`, `unduh`) kini langsung muncul di daftar
  unduhan sebagai "Menghubungkan…" dengan waktu berjalan, bisa dibatalkan, dan begitu server
  mengirim, PDF langsung dibaca dari respons yang sama (tidak meminta ulang ke server).
- Log mencatat setiap PDF: ukuran, lama, kecepatan, dan alasan bila gagal.
- **Pemasang baru**: pilihan Install (Program Files, admin) atau Portable (tanpa admin). Update
  instalasi Program Files diunduh di latar lalu dipasang dengan izin admin saat Ovner dibuka ulang.

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
| Ctrl+N | Jendela baru |
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
[Releases](https://github.com/profrendi/ovner/releases/latest) and run it, then choose **Install**
(Program Files for all users, needs admin; updates ask for admin too) or **Portable** (no admin,
any folder, data next to the exe). No Visual C++ Redistributable is needed.
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
