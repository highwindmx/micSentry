//! 麦克风图标绘制：**SDF（有符号距离场）+ 解析抗锯齿**，几何按标准话筒图标重建。
//!
//! # 为什么推翻原来的画法
//! 旧版是"椭圆 + 两根竖线 + 半圆 + 1px 斜杠"的拼凑：所有边缘都是硬边（无抗锯齿），
//! 斜杠只有 1 个像素且是"楼梯"，支架是两条 2px 直线 —— 在 16px 托盘尺寸下糊成一团，
//! 在大尺寸下也能看出线段拼接的接缝。
//!
//! 现在改成**工业标准话筒轮廓**的几何重建（胶囊头 + 下半环托架 + 立柱 + 圆角底座，
//! 与 FontAwesome `microphone` 同一套比例关系），用距离场算每个像素的覆盖率做抗锯齿。
//! 好处：任意尺寸（托盘 16px ~ 悬浮窗 96px+）边缘都平滑，且不依赖字体或图标文件。
//!
//! # 三态
//! 以前只有两态：读不到设备时按"未静音"画绿色 —— 用户看到的是
//! "程序明明找不到麦克风，却告诉我麦克风是开着的"。现在有独立的**未知态（灰）**。
//!
//! 设计坐标系：384 × 512（宽 × 高），再等比缩放贴进 N × N 的正方形，四边留白。

use slint::Rgba8Pixel;

/// 图标三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    /// 麦克风可用（未静音）
    On,
    /// 已静音
    Off,
    /// 状态未知（读取失败 / 没找到可用设备）
    Unknown,
}

impl State {
    /// 由快照里的 `muted` 得出图标状态；`None`（读取失败）= 未知态。
    pub fn from_muted(muted: Option<bool>) -> Self {
        match muted {
            Some(true) => State::Off,
            Some(false) => State::On,
            None => State::Unknown,
        }
    }

    /// 主色。刻意选**中调亮度**：深色任务栏（默认）与浅色任务栏上都要看得清。
    /// 旧版绿色 `#107C10` 太暗，在深色任务栏上接近发黑 —— 这是"图标难看"的原因之一。
    pub fn rgb(self) -> (u8, u8, u8) {
        match self {
            State::On => (0x22, 0xB1, 0x4C),      // 绿 = 麦克风开着
            State::Off => (0xE8, 0x11, 0x23),     // 红 = 已静音
            State::Unknown => (0x9A, 0xA0, 0xA6), // 灰 = 未知（刻意非红非绿）
        }
    }

    /// 是否画"静音斜杠"（只有静音态画）。
    fn has_slash(self) -> bool {
        matches!(self, State::Off)
    }
}

// ---------------------------------------------------------------------------
// 设计坐标系几何（384 × 512）
// ---------------------------------------------------------------------------

const DW: f32 = 384.0;
const DH: f32 = 512.0;

/// 话筒头：竖直胶囊，中线 (192,96) → (192,256)，半径 96
const HEAD_A: (f32, f32) = (192.0, 96.0);
const HEAD_B: (f32, f32) = (192.0, 256.0);
const HEAD_R: f32 = 96.0;

/// 托架：下半圆环，圆心 (192,256)，中半径 152，笔画半宽 24
const CRADLE_C: (f32, f32) = (192.0, 256.0);
const CRADLE_R: f32 = 152.0;
const CRADLE_HW: f32 = 24.0;

/// 立柱：托架底部 → (192,464)，半径 24
const STEM_A: (f32, f32) = (192.0, 408.0);
const STEM_B: (f32, f32) = (192.0, 464.0);
const STEM_R: f32 = 24.0;

/// 底座：横胶囊 (120,488) → (264,488)，半径 24
const BASE_A: (f32, f32) = (120.0, 488.0);
const BASE_B: (f32, f32) = (264.0, 488.0);
const BASE_R: f32 = 24.0;

