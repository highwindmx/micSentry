//! 极简 PNG 编码器 + 拼图画布（零依赖，只用标准库）。
//!
//! 用途单一：把程序渲染出来的图标落成 PNG，便于**肉眼验收**（`--export-icons`）。
//! 之所以自己写而不引入 `png`/`image` crate：只有一张 RGBA8、无调色板、无隔行，
//! 用 zlib 的 "stored"（不压缩）块几十行就能搞定，不值得为此多一个依赖。

/// 一张 RGBA8 画布（直通 alpha）。
pub struct Canvas {
    pub w: u32,
    pub h: u32,
    /// 长度 = w * h * 4，顺序 R,G,B,A
    pub px: Vec<u8>,
}

impl Canvas {
    pub fn new(w: u32, h: u32, bg: (u8, u8, u8)) -> Self {
        let mut px = vec![0u8; (w as usize * h as usize) * 4];
        for i in (0..px.len()).step_by(4) {
            px[i] = bg.0;
            px[i + 1] = bg.1;
            px[i + 2] = bg.2;
            px[i + 3] = 0xFF;
        }
        Canvas { w, h, px }
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: u32, h: u32, rgb: (u8, u8, u8)) {
        for yy in y..(y + h as i32) {
            if yy < 0 || yy >= self.h as i32 {
                continue;
            }
            for xx in x..(x + w as i32) {
                if xx < 0 || xx >= self.w as i32 {
                    continue;
                }
                let i = ((yy as u32 * self.w + xx as u32) * 4) as usize;
                self.px[i] = rgb.0;
                self.px[i + 1] = rgb.1;
                self.px[i + 2] = rgb.2;
                self.px[i + 3] = 0xFF;
            }
        }
    }

    /// 把一张直通 RGBA 小图合成到 (dx, dy)。`a` 为 0 的像素直接跳过。
    pub fn blit(&mut self, src: &[u8], sw: u32, sh: u32, dx: i32, dy: i32) {
        self.blit_impl(src, sw, sh, dx, dy, 1);
    }

    /// 最近邻放大 `zoom` 倍后合成（用来看清 16px 下的抗锯齿质量）。
    pub fn blit_zoom(&mut self, src: &[u8], sw: u32, sh: u32, dx: i32, dy: i32, zoom: u32) {
        self.blit_impl(src, sw, sh, dx, dy, zoom.max(1));
    }

    fn blit_impl(&mut self, src: &[u8], sw: u32, sh: u32, dx: i32, dy: i32, zoom: u32) {
        for sy in 0..sh {
            for sx in 0..sw {
                let si = ((sy * sw + sx) * 4) as usize;
                let (r, g, b, a) = (src[si], src[si + 1], src[si + 2], src[si + 3]);
                if a == 0 {
                    continue;
                }
                let alpha = a as u32;
                for zy in 0..zoom {
                    for zx in 0..zoom {
                        let x = dx + (sx * zoom + zx) as i32;
                        let y = dy + (sy * zoom + zy) as i32;
                        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
                            continue;
                        }
                        let i = ((y as u32 * self.w + x as u32) * 4) as usize;
                        // 直通 alpha 混合：dst = fg*a + dst*(1-a)
                        self.px[i] = ((r as u32 * alpha + self.px[i] as u32 * (255 - alpha)) / 255) as u8;
                        self.px[i + 1] =
                            ((g as u32 * alpha + self.px[i + 1] as u32 * (255 - alpha)) / 255) as u8;
                        self.px[i + 2] =
                            ((b as u32 * alpha + self.px[i + 2] as u32 * (255 - alpha)) / 255) as u8;
                        self.px[i + 3] = 0xFF;
                    }
                }
            }
        }
    }

    /// 编码成 PNG 字节（8bit RGBA，无压缩存储块）。
    pub fn to_png(&self) -> Vec<u8> {
        let mut raw = Vec::with_capacity((self.h * (1 + self.w * 4)) as usize);
        for y in 0..self.h {
            raw.push(0u8); // 行过滤器：None
            let start = (y * self.w * 4) as usize;
            raw.extend_from_slice(&self.px[start..start + (self.w * 4) as usize]);
        }

        let mut out = Vec::with_capacity(raw.len() + 128);
        out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        let mut ihdr = Vec::with_capacity(13);
        ihdr.extend_from_slice(&self.w.to_be_bytes());
        ihdr.extend_from_slice(&self.h.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8bit、RGBA、无压缩/过滤/隔行
        out.extend_from_slice(&chunk(b"IHDR", &ihdr));
        out.extend_from_slice(&chunk(b"IDAT", &zlib_stored(&raw)));
        out.extend_from_slice(&chunk(b"IEND", &[]));
        out
    }
}

fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 12);
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_in = Vec::with_capacity(4 + data.len());
    crc_in.extend_from_slice(kind);
    crc_in.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_in).to_be_bytes());
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut c: u32 = 0xFFFF_FFFF;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
        }
    }
    c ^ 0xFFFF_FFFF
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// 用 zlib 的 "stored" 块包装裸数据（合法但不压缩，无需 zlib 实现）。
fn zlib_stored(raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    if raw.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]);
    } else {
        let mut i = 0usize;
        while i < raw.len() {
            let n = (raw.len() - i).min(65535);
            let last = if i + n >= raw.len() { 1u8 } else { 0u8 };
            out.push(last);
            out.extend_from_slice(&(n as u16).to_le_bytes());
            out.extend_from_slice(&(!(n as u16)).to_le_bytes());
            out.extend_from_slice(&raw[i..i + n]);
            i += n;
        }
    }
    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}
