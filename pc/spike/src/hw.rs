//! FFmpeg 硬解：设备探测 + **流式**解码 + 硬校验 + 回读（spike 核心）。
//!
//! **为什么直接调 `sys`（FFI）**：`ffmpeg-next` 高层**没有** hwaccel API
//! （另一主流绑定 `ffmpeg-the-third` 同样没有），而它**官方暴露**了 FFI 层
//! （`pub extern crate ffmpeg_sys_next as sys`）⇒ 硬解走这一层是**设计内的用法**，不是绕过。
//!
//! ⚠️ 本 spike **只实现 Windows / D3D11**（方案 §3.1 默认），所以硬件帧格式
//! 硬编码为 `AV_PIX_FMT_D3D11`。

use std::ffi::{c_char, c_int, CStr, CString};
use std::path::Path;
use std::ptr;

use ffmpeg_next::sys;

/// `AVERROR_EOF` —— FFmpeg 用 `MKTAG` 编码的错误码，**bindgen 不导出该宏**。
///
/// `MKTAG('E','O','F',' ')` = `0x20464F45`，取负即此值。
const AVERROR_EOF: c_int = -0x20464F45;

/// `AVERROR(EAGAIN)` —— 系统 `errno` 取负（`EAGAIN` 在 MSVC 与 POSIX 上都是 11）。
const AVERROR_EAGAIN: c_int = -11;

/// 硬件帧应有的像素格式 —— **唯一的硬解判据**。
const HW_FMT: c_int = sys::AVPixelFormat::AV_PIX_FMT_D3D11 as c_int;

/// `AV_NOPTS_VALUE` —— FFmpeg 的"无时间戳"哨兵值（**bindgen 不导出该宏**，其值即 `INT64_MIN`）。
const AV_NOPTS_VALUE: i64 = i64::MIN;

/// 解码链路的**失败阶段**。
///
/// 方案 §3.3B 要求准入失败必须**按阶段分开上报** —— 因为「建解码器失败」与
/// 「首帧不是硬件帧」对上游是完全不同的结论（后者说明该 profile/尺寸**只能软解**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// 建硬解设备 / 打开容器 / 建解码器。
    Open,
    /// 解首帧。
    FirstFrame,
    /// **首帧不是硬件帧** —— 即 FFmpeg 静默回落软解。
    NotHardware,
    /// `av_hwframe_transfer_data` 回读。
    Transfer,
    /// 回读出的格式不是本 spike 实现的 NV12。
    Format,
}

impl Stage {
    /// 阶段名（日志与 UI 用，ASCII 以免 egui 默认字体缺字）。
    pub fn name(self) -> &'static str {
        match self {
            Stage::Open => "decoder-open",
            Stage::FirstFrame => "first-frame",
            Stage::NotHardware => "not-hardware-frame",
            Stage::Transfer => "readback",
            Stage::Format => "readback-format",
        }
    }
}

/// 带阶段标记的解码失败。
#[derive(Debug)]
pub struct DecodeError {
    /// 失败阶段。
    pub stage: Stage,
    /// 可读原因。
    pub detail: String,
}