/// 静音斜杠：45° 穿过设计框。受宽度限制，最长只能到 (0,64) → (384,448)。
const SLASH_A: (f32, f32) = (0.0, 64.0);
const SLASH_B: (f32, f32) = (384.0, 448.0);
/// 斜杠笔画半宽（总宽约 44 设计单位）
const SLASH_HW: f32 = 22.0;
/// 斜杠两侧挖空的间隙宽度：让"切断"看得出来（标准 mic-slash 图标的做法）
const SLASH_GAP: f32 = 13.0;

/// 点到线段的距离减去半径 = 胶囊（含圆头）的有符号距离。
#[inline]
fn sd_capsule(px: f32, py: f32, a: (f32, f32), b: (f32, f32), r: f32) -> f32 {
    let (bax, bay) = (b.0 - a.0, b.1 - a.1);
    let (pax, pay) = (px - a.0, py - a.1);
    let len2 = bax * bax + bay * bay;
    let h = if len2 > 0.0 {
        ((pax * bax + pay * bay) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (dx, dy) = (pax - bax * h, pay - bay * h);
    (dx * dx + dy * dy).sqrt() - r
}

/// 下半圆环（带圆头端点）的有符号距离。`hw` = 笔画半宽。
#[inline]
fn sd_lower_arc(px: f32, py: f32, c: (f32, f32), r: f32, hw: f32) -> f32 {
    let (dx, dy) = (px - c.0, py - c.1);
    if dy >= 0.0 {
        // 角度在下半环范围内：到以 c 为心、半径 r 的圆环的距离
        ((dx * dx + dy * dy).sqrt() - r).abs() - hw
    } else {
        // 超出角度范围：到最近端点圆头的距离
        let d1 = ((px - (c.0 - r)) * (px - (c.0 - r)) + dy * dy).sqrt();
        let d2 = ((px - (c.0 + r)) * (px - (c.0 + r)) + dy * dy).sqrt();
        d1.min(d2) - hw
    }
}

/// 话筒主体：各部件取并集（并集 = 取最小距离）。
///
/// `cradle_hw` / `stem_r` 由调用方给出（小尺寸下会被加粗，见 `render` 里的光学修正）。
#[inline]
fn sd_mic(px: f32, py: f32, cradle_hw: f32, stem_r: f32) -> f32 {
    sd_capsule(px, py, HEAD_A, HEAD_B, HEAD_R)
        .min(sd_lower_arc(px, py, CRADLE_C, CRADLE_R, cradle_hw))
        .min(sd_capsule(px, py, STEM_A, STEM_B, stem_r))
        .min(sd_capsule(px, py, BASE_A, BASE_B, stem_r.max(BASE_R)))
}

/// 距离场（像素单位）→ 覆盖率 0..1。1px 线性过渡即解析抗锯齿。
#[inline]
fn coverage(d_px: f32) -> f32 {
    (0.5 - d_px).clamp(0.0, 1.0)
}

/// 渲染成**直通（非预乘）RGBA8**，尺寸 `size × size`。
///
/// `slint::Image::from_rgba8` 与 `tray_icon::Icon::from_rgba` 都吃直通 RGBA，
/// 不要预乘（slint 另有 `from_rgba8_premultiplied` 用于预乘数据）。
pub fn render(state: State, size: u32) -> Vec<u8> {
    let size = size.max(8);
    let (r, g, b) = state.rgb();

    // 四边留白：小尺寸贴着边画（16px 时留白 1px 就是 12% 的损失），大尺寸约 5%
    let pad = (size as f32 * 0.05).max(if size <= 20 { 0.5 } else { 1.0 });
    let scale = (size as f32 - 2.0 * pad) / DH;
    let off_x = (size as f32 - DW * scale) / 2.0;
    let off_y = pad;

    // 小尺寸"光学修正"：细笔画必须加粗到能连成一条线，
    // 否则 16px 时托架（原本 1.3px 的圆环）会被 AA 覆盖率打碎成几个点。
    // 这是图标设计的常规做法：同一个造型，小尺寸用更粗的笔画去"顶住"像素栅格。
    let min_stroke_px = if size <= 20 {
        2.0
    } else if size <= 28 {
        1.6
    } else {
        0.0
    };
    let bump = |design_hw: f32| {
        if min_stroke_px <= 0.0 {
            design_hw
        } else {
            design_hw.max(min_stroke_px * 0.5 / scale)
        }
    };
    let cradle_hw = bump(CRADLE_HW);
    let stem_r = bump(STEM_R);

    // 斜杠同样有下限：16px 时不足 1px 的斜杠几乎看不见
    let slash_hw = if state.has_slash() {
        bump(SLASH_HW.max(0.62 / scale))
    } else {
        SLASH_HW
    };

    let mut out = vec![0u8; (size as usize * size as usize) * 4];
    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 + 0.5 - off_x) / scale;
            let dy = (y as f32 + 0.5 - off_y) / scale;

            let mut a = coverage(sd_mic(dx, dy, cradle_hw, stem_r) * scale);
            if state.has_slash() {
                // 沿斜杠两侧挖出透明间隙，再把斜杠画在间隙中央 —— 得到"被切断"的观感
                let d_slash = sd_capsule(dx, dy, SLASH_A, SLASH_B, slash_hw) * scale;
                let d_gap =
                    sd_capsule(dx, dy, SLASH_A, SLASH_B, slash_hw + SLASH_GAP) * scale;
                a = (a * (1.0 - coverage(d_gap))).max(coverage(d_slash));
            }
            if a <= 0.0 {
                continue;
            }

            let i = ((y as usize * size as usize) + x as usize) * 4;
            out[i] = r;
            out[i + 1] = g;
            out[i + 2] = b;
            out[i + 3] = (a * 255.0 + 0.5) as u8;
        }
    }
    out
}

