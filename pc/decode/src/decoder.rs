//! 裸 Annex-B（H.264 / HEVC）→ 硬解 → 回读成 NV12。
//!
//! # 为什么不用解复用器
//!
//! 真实链路送过来的是 `0x01` 帧的 **Annex-B 裸码流**（无容器、无 CSD 交换），
//! 而且是一帧一帧喂 —— 所以本模块**完全不碰 `avformat`**：
//! 直接把每一帧的 Annex-B 数据当成一个 access unit 交给 `avcodec_send_packet`。
//! 裸流的两个后果必须记住：
//!
//! - **没有容器时间戳** ⇒ 帧率**不得**从裸流推导（实测 FFmpeg 会猜出 120/1 与 25/1，
//!   而真实是 60）⇒ 帧率由调用方从**协商参数**传入（[`VideoParams::fps`]）；
//! - **每个 IDR 自带 SPS/PPS**（agent 侧 `PREPEND_HEADER_TO_SYNC_FRAMES = 1`）
//!   ⇒ 中途切入即可解，不需要任何配置消息。
//!
//! # 「绝不软解」的两道闸
//!
//! 1. **结构性保证**：[`get_format_hw`] 回调只接受 `AV_PIX_FMT_D3D11`，找不到就返回
//!    `AV_PIX_FMT_NONE` ⇒ `avcodec_open2` 直接失败。FFmpeg 默认会**静默回落软解**
//!    （退出码仍为 0），这个回调让「拿不到硬解」变成**明确的打开失败**；
//! 2. **首帧硬校验**：解码器打开成功**不能证明**走了硬解（不支持的 profile / 尺寸
//!    会静默降级），唯一可靠判据是**首帧确实是硬件帧**。
//!
//! 两条都要，缺一不可。

use std::collections::VecDeque;
use std::ffi::c_int;
use std::ptr;
use std::time::Instant;

use ffmpeg_next::sys;

use crate::hw::{av_err, pix_fmt_name, HwDevice};

/// `AVERROR_EOF` —— FFmpeg 用 `MKTAG` 编码的错误码，**bindgen 不导出该宏**。
///
/// `MKTAG('E','O','F',' ')` = `0x20464F45`，取负即此值。
const AVERROR_EOF: c_int = -0x20464F45;

/// `AVERROR(EAGAIN)` —— 系统 `errno` 取负（`EAGAIN` 在 MSVC 与 MinGW 上都是 11）。
const AVERROR_EAGAIN: c_int = -11;

/// 硬件帧应有的像素格式 —— **唯一的硬解判据**。
const HW_FMT: c_int = sys::AVPixelFormat::AV_PIX_FMT_D3D11 as c_int;

/// `AV_NOPTS_VALUE` —— FFmpeg 的「无时间戳」哨兵（bindgen 不导出该宏，其值即 `i64::MIN`）。
const AV_NOPTS_VALUE: i64 = i64::MIN;

/// 待取帧队列的容量。
///
/// 它存在只是为了满足 FFmpeg 的调用契约（`send_packet` 返回 `EAGAIN` 时必须先收帧再重发），
/// 正常路径下它几乎总是空的。上限取 **2 帧** —— 与 `pc.md` §2 的「缓冲下限 2 帧」一致，
/// 超出就丢最旧的（直播语义：**到达即上屏**，打穿后按丢帧重对齐，不是降级）。
const READY_CAP: usize = 2;

/// 解码链路的**失败阶段**。
///
/// 阶段必须分开报：`Open` 失败（建解码器）与 `NotHardware`（该参数只能软解）
/// 对上游是完全不同的结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// 建解码器（含 `get_format` 拿不到硬解而导致的 `avcodec_open2` 失败）。
    Open,
    /// 送包 / 收帧本身失败。
    Stream,
    /// **首帧不是硬件帧** —— 即 FFmpeg 静默回落软解。
    NotHardware,
    /// `av_hwframe_transfer_data` 回读失败。
    Transfer,
    /// 回读出的格式不是本 crate 实现的 NV12。
    Format,
}

impl Stage {
    /// 阶段名（日志与 UI 用，ASCII 以免 egui 默认字体缺字）。
    pub fn name(self) -> &'static str {
        match self {
            Self::Open => "decoder-open",
            Self::Stream => "decode-stream",
            Self::NotHardware => "not-hardware-frame",
            Self::Transfer => "readback",
            Self::Format => "readback-format",
        }
    }
}

