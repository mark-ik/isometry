//! Minimal, dependency-free PNG + base64 encoder for baked sheets.
//!
//! A baked [`Sheet`] binds to the board as a CSS `background-image:
//! url("data:image/png;base64,...")`, the same path isometry's placeholder
//! tileset already uses. Rather than pull an image crate, this encodes PNG
//! with *stored* (uncompressed) DEFLATE blocks: no compressor needed, still a
//! valid PNG a browser renders. Sprites are small, so size is a non-issue.

use crate::bake::Sheet;

fn crc32(bytes: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (n, slot) in table.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *slot = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in bytes {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

// zlib stream wrapping `raw` in stored DEFLATE blocks (BTYPE 00), <=65535 each.
fn zlib_stored(raw: &[u8]) -> Vec<u8> {
    let mut z = vec![0x78, 0x01]; // CMF/FLG: deflate, 32K window, check bits ok
    if raw.is_empty() {
        z.push(0x01); // final empty stored block
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&(!0u16).to_le_bytes());
    } else {
        let mut i = 0;
        while i < raw.len() {
            let end = (i + 65535).min(raw.len());
            let block = &raw[i..end];
            z.push(if end == raw.len() { 0x01 } else { 0x00 }); // BFINAL, BTYPE=00
            let len = block.len() as u16;
            z.extend_from_slice(&len.to_le_bytes());
            z.extend_from_slice(&(!len).to_le_bytes());
            z.extend_from_slice(block);
            i = end;
        }
    }
    z.extend_from_slice(&adler32(raw).to_be_bytes());
    z
}

fn encode(sheet: &Sheet) -> Vec<u8> {
    let (w, h) = (sheet.w as usize, sheet.h as usize);
    // Filtered scanlines: a filter-type byte (0 = None) then the RGBA row.
    let mut raw = Vec::with_capacity(h * (1 + w * 4));
    for y in 0..h {
        raw.push(0);
        raw.extend_from_slice(&sheet.rgba[y * w * 4..(y + 1) * w * 4]);
    }
    let mut out = Vec::new();
    out.extend_from_slice(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(sheet.w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(sheet.h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit, RGBA, no compression/filter/interlace
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib_stored(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = ((c[0] as u32) << 16)
            | ((*c.get(1).unwrap_or(&0) as u32) << 8)
            | (*c.get(2).unwrap_or(&0) as u32);
        s.push(T[((n >> 18) & 63) as usize] as char);
        s.push(T[((n >> 12) & 63) as usize] as char);
        s.push(if c.len() > 1 { T[((n >> 6) & 63) as usize] as char } else { '=' });
        s.push(if c.len() > 2 { T[(n & 63) as usize] as char } else { '=' });
    }
    s
}

impl Sheet {
    /// Encode as a PNG byte stream (RGBA, uncompressed DEFLATE).
    pub fn to_png(&self) -> Vec<u8> {
        encode(self)
    }

    /// Encode as a `data:image/png;base64,...` URI for a CSS `background-image`.
    pub fn to_png_data_uri(&self) -> String {
        format!("data:image/png;base64,{}", base64(&encode(self)))
    }
}
