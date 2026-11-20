# Ukur RAM semua rilis Ovner dengan cara yang sama persis (dijalankan di GitHub Actions,
# workflow "RAM lintas versi"). Tiap versi: unduh Ovner-Portable.zip, jalankan bersih
# (folder baru), ukur RAM saat diam (1 tab) lalu dengan 10 halaman uji lokal, matikan semua
# proses, lanjut ke versi berikutnya. Versi terbaru diukur dua kali (awal & akhir) untuk melihat
# selisih acak mesin.
#
# RAM = "Working Set - Private" (kolom Memory di Task Manager) dari ovner.exe + semua
# msedgewebview2.exe; median 5 sampel dalam 10 detik.
param([string]$Versions = "semua", [int]$Tabs = 10)
$ErrorActionPreference = "Continue"
$root = "C:\ovner-bench"
Remove-Item -Recurse -Force $root -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$root\site" | Out-Null

# Halaman uji: teks, tabel, gambar SVG (sama untuk semua versi).
foreach ($i in 1..$Tabs) {
  $rows = (1..40 | ForEach-Object { "<tr><td>Baris $_</td><td>$('lorem ipsum dolor sit amet ' * 4)</td></tr>" }) -join ""
  $body = "<!doctype html><meta charset=utf-8><title>Halaman $i</title><h1>Halaman uji $i</h1><p>$('Teks contoh untuk mengukur RAM browser. ' * 80)</p><table>$rows</table><svg width=400 height=200><circle cx=100 cy=100 r=80 fill=teal /></svg>"
  Set-Content -Path "$root\site\p$i.html" -Value $body -Encoding utf8
}
$srv = Start-Process python -ArgumentList "-m", "http.server", "8765", "--bind", "127.0.0.1" -WorkingDirectory "$root\site" -PassThru -WindowStyle Hidden
Start-Sleep 3
$urls = 1..$Tabs | ForEach-Object { "http://127.0.0.1:8765/p$_.html" }

function Stop-Ovner {
  Get-Process ovner, msedgewebview2 -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
  Start-Sleep 3
}

function Measure-Ram {
  $s = @()
  for ($k = 0; $k -lt 5; $k++) {
    $ps = Get-CimInstance Win32_PerfFormattedData_PerfProc_Process | Where-Object { $_.Name -like 'ovner*' -or $_.Name -like 'msedgewebview2*' }
    $tot = ($ps | Measure-Object WorkingSetPrivate -Sum).Sum
    $ov = ($ps | Where-Object { $_.Name -like 'ovner*' } | Measure-Object WorkingSetPrivate -Sum).Sum
    $ovc = ($ps | Where-Object { $_.Name -like 'ovner*' } | Measure-Object PrivateBytes -Sum).Sum
    $cm = ($ps | Measure-Object PrivateBytes -Sum).Sum
    $s += [pscustomobject]@{ tot = [int]($tot / 1MB); ov = [int]($ov / 1MB); ovc = [int]($ovc / 1MB); cm = [int]($cm / 1MB); n = @($ps).Count }
    Start-Sleep 2
  }
  $sorted = $s | Sort-Object tot
  return $sorted[[int][math]::Floor($sorted.Count / 2)]
}

function Count-Renderers {
  @(Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Where-Object { $_.CommandLine -match '--type=renderer' }).Count
}

$all = gh api "repos/$env:GITHUB_REPOSITORY/releases?per_page=100" --paginate --jq '.[].tag_name' | Where-Object { $_ -match '^ovner-v\d+\.\d+\.\d+$' }
$all = $all | Sort-Object { [version]($_ -replace '^ovner-v', '') }
if ($Versions -ne "semua" -and $Versions.Trim() -ne "") {
  $want = $Versions.Split(",") | ForEach-Object { "ovner-v" + $_.Trim().TrimStart("v") }
  $all = $all | Where-Object { $want -contains $_ }
}
$latest = $all | Select-Object -Last 1
$plan = @($latest) + $all   # terbaru diukur di awal juga (pembanding selisih acak)
Write-Host "Mengukur $($plan.Count) kali: $($plan -join ', ')"

