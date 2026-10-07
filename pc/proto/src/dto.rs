//! 各控制消息的 JSON 结构。
//!
//! 字段名与协议**逐字一致**（camelCase），所以每个字段都显式写了 `rename` ——
//! 不依赖 serde 的命名约定推断，因为「字段名写错」正是 CRC 要自检的那类实现 bug。
//!
//! # 三条形态纪律
//!
//! 1. **未知字段被忽略**（serde 的默认行为）⇒ 对端加字段不会让本端解析失败；
//! 2. **枚举是封闭集合**（[`Codec`] / [`ColorStandard`] / …）—— 遇到未知取值就报错，
//!    因为协议版本是**精确匹配**的，取值集合不同说明对端根本不是这个协议版本；
//! 3. **请求侧用整数、回报侧用浮点**（`refreshRate`）—— 请求给 `60.0` 会被对端的
//!    整数字段拒收，回报给 `217.6` 只有浮点装得下。

use serde::{Deserialize, Serialize};

/// 协议版本号（与产品版本**分开**，协议变更即 +1）。
///
/// 握手要求**精确相等**：agent 是推到设备上的 jar，很容易残留旧版 ——
/// 精确匹配能立刻发现「设备上跑的是旧 jar」，而不是产生一堆诡异行为。
pub const PROTOCOL_VERSION: i32 = 1;

/// 编码器类别。
///
/// 取值只有两个：PC 侧只做这两种的硬解（`docs/spec/tech-stack/pc.md` §2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Codec {
    /// H.264 / AVC（缺省首选）。
    #[serde(rename = "H.264")]
    H264,
    /// H.265 / HEVC。
    #[serde(rename = "HEVC")]
    Hevc,
}

/// 色彩标准（矩阵 / primaries）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorStandard {
    /// BT.709 —— 与 sRGB 的 primaries 相同，是本项目缺省。
    #[serde(rename = "BT709")]
    Bt709,
    /// BT.601 NTSC。
    #[serde(rename = "BT601_NTSC")]
    Bt601Ntsc,
    /// BT.601 PAL。
    #[serde(rename = "BT601_PAL")]
    Bt601Pal,
}

/// 色彩范围。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorRange {
    /// 有限范围（Y 压进 16–235，浪费约 14% 码值）。
    #[serde(rename = "LIMITED")]
    Limited,
    /// 全范围（0–255）—— 源头就是 sRGB，本项目缺省。
    #[serde(rename = "FULL")]
    Full,
}

/// 传输特性（OETF / EOTF）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorTransfer {
    /// SDR 视频（本项目缺省）。
    #[serde(rename = "SDR_VIDEO")]
    SdrVideo,
    /// PQ（HDR10）—— 本项目暂不做，登记以保取值集合完整。
    #[serde(rename = "ST2084")]
    St2084,
    /// HLG —— 同上。
    #[serde(rename = "HLG")]
    Hlg,
}

/// 码率模式（独立于「码率怎么表达」的一个参数）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BitrateMode {
    /// 可变码率：`bitrate` 是**目标平均**，瞬时可超可低（缺省）。
    #[serde(rename = "VBR")]
    Vbr,
    /// 恒定码率：带宽平稳，画质可能被牺牲。
    #[serde(rename = "CBR")]
    Cbr,
}

// ---------------------------------------------------------------- 握手

/// 握手（PC→agent，`0x10`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    /// 本端的协议版本号。
    pub version: i32,
}

/// 握手应答（agent→PC，`0x11`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloAck {
    /// 对端的协议版本号（不相等即**明确失败**，回报双方版本后断开）。
    pub version: i32,
    /// 对端自身状态，取值见 [`HelloAck::STATE_READY`] / [`HelloAck::STATE_NO_HW_ENCODER`]。
    pub state: String,
}

impl HelloAck {
    /// 就绪 —— 可以接受「申请屏幕」。
    pub const STATE_READY: &'static str = "ready";
    /// 无硬编编码器可用 —— 后续「申请屏幕」一律被拒。
    pub const STATE_NO_HW_ENCODER: &'static str = "noHwEncoder";
}

// ---------------------------------------------------------------- 能力表

/// 请求能力表（PC→agent，`0x12`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapsRequest {
    /// 关联号：PC 生成、单调递增，回报必须携带同一 id。
    #[serde(rename = "requestId")]
    pub request_id: i32,
}