/// 带阶段标记的解码失败。
#[derive(Debug, Clone)]
pub struct DecodeError {
    /// 失败阶段。
    pub stage: Stage,
    /// 可读原因（含 FFmpeg 的返回码文本）。
    pub detail: String,
}

impl DecodeError {
    /// 造一个阶段失败。
    pub(crate) fn new(stage: Stage, detail: impl Into<String>) -> Self {
        Self { stage, detail: detail.into() }
    }
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.stage.name(), self.detail)
    }
}

impl std::error::Error for DecodeError {}

/// 解码链路的 `Result` 简写。
pub type DecodeResult<T> = std::result::Result<T, DecodeError>;

/// 编码器类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    /// H.264 / AVC。
    H264,
    /// H.265 / HEVC。
    Hevc,
}

impl Codec {
    /// 协议里的名字（与 `proto::Codec` 的 JSON 取值一致）。
    pub fn name(self) -> &'static str {
        match self {
            Self::H264 => "H.264",
            Self::Hevc => "HEVC",
        }
    }

    /// FFmpeg 的解码器 id。
    fn codec_id(self) -> sys::AVCodecID {
        match self {
            Self::H264 => sys::AVCodecID::AV_CODEC_ID_H264,
            Self::Hevc => sys::AVCodecID::AV_CODEC_ID_HEVC,
        }
    }
}

/// 打开解码器需要的参数 —— **全部来自协商**，不从码流里猜。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoParams {
    /// 编码器类别。
    pub codec: Codec,
    /// 协商的帧率（Hz）= 虚拟屏刷新率。
    ///
    /// **必须 > 0**：裸流没有时间戳，帧率无从推导，而它决定码率控制的分配
    /// 与渲染节拍 —— 猜错比没有更糟。
    pub fps: u32,
    /// 协商的宽度；`0` = 未知（由码流决定）。
    ///
    /// 尺寸与帧率不同：**尺寸是码流自带的**（SPS 里有），所以允许交给解码器；
    /// 而帧率在裸流里根本不存在。
    pub width: u32,
    /// 协商的高度；`0` = 未知（由码流决定）。
    pub height: u32,
}

/// 首帧的**硬校验**与回读结果。
#[derive(Debug, Clone, Copy)]
pub struct FirstFrame {
    /// `AVFrame.format` —— 硬解时应为 `AV_PIX_FMT_D3D11`。
    pub hw_format: i32,
    /// `AVFrame.hw_frames_ctx` 是否非空。
    pub has_hw_frames_ctx: bool,
    /// 回读后的软件帧格式。
    pub sw_format: i32,
    /// 首帧的回读耗时（毫秒）。
    pub transfer_ms: f64,
}

impl FirstFrame {
    /// 是否判定为**真硬解**。
    pub fn is_hardware(&self) -> bool {
        self.has_hw_frames_ctx && self.hw_format == HW_FMT
    }
}

/// 一帧**回读后**的 NV12 数据（可跨线程传递）。
pub struct Nv12Frame {
    /// 平面 0（Y）：`width * height` 字节。
    pub y: Vec<u8>,
    /// 平面 1（UV 交错）：`width * height / 2` 字节。
    pub uv: Vec<u8>,
    /// 帧宽（**以实际解出的帧为准** —— 裸流在打开解码器时往往还不知道尺寸）。
    pub width: u32,
    /// 帧高。
    pub height: u32,
    /// 本路解出的第几帧（从 0 起）。
    pub index: u64,
    /// 产生这一帧的那次 [`VideoDecoder::feed`] 携带的呈现时刻（微秒）。
    ///
    /// **仅用于显示与统计，不参与任何节流** —— 真实链路是直播语义（到达即上屏），
    /// 而且无 B 帧 ⇒ 一进一出，这个值就是该帧自己的时间戳。
    pub pts_us: Option<i64>,
    /// 回读完成（= 解码链路交付）的时刻。
    pub ready_at: Instant,
    /// 「收帧 → 回读完成」的耗时（毫秒）—— T1 通路的核心开销。
    pub transfer_ms: f64,
}

