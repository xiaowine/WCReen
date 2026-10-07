//! 能力交集与参数选择（**交集由 PC 算**）。
//!
//! 依据 `docs/spec/agent/transport-handshake.md` §5：agent 提供能力表 + 可行性否决，
//! **PC 求交集**（自己的硬解能力 ∩ agent 能力表）+ 选参数 + 下发 —— 因为只有 PC 知道
//! 自己的解码器支持什么。
//!
//! # 选择规则（全部「取第一个支持的」，不静默降级）
//!
//! | 参数 | 规则 |
//! |---|---|
//! | 编码器 | 按 [`Codec::H264`] → [`Codec::Hevc`] 的偏好取**第一个双方都支持**的 |
//! | profile | agent 的缺省值（在支持列表里就用它）→ 否则取列表**第一个** |
//! | 位深 | 缺省值 → 否则取第一个；**必须是 8**（PC 只实现 NV12） |
//! | 色彩 | **必须** BT.709 + FULL + SDR_VIDEO —— 渲染着色器固定按它转换（见 `decode::render`） |
//! | 尺寸 / 帧率 | CLI 显式给了就**必须满足**（含对齐步进与 PC 硬解上限），不给则由 agent 推导 |
//! | 密度 | CLI 显式给了就**原样下发** —— 能力表没有 dpi 的范围数据，不凭空造校验；不给则由 agent 推导 |
//!
//! 「必须满足」是硬要求：`encode-probe.md` §5 的三条纪律之一 ——
//! **PC 显式给的值不满足即拒绝，不静默修正、不静默降级**（尺寸直接决定 PC 侧窗口大小）。

use decode::{Codec as PcCodec, HardwareCapability};
use proto::{Caps, Codec, CodecCaps, ColorRange, ColorStandard, ColorTransfer, ScreenParams};

/// PC 侧对本次建屏的**显式要求**（来自命令行，全部可缺省）。
#[derive(Debug, Clone, Copy, Default)]
pub struct Request {
    /// 要求的屏宽；`None` = 由 agent 推导。
    pub width: Option<u32>,
    /// 要求的屏高；`None` = 由 agent 推导。
    pub height: Option<u32>,
    /// 要求的屏密度（dpi）；`None` = 由 agent 推导（= 主屏 `densityDpi`）。
    pub dpi: Option<u32>,
    /// 要求的刷新率；`None` = 用能力表的缺省。
    pub fps: Option<u32>,
    /// 要求的码率系数；`None` = 用能力表的缺省。
    pub bitrate_bpp: Option<f64>,
}

/// 交集算出来的**建屏选择**。
#[derive(Debug, Clone)]
pub struct Choice {
    /// 选中的编码器类别。
    pub codec: Codec,
    /// 选中的 profile。
    pub profile: String,
    /// 选中的位深。
    pub bit_depth: u32,
    /// 选中的刷新率（= 解码器要用的帧率）。
    pub fps: u32,
    /// 选中的尺寸（`None` = agent 推导）。
    pub width: Option<u32>,
    /// 选中的尺寸（`None` = agent 推导）。
    pub height: Option<u32>,
    /// 选中编码器的实现名（日志用）。
    pub encoder: String,
    /// 下发给 agent 的参数。
    pub params: ScreenParams,
}