impl DecodeError {
    /// 造一个阶段失败。
    fn new(stage: Stage, detail: impl Into<String>) -> Self {
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

/// 把一个 FFmpeg 返回码转成可读文本。
pub fn av_err(ret: c_int) -> String {
    let mut buf = [0 as c_char; 256];
    unsafe { sys::av_strerror(ret, buf.as_mut_ptr(), buf.len()) };
    unsafe { CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned()
}

/// 把一个 FFmpeg 的 `char*` 转成 `String`（空指针给占位符）。
unsafe fn cstr_or(p: *const c_char, fallback: &str) -> String {
    if p.is_null() {
        fallback.to_string()
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

// ---------------------------------------------------------------- 设备探测

/// 硬解设备的探测结果。
#[derive(Debug)]
pub struct DeviceProbe {
    /// 设备类型名（如 `d3d11va`），来自 `av_hwdevice_get_type_name`。
    pub name: String,
    /// 能否**真的**创建出来（`av_hwdevice_ctx_create` 是否成功）。
    pub creatable: bool,
    /// 创建失败时的可读原因。
    pub error: Option<String>,
}

/// 枚举本机**编译进来**的全部硬解设备，并逐个**实试创建**。
///
/// ⚠️ **"编译进来" ≠ "本机能用"** —— 所以 `creatable` 必须实试，不能只看枚举
/// （这与「探测给候选，试错给结论」一致）。
pub fn probe_devices() -> Vec<DeviceProbe> {
    let mut out = Vec::new();
    let mut t = sys::AVHWDeviceType::AV_HWDEVICE_TYPE_NONE;
    loop {
        t = unsafe { sys::av_hwdevice_iterate_types(t) };
        if t == sys::AVHWDeviceType::AV_HWDEVICE_TYPE_NONE {
            break;
        }
        let name = unsafe { cstr_or(sys::av_hwdevice_get_type_name(t), "?") };
        let mut ctx: *mut sys::AVBufferRef = ptr::null_mut();
        let ret = unsafe {
            sys::av_hwdevice_ctx_create(&mut ctx, t, ptr::null(), ptr::null_mut(), 0)
        };
        let (creatable, error) = if ret < 0 {
            (false, Some(av_err(ret)))
        } else {
            unsafe { sys::av_buffer_unref(&mut ctx) };
            (true, None)
        };
        out.push(DeviceProbe { name, creatable, error });
    }
    out
}

/// 按名字找硬解设备类型（`av_hwdevice_find_type_by_name`）。
pub fn device_by_name(name: &str) -> DecodeResult<sys::AVHWDeviceType> {
    let c = CString::new(name)
        .map_err(|_| DecodeError::new(Stage::Open, format!("设备名含 NUL: {name}")))?;
    let t = unsafe { sys::av_hwdevice_find_type_by_name(c.as_ptr()) };
    if t == sys::AVHWDeviceType::AV_HWDEVICE_TYPE_NONE {
        return Err(DecodeError::new(Stage::Open, format!("未知硬解设备: {name}")));
    }
    Ok(t)
}

/// 像素格式的短名（`av_get_pix_fmt_name`）。
///
/// `AVFrame.format` 是 `c_int`，而 `av_get_pix_fmt_name` 收 enum ⇒ 需要转回。
/// **FFmpeg 返回的格式值都是合法枚举**，所以这里的 `transmute` 安全。
pub fn pix_fmt_name(fmt: i32) -> String {
    let f: sys::AVPixelFormat = unsafe { std::mem::transmute(fmt) };
    unsafe {
        let p = sys::av_get_pix_fmt_name(f);
        if p.is_null() {
            format!("(未知 {fmt})")
        } else {
            CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }
}

// ---------------------------------------------------------------- 硬解设备

/// 一个硬解设备（`AVBufferRef`），**多路解码器共享**。
///
/// 共享而非每路一个：产品里所有路都挂在**同一个 D3D11 设备**上，
/// 每路各建一个会虚高设备数、把容量数据测歪。
pub struct HwDevice {
    ptr: *mut sys::AVBufferRef,
    /// 设备类型名（如 `d3d11va`）。
    pub name: String,
}

// `av_hwdevice_ctx_create` 返回的是**原子引用计数**的 `AVBufferRef`，
// 且 FFmpeg 建的 D3D11 设备默认 multithread-protected
// ⇒ 跨线程共享是 FFmpeg 的既定用法（`av_buffer_ref` 是原子的）。
unsafe impl Send for HwDevice {}
unsafe impl Sync for HwDevice {}

impl HwDevice {
    /// 建一个硬解设备（名字如 `d3d11va`）。
    pub fn create(name: &str) -> DecodeResult<Self> {
        let dev_type = device_by_name(name)?;
        let mut ptr: *mut sys::AVBufferRef = ptr::null_mut();
        let ret = unsafe {
            sys::av_hwdevice_ctx_create(&mut ptr, dev_type, ptr::null(), ptr::null_mut(), 0)
        };
        if ret < 0 {
            return Err(DecodeError::new(
                Stage::Open,
                format!("建硬解设备 {name} 失败: {}（{ret}）", av_err(ret)),
            ));
        }
        Ok(Self { ptr, name: name.to_string() })
    }
}

impl Drop for HwDevice {
    fn drop(&mut self) {
        unsafe { sys::av_buffer_unref(&mut self.ptr) };
    }
}

// ---------------------------------------------------------------- 解码

/// `get_format` 回调：**只从候选里选 `AV_PIX_FMT_D3D11`**。
///
/// 找不到就返回 `AV_PIX_FMT_NONE` ⇒ `avcodec_open2` 失败 ——
/// 这是「**绝不软解**」的落点：FFmpeg 默认会**静默回退到软解**（退出码仍为 0），
/// 而本回调让"拿不到硬解"直接变成**明确的打开失败**。
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

/// 首帧的**硬校验**与回读结果。
#[derive(Debug, Clone, Copy)]
pub struct FirstFrame {
    /// `AVFrame.format` —— 硬解时应为 `AV_PIX_FMT_D3D11`。
    pub hw_format: i32,
    /// `AVFrame.hw_frames_ctx` 是否非空 —— **唯一的硬解判据**。
    pub has_hw_frames_ctx: bool,
    /// 回读后的软件帧格式。
    pub sw_format: i32,
    /// 回读耗时（毫秒）。
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
    pub width: u32,
    pub height: u32,
    /// 本路解出的帧序号（从 0 起）。
    pub index: u64,
    /// 该帧在**源时间轴**上的时刻（毫秒）。
    ///
    /// `None` = **源无时间戳**（裸 Annex-B 流就是这样）⇒ 按**到达节奏**呈现，
    /// 不做时间轴节流。这正是**直播**语义：真实链路送过来的是实时流，
    /// 没有可对齐的"源时间轴"，也不需要节流。
    pub pts_ms: Option<f64>,
    /// 该帧**回读完成**（= 解码链路交付）的墙钟时刻 —— 端到端延迟的起点。
    pub ready_at: std::time::Instant,
    /// 「收帧 → 回读完成」的耗时（毫秒）—— T1 通路的核心开销。
    pub transfer_ms: f64,
}

/// `open` 期间各 FFmpeg 句柄的**临时所有者**：出错时按序释放，成功时交给 `HwDecoder`。
///
/// 存在理由：`HwDecoder` 自身有 `Drop`，没法"部分构造后返回 `Err`"，
/// 所以中途失败必须有人负责收尾。
struct Pending {
    fmt: *mut sys::AVFormatContext,
    ctx: *mut sys::AVCodecContext,
    pkt: *mut sys::AVPacket,
    frame: *mut sys::AVFrame,
    sw: *mut sys::AVFrame,
}

impl Pending {
    /// 空的所有者（`open` 的起点）。
    fn new() -> Self {
        Self {
            fmt: ptr::null_mut(),
            ctx: ptr::null_mut(),
            pkt: ptr::null_mut(),
            frame: ptr::null_mut(),
            sw: ptr::null_mut(),
        }
    }

    /// 交出所有权：各字段置空 ⇒ 之后的 `Drop` 是空操作。
    ///
    /// ⚠️ **必须返回裸指针元组，不能返回 `Pending`** —— 返回 `Pending` 的话，
    /// 调用方读完字段后那个临时值会析构，把刚交出去的句柄全部释放（use-after-free）。
    fn take(
        &mut self,
    ) -> (
        *mut sys::AVFormatContext,
        *mut sys::AVCodecContext,
        *mut sys::AVPacket,
        *mut sys::AVFrame,
        *mut sys::AVFrame,
    ) {
        let out = (self.fmt, self.ctx, self.pkt, self.frame, self.sw);
        self.fmt = ptr::null_mut();
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
            if !self.fmt.is_null() {
                sys::avformat_close_input(&mut self.fmt);
            }
        }
    }
}

/// 输入容器的强制类型。
///
/// **真实链路送过来的是裸 Annex-B 码流**（无容器、无 CSD 交换）⇒ 必须强制用
/// `h264` / `hevc` 裸解复用器，否则 `avformat_open_input` 猜不出格式。
/// spike 用本地 mp4 时走 [`InputFormat::Auto`]。
#[derive(Clone, Debug)]
pub enum InputFormat {
    /// 自动探测（本地 mp4）。
    Auto,
    /// 强制某个裸解复用器（`h264` / `hevc`）。
    Forced(String),
}

/// 一路**流式**硬解解码器。
///
/// 与「一次解完拿报告」的区别：本类型**逐帧产出**，供播放链路使用；
/// 且**首帧必做硬校验** —— 拿到非硬件帧立即报错，绝不静默软解。
pub struct HwDecoder {
    /// 容器（本结构持有，负责关闭）。
    fmt: *mut sys::AVFormatContext,
    /// 解码器上下文（本结构持有，负责释放）。
    ctx: *mut sys::AVCodecContext,
    pkt: *mut sys::AVPacket,
    frame: *mut sys::AVFrame,
    sw: *mut sys::AVFrame,
    /// 视频流下标。
    vs: i32,
    /// 视频流的时基（把 `AVFrame.pts` 换算成毫秒）。
    tb: sys::AVRational,
    /// 输入是否已到文件尾。
    eof: bool,
    /// 是否已送过冲帧用的空包（**只能送一次**）。
    flushed: bool,
    /// 解码器名。
    pub codec: String,
    pub width: u32,
    pub height: u32,
    /// 已解出的帧数。
    pub decoded: u64,
    /// 首帧的硬校验结果（首帧出来后填充）。
    pub first: Option<FirstFrame>,
}

impl HwDecoder {
    /// 用共享硬解设备打开样片。
    ///
    /// **不在这里做硬校验** —— 校验发生在首帧真正解出时（[`Self::next_frame`]），
    /// 因为「解码器打开成功」根本不能证明走了硬解。
    pub fn open(path: &Path, dev: &HwDevice, fmt_in: &InputFormat) -> DecodeResult<Self> {
        /// `open` 阶段的失败都是同一类，所以就地包一个简写。
        fn e(msg: impl Into<String>) -> DecodeError {
            DecodeError::new(Stage::Open, msg)
        }
        let mut p = Pending::new();

        // ① 打开输入（裸流必须强制解复用器）
        let cpath = CString::new(path.to_string_lossy().as_bytes()).map_err(|_| e("路径含 NUL"))?;
        let ifmt = match fmt_in {
            InputFormat::Auto => ptr::null(),
            InputFormat::Forced(name) => {
                let c = CString::new(name.as_str()).map_err(|_| e("格式名含 NUL"))?;
                let f = unsafe { sys::av_find_input_format(c.as_ptr()) };
                if f.is_null() {
                    return Err(e(format!("FFmpeg 没有名为 {name} 的解复用器")));
                }
                f
            }
        };
        let ret = unsafe {
            sys::avformat_open_input(&mut p.fmt, cpath.as_ptr(), ifmt, ptr::null_mut())
        };
        if ret < 0 {
            return Err(e(format!("打开 {} 失败: {}", path.display(), av_err(ret))));
        }
        if unsafe { sys::avformat_find_stream_info(p.fmt, ptr::null_mut()) } < 0 {
            return Err(e("find_stream_info 失败"));
        }

        // ② 找视频流
        let nb = unsafe { (*p.fmt).nb_streams } as usize;
        let mut vs: i32 = -1;
        for i in 0..nb {
            let st = unsafe { *(*p.fmt).streams.add(i) };
            if unsafe { (*(*st).codecpar).codec_type } == sys::AVMediaType::AVMEDIA_TYPE_VIDEO {
                vs = i as i32;
                break;
            }
        }
        if vs < 0 {
            return Err(e(format!("{} 里没有视频流", path.display())));
        }
        let par = unsafe { (*(*(*p.fmt).streams.add(vs as usize))).codecpar };
        let tb = unsafe { (*(*(*p.fmt).streams.add(vs as usize))).time_base };

        // ③ 解码器：**先设 hw 相关字段，再 open**
        let codec = unsafe { sys::avcodec_find_decoder((*par).codec_id) };
        if codec.is_null() {
            return Err(e("找不到解码器"));
        }
        let codec_name = unsafe { cstr_or((*codec).name, "?") };
        p.ctx = unsafe { sys::avcodec_alloc_context3(codec) };
        if p.ctx.is_null() {
            return Err(e("avcodec_alloc_context3 失败"));
        }
        if unsafe { sys::avcodec_parameters_to_context(p.ctx, par) } < 0 {
            return Err(e("parameters_to_context 失败"));
        }
        unsafe {
            (*p.ctx).hw_device_ctx = sys::av_buffer_ref(dev.ptr);
            (*p.ctx).get_format = Some(get_format_hw);
        }
        let ret = unsafe { sys::avcodec_open2(p.ctx, codec, ptr::null_mut()) };
        if ret < 0 {
            return Err(e(format!(
                "avcodec_open2 失败（**该硬解在此 codec/尺寸上不可用**）: {}（{ret}）",
                av_err(ret)
            )));
        }

        // ④ 缓冲
        p.pkt = unsafe { sys::av_packet_alloc() };
        p.frame = unsafe { sys::av_frame_alloc() };
        p.sw = unsafe { sys::av_frame_alloc() };
        if p.pkt.is_null() || p.frame.is_null() || p.sw.is_null() {
            return Err(e("alloc packet/frame 失败"));
        }

        let (width, height) = unsafe { ((*p.ctx).width as u32, (*p.ctx).height as u32) };
        let (fmt, ctx, pkt, frame, sw) = p.take();
        Ok(Self {
            fmt,
            ctx,
            pkt,
            frame,
            sw,
            vs,
            tb,
            eof: false,
            flushed: false,
            codec: codec_name,
            width,
            height,
            decoded: 0,
            first: None,
        })
    }

    /// 解出下一帧并回读为 NV12；到文件尾返回 `Ok(None)`。
    ///
    /// **首帧做硬校验**：非硬件帧直接 `Err`。
    pub fn next_frame(&mut self) -> DecodeResult<Option<Nv12Frame>> {
        loop {
            // 需要更多输入时读包
            if !self.eof {
                let ret = unsafe { sys::av_read_frame(self.fmt, self.pkt) };
                if ret == AVERROR_EOF {
                    self.eof = true;
                } else if ret < 0 {
                    return Err(DecodeError::new(
                        Stage::FirstFrame,
                        format!("read_frame 失败: {}（{ret}）", av_err(ret)),
                    ));
                } else if unsafe { (*self.pkt).stream_index } != self.vs {
                    unsafe { sys::av_packet_unref(self.pkt) };
                    continue;
                } else {
                    let ret = unsafe { sys::avcodec_send_packet(self.ctx, self.pkt) };
                    unsafe { sys::av_packet_unref(self.pkt) };
                    if ret < 0 && ret != AVERROR_EAGAIN {
                        return Err(DecodeError::new(
                            Stage::FirstFrame,
                            format!("send_packet 失败: {}（{ret}）", av_err(ret)),
                        ));
                    }
                }
            }
            // 输入结束：送一个空包把解码器里的帧冲出来（只能送一次）
            if self.eof && !self.flushed {
                self.flushed = true;
                unsafe { sys::avcodec_send_packet(self.ctx, ptr::null()) };
            }

            let ret = unsafe { sys::avcodec_receive_frame(self.ctx, self.frame) };
            if ret == AVERROR_EAGAIN {
                if self.eof {
                    return Ok(None);
                }
                continue;
            }
            if ret == AVERROR_EOF {
                return Ok(None);
            }
            if ret < 0 {
                return Err(DecodeError::new(
                    Stage::FirstFrame,
                    format!("receive_frame 失败: {}（{ret}）", av_err(ret)),
                ));
            }
            return self.take_frame().map(Some);
        }
    }

    /// 对 `self.frame` 做硬校验 + 回读成 NV12，并 `unref` 原帧。
    fn take_frame(&mut self) -> DecodeResult<Nv12Frame> {
        let t0 = std::time::Instant::now();
        let has_hw = unsafe { !(*self.frame).hw_frames_ctx.is_null() };
        let hw_fmt = unsafe { (*self.frame).format };
        // 源时间轴上的时刻（毫秒）。用 `best_effort_timestamp`：部分封装只填它不填 `pts`。
        // **拿不到就是 `None`** —— 裸 Annex-B 流完全没有时间戳，此时按到达节奏呈现。
        let ts = unsafe { (*self.frame).best_effort_timestamp };
        let pts_ms = if ts == AV_NOPTS_VALUE || self.tb.den == 0 {
            None
        } else {
            Some(ts as f64 * self.tb.num as f64 / self.tb.den as f64 * 1000.0)
        };

        // ★ 硬校验：FFmpeg 对不支持的 profile / 尺寸**不报错，而是静默回落软解**
        //   （退出码仍为 0）⇒「建解码器成功」「没报错」都不能证明走了硬解。
        //   唯一可靠判据是**帧本身是硬件帧**。
        if self.first.is_none() && !(has_hw && hw_fmt == HW_FMT) {
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
                format!("av_hwframe_transfer_data 失败: {}（{ret}）", av_err(ret)),
            ));
        }

        let sw = self.sw;
        let (w, h, sw_fmt) = unsafe { ((*sw).width as u32, (*sw).height as u32, (*sw).format) };
        // 尺寸以**实际解出的帧**为准：裸流（无容器）在 `open` 时往往还拿不到宽高。
        self.width = w;
        self.height = h;
        // 硬解回读的软件格式：8-bit → NV12，10-bit → P010。spike 只实现 NV12。
        if sw_fmt != sys::AVPixelFormat::AV_PIX_FMT_NV12 as c_int {
            unsafe { sys::av_frame_unref(self.sw) };
            return Err(DecodeError::new(
                Stage::Format,
                format!("回读格式是 {}，spike 只实现 NV12", pix_fmt_name(sw_fmt)),
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
            pts_ms,
            ready_at: std::time::Instant::now(),
            transfer_ms,
        })
    }
}

impl Drop for HwDecoder {
    fn drop(&mut self) {
        unsafe {
            sys::av_frame_free(&mut self.sw);
            sys::av_frame_free(&mut self.frame);
            sys::av_packet_free(&mut self.pkt);
            sys::avcodec_free_context(&mut self.ctx);
            sys::avformat_close_input(&mut self.fmt);
        }
    }
}