/// 能力表（agent→PC，`0x13`）。
///
/// agent **启动时构建一次**、全进程不变，所以任何时候请求都返回同一份（无 `forceRefresh`）。
/// 表里**不含动态额度**（「当前还能开几路」只能靠试跑）—— 见
/// `docs/spec/agent/transport-handshake.md` §4.3。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Caps {
    /// 关联号：与请求里的同一个 id。
    #[serde(rename = "requestId")]
    pub request_id: i32,
    /// **逐编码器**列出的能力（同一 mime 下可能有多个编码器且能力差极大）。
    pub codecs: Vec<CodecCaps>,
    /// 各参数的缺省取值 —— 每一条「可选项」都必须在这里能查到缺省，
    /// 否则 PC 不填字段时就无法**事先**知道 agent 会选什么。
    pub defaults: CapsDefaults,
}

/// 单个编码器的能力。
///
/// 三类形式齐全（`docs/spec/agent/encode-probe.md` §4）：**有序枚举**
/// （[`Self::profiles`] / [`Self::bit_depths`] / 色彩三项，顺序即「取第一个支持的」的语义）、
/// **范围 + 步进**（[`Self::width`] / [`Self::height`] / [`Self::refresh_rate`] / [`Self::bitrate`]）、
/// **整数**（[`Self::max_instances`]）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodecCaps {
    /// 编码器实现名（如 `c2.qti.avc.encoder`）—— 只用于日志与回报里的 `codecName`。
    pub name: String,
    /// 编码器 mime（如 `video/avc`）。
    pub mime: String,
    /// 编码器类别。
    pub codec: Codec,
    /// 是否硬件编码器。`false` = agent 侧没有硬编时的**软编回退**（如实上报，
    /// 选择时**优先硬编**、只有软编可用才用它并显式提示）。
    pub hardware: bool,
    /// 支持的位深，**有序**。
    #[serde(rename = "bitDepths")]
    pub bit_depths: Vec<u32>,
    /// 支持的 profile，**有序**。
    pub profiles: Vec<String>,
    /// 支持的色彩标准，**有序**。
    #[serde(rename = "colorStandards")]
    pub color_standards: Vec<ColorStandard>,
    /// 支持的颜色范围，**有序**。
    #[serde(rename = "colorRanges")]
    pub color_ranges: Vec<ColorRange>,
    /// 支持的传输特性，**有序**。
    #[serde(rename = "colorTransfers")]
    pub color_transfers: Vec<ColorTransfer>,
    /// 宽度范围与**对齐步进**（本机实测是 2，不是 16 ⇒ 不得假定常见值）。
    pub width: IntRange,
    /// 高度范围与对齐步进。
    pub height: IntRange,
    /// 刷新率范围（**与尺寸耦合**：上限随尺寸变化，所以最终判据是「尺寸 + 帧率成对」）。
    #[serde(rename = "refreshRate")]
    pub refresh_rate: IntRange,
    /// 码率范围。
    pub bitrate: IntRange,
    /// 系统**声明**的并发实例数（是上界候选，不是实测值）。
    #[serde(rename = "maxInstances")]
    pub max_instances: u32,
}

/// 整数范围 + 步进。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntRange {
    /// 下界（含）。
    pub min: i64,
    /// 上界（含）。
    pub max: i64,
    /// 步进（尺寸对齐 / 取值粒度）。
    pub step: i64,
}

/// 能力表里的缺省取值。
///
/// 格式组的缺省与其余参数的缺省放在一起 —— 「PC 什么都不填」时也能事先知道结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapsDefaults {
    /// 缺省编码器。
    pub codec: Codec,
    /// 缺省位深。
    #[serde(rename = "bitDepth")]
    pub bit_depth: u32,
    /// 缺省 profile。
    pub profile: String,
    /// 缺省色彩标准。
    #[serde(rename = "colorStandard")]
    pub color_standard: ColorStandard,
    /// 缺省颜色范围。
    #[serde(rename = "colorRange")]
    pub color_range: ColorRange,
    /// 缺省传输特性。
    #[serde(rename = "colorTransfer")]
    pub color_transfer: ColorTransfer,
    /// 缺省码率系数（bits per pixel per frame）—— 缺省码率 = 宽 × 高 × 刷新率 × 本值。
    #[serde(rename = "bitrateBpp")]
    pub bitrate_bpp: f64,
    /// 缺省码率模式。
    #[serde(rename = "bitrateMode")]
    pub bitrate_mode: BitrateMode,
    /// 缺省刷新率（= 编码帧率，两者恒等）。
    #[serde(rename = "refreshRate")]
    pub refresh_rate: u32,
    /// 关键帧间隔（秒）。
    #[serde(rename = "iFrameInterval")]
    pub i_frame_interval: u32,
    /// 缺省是否显示系统装饰（状态栏 / 导航栏）。
    #[serde(rename = "showSystemDecorations")]
    pub show_system_decorations: bool,
    /// 缺省是否常亮解锁。
    #[serde(rename = "alwaysUnlocked")]
    pub always_unlocked: bool,
}