/// 求交集并选参数。
///
/// @param caps agent 上报的能力表
/// @param pc 本机硬解能力（[`decode::probe_hardware_capability`] 的结果）
/// @param req PC 的显式要求
/// @return 成功给出选择；**任何一项做不到都给可诊断的失败**（绝不静默降级）
pub fn choose(caps: &Caps, pc: &HardwareCapability, req: &Request) -> Result<Choice, String> {
    if !pc.supports_nv12() {
        return Err(format!(
            "本机硬解回读不出 NV12（设备 {} 报告的支持格式：{:?}）⇒ 本实现只做 8-bit 4:2:0",
            pc.device, pc.sw_formats
        ));
    }

    // ① 编码器：按偏好取第一个「PC 能硬解」且「agent 有对应编码器」的。
    //    **优先硬编**；agent 侧只有软编时**接受软编**（用户取向 2026-10-07：模拟器等无硬编环境
    //    也要能出画），但必须**显式提示** —— 画质/延迟/设备 CPU 都会变差。
    let mut tried = Vec::new();
    let mut software_fallback: Option<(Codec, &CodecCaps)> = None;
    for wanted in [Codec::H264, Codec::Hevc] {
        let pc_supports = pc.codecs.contains(&pc_codec(wanted));
        let hardware = caps.codecs.iter().find(|c| c.codec == wanted && c.hardware);
        let software = caps.codecs.iter().find(|c| c.codec == wanted && !c.hardware);
        match (pc_supports, hardware) {
            (true, Some(entry)) => {
                return build(wanted, entry, caps, pc, req);
            }
            (false, _) => tried.push(format!("{}：PC 硬解不支持", name(wanted))),
            (true, None) => match software {
                Some(entry) => {
                    if software_fallback.is_none() {
                        software_fallback = Some((wanted, entry));
                    }
                    tried.push(format!("{}：agent 无硬编（有软编，列为备选）", name(wanted)));
                }
                None => tried.push(format!("{}：agent 没有编码器", name(wanted))),
            },
        }
    }
    if let Some((wanted, entry)) = software_fallback {
        println!(
            "⚠️ agent 侧没有硬编编码器 ⇒ 回落到**软编** {}（{}）：帧率与延迟可能变差、设备 CPU 占用高",
            name(wanted),
            entry.name
        );
        return build(wanted, entry, caps, pc, req);
    }
    Err(format!(
        "交集为空 —— PC 能硬解 {:?}，agent 提供的编码器是 {:?}（逐项：{}）\
         ⇒ 明确失败，不做静默降级",
        pc.codecs.iter().map(|c| pc_codec_name(*c)).collect::<Vec<_>>(),
        caps.codecs
            .iter()
            .map(|c| format!("{}{}", name(c.codec), if c.hardware { "(硬编)" } else { "(软编)" }))
            .collect::<Vec<_>>(),
        tried.join("；")
    ))
}

/// 在选定的编码器上把其余参数定下来。
fn build(
    codec: Codec,
    entry: &CodecCaps,
    caps: &Caps,
    pc: &HardwareCapability,
    req: &Request,
) -> Result<Choice, String> {
    let label = name(codec);

    // ② profile：缺省值在支持列表里就用它，否则取列表第一个（枚举有序 ⇒ 「第一个支持的」）。
    let profile = if entry.profiles.iter().any(|p| *p == caps.defaults.profile) {
        caps.defaults.profile.clone()
    } else {
        entry
            .profiles
            .first()
            .cloned()
            .ok_or_else(|| format!("{label} 没有报告任何可用 profile"))?
    };

    // ③ 位深：本实现只做 NV12（8-bit 4:2:0）。
    let bit_depth = if entry.bit_depths.contains(&caps.defaults.bit_depth) {
        caps.defaults.bit_depth
    } else {
        *entry
            .bit_depths
            .first()
            .ok_or_else(|| format!("{label} 没有报告任何可用位深"))?
    };
    if bit_depth != 8 {
        return Err(format!(
            "{label} 只能编 {bit_depth}-bit（支持 {:?}），而本实现的硬解回读只做 8-bit NV12 \
             ⇒ 明确失败，不回落软解",
            entry.bit_depths
        ));
    }

    // ④ 色彩：渲染着色器固定按 BT.709 全范围转换，所以这三项必须谈成。
    require(entry.color_standards.contains(&ColorStandard::Bt709), || {
        format!(
            "{label} 不支持 colorStandard=BT709（支持 {:?}）—— 渲染管线固定按 BT.709 转换",
            entry.color_standards
        )
    })?;
    require(entry.color_ranges.contains(&ColorRange::Full), || {
        format!(
            "{label} 不支持 colorRange=FULL（支持 {:?}）—— 渲染管线按全范围转换，\
             有限范围会让画面偏暗",
            entry.color_ranges
        )
    })?;
    require(entry.color_transfers.contains(&ColorTransfer::SdrVideo), || {
        format!(
            "{label} 不支持 colorTransfer=SDR_VIDEO（支持 {:?}）—— 本项目暂不做 HDR",
            entry.color_transfers
        )
    })?;

    // ⑤ 尺寸：显式给了就必须满足（范围 + 对齐 + PC 硬解上限）。
    if let Some(w) = req.width {
        check_axis("宽", w, entry.width, pc.max_width)?;
    }
    if let Some(h) = req.height {
        check_axis("高", h, entry.height, pc.max_height)?;
    }

    // ⑥ 刷新率：显式给了就必须满足。
    let fps = req.fps.unwrap_or(caps.defaults.refresh_rate);
    if fps == 0 {
        return Err("刷新率必须是正整数（0 表示未指定，这里连缺省值也是 0）".into());
    }
    if let Some(f) = req.fps {
        let r = entry.refresh_rate;
        if (f as i64) < r.min || (f as i64) > r.max || (r.step > 0 && f as i64 % r.step != 0) {
            return Err(format!(
                "要求的刷新率 {f} 不在 {label} 的可编范围内（min={} max={} step={}）",
                r.min, r.max, r.step
            ));
        }
    }

    // ⑦ 码率系数：显式给了就用，否则用缺省。
    let bitrate_bpp = match req.bitrate_bpp {
        Some(v) if v > 0.0 => v,
        Some(v) => return Err(format!("码率系数必须是正数，收到 {v}")),
        None => caps.defaults.bitrate_bpp,
    };

    let params = ScreenParams {
        width: req.width,
        height: req.height,
        refresh_rate: Some(fps),
        dpi: req.dpi,
        bitrate_bpp: Some(bitrate_bpp),
        bitrate: None,
        bitrate_mode: Some(caps.defaults.bitrate_mode),
        codec: Some(codec),
        bit_depth: Some(bit_depth),
        profile: Some(profile.clone()),
        color_standard: Some(ColorStandard::Bt709),
        color_range: Some(ColorRange::Full),
        color_transfer: Some(ColorTransfer::SdrVideo),
        // 屏行为：能力表已经给出设备推导的缺省，显式回传让「我们选了什么」可见。
        show_system_decorations: Some(caps.defaults.show_system_decorations),
        always_unlocked: Some(caps.defaults.always_unlocked),
        // **不指定屏名** —— 由 agent 派生 `WCReen-<路号>`。
        // 原来这里写死 `WCReen`：多路下所有屏同名，`dumpsys display` 就分不出谁是谁了
        // （屏名是排查用的，`docs/spec/agent/display.md` §2）。
        screen_name: None,
    };

    Ok(Choice {
        codec,
        profile,
        bit_depth,
        fps,
        width: req.width,
        height: req.height,
        encoder: entry.name.clone(),
        params,
    })
}