/// 构造期间各 FFmpeg 句柄的**临时所有者**：出错时按序释放，成功时交给 [`VideoDecoder`]。
///
/// 存在理由：`VideoDecoder` 自身有 `Drop`，没法「部分构造后返回 `Err`」，
/// 所以中途失败必须有人负责收尾。
struct Pending {
    /// 解码器上下文。
    ctx: *mut sys::AVCodecContext,
    /// 复用的输入包。
    pkt: *mut sys::AVPacket,
    /// 复用的硬件帧。
    frame: *mut sys::AVFrame,
    /// 复用的软件帧（回读目标）。
    sw: *mut sys::AVFrame,
}

impl Pending {
    /// 空的所有者。
    fn new() -> Self {
        Self { ctx: ptr::null_mut(), pkt: ptr::null_mut(), frame: ptr::null_mut(), sw: ptr::null_mut() }
    }

    /// 交出所有权：各字段置空 ⇒ 之后的 `Drop` 是空操作。
    ///
    /// ⚠️ **必须返回裸指针元组，不能返回 `Pending` 本身** —— 返回 `Pending` 的话，
    /// 调用方读完字段后那个临时值会析构，把刚交出去的句柄全部释放（use-after-free，
    /// spike 期间真的踩到过）。
    fn take(&mut self) -> (*mut sys::AVCodecContext, *mut sys::AVPacket, *mut sys::AVFrame, *mut sys::AVFrame) {
        let out = (self.ctx, self.pkt, self.frame, self.sw);
        self.ctx = ptr::null_mut();
        self.pkt = ptr::null_mut();
        self.frame = ptr::null_mut();
        self.sw = ptr::null_mut();
        out
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        unsafe {
            if !self.sw.is_null() {
                sys::av_frame_free(&mut self.sw);
            }
            if !self.frame.is_null() {
                sys::av_frame_free(&mut self.frame);
            }
            if !self.pkt.is_null() {
                sys::av_packet_free(&mut self.pkt);
            }
            if !self.ctx.is_null() {
                sys::avcodec_free_context(&mut self.ctx);
            }
        }
    }
}

/// `get_format` 回调：**只从候选里选 `AV_PIX_FMT_D3D11`**。
///
/// 找不到就返回 `AV_PIX_FMT_NONE` ⇒ `avcodec_open2` 失败 —— 这是「**绝不软解**」的
/// 结构性落点（详见模块头注释）。
///
/// 逐项按 `c_int` 读取（而非按 enum 读）—— 避免对 Rust enum 构造非法值。
unsafe extern "C" fn get_format_hw(
    _ctx: *mut sys::AVCodecContext,
    fmt: *const sys::AVPixelFormat,
) -> sys::AVPixelFormat {
    let mut p = fmt as *const c_int;
    loop {
        let v = unsafe { ptr::read(p) };
        if v == sys::AVPixelFormat::AV_PIX_FMT_NONE as c_int {
            break;
        }
        if v == HW_FMT {
            return sys::AVPixelFormat::AV_PIX_FMT_D3D11;
        }
        p = unsafe { p.add(1) };
    }
    sys::AVPixelFormat::AV_PIX_FMT_NONE
}

/// 一路**流式**硬解解码器：一帧一帧喂，一帧一帧取。
pub struct VideoDecoder {
    /// 解码器上下文（本结构持有）。
    ctx: *mut sys::AVCodecContext,
    /// 复用的输入包。
    pkt: *mut sys::AVPacket,
    /// 复用的硬件帧。
    frame: *mut sys::AVFrame,
    /// 复用的软件帧（回读目标）。
    sw: *mut sys::AVFrame,
    /// `EAGAIN` 时先收下的帧（见 [`READY_CAP`]）。
    ready: VecDeque<Nv12Frame>,
    /// 编码器类别。
    codec: Codec,
    /// 最近一次喂入的呈现时刻（微秒）。
    last_pts_us: Option<i64>,
    /// 已解出的帧数。
    decoded: u64,
    /// 因待取队列满而丢掉的帧数（直播语义下的正常现象）。
    dropped: u64,
    /// 首帧的硬校验结果（首帧出来后填充）。
    first: Option<FirstFrame>,
}