// ---------------------------------------------------------------- 心跳

/// 心跳（双向，`0x30`）：负载是**空 JSON 对象**，不承载任何协议内容。
///
/// 周期固定 3s；判活判据是「`T_dead`（10s）内收到对端**任何**消息」，不是「收到心跳」。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heartbeat {}

// ---------------------------------------------------------------- 指令

/// 指令信封（PC→agent，`0x20`）：**关联号 + 载荷**。
///
/// 线形状是「信封 + 载荷结构体」——`cmd` / `requestId` 在信封层，指令自己的字段
/// 全在 `payload` 里：
///
/// ```json
/// { "cmd": "applyScreen", "requestId": 7, "payload": { "streamId": 0, "params": { … } } }
/// ```
///
/// 无字段的指令（`requestSyncFrame` / `stopAgent`）**省略 `payload`**
/// （[`CommandBody`] 里写成 unit 变体 ⇒ serde 不编这个字段，不是编成 `{}`）。
///
/// 只登记本实现**真的会发**的几条 —— 协议里另有 `endSession` / `enumerateApps` /
/// `launchApp` / `setPolicy`，它们各有归属的分片，不在这里留空壳。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Command {
    /// 关联号：回报原样带回（输入指令只在**失败**时带回）。
    #[serde(rename = "requestId")]
    pub request_id: i32,
    /// 载荷：`cmd` 与它的字段（见 [`CommandBody`]）。
    #[serde(flatten)]
    pub body: CommandBody,
}

impl Command {
    /// 本指令的关联号。
    pub fn request_id(&self) -> i32 {
        self.request_id
    }

    /// 构造 `applyScreen` 指令。
    pub fn apply_screen(request_id: i32, stream_id: i32, params: ScreenParams) -> Self {
        Self { request_id, body: CommandBody::ApplyScreen(ApplyScreenArgs { stream_id, params }) }
    }

    /// 构造 `input` 指令。
    pub fn input(request_id: i32, args: InputArgs) -> Self {
        Self { request_id, body: CommandBody::Input(args) }
    }

    /// 构造 `stopAgent` 指令。
    pub fn stop_agent(request_id: i32) -> Self {
        Self { request_id, body: CommandBody::StopAgent }
    }

    /// 构造 `requestSyncFrame` 指令。
    pub fn request_sync_frame(request_id: i32) -> Self {
        Self { request_id, body: CommandBody::RequestSyncFrame }
    }
}

/// 指令载荷：`cmd` 选出变体，有字段的指令把字段装进 `payload`。
///
/// `#[serde(tag = "cmd", content = "payload")]`（**相邻标签**）配 [`Command`] 的
/// `#[serde(flatten)]`：序列化时 `cmd` / `payload` 与 `requestId` 平铺在同一层，
/// 但**载荷有自己的边界** —— 对端可以把 `payload` 先当不透明 JSON 收下，
/// 再按 `cmd` 单独解成载荷结构体（两段式解析）。
///
/// unit 变体（无字段指令）序列化时**不带 `payload` 字段**。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "payload")]
pub enum CommandBody {
    /// 申请屏幕：建屏 / 重连认领 / 参数变更（三件事共用一个指令）。
    #[serde(rename = "applyScreen")]
    ApplyScreen(ApplyScreenArgs),
    /// 请求同步帧：主动刷新 / 丢包重同步（agent 立刻产一个 IDR）。
    #[serde(rename = "requestSyncFrame")]
    RequestSyncFrame,
    /// 停止 agent：全局收尾（agent 回报 `ok` 后关连接并退出进程）。
    #[serde(rename = "stopAgent")]
    StopAgent,
    /// 输入事件：触摸 / 滚轮 / 按键（`docs/spec/agent/transport-shapes.md` §5）。
    ///
    /// ⚠️ **成功不回 ack** —— 它是高频流（`docs/spec/tech-stack/pc.md` §4 的定频采样就是为了
    /// 压它的条数），只有**失败**才回报（`noScreen` / `badInput` / `inputDenied` / `inputFailed`）。
    /// `requestId` 因此只在失败时被带回来。
    #[serde(rename = "input")]
    Input(InputArgs),
}