$res = @()
$n = 0
foreach ($tag in $plan) {
  $n++
  $ver = $tag -replace '^ovner-v', ''
  $dir = "$root\v$ver-$n"
  New-Item -ItemType Directory -Force $dir | Out-Null
  gh release download $tag -p Ovner-Portable.zip -D $dir -R $env:GITHUB_REPOSITORY 2>$null
  $zip = Join-Path $dir "Ovner-Portable.zip"
  if (-not (Test-Path $zip)) { Write-Host "${ver}: Ovner-Portable.zip tidak ada"; continue }
  Expand-Archive $zip -DestinationPath $dir -Force
  $exe = Get-ChildItem $dir -Recurse -Filter ovner.exe | Select-Object -First 1
  if (-not $exe) { Write-Host "${ver}: ovner.exe tidak ada di zip"; continue }
  Stop-Ovner
  Remove-Item -Recurse -Force "$env:LOCALAPPDATA\Ovner", "$env:APPDATA\Ovner" -ErrorAction SilentlyContinue

  # A: diam, 1 tab (halaman awal)
  $p = Start-Process $exe.FullName -ArgumentList "--portable" -WorkingDirectory $exe.DirectoryName -PassThru
  Start-Sleep 25
  $a = Measure-Ram
  Stop-Ovner
  Remove-Item -Recurse -Force "$env:LOCALAPPDATA\Ovner", "$env:APPDATA\Ovner" -ErrorAction SilentlyContinue

  # B: 10 halaman uji dibuka satu per satu lewat instance yang sudah jalan (seperti mengeklik
  # tautan dari aplikasi lain). Membuka banyak alamat sekaligus di baris perintah hanya memuat
  # tab pertama di semua versi (hasil uji 2026-11-20: renderer selalu 2).
  $p = Start-Process $exe.FullName -ArgumentList "--portable" -WorkingDirectory $exe.DirectoryName -PassThru
  Start-Sleep 10
  foreach ($u in $urls) { Start-Process $exe.FullName -ArgumentList @("--portable", $u) -WorkingDirectory $exe.DirectoryName; Start-Sleep 2 }
  Start-Sleep 25
  $b = Measure-Ram
  $r = Count-Renderers
  Stop-Ovner

  $label = if ($n -eq 1) { "$ver (awal)" } else { $ver }
  $row = [pscustomobject]@{
    versi = $label; diam_total = $a.tot; diam_ovner = $a.ov; diam_ovner_komit = $a.ovc; diam_proses = $a.n
    tab10_total = $b.tot; tab10_ovner = $b.ov; tab10_komit_total = $b.cm; tab10_renderer = $r; tab10_proses = $b.n
  }
  $res += $row
  Write-Host ("{0,-14} diam {1,4} MB (ovner.exe {2,3} / komit {3,3}, {4,2} proses) | 10 halaman {5,4} MB (ovner.exe {6,3}, renderer {7,2}, {8,2} proses)" -f $label, $a.tot, $a.ov, $a.ovc, $a.n, $b.tot, $b.ov, $r, $b.n)
  Remove-Item -Recurse -Force $dir -ErrorAction SilentlyContinue
}
Stop-Process -Id $srv.Id -Force -ErrorAction SilentlyContinue

$res | Export-Csv "$root\hasil.csv" -NoTypeInformation -Encoding utf8
$md = @("## RAM Ovner lintas versi (MB, Working Set - Private, median 5 sampel)", "",
  "| Versi | Diam total | ovner.exe | ovner.exe komit | Proses | 10 halaman total | ovner.exe | Renderer | Proses |",
  "|---|---:|---:|---:|---:|---:|---:|---:|---:|")
foreach ($x in $res) { $md += "| $($x.versi) | $($x.diam_total) | $($x.diam_ovner) | $($x.diam_ovner_komit) | $($x.diam_proses) | $($x.tab10_total) | $($x.tab10_ovner) | $($x.tab10_renderer) | $($x.tab10_proses) |" }
$md += ""
$md += "Renderer < 10 pada kolom 10 halaman = versi itu tidak membuka alamat dari baris perintah, menunda pemuatan tab, atau sudah menidurkan tab (fitur hemat RAM)."
$md | Set-Content "$root\hasil.md" -Encoding utf8
$md | Out-File -Append -FilePath $env:GITHUB_STEP_SUMMARY -Encoding utf8
Get-Content "$root\hasil.md"