/// 生成 slint 悬浮窗用的图（`size × size` 像素，直通 RGBA）。
pub fn image(state: State, size: u32) -> slint::Image {
    let size = size.max(8);
    let rgba = render(state, size);
    let mut buf = slint::SharedPixelBuffer::<Rgba8Pixel>::new(size, size);
    {
        let slice = buf.make_mut_slice();
        for (i, p) in slice.iter_mut().enumerate() {
            let o = i * 4;
            *p = Rgba8Pixel {
                r: rgba[o],
                g: rgba[o + 1],
                b: rgba[o + 2],
                a: rgba[o + 3],
            };
        }
    }
    slint::Image::from_rgba8(buf)
}

/// 生成托盘图标；失败返回 `None` 并写日志（GUI 程序 panic 等于无声退出）。
pub fn icon(state: State, size: u32) -> Option<tray_icon::Icon> {
    let size = size.max(8);
    match tray_icon::Icon::from_rgba(render(state, size), size, size) {
        Ok(i) => Some(i),
        Err(e) => {
            crate::log::line(&format!("ERROR Icon::from_rgba({size}x{size}) 失败: {e}"));
            None
        }
    }
}

/// 托盘图标该渲染成多大：16 逻辑像素 × 当前缩放（100% → 16，125% → 20，150% → 24）。
///
/// 按**原生尺寸**渲染比"渲染 32 再让系统缩到 16"清晰得多 —— 后者是二次重采样，
/// 小尺寸下会明显发虚（旧版就是固定 32）。
pub fn tray_px(scale_factor: f32) -> u32 {
    let sf = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    ((16.0 * sf).round() as i64).clamp(16, 64) as u32
}

/// 悬浮窗图标像素尺寸（窗口逻辑边长 96）。
pub fn overlay_px(scale_factor: f32) -> u32 {
    let sf = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    ((96.0 * sf).round() as i64).clamp(32, 512) as u32
}