/// 「申请屏幕」的载荷（`applyScreen`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApplyScreenArgs {
    /// 目标屏 id：`0` = 建新屏；非 0 = 认领（存在则比较参数，不存在则 agent 分配新 id 并回报）。
    #[serde(rename = "streamId")]
    pub stream_id: i32,
    /// 建屏参数（全部可缺省）。
    pub params: ScreenParams,
}

/// `input` 的载荷。
///
/// 三个可选字段按 `action` 取用（`docs/spec/agent/transport-shapes.md` §5）：
/// 触摸类（`down` / `move` / `up` / `cancel`）带 [`Self::pointers`]、
/// `scroll` 带 [`Self::scroll`]、`keyDown` / `keyUp` 带 [`Self::key`]。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputArgs {
    /// 事件种类。
    pub action: InputAction,
    /// 触点快照 —— **变更后**的当前触点集合（`up` 给「抬起之后剩下的」，可能为空）。
    /// 触摸类事件必给。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pointers: Option<Vec<InputPointer>>,
    /// 滚轮增量与位置。`action = scroll` 时必给。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scroll: Option<InputScroll>,
    /// 按键。`action = keyDown` / `keyUp` 时必给。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<InputKey>,
}

/// 「申请屏幕」的参数。
///
/// **每个字段都可缺省**：`None` = 未指定，由 agent 按设备推导，**实际生效值必须回报**。
/// 用 `Option` + 跳过序列化来表达「未指定」，而不是塞 `0` / `""` 这类哨兵 ——
/// 对端把缺省当 `0`，语义与「字段不存在」完全相同，但本端不会误发一个看起来像真值的 0。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScreenParams {
    /// 屏宽（像素）。**编码尺寸 = 屏尺寸**，中间没有缩放。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    /// 屏高（像素）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    /// 刷新率（Hz）= 编码帧率。请求侧是**整数**（对端的字段可能是整数类型）。
    #[serde(rename = "refreshRate", skip_serializing_if = "Option::is_none")]
    pub refresh_rate: Option<u32>,
    /// 屏密度（dpi）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dpi: Option<u32>,
    /// 码率系数（bpp）。与 `bitrate` 并存、互不排斥；**系数优先**。
    #[serde(rename = "bitrateBpp", skip_serializing_if = "Option::is_none")]
    pub bitrate_bpp: Option<f64>,
    /// 固定码率（bit/s）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bitrate: Option<i64>,
    /// 码率模式。
    #[serde(rename = "bitrateMode", skip_serializing_if = "Option::is_none")]
    pub bitrate_mode: Option<BitrateMode>,
    /// 编码器类别。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codec: Option<Codec>,
    /// 位深。
    #[serde(rename = "bitDepth", skip_serializing_if = "Option::is_none")]
    pub bit_depth: Option<u32>,
    /// profile。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// 色彩标准。
    #[serde(rename = "colorStandard", skip_serializing_if = "Option::is_none")]
    pub color_standard: Option<ColorStandard>,
    /// 颜色范围。
    #[serde(rename = "colorRange", skip_serializing_if = "Option::is_none")]
    pub color_range: Option<ColorRange>,
    /// 传输特性。
    #[serde(rename = "colorTransfer", skip_serializing_if = "Option::is_none")]
    pub color_transfer: Option<ColorTransfer>,
    /// 是否显示系统装饰。
    #[serde(rename = "showSystemDecorations", skip_serializing_if = "Option::is_none")]
    pub show_system_decorations: Option<bool>,
    /// 是否常亮解锁。
    #[serde(rename = "alwaysUnlocked", skip_serializing_if = "Option::is_none")]
    pub always_unlocked: Option<bool>,
    /// 屏名（用于日志 / 调试识别）。
    #[serde(rename = "screenName", skip_serializing_if = "Option::is_none")]
    pub screen_name: Option<String>,
}