impl VideoDecoder {
    /// 打开一路解码器。
    ///
    /// **不在这里做硬校验** —— 校验发生在首帧真正解出时（[`Self::receive`]），
    /// 因为「解码器打开成功」根本不能证明走了硬解。
    ///
    /// @param device 共享的硬解设备
    /// @param params 协商来的参数（帧率必需，尺寸可缺省）
    /// @return 成功给出解码器；参数非法或硬解不可用给出 [`Stage::Open`] 失败
    pub fn open(device: &HwDevice, params: &VideoParams) -> DecodeResult<Self> {
        if params.fps == 0 {
            return Err(DecodeError::new(
                Stage::Open,
                "帧率必须由调用方从协商参数传入且 > 0（裸流没有时间戳，无法推导）",
            ));
        }
        let codec_id = params.codec.codec_id();
        let codec = unsafe { sys::avcodec_find_decoder(codec_id) };
        if codec.is_null() {
            return Err(DecodeError::new(
                Stage::Open,
                format!("FFmpeg 里没有 {} 解码器", params.codec.name()),
            ));
        }
        let codec_name = unsafe {
            let p = (*codec).name;
            if p.is_null() { "?".to_string() } else { std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned() }
        };

        let mut p = Pending::new();
        p.ctx = unsafe { sys::avcodec_alloc_context3(codec) };
        if p.ctx.is_null() {
            return Err(DecodeError::new(Stage::Open, "avcodec_alloc_context3 失败"));
        }
        unsafe {
            let ctx = &mut *p.ctx;
            // 尺寸：0 = 未知，交给码流（SPS 里自带）。非 0 时先摆上，
            // 让硬解帧池按正确尺寸分配；与码流不一致时 libavcodec 会以码流为准。
            ctx.width = params.width as c_int;
            ctx.height = params.height as c_int;
            // 帧率：**必须显式给**。裸流没有任何可推导的来源，
            // 它决定码率控制的分配与渲染节拍。
            ctx.framerate = sys::AVRational { num: params.fps as c_int, den: 1 };
            ctx.time_base = sys::AVRational { num: 1, den: params.fps as c_int };
            ctx.pkt_timebase = ctx.time_base;
            // 单线程 + 低延迟标志：硬解器有帧池深度，多线程会引入额外的排队延迟。
            ctx.thread_count = 1;
            ctx.flags |= sys::AV_CODEC_FLAG_LOW_DELAY as c_int;
            ctx.get_format = Some(get_format_hw);
            ctx.hw_device_ctx = sys::av_buffer_ref(device.ptr);
        }

        let ret = unsafe { sys::avcodec_open2(p.ctx, codec, ptr::null_mut()) };
        if ret < 0 {
            return Err(DecodeError::new(
                Stage::Open,
                format!(
                    "avcodec_open2({codec_name}) 失败（**该硬解在此 codec / 尺寸上不可用**）：\
                     {}（{ret}）",
                    av_err(ret)
                ),
            ));
        }

        p.pkt = unsafe { sys::av_packet_alloc() };
        p.frame = unsafe { sys::av_frame_alloc() };
        p.sw = unsafe { sys::av_frame_alloc() };
        if p.pkt.is_null() || p.frame.is_null() || p.sw.is_null() {
            return Err(DecodeError::new(Stage::Open, "alloc packet / frame 失败"));
        }

        let (ctx, pkt, frame, sw) = p.take();
        Ok(Self {
            ctx,
            pkt,
            frame,
            sw,
            ready: VecDeque::with_capacity(READY_CAP),
            codec: params.codec,
            last_pts_us: None,
            decoded: 0,
            dropped: 0,
            first: None,
        })
    }

