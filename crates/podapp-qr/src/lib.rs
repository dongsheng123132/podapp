//! 二维码能力 —— `qr.scan` 与 `qr.encode`。
//!
//! ## 为什么单独一个 crate，而不是加进运行时
//!
//! 这是[能力注册表](podapp_runtime::capability)的第一个真实用例，也是对它的检验：
//! 扫码要引 `rqrr` 和 `qrcode` 两个依赖，而运行时到现在只有四个
//! （serde / serde_json / flate2 / tar）。把它塞进核心，等于让每个只想切个图的宿主
//! 也背上 QR 解码器。
//!
//! 宿主自己决定装不装：
//!
//! ```ignore
//! let caps = Capabilities::builtin().with(podapp_qr::QrCapability);
//! ```
//!
//! **代价要说清楚**：用了 `qr.*` 的程序舱，在没装这个能力的宿主上跑不起来 ——
//! 会拿到 `unknown_capability: qr.scan`。这是可扩展性的固有代价，
//! 换来的是核心不必无限膨胀。错误信息是明确的，不是静默失败。
//!
//! ## 为什么扫码必须在宿主这一侧
//!
//! 桥上的 `pixels` 卡在 100 万像素以内（几十 MB 的 JSON 数字数组过桥是不可接受的），
//! 而整张海报动辄几百万像素 —— 程序舱自己扫整图必被拒。
//! 识别类计算放宿主，和 `ringStats` 是同一个道理。

use podapp_runtime::capability::{CapCtx, Capability};
use podapp_runtime::image::{self, Img};
use podapp_runtime::Cap;
use serde_json::{json, Value};

pub struct QrCapability;

impl Capability for QrCapability {
    fn name(&self) -> &'static str {
        "qr"
    }

    fn handles(&self, verb: &str) -> bool {
        matches!(verb, "qr.scan" | "qr.encode")
    }

    /// 不需要权限：只在内存里算，碰不到网络也碰不到用户的盘。
    /// 要闸的是「图从哪来」和「图往哪去」，不是中间的识别。
    fn required(&self, _verb: &str) -> Option<Cap> {
        None
    }

    fn call(&self, _ctx: &CapCtx, verb: &str, args: &Value) -> Result<Value, String> {
        // 桥用 Proxy 透传位置参数，所以 args 是个数组
        let a = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
        match verb {
            "qr.scan" => scan(&a(0), &a(1)),
            _ => encode(&a(0), &a(1)),
        }
    }
}

/// 扫一张图（或图上的一块）里的二维码。
///
/// 返回**所有**找到的码，不是第一个：海报上可能同时有公众号码和支付码，
/// 只返回一个会让调用方以为另一个不存在。
fn scan(src: &Value, rect: &Value) -> Result<Value, String> {
    let id = src.as_str().ok_or("invalid_input: qr.scan 需要图像句柄")?;
    let img = image::get_img(id)?;

    // 只扫指定区域时先裁一刀。海报整图扫得慢，而调用方通常知道码在哪。
    let img = match crop_of(rect, &img) {
        Some(sub) => sub,
        None => img,
    };

    let mut prepared = rqrr::PreparedImage::prepare_from_greyscale(
        img.w as usize,
        img.h as usize,
        |x, y| luma_at(&img, x, y),
    );
    let mut found = vec![];
    for grid in prepared.detect_grids() {
        // 单个码解不出来不该让整次扫描失败 —— 图上可能有个装饰性的假码
        if let Ok((meta, text)) = grid.decode() {
            let b = grid.bounds;
            found.push(json!({
                "text": text,
                "version": meta.version.0,
                "corners": b.iter().map(|p| json!({ "x": p.x, "y": p.y })).collect::<Vec<_>>(),
            }));
        }
    }
    Ok(json!({ "count": found.len(), "found": found }))
}