/// 校验一条尺寸轴：范围 + 对齐步进 + PC 硬解上限。
///
/// @param what 轴名（「宽」/「高」）
/// @param value 请求值
/// @param range agent 报的范围与步进
/// @param pc_max PC 硬解的尺寸上限（`0` = 设备未报告上限）
fn check_axis(what: &str, value: u32, range: proto::IntRange, pc_max: u32) -> Result<(), String> {
    let v = value as i64;
    if v < range.min || v > range.max {
        return Err(format!(
            "要求的{what} {value} 超出编码器范围（min={} max={}）",
            range.min, range.max
        ));
    }
    if range.step > 0 && v % range.step != 0 {
        return Err(format!(
            "要求的{what} {value} 不满足 {}-像素对齐（编码器要求 step={}）",
            range.step, range.step
        ));
    }
    if pc_max > 0 && value > pc_max {
        return Err(format!(
            "要求的{what} {value} 超出本机硬解的尺寸上限 {pc_max}"
        ));
    }
    Ok(())
}

/// 「不满足就失败」的简写 —— 让每条规则的错误信息都在调用处。
fn require(ok: bool, msg: impl FnOnce() -> String) -> Result<(), String> {
    if ok { Ok(()) } else { Err(msg()) }
}

/// 协议侧编码器 → 解码侧的编码器。
fn pc_codec(codec: Codec) -> PcCodec {
    match codec {
        Codec::H264 => PcCodec::H264,
        Codec::Hevc => PcCodec::Hevc,
    }
}

/// 协议侧编码器的名字（用于错误信息）。
fn name(codec: Codec) -> &'static str {
    match codec {
        Codec::H264 => "H.264",
        Codec::Hevc => "HEVC",
    }
}

/// 解码侧编码器的名字（用于错误信息）。
fn pc_codec_name(codec: PcCodec) -> &'static str {
    codec.name()
}