    /// 喂一帧 Annex-B 数据。
    ///
    /// 数据应当是一个完整的 access unit（`MediaCodec` 的输出就是这样），
    /// 不需要调用方再切 NAL —— 解码器内部会切。
    ///
    /// @param annex_b Annex-B 码流（`00 00 01` 起始码）
    /// @param pts_us 该帧的呈现时刻（微秒），仅用于显示与统计
    /// @return 成功；送包失败（含码流非法）给出 [`Stage::Stream`] 失败
    pub fn feed(&mut self, annex_b: &[u8], pts_us: i64) -> DecodeResult<()> {
        if annex_b.is_empty() {
            return Ok(());
        }
        let len = c_int::try_from(annex_b.len()).map_err(|_| {
            DecodeError::new(Stage::Stream, format!("帧数据 {} 字节，超出 FFmpeg 的 int 上限", annex_b.len()))
        })?;
        // FFmpeg 要求包的缓冲由 av_malloc 分配（av_packet_unref 会用 av_free 释放），
        // 并且尾部要有 AV_INPUT_BUFFER_PADDING_SIZE 的零填充（优化过的位读取器会多读几个字节）。
        // 这两条都只有 `av_new_packet` 能满足 ⇒ 这里必须有一次拷贝（≈30 KB/帧，
        // 相对 4 ms 级的回读可忽略）。
        let ret = unsafe { sys::av_new_packet(self.pkt, len) };
        if ret < 0 {
            return Err(DecodeError::new(
                Stage::Stream,
                format!("av_new_packet({len}) 失败：{}（{ret}）", av_err(ret)),
            ));
        }
        unsafe {
            ptr::copy_nonoverlapping(annex_b.as_ptr(), (*self.pkt).data, annex_b.len());
            // 裸流没有容器时间戳 ⇒ 包不带 pts（时间戳走协议，不走 FFmpeg）。
            (*self.pkt).pts = AV_NOPTS_VALUE;
        }

        // `EAGAIN` = 解码器内部缓冲满，此时包**没有被消费** ⇒ 必须先收帧再重发同一个包。
        let mut send = unsafe { sys::avcodec_send_packet(self.ctx, self.pkt) };
        while send == AVERROR_EAGAIN {
            self.drain_ready()?;
            send = unsafe { sys::avcodec_send_packet(self.ctx, self.pkt) };
        }
        unsafe { sys::av_packet_unref(self.pkt) };
        if send < 0 {
            return Err(DecodeError::new(
                Stage::Stream,
                format!("send_packet 失败：{}（{send}）", av_err(send)),
            ));
        }
        self.last_pts_us = Some(pts_us);
        Ok(())
    }

    /// 取下一帧已解出的 NV12 帧。
    ///
    /// @return `Ok(None)` = 暂时没有可取的帧（继续 [`Self::feed`]）；
    ///   硬校验失败或回读失败给出对应阶段的错误
    pub fn receive(&mut self) -> DecodeResult<Option<Nv12Frame>> {
        if let Some(frame) = self.ready.pop_front() {
            return Ok(Some(frame));
        }
        let ret = unsafe { sys::avcodec_receive_frame(self.ctx, self.frame) };
        if ret == AVERROR_EAGAIN || ret == AVERROR_EOF {
            return Ok(None);
        }
        if ret < 0 {
            return Err(DecodeError::new(
                Stage::Stream,
                format!("receive_frame 失败：{}（{ret}）", av_err(ret)),
            ));
        }
        self.take_frame().map(Some)
    }

    /// 编码器类别。
    pub fn codec(&self) -> Codec {
        self.codec
    }

    /// 已解出的帧数。
    pub fn decoded(&self) -> u64 {
        self.decoded
    }

    /// 因待取队列满而丢掉的帧数。
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// 首帧的硬校验结果；首帧还没解出来时为 `None`。
    pub fn first_frame(&self) -> Option<FirstFrame> {
        self.first
    }

    /// 把解码器里已解出的帧收进待取队列（`EAGAIN` 时用）。
    fn drain_ready(&mut self) -> DecodeResult<()> {
        loop {
            let ret = unsafe { sys::avcodec_receive_frame(self.ctx, self.frame) };
            if ret == AVERROR_EAGAIN || ret == AVERROR_EOF {
                return Ok(());
            }
            if ret < 0 {
                return Err(DecodeError::new(
                    Stage::Stream,
                    format!("receive_frame 失败：{}（{ret}）", av_err(ret)),
                ));
            }
            let frame = self.take_frame()?;
            if self.ready.len() >= READY_CAP {
                self.ready.pop_front();
                self.dropped += 1;
            }
            self.ready.push_back(frame);
        }
    }