/// 生成二维码，返回 PNG data URL。
///
/// 刻意不直接返回图像句柄：调用方拿到 data URL 后走 `pod.image.decode` 入库，
/// 这样「图像句柄从哪来」在整个系统里只有一条路。
fn encode(text: &Value, opts: &Value) -> Result<Value, String> {
    let text = text.as_str().unwrap_or("");
    if text.is_empty() {
        return Err("invalid_input: qr.encode 需要文本".into());
    }
    // 纠错等级钉在 High：二维码要被贴到海报上、可能被压图或部分遮挡，
    // 省那点密度换来的是扫不出来。
    let code = qrcode::QrCode::with_error_correction_level(text, qrcode::EcLevel::H)
        .map_err(|e| format!("invalid_input: 这段文本编不成二维码：{e}"))?;

    let quiet = 4usize; // 静区。少于 4 个模块，很多扫码器就认不出来了
    let scale = opts
        .get("scale")
        .and_then(|v| v.as_u64())
        .unwrap_or(8)
        .clamp(1, 64) as usize;

    let colors = code.to_colors();
    let side = (colors.len() as f64).sqrt() as usize;
    let out_side = (side + quiet * 2) * scale;
    let mut px = vec![255u8; out_side * out_side * 4];
    for (i, c) in colors.iter().enumerate() {
        if *c != qrcode::types::Color::Dark {
            continue;
        }
        let (mx, my) = (i % side, i / side);
        for dy in 0..scale {
            for dx in 0..scale {
                let x = (mx + quiet) * scale + dx;
                let y = (my + quiet) * scale + dy;
                let o = (y * out_side + x) * 4;
                px[o] = 0;
                px[o + 1] = 0;
                px[o + 2] = 0;
            }
        }
    }
    let img = Img { w: out_side as u32, h: out_side as u32, px };
    Ok(json!(format!(
        "data:image/png;base64,{}",
        b64(&image::encode_png(&img))
    )))
}

fn crop_of(rect: &Value, img: &Img) -> Option<Img> {
    let g = |k: &str| rect.get(k)?.as_f64();
    let (x, y, w, h) = (g("x")?, g("y")?, g("w")?, g("h")?);
    let x = (x.max(0.0) as u32).min(img.w.saturating_sub(1));
    let y = (y.max(0.0) as u32).min(img.h.saturating_sub(1));
    let w = (w.max(1.0) as u32).min(img.w - x);
    let h = (h.max(1.0) as u32).min(img.h - y);
    let mut px = vec![0u8; (w * h * 4) as usize];
    for row in 0..h {
        let src = (((y + row) * img.w + x) * 4) as usize;
        let dst = (row * w * 4) as usize;
        px[dst..dst + (w * 4) as usize].copy_from_slice(&img.px[src..src + (w * 4) as usize]);
    }
    Some(Img { w, h, px })
}

/// 单点 RGBA → 灰度。
///
/// 用 Rec.601 亮度权重而不是三通道平均：平均会让红底白码这类配色对比度塌掉，
/// 而那正是海报上最常见的二维码样式。
///
/// 逐点算而不是先摊一整张灰度图：`prepare_from_greyscale` 本来就按需取点，
/// 中间那份缓冲对一张几百万像素的海报是白白多占几 MB。
fn luma_at(img: &Img, x: usize, y: usize) -> u8 {
    let o = (y * img.w as usize + x) * 4;
    let (r, g, b) = (img.px[o] as f32, img.px[o + 1] as f32, img.px[o + 2] as f32);
    (0.299 * r + 0.587 * g + 0.114 * b) as u8
}

fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        for k in 0..4 {
            if k <= c.len() {
                out.push(T[((n >> (18 - 6 * k)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use podapp_runtime::{Capabilities, HeadlessHost};

    fn dispatch(verb: &str, args: Value) -> Result<Value, String> {
        let caps = Capabilities::builtin().with(QrCapability);
        let host = HeadlessHost::new();
        let ctx = CapCtx { pod_id: "test", host: &host, execution_id: "t" };
        caps.dispatch(&ctx, verb, &args)
    }

    /// 生成 → 入库 → 扫回来。这条闭环是整个能力唯一有意义的验收：
    /// 单独验编码只能证明「画出来了」，单独验扫描没有已知答案可比。
    #[test]
    fn a_generated_code_scans_back_to_the_same_text() {
        let text = "https://podapp.net/pods/qr?from=selftest";
        let data_url = dispatch("qr.encode", json!([text, {}])).unwrap();
        let handle = image::dispatch("decode", &json!([data_url])).unwrap();
        let id = handle["id"].as_str().unwrap();

        let r = dispatch("qr.scan", json!([id, null])).unwrap();
        assert_eq!(r["count"], 1, "该扫到且只扫到一个码");
        assert_eq!(r["found"][0]["text"], text, "扫出来的内容和编进去的不一样");
        image::clear_session();
    }

    #[test]
    fn scaling_up_still_scans() {
        // 贴到海报上会被放大。放大后扫不出来就等于没用。
        for scale in [4u64, 8, 16] {
            let text = "PODAPP-SCALE-TEST";
            let url = dispatch("qr.encode", json!([text, { "scale": scale }])).unwrap();
            let h = image::dispatch("decode", &json!([url])).unwrap();
            let r = dispatch("qr.scan", json!([h["id"], null])).unwrap();
            assert_eq!(r["found"][0]["text"], text, "scale={scale} 时扫不出来");
        }
        image::clear_session();
    }

    #[test]
    fn a_blank_image_finds_nothing_rather_than_erroring() {
        // 图上没码是正常情况（用户还没贴），不是错误 —— 报错会让界面弹一个吓人的框
        let blank = Img { w: 200, h: 200, px: vec![255u8; 200 * 200 * 4] };
        let id = image::put_img(blank);
        let r = dispatch("qr.scan", json!([id, null])).unwrap();
        assert_eq!(r["count"], 0);
        assert_eq!(r["found"].as_array().unwrap().len(), 0);
        image::clear_session();
    }

    #[test]
    fn scanning_only_a_region_finds_the_code_inside_it() {
        // 把码贴在大图右下角，只扫那一块也要能找到 —— 海报场景就是这样用的
        let text = "REGION-ONLY";
        let url = dispatch("qr.encode", json!([text, { "scale": 6 }])).unwrap();
        let qr = image::dispatch("decode", &json!([url])).unwrap();
        let qr_img = image::get_img(qr["id"].as_str().unwrap()).unwrap();

        let (w, h) = (900u32, 700u32);
        let mut px = vec![210u8; (w * h * 4) as usize];
        let (ox, oy) = (w - qr_img.w - 20, h - qr_img.h - 20);
        for row in 0..qr_img.h {
            let s = (row * qr_img.w * 4) as usize;
            let d = (((oy + row) * w + ox) * 4) as usize;
            px[d..d + (qr_img.w * 4) as usize]
                .copy_from_slice(&qr_img.px[s..s + (qr_img.w * 4) as usize]);
        }
        let poster = image::put_img(Img { w, h, px });

        let r = dispatch(
            "qr.scan",
            json!([poster, { "x": ox - 10, "y": oy - 10, "w": qr_img.w + 20, "h": qr_img.h + 20 }]),
        )
        .unwrap();
        assert_eq!(r["found"][0]["text"], text, "指定区域里的码没扫到");
        image::clear_session();
    }

    #[test]
    fn empty_text_is_refused() {
        assert!(dispatch("qr.encode", json!(["", {}])).is_err());
    }

    #[test]
    fn the_capability_is_absent_until_a_host_registers_it() {
        // 「可插拔」的另一半：不装就没有，而且报错是明确的，不是静默失败
        let caps = Capabilities::builtin();
        let host = HeadlessHost::new();
        let ctx = CapCtx { pod_id: "test", host: &host, execution_id: "t" };
        let e = caps.dispatch(&ctx, "qr.scan", &json!([])).unwrap_err();
        assert!(e.starts_with("unknown_capability"), "实际: {e}");
    }
}