// ---------------------------------------------------------------- 输入

/// `input` 指令的 `action` 取值（`docs/spec/agent/transport-shapes.md` §5）。
///
/// 封闭集合：协议版本是**精确匹配**的，取值对不上就说明对端不是这个协议版本。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputAction {
    /// 触点按下（首个触点）。
    #[serde(rename = "down")]
    Down,
    /// 触点移动。
    #[serde(rename = "move")]
    Move,
    /// 触点抬起。
    #[serde(rename = "up")]
    Up,
    /// 手势取消。
    #[serde(rename = "cancel")]
    Cancel,
    /// 滚轮。
    #[serde(rename = "scroll")]
    Scroll,
    /// 按键按下。
    #[serde(rename = "keyDown")]
    KeyDown,
    /// 按键抬起。
    #[serde(rename = "keyUp")]
    KeyUp,
}

/// 一个触点（屏像素整数）。
///
/// `id` 由 PC 分配、手势内稳定 —— agent 由它推导 Android 的 `pointerIndex`
/// （`docs/spec/agent/input.md` §2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputPointer {
    /// 触点标识。
    pub id: i32,
    /// 屏像素 x。
    pub x: i32,
    /// 屏像素 y。
    pub y: i32,
}

/// 滚轮：**增量**位移 + 发生位置。
///
/// `dx` / `dy` 是**增量**语义（agent 侧合并时累加，绝不覆盖）⇒ PC 侧也**绝不能丢**，
/// 只能先累加、再按上限频率上报（`docs/spec/tech-stack/pc.md` §4.1）。
/// `x` / `y` 是滚轮位置：Android 的 `ACTION_SCROLL` 要用坐标做命中判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputScroll {
    /// 横向增量。
    pub dx: i32,
    /// 纵向增量。
    pub dy: i32,
    /// 位置 x（屏像素）。
    pub x: i32,
    /// 位置 y（屏像素）。
    pub y: i32,
}

/// 按键：直接给 Android 的 `keyCode` 与 `metaState`（agent 侧**零映射**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputKey {
    /// Android keyCode。
    pub code: i32,
    /// metaState（Shift / Ctrl / Alt 等修饰键）。
    pub meta: i32,
}

// ---------------------------------------------------------------- 指令回报

/// 指令回报（agent→PC，`0x21`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandAck {
    /// 关联号：必须与请求里的同一个 id（心跳与主动上报不带它）。
    #[serde(rename = "requestId")]
    pub request_id: i32,
    /// 是否成功。
    pub ok: bool,
    /// 成功时的结果（「申请屏幕」才有）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<ApplyScreenResult>,
    /// 失败时**已生效的那部分**（允许部分成功 —— 例如码率生效了但尺寸不能原地改）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applied: Option<AppliedScreen>,
    /// 失败时的错误对象。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObj>,
}

/// 「申请屏幕」成功时的结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApplyScreenResult {
    /// agent 分配 / 认领的屏 id —— PC 记住它，重连时带回来才能解冻同一块屏。
    #[serde(rename = "streamId")]
    pub stream_id: i32,
    /// 每个参数的**实际生效值**。
    pub applied: AppliedScreen,
}