    /// 对 `self.frame` 做硬校验 + 回读成 NV12，并 `unref` 原帧。
    fn take_frame(&mut self) -> DecodeResult<Nv12Frame> {
        let t0 = Instant::now();
        let has_hw = unsafe { !(*self.frame).hw_frames_ctx.is_null() };
        let hw_fmt = unsafe { (*self.frame).format };

        // ★ 硬校验：FFmpeg 对不支持的 profile / 尺寸**不报错，而是静默回落软解**
        //   （退出码仍为 0）⇒「建解码器成功」「没报错」都不能证明走了硬解。
        if self.first.is_none() && !(has_hw && hw_fmt == HW_FMT) {
            unsafe { sys::av_frame_unref(self.frame) };
            return Err(DecodeError::new(
                Stage::NotHardware,
                format!(
                    "首帧不是硬件帧（format={}, hw_frames_ctx={}）⇒ **静默软解**，拒绝该路",
                    pix_fmt_name(hw_fmt),
                    if has_hw { "非空" } else { "空" }
                ),
            ));
        }

        let ret = unsafe { sys::av_hwframe_transfer_data(self.sw, self.frame, 0) };
        let transfer_ms = t0.elapsed().as_secs_f64() * 1000.0;
        unsafe { sys::av_frame_unref(self.frame) };
        if ret < 0 {
            return Err(DecodeError::new(
                Stage::Transfer,
                format!("av_hwframe_transfer_data 失败：{}（{ret}）", av_err(ret)),
            ));
        }

        let sw = self.sw;
        let (w, h, sw_fmt) = unsafe { ((*sw).width as u32, (*sw).height as u32, (*sw).format) };
        // 硬解回读的软件格式：8-bit → NV12，10-bit → P010。本 crate 只实现 NV12。
        if sw_fmt != sys::AVPixelFormat::AV_PIX_FMT_NV12 as c_int {
            unsafe { sys::av_frame_unref(self.sw) };
            return Err(DecodeError::new(
                Stage::Format,
                format!(
                    "回读格式是 {}，本实现只支持 NV12（8-bit 4:2:0）",
                    pix_fmt_name(sw_fmt)
                ),
            ));
        }

        // 逐行拷贝：**行跨距（linesize）通常大于宽度**，不能整块 memcpy。
        let ls0 = unsafe { (*sw).linesize[0] } as usize;
        let ls1 = unsafe { (*sw).linesize[1] } as usize;
        let p0 = unsafe { (*sw).data[0] };
        let p1 = unsafe { (*sw).data[1] };
        let (wu, hu) = (w as usize, h as usize);
        let mut y = vec![0u8; wu * hu];
        let mut uv = vec![0u8; wu * hu / 2];
        unsafe {
            for r in 0..hu {
                ptr::copy_nonoverlapping(p0.add(r * ls0), y.as_mut_ptr().add(r * wu), wu);
            }
            for r in 0..hu / 2 {
                ptr::copy_nonoverlapping(p1.add(r * ls1), uv.as_mut_ptr().add(r * wu), wu);
            }
        }
        unsafe { sys::av_frame_unref(self.sw) };

        if self.first.is_none() {
            self.first = Some(FirstFrame {
                hw_format: hw_fmt,
                has_hw_frames_ctx: has_hw,
                sw_format: sw_fmt,
                transfer_ms,
            });
        }
        let index = self.decoded;
        self.decoded += 1;
        Ok(Nv12Frame {
            y,
            uv,
            width: w,
            height: h,
            index,
            pts_us: self.last_pts_us,
            ready_at: Instant::now(),
            transfer_ms,
        })
    }
}

impl Drop for VideoDecoder {
    fn drop(&mut self) {
        unsafe {
            sys::av_frame_free(&mut self.sw);
            sys::av_frame_free(&mut self.frame);
            sys::av_packet_free(&mut self.pkt);
            sys::avcodec_free_context(&mut self.ctx);
        }
    }
}

// 裸指针只是 FFmpeg 句柄的**独占所有权**，不表示共享可变状态：
// 解码器在会话线程里创建、也在会话线程里使用，跨线程只发生**所有权转移**（从不并发访问）。
// FFmpeg 的编解码上下文本身没有线程亲和性（硬件设备上下文才是 multithread-protected 的，
// 那部分由 [`HwDevice`] 的 `Send + Sync` 覆盖）。
unsafe impl Send for VideoDecoder {}
