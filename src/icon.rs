//! Ikon Ovner digambar lewat kode (tanpa file aset): lingkaran gradasi biru-toska dengan
//! cincin putih (huruf "O") dan tameng kecil oranye (pemblokir iklan).

/// Piksel RGBA ikon ukuran `size`×`size`, dengan tepi halus (anti-alias).
pub fn rgba(size: u32) -> Vec<u8> {
    let n = size as f32;
    let c = (n - 1.0) / 2.0;
    let mut px = Vec::with_capacity((size * size * 4) as usize);
    // Cakupan halus: 1 di dalam, 0 di luar, transisi selebar 1 piksel.
    let cover = |d: f32, r: f32| (r - d + 0.5).clamp(0.0, 1.0);
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32, y as f32);
            let d = ((fx - c).powi(2) + (fy - c).powi(2)).sqrt();
            let outer = n / 2.0 - 0.5;
            let a_bg = cover(d, outer);
            // gradasi diagonal: biru (kiri atas) -> toska (kanan bawah)
            let t = ((fx + fy) / (2.0 * n)).clamp(0.0, 1.0);
            let bg = [
                (24.0 + (14.0 - 24.0) * t) / 255.0,
                (99.0 + (166.0 - 99.0) * t) / 255.0,
                (220.0 + (160.0 - 220.0) * t) / 255.0,
            ];
            // cincin putih "O"
            let ring_r = n * 0.29;
            let ring_w = (n * 0.085).max(1.2);
            let a_ring = cover((d - ring_r).abs(), ring_w);
            // tameng oranye kecil di kanan bawah
            let (sx, sy, sr) = (n * 0.72, n * 0.72, n * 0.17);
            let ds = ((fx - sx).powi(2) + (fy - sy).powi(2)).sqrt();
            let a_shield = cover(ds, sr);
            let a_shield_edge = cover(ds, sr + (n * 0.05).max(1.0));

            let mut rgb = bg;
            rgb = mix(rgb, [1.0, 1.0, 1.0], a_ring);
            rgb = mix(rgb, [1.0, 1.0, 1.0], a_shield_edge);
            rgb = mix(rgb, [1.0, 0.55, 0.1], a_shield);
            let alpha = a_bg.max(a_shield_edge);
            px.extend_from_slice(&[
                (rgb[0] * 255.0).round() as u8,
                (rgb[1] * 255.0).round() as u8,
                (rgb[2] * 255.0).round() as u8,
                (alpha * 255.0).round() as u8,
            ]);
        }
    }
    px
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// File .ico berisi beberapa ukuran PNG (didukung Windows Vista ke atas).
pub fn ico(sizes: &[u32]) -> Vec<u8> {
    let images: Vec<(u32, Vec<u8>)> = sizes.iter().map(|&s| (s, png(s, s, &rgba(s)))).collect();
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (s, data) in &images {
        let dim = if *s >= 256 { 0 } else { *s as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in &images {
        out.extend_from_slice(data);
    }
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// Enkode RGBA ke PNG (deflate "stored", tanpa kompresi). Cukup untuk ikon kecil.
pub fn png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((width * 4 + 1) as usize * height as usize);
    for row in rgba.chunks((width * 4) as usize) {
        raw.push(0); // filter: none
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    let mut chunks = raw.chunks(65535).peekable();
    if chunks.peek().is_none() {
        z.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    while let Some(c) = chunks.next() {
        z.push(u8::from(chunks.peek().is_none()));
        let len = c.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(c);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut c = kind.to_vec();
        c.extend_from_slice(data);
        out.extend_from_slice(&c);
        out.extend_from_slice(&crc32(&c).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksums() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn icon_shapes() {
        let px = rgba(32);
        assert_eq!(px.len(), 32 * 32 * 4);
        let p = png(32, 32, &px);
        assert_eq!(&p[1..4], b"PNG");
        let ico = ico(&[16, 32]);
        assert_eq!(&ico[0..6], &[0, 0, 1, 0, 2, 0]);
    }
}