/// 实际生效的建屏 / 编码参数。
///
/// 只有 [`Self::stream_id`] 是必需字段（PC 的重连依赖它）；其余都是 agent 的
/// **陈述性回报**：PC 只把它们用于校验与显示，缺一个字段不该让整条会话失败 ——
/// 真正决定渲染的是**解出来的帧本身**（尺寸由码流决定，色彩由 VUI 决定）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppliedScreen {
    /// 屏 id。
    #[serde(rename = "streamId")]
    pub stream_id: i32,
    /// 实际生效的屏宽。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    /// 实际生效的屏高。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    /// 实际生效的刷新率（可能不是整数，所以是浮点）。
    #[serde(rename = "refreshRate", skip_serializing_if = "Option::is_none")]
    pub refresh_rate: Option<f64>,
    /// 实际生效的 dpi。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dpi: Option<u32>,
    /// 实际生效的码率（bit/s）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bitrate: Option<i64>,
    /// 实际生效的码率模式。
    #[serde(rename = "bitrateMode", skip_serializing_if = "Option::is_none")]
    pub bitrate_mode: Option<BitrateMode>,
    /// 实际生效的编码器类别。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codec: Option<Codec>,
    /// 实际生效的编码器实现名。
    #[serde(rename = "codecName", skip_serializing_if = "Option::is_none")]
    pub codec_name: Option<String>,
    /// 实际生效的位深。
    #[serde(rename = "bitDepth", skip_serializing_if = "Option::is_none")]
    pub bit_depth: Option<u32>,
    /// 实际生效的 profile。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// 实际生效的色彩标准。
    #[serde(rename = "colorStandard", skip_serializing_if = "Option::is_none")]
    pub color_standard: Option<ColorStandard>,
    /// 实际生效的颜色范围。
    #[serde(rename = "colorRange", skip_serializing_if = "Option::is_none")]
    pub color_range: Option<ColorRange>,
    /// 实际生效的传输特性。
    #[serde(rename = "colorTransfer", skip_serializing_if = "Option::is_none")]
    pub color_transfer: Option<ColorTransfer>,
    /// 实际是否显示系统装饰。
    #[serde(rename = "showSystemDecorations", skip_serializing_if = "Option::is_none")]
    pub show_system_decorations: Option<bool>,
    /// 实际是否常亮解锁。
    #[serde(rename = "alwaysUnlocked", skip_serializing_if = "Option::is_none")]
    pub always_unlocked: Option<bool>,
    /// 该屏是否独占 display group（**派生位**，PC 不可配，只回报结果）。
    #[serde(rename = "ownDisplayGroup", skip_serializing_if = "Option::is_none")]
    pub own_display_group: Option<bool>,
    /// 实际生效的屏名。
    #[serde(rename = "screenName", skip_serializing_if = "Option::is_none")]
    pub screen_name: Option<String>,
}

// ---------------------------------------------------------------- 主动上报

/// 主动上报（agent→PC，`0x22`）：**agent 单方面改变的事实**。
///
/// PC 自己发的 `endSession` **不发这个** —— 它的回报已覆盖，不重复。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    /// 事件名，取值见 [`Event::SCREEN_RELEASED`] / [`Event::SUBSYSTEM_UNAVAILABLE`] / [`Event::FATAL`]。
    pub event: String,
    /// 相关屏 id（只有 `screenReleased` 有）。
    #[serde(rename = "streamId", skip_serializing_if = "Option::is_none")]
    pub stream_id: Option<i32>,
    /// 原因（`screenReleased` 的取值见 [`Event::REASON_N_ACCESS_FAILURES`] / [`Event::REASON_FATAL`]）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// 子系统名（只有 `subsystemUnavailable` 有）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subsystem: Option<String>,
}

impl Event {
    /// agent 主动拆屏（`reason` 说明是谁触发的）。
    pub const SCREEN_RELEASED: &'static str = "screenReleased";
    /// 首次判定某子系统能力缺失。
    pub const SUBSYSTEM_UNAVAILABLE: &'static str = "subsystemUnavailable";
    /// 进程即将收尾（尽力发；socket 已坏时只能靠退出码）。
    pub const FATAL: &'static str = "fatal";
    /// 拆屏原因：连续接入失败达到阈值。
    pub const REASON_N_ACCESS_FAILURES: &'static str = "nAccessFailures";
    /// 拆屏原因：进程级故障。
    pub const REASON_FATAL: &'static str = "fatal";
}

// ---------------------------------------------------------------- 错误对象

/// 错误对象（`transport-messages.md` §4.1）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorObj {
    /// 错误分级，取值见 [`ErrorObj::CLASS_FATAL`] / [`ErrorObj::CLASS_SUBSYSTEM`] /
    /// [`ErrorObj::CLASS_STREAM`] / [`ErrorObj::CLASS_RECOVERABLE`]。
    pub class: String,
    /// **稳定的机器可读标识** —— PC 靠它分支，所以不能随手改字符串。
    pub code: String,
    /// 给人看的可诊断说明。
    pub message: String,
}

impl ErrorObj {
    /// 进程级：agent 无法继续。
    pub const CLASS_FATAL: &'static str = "fatal";
    /// 子系统级：某能力（如编码器）不可用。
    pub const CLASS_SUBSYSTEM: &'static str = "subsystem";
    /// 单路级：只影响这一条连接。
    pub const CLASS_STREAM: &'static str = "stream";
    /// 可恢复：参数不被接受之类，改了再来即可。
    pub const CLASS_RECOVERABLE: &'static str = "recoverable";
}
