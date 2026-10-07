//! PC 端 spike 的可执行入口：**egui + wgpu + FFmpeg 硬解**的多路验证链路。
//!
//! 目的见 `docs/plans/2026-10-06-pc-spike.md` §1：用**实测数据**回答三个问题
//! （FFmpeg 硬解帧能否与 wgpu 协作、平台硬解容量、超长尺寸能否硬解）。
//! **本 crate 不产出产品功能** —— spike 是一次性验证代码，验收后按结论决定保留 / 重写。
//!
//! 帧通路（方案 §3.1 的 **T1**）：
//! `硬解 → av_hwframe_transfer_data 回读 NV12 → 两个平面纹理 → 着色器转 RGB → egui 上屏`
//!
//! 准入流程按方案 §3.3B：**每新增一路就是一次真实探测**（建解码器 → 首帧硬校验 →
//! 回读 → 建纹理 → 上传），任一步失败就**完整回滚该路**并返回**带阶段**的原因。
//!
//! 用法：`spike [样片路径] [硬解设备名] [裸解复用器]`
//!
//! 第三个参数省略 = 自动探测（本地 mp4）；给 `h264` / `hevc` 则走**裸 Annex-B**
//! 路径 —— 那才是真实链路（F4 送裸码流、无容器）走的解复用方式。
//!
//! 故障注入（方案 §3.3E，否则回收/回滚代码永远不会被执行）：
//! `SPIKE_FAIL_OPEN=<第N路>`、`SPIKE_FAIL_TIMEOUT=<第N路>`

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::{egui, egui_wgpu, wgpu};

use spike::hw;
use spike::render::{Nv12Pipeline, StreamTarget};

/// 默认样片：1080p60 —— 验收 3「单路持续播放」的测量对象。
const DEFAULT_SAMPLE: &str = r"D:\wcreen-samples\t-1920x1080-h264.mp4";

/// 默认硬解设备。
const DEFAULT_DEVICE: &str = "d3d11va";

/// 通道容量 —— 取小值让解码线程被 UI 的刷新节奏**反压**。
const CHANNEL_CAP: usize = 2;

/// 展示队列容量（帧）。UI 只取队列装得下的数量，**剩下的留在通道里**，
/// 于是解码器被「通道 + 队列」一起反压，既不会跑飞，也不会积压到看不出延迟。
const QUEUE_CAP: usize = 4;

/// 缓冲深度（帧）—— **延迟与平滑度的旋钮**。
///
/// 总缓冲 = 展示队列 + 通道，稳态下约占满 ⇒ 链路延迟 ≈ 总帧数 × 帧间隔。
/// 实测 `4 + 2 = 6` 帧 @60fps ⇒ **105 ms**（验收 8）。
/// 可用 `SPIKE_BUFFER=<帧数>` 覆盖以标定该旋钮。
#[derive(Clone, Copy)]
struct Buffer {
    /// 展示队列容量。
    queue: usize,
    /// 通道容量。
    channel: usize,
}

impl Buffer {
    /// 默认（读 `SPIKE_BUFFER`，缺省 `QUEUE_CAP + CHANNEL_CAP`）。
    fn from_env() -> Self {
        match std::env::var("SPIKE_BUFFER").ok().and_then(|v| v.parse::<usize>().ok()) {
            // 通道至少 1 帧，其余给展示队列；都至少 1，否则反压会死锁
            Some(n) if n >= 2 => Self { channel: 1, queue: n - 1 },
            _ => Self { queue: QUEUE_CAP, channel: CHANNEL_CAP },
        }
    }

    /// 总缓冲帧数。
    fn frames(self) -> usize {
        self.queue + self.channel
    }

    /// 「落后太多」的阈值 —— **必须显著大于总缓冲时长**，
    /// 否则稳态下每帧都恰好"迟到"，丢帧策略会持续误触发。
    ///
    /// 取 `总帧数 × 2 帧间隔`（@60fps 即 `frames × 33 ms`），下限 250 ms。
    fn max_lag(self) -> Duration {
        Duration::from_millis((self.frames() as u64 * 33).max(250))
    }
}

/// 耗时样本的保留条数（约 10 秒 @60fps）。
const SAMPLE_CAP: usize = 600;

/// 准入超时：从起线程到收到「首帧过硬校验」的最长等待。
const ADMIT_TIMEOUT: Duration = Duration::from_secs(5);

/// 容量探测的路数上限（防止把机器跑爆）。
const PROBE_MAX: usize = 16;

/// UI 里最多显示几路画面（其余路**照常跑完整链路**，只是不画到窗口里）。
const DISPLAY_MAX: usize = 4;

/// 设备级错误的收集器。
///
/// 存在理由：**wgpu 的 `create_texture` 不返回 `Result`** —— 显存耗尽之类的失败
/// 只能通过 device error 暴露。方案 §3.3B 要求「纹理创建失败」是独立的一类，
/// 所以必须在准入时能把这段区间内的设备错误捞出来。
#[derive(Clone, Default)]
struct DeviceErrors(Arc<parking_lot::Mutex<Vec<String>>>);

impl DeviceErrors {
    /// 取走并清空自上次调用以来累积的错误。
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut self.0.lock())
    }
}

/// 一路的规格。
#[derive(Clone)]
struct Spec {
    /// 样片路径。
    path: PathBuf,
    /// 输入容器类型（裸 Annex-B 要强制解复用器）。
    fmt: hw::InputFormat,
    /// 分辨率标签（容量曲线按它分组）。
    label: String,
}

/// 故障注入开关（方案 §3.3E）。
#[derive(Clone, Copy, Default)]
struct Fault {
    /// 第 N 路（从 0 起）强制「建解码器失败」。
    fail_open: Option<usize>,
    /// 第 N 路强制「首帧超时」（线程起来后干等着，不产出）。
    fail_timeout: Option<usize>,
}

impl Fault {
    /// 从环境变量读开关。
    fn from_env() -> Self {
        let parse = |k: &str| std::env::var(k).ok().and_then(|v| v.parse().ok());
        Self {
            fail_open: parse("SPIKE_FAIL_OPEN"),
            fail_timeout: parse("SPIKE_FAIL_TIMEOUT"),
        }
    }
}

/// 一路就绪时的**硬解自证**信息（验收 2 要显示的东西）。
struct ReadyInfo {
    /// 硬解设备名。
    device: String,
    /// 解码器名。
    codec: String,
    /// 分辨率。
    width: u32,
    height: u32,
    /// 首帧硬校验结果。
    first: hw::FirstFrame,
}

/// 解码线程 → UI 线程的消息。
enum Msg {
    /// 该路已就绪：首帧**已通过硬校验**。
    Ready(ReadyInfo),
    /// 一帧回读好的 NV12。
    Frame(hw::Nv12Frame),
    /// 该路结束：`None` = 正常到尾，`Some` = 失败（**带阶段**）。
    Done(Option<hw::DecodeError>),
}

/// 准入失败的分类（方案 §3.3B：五类必须分开）。
#[derive(Debug)]
enum AdmitError {
    /// 建硬解设备 / 打开解码器失败。
    DecoderOpen(String),
    /// 首帧超时。
    FirstFrameTimeout,
    /// **首帧不是硬件帧** —— 即静默软解。
    NotHardware(String),
    /// 回读失败。
    Transfer(String),
    /// 建纹理 / 注册纹理失败。
    Texture(String),
}

impl std::fmt::Display for AdmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdmitError::DecoderOpen(e) => write!(f, "decoder-open: {e}"),
            AdmitError::FirstFrameTimeout => write!(f, "first-frame-timeout"),
            AdmitError::NotHardware(e) => write!(f, "not-hardware-frame: {e}"),
            AdmitError::Transfer(e) => write!(f, "readback: {e}"),
            AdmitError::Texture(e) => write!(f, "texture: {e}"),
        }
    }
}

/// 只保留最近 [`SAMPLE_CAP`] 个样本。
fn push_capped(v: &mut Vec<f64>, x: f64) {
    if v.len() >= SAMPLE_CAP {
        v.remove(0);
    }
    v.push(x);
}

/// 取样本的 (P50, P95)（毫秒）。
fn p50_p95(v: &[f64]) -> (f64, f64) {
    if v.is_empty() {
        return (0.0, 0.0);
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    (s[s.len() / 2], s[(s.len() * 95 / 100).min(s.len() - 1)])
}

/// 解码线程：打开一路 → 首帧硬校验 → 逐帧回读 → 送 UI。
///
/// `loop_file` 为真时到文件尾会 seek 回开头 —— 容量探测需要**持续**的流，
/// 否则样片放完就自然结束了。
fn decode_loop(
    spec: Spec,
    dev: Arc<hw::HwDevice>,
    tx: SyncSender<Msg>,
    ctx: egui::Context,
    fault: Fault,
    loop_file: bool,
) {
    // 故障注入：强制「首帧超时」= 线程起来后什么都不做，让准入超时触发
    if fault.fail_timeout.is_some() {
        eprintln!("[故障注入] 强制首帧超时");
        std::thread::sleep(Duration::from_secs(3600));
        return;
    }

    let r = (|| -> Result<(), hw::DecodeError> {
        let mut dec = hw::HwDecoder::open(&spec.path, &dev, &spec.fmt)?;

        // 首帧单独走一遍：**它能出来就说明已通过硬校验**（判定在 `HwDecoder` 内部，
        // 非硬件帧会直接 `Err`），所以这里可以放心把结论当"真硬解"上报。
        let mut f = dec.next_frame()?.ok_or_else(|| hw::DecodeError {
            stage: hw::Stage::FirstFrame,
            detail: "一帧都没解出来".into(),
        })?;
        let ready = Msg::Ready(ReadyInfo {
            device: dev.name.clone(),
            codec: dec.codec.clone(),
            width: dec.width,
            height: dec.height,
            first: dec.first.expect("首帧已出，校验结果必然存在"),
        });
        // **先叫醒 UI 再送帧** —— `send` 可能阻塞在满通道上；若 `request_repaint` 放在
        // `send` 之后，一旦阻塞就没人能叫醒 UI ⇒ 双方互等，播放停死。
        ctx.request_repaint();
        if tx.send(ready).is_err() || tx.send(Msg::Frame(f)).is_err() {
            return Ok(());
        }

        loop {
            f = match dec.next_frame()? {
                Some(f) => f,
                None => {
                    if !loop_file {
                        break;
                    }
                    // 重开一路（简单可靠，容量探测只关心"持续有帧"）
                    dec = hw::HwDecoder::open(&spec.path, &dev, &spec.fmt)?;
                    continue;
                }
            };
            // 同上：**先叫醒，再送**（顺序不能换）
            ctx.request_repaint();
            // 送不出去 = UI 已退出，安静收工
            if tx.send(Msg::Frame(f)).is_err() {
                return Ok(());
            }
        }
        Ok(())
    })();

    if let Err(e) = &r {
        eprintln!("**解码失败** {} [{}] {}", spec.label, e.stage.name(), e.detail);
    }
    let _ = tx.send(Msg::Done(r.err()));
}

/// 保持宽高比，把 `src` 塞进 `dst`。
fn fit(src: [f32; 2], dst: egui::Vec2) -> egui::Vec2 {
    let s = (dst.x / src[0]).min(dst.y / src[1]);
    egui::vec2(src[0] * s, src[1] * s)
}

/// 一路的运行时状态。
struct Stream {
    /// 规格。
    spec: Spec,
    /// 缓冲深度（延迟旋钮）。
    buf: Buffer,
    /// 消息口。
    rx: Receiver<Msg>,
    /// 展示队列。
    queue: VecDeque<hw::Nv12Frame>,
    /// 播放时钟：`(首帧上屏的墙钟, 首帧的源时刻 ms)`。
    clock: Option<(Instant, f64)>,
    /// 该路的纹理。
    target: Option<StreamTarget>,
    /// 注册给 egui 的纹理 id（只有前 [`DISPLAY_MAX`] 路注册）。
    tex_id: Option<egui::TextureId>,
    /// 硬解自证。
    ready: Option<ReadyInfo>,
    /// 回读耗时样本（毫秒）。
    transfer: Vec<f64>,
    /// 上传 + 渲染耗时样本（毫秒）。
    upload: Vec<f64>,
    /// **链路延迟**样本（毫秒）：回读完成 → 提交上屏。
    lat_pipe: Vec<f64>,
    /// **端到端延迟**样本（毫秒）：源时间轴应上屏时刻 → 实际提交上屏。
    ///
    /// 含排队时间，是"观众感知到的滞后"的量化（验收 8）。
    lat_e2e: Vec<f64>,
    /// 已上屏帧数。
    shown: u64,
    /// 因落后被丢掉的帧数。
    dropped: u64,
    /// 最近上屏的帧号。
    last_index: u64,
    /// FPS 统计窗口起点。
    fps_window: Instant,
    /// 窗口内已上屏帧数。
    fps_count: u32,
    /// 最近一次测得的 FPS —— **「可持续」的直接判据**。
    fps: f32,
    /// 状态文字（ASCII）。
    status: String,
}

impl Stream {
    /// 该帧在**播放时间轴**上相对开播的到期偏移。
    ///
    /// 时钟未启动、或**源无时间戳**（直播）时恒为 0 ⇒ 立即到期、不做节流。
    fn due_offset(&self, f: &hw::Nv12Frame) -> Duration {
        match (self.clock, f.pts_ms) {
            (Some((_, pts0)), Some(p)) => Duration::from_secs_f64(((p - pts0) / 1000.0).max(0.0)),
            _ => Duration::ZERO,
        }
    }

    /// 开播以来经过的时长（时钟未启动时为 0）。
    fn elapsed(&self) -> Duration {
        self.clock.map(|(t0, _)| t0.elapsed()).unwrap_or_default()
    }

    /// 把消息搬进展示队列 —— **只取队列装得下的数量**（其余留在通道里反压解码线程）。
    fn fill_queue(&mut self) {
        while self.queue.len() < self.buf.queue {
            match self.rx.try_recv() {
                Ok(Msg::Ready(r)) => self.ready = Some(r),
                Ok(Msg::Frame(f)) => self.queue.push_back(f),
                Ok(Msg::Done(r)) => {
                    self.status = match r {
                        None => "end of file".to_string(),
                        Some(e) => format!("FAILED [{}]", e.stage.name()),
                    };
                    break;
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
    }

    /// 推进一路：收帧 → 按源时间轴节流 → 上传 → 转 RGB。
    ///
    /// **每路都跑完整链路**（含 `render`）—— 只有「画到窗口里」这一步是前
    /// [`DISPLAY_MAX`] 路独有，否则容量数据会失真。
    fn tick(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipe: &Nv12Pipeline,
        ctx: &egui::Context,
    ) {
        self.fill_queue();
        let Some(front) = self.queue.front() else {
            return;
        };

        // **源无时间戳 = 直播**：真实链路送的是实时流，没有可对齐的"源时间轴"，
        // 也不该节流 —— 按**到达节奏**呈现即可（到达速率就是编码器的速率）。
        let live = front.pts_ms.is_none();
        let due = self.due_offset(front);
        let now = self.elapsed();

        if !live {
            if now < due {
                // 还没到点：安排到点再画（解码线程送帧时也会叫醒，这里只保证不早画）
                ctx.request_repaint_after(due - now);
                return;
            }
            if now - due > self.buf.max_lag() {
                // 落后太多 —— **丢掉积压并把时钟重新对齐到最新一帧**。
                //
                // 不能逐帧追赶：追赶只会让播放速度超过源速度，而且积压越追越多。
                // 重新对齐后播放速度回到源速度，**实际的帧率缺口由 `fps` 字段暴露**
                // （这正是容量探测要的判据）。
                while self.queue.len() > 1 {
                    self.queue.pop_front();
                    self.dropped += 1;
                }
                let f = self.queue.pop_front().expect("刚判过非空");
                self.dropped += 1;
                self.clock = Some((Instant::now(), f.pts_ms.expect("非 live 必有 pts")));
                self.present(&f, device, queue, pipe);
                // 重对齐后本帧的"应上屏时刻"就是现在 ⇒ 端到端延迟为 0（积压已丢弃）
                push_capped(&mut self.lat_e2e, 0.0);
                ctx.request_repaint();
                return;
            }
        }

        let f = self.queue.pop_front().expect("刚判过非空");
        if self.clock.is_none() {
            if let Some(p) = f.pts_ms {
                self.clock = Some((Instant::now(), p));
            }
        }
        self.present(&f, device, queue, pipe);
        if !live {
            // 端到端口径只在**有时间轴**时才有意义
            push_capped(
                &mut self.lat_e2e,
                now.as_secs_f64() * 1000.0 - due.as_secs_f64() * 1000.0,
            );
        }

        // 排下一帧。
        //
        // **队列空时必须立刻再跑一次**：`fill_queue` 在队列满时**不取通道**，
        // 于是显示完最后一帧后，通道里可能还压着一帧、而解码线程正阻塞在 `send` 上
        // —— 它的唤醒标志在阻塞**之前**就已经用掉了。此时若什么都不排，
        // 双方互等，播放就停死在这里（`queue` 容量 1 时每帧都会撞上）。
        match self.queue.front() {
            Some(next) => {
                let d = self.due_offset(next);
                let n = self.elapsed();
                if n >= d {
                    ctx.request_repaint();
                } else {
                    ctx.request_repaint_after(d - n);
                }
            }
            None => ctx.request_repaint(),
        }
    }

    /// 把一帧真正送上去：上传 → 转 RGB → 记统计。
    fn present(
        &mut self,
        f: &hw::Nv12Frame,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipe: &Nv12Pipeline,
    ) {
        if let Some(t) = &self.target {
            let t0 = Instant::now();
            t.upload(queue, f);
            t.render(device, queue, pipe);
            push_capped(&mut self.upload, t0.elapsed().as_secs_f64() * 1000.0);
        }
        push_capped(&mut self.transfer, f.transfer_ms);
        push_capped(&mut self.lat_pipe, f.ready_at.elapsed().as_secs_f64() * 1000.0);
        self.shown += 1;
        self.last_index = f.index;
        self.fps_count += 1;
        let el = self.fps_window.elapsed().as_secs_f32();
        if el >= 1.0 {
            self.fps = self.fps_count as f32 / el;
            self.fps_window = Instant::now();
            self.fps_count = 0;
        }
    }
}

/// spike 的应用状态。
struct SpikeApp {
    /// egui 的 wgpu 渲染状态（含 `device` / `queue` / `renderer`）。
    rs: egui_wgpu::RenderState,
    /// egui 上下文 —— 解码线程要拿它叫醒 UI。
    egui_ctx: egui::Context,
    /// 共享的 NV12 → RGB 管线。
    pipe: Nv12Pipeline,
    /// 共享硬解设备（**所有路挂在同一个设备上**，与产品一致）。
    device: Option<Arc<hw::HwDevice>>,
    /// 缓冲深度（延迟旋钮）。
    buf: Buffer,
    /// 设备级错误收集器（纹理/显存失败靠它分类）。
    errs: DeviceErrors,
    /// 已准入的流。
    streams: Vec<Stream>,
    /// 故障注入开关。
    fault: Fault,
    /// 待新增的规格（下一个 `+` 按钮用）。
    next_spec: Spec,
    /// 已尝试的准入次数（故障注入按下标判断第几路）。
    attempts: usize,
    /// 容量探测状态：`Some` 表示正在探测。
    probe: Option<ProbeState>,
    /// 结果记录（也是**证据**，同时打到 stdout）。
    log: Vec<String>,
    /// 上次周期性汇总的时刻。
    last_report: Instant,
}

/// 周期性汇总的间隔。
const REPORT_EVERY: Duration = Duration::from_secs(10);

/// 容量探测的进行状态。
struct ProbeState {
    /// 目标路数。
    target: usize,
    /// 下一路的准入发起时刻（错开一点，避免同一瞬间挤在一起）。
    next_at: Instant,
}

impl SpikeApp {
    /// 建应用：起管线、建共享硬解设备。
    fn new(cc: &eframe::CreationContext<'_>, sample: PathBuf, dev_name: String, fmt_name: String) -> Self {
        let rs = cc
            .wgpu_render_state
            .clone()
            .expect("spike 必须跑在 wgpu 后端上");
        let pipe = Nv12Pipeline::new(&rs.device);
        // wgpu 的 `create_texture` 不返回 `Result` ⇒ 装个全局回调把设备错误收下来，
        // 准入时才能把「纹理创建失败」单独归一类（方案 §3.3B 五类之一）。
        let errs = DeviceErrors::default();
        {
            let sink = errs.clone();
            rs.device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
                sink.0.lock().push(e.to_string());
            }));
        }
        let fault = Fault::from_env();
        if fault.fail_open.is_some() || fault.fail_timeout.is_some() {
            println!(
                "[故障注入] fail_open={:?} fail_timeout={:?}",
                fault.fail_open, fault.fail_timeout
            );
        }
        let device = match hw::HwDevice::create(&dev_name) {
            Ok(d) => Some(Arc::new(d)),
            Err(e) => {
                eprintln!("**建硬解设备失败**: {e}");
                None
            }
        };
        let buf = Buffer::from_env();
        println!("缓冲深度 {} 帧（queue {} + channel {}）", buf.frames(), buf.queue, buf.channel);
        let label = PathBuf::from(&sample)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "default".into());
        // 输入格式：空串 = 自动探测；否则强制该裸解复用器
        let fmt = if fmt_name.is_empty() {
            hw::InputFormat::Auto
        } else {
            println!("输入格式强制为 {fmt_name}（裸 Annex-B 路径）");
            hw::InputFormat::Forced(fmt_name)
        };
        let mut app = Self {
            rs,
            egui_ctx: cc.egui_ctx.clone(),
            pipe,
            device,
            buf,
            errs,
            streams: Vec::new(),
            fault,
            next_spec: Spec { path: sample, fmt, label },
            attempts: 0,
            probe: None,
            log: Vec::new(),
            last_report: Instant::now(),
        };
        // 首路走完整准入（含首帧硬校验 + 建纹理 + 上传），失败也照常开窗 —— 失败原因会显示在日志里
        app.add_one(false);
        // `SPIKE_PROBE=<n>`：起手就自动探测到 n 路（让容量曲线可命令行复现，不必手点按钮）
        if let Ok(n) = std::env::var("SPIKE_PROBE").unwrap_or_default().parse::<usize>() {
            app.note(format!("PROBE start: target {n}"));
            app.probe = Some(ProbeState { target: n, next_at: Instant::now() });
        }
        app
    }

    /// 记一条结果（同时打到 stdout —— 那是**权威证据**）。
    ///
    /// **必须显式 flush**：stdout 被重定向到文件时是**块缓冲**，进程被强杀会丢缓冲，
    /// 证据就没了。
    fn note(&mut self, s: String) {
        use std::io::Write;
        println!("{s}");
        let _ = std::io::stdout().flush();
        self.log.push(s);
    }

    /// **准入一路**（方案 §3.3B）：起线程 → 等首帧过硬校验 → 建纹理 → 上传首帧。
    ///
    /// 任一步失败 ⇒ **完整回滚**（丢掉该路；线程因通道断开自行退出）并返回带阶段的原因。
    fn admit(&mut self, spec: Spec, loop_file: bool) -> Result<Stream, AdmitError> {
        let Some(dev) = self.device.clone() else {
            return Err(AdmitError::DecoderOpen("硬解设备不可用".into()));
        };
        let idx = self.attempts;
        self.attempts += 1;

        // 故障注入：强制「建解码器失败」= 直接在这一步失败，线程都不用起
        if self.fault.fail_open == Some(idx) {
            return Err(AdmitError::DecoderOpen("故障注入：强制建解码器失败".into()));
        }
        let fault = if self.fault.fail_timeout == Some(idx) {
            Fault { fail_open: None, fail_timeout: Some(idx) }
        } else {
            Fault::default()
        };

        let (tx, rx) = sync_channel::<Msg>(self.buf.channel);
        {
            let spec2 = spec.clone();
            let ctx = self.egui_ctx.clone();
            std::thread::Builder::new()
                .name(format!("decode-{idx}"))
                .spawn(move || decode_loop(spec2, dev, tx, ctx, fault, loop_file))
                .map_err(|e| AdmitError::DecoderOpen(format!("起解码线程失败: {e}")))?;
        }

        // ★ 准入关键：**等首帧过硬校验**（`Msg::Ready` 只在首帧通过硬校验后才发）
        let first = match rx.recv_timeout(ADMIT_TIMEOUT) {
            Ok(Msg::Ready(r)) => r,
            Ok(Msg::Frame(_)) => {
                return Err(AdmitError::DecoderOpen("协议错：先收到帧而非 Ready".into()));
            }
            Ok(Msg::Done(None)) => {
                return Err(AdmitError::DecoderOpen("线程未就绪即结束".into()));
            }
            Ok(Msg::Done(Some(e))) => return Err(classify(e)),
            Err(RecvTimeoutError::Timeout) => return Err(AdmitError::FirstFrameTimeout),
            Err(RecvTimeoutError::Disconnected) => {
                return Err(AdmitError::DecoderOpen("解码线程提前退出".into()));
            }
        };

        // 纹理这一步也在准入内 —— 真正耗尽的往往是**显存**而不是解码会话数
        self.errs.take(); // 清掉此前累积的，只看本路这一段
        let target = StreamTarget::new(&self.rs.device, &self.pipe, first.width, first.height);
        let dev_errs = self.errs.take();
        if !dev_errs.is_empty() {
            return Err(AdmitError::Texture(dev_errs.join("; ")));
        }

        let mut s = Stream {
            spec,
            buf: self.buf,
            rx,
            queue: VecDeque::new(),
            clock: None,
            target: Some(target),
            tex_id: None,
            ready: Some(first),
            transfer: Vec::new(),
            upload: Vec::new(),
            lat_pipe: Vec::new(),
            lat_e2e: Vec::new(),
            shown: 0,
            dropped: 0,
            last_index: 0,
            fps_window: Instant::now(),
            fps_count: 0,
            fps: 0.0,
            status: "admitting".into(),
        };

        // 首帧已在通道里 ⇒ 立刻跑一遍「上传 + 转 RGB」，把整条链路走完才算准入成功
        s.fill_queue();
        if let Some(f) = s.queue.pop_front() {
            if let Some(t) = &s.target {
                let t0 = Instant::now();
                t.upload(&self.rs.queue, &f);
                t.render(&self.rs.device, &self.rs.queue, &self.pipe);
                push_capped(&mut s.upload, t0.elapsed().as_secs_f64() * 1000.0);
            }
            push_capped(&mut s.transfer, f.transfer_ms);
            s.shown = 1;
            s.last_index = f.index;
        }
        s.status = "running".into();
        Ok(s)
    }

    /// 加一路（准入 + 失败回滚 + 记录）。返回是否成功。
    fn add_one(&mut self, loop_file: bool) -> bool {
        let spec = self.next_spec.clone();
        let idx = self.attempts;
        match self.admit(spec, loop_file) {
            Ok(s) => {
                let r = s.ready.as_ref().expect("准入成功必有自证");
                self.note(format!(
                    "ADMITTED #{} {} {}x{} {} hw_frames_ctx=set readback={:.2}ms",
                    idx,
                    r.device,
                    r.width,
                    r.height,
                    r.codec,
                    r.first.transfer_ms
                ));
                self.streams.push(s);
                true
            }
            Err(e) => {
                // **完整回滚**：该路没进 `streams`，线程随通道断开自行退出
                self.note(format!("REJECTED #{} ({e})", idx));
                false
            }
        }
    }
}

/// 把带阶段的解码失败映射成准入失败分类。
fn classify(e: hw::DecodeError) -> AdmitError {
    match e.stage {
        hw::Stage::Open | hw::Stage::FirstFrame => AdmitError::DecoderOpen(e.detail),
        hw::Stage::NotHardware => AdmitError::NotHardware(e.detail),
        hw::Stage::Transfer | hw::Stage::Format => AdmitError::Transfer(e.detail),
    }
}

impl eframe::App for SpikeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // ① 推进容量探测（每 0.5s 加一路，直到被拒或到上限）
        let step_probe = self.probe.as_ref().is_some_and(|p| Instant::now() >= p.next_at);
        if step_probe {
            let target = self.probe.as_ref().map(|p| p.target).unwrap_or(0);
            let alive = self.streams.len();
            if alive >= target {
                self.note(format!("PROBE done: {alive} streams sustained (target {target})"));
                self.probe = None;
            } else if !self.add_one(true) {
                self.note(format!("PROBE stopped: rejected at {alive} streams"));
                self.probe = None;
            } else if let Some(p) = &mut self.probe {
                p.next_at = Instant::now() + Duration::from_millis(500);
            }
        }

        // ② 推进每一路（**全部**跑完整链路）
        let ctx = ui.ctx().clone();
        for i in 0..self.streams.len() {
            self.streams[i].tick(&self.rs.device, &self.rs.queue, &self.pipe, &ctx);
            // 前 DISPLAY_MAX 路把离屏纹理注册给 egui（重建时须释放，否则泄漏）
            if i < DISPLAY_MAX && self.streams[i].tex_id.is_none() {
                if let Some(t) = &self.streams[i].target {
                    let id = self.rs.renderer.write().register_native_texture(
                        &self.rs.device,
                        &t.view,
                        wgpu::FilterMode::Linear,
                    );
                    self.streams[i].tex_id = Some(id);
                }
            }
        }

        // ②b 周期性汇总到 stdout —— **证据要能留档**，不能只活在窗口里
        if self.last_report.elapsed() >= REPORT_EVERY {
            self.last_report = Instant::now();
            let lines: Vec<String> = self
                .streams
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let (t50, t95) = p50_p95(&s.transfer);
                    let (p50, p95) = p50_p95(&s.lat_pipe);
                    let (e50, e95) = p50_p95(&s.lat_e2e);
                    format!(
                        "STAT #{i} fps={:.1} readback={t50:.1}/{t95:.1}ms lat={p50:.1}/{p95:.1}ms e2e={e50:.1}/{e95:.1}ms drop={} shown={}",
                        s.fps, s.dropped, s.shown
                    )
                })
                .collect();
            for l in lines {
                self.note(l);
            }
        }

        // ③ 顶部：控制 + 硬解自证
        egui::Panel::top("ctrl").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("+1").clicked() {
                    self.add_one(true);
                }
                if ui.button("+4").clicked() {
                    for _ in 0..4 {
                        self.add_one(true);
                    }
                }
                if ui.button("-1").clicked() {
                    if let Some(s) = self.streams.pop() {
                        if let Some(id) = s.tex_id {
                            self.rs.renderer.write().free_texture(&id);
                        }
                    }
                }
                if ui.button("probe").clicked() {
                    self.note(format!("PROBE start: target {PROBE_MAX}"));
                    self.probe = Some(ProbeState {
                        target: PROBE_MAX,
                        next_at: Instant::now(),
                    });
                }
                if ui.button("stop").clicked() {
                    self.probe = None;
                }
                ui.separator();
                let n = self.streams.len();
                let ok = self
                    .streams
                    .iter()
                    .filter(|s| s.ready.as_ref().is_some_and(|r| r.first.is_hardware()))
                    .count();
                ui.monospace(format!("streams {n}  hw-verified {ok}"));
                if let Some(p) = &self.probe {
                    ui.separator();
                    ui.monospace(format!("probing -> {}", p.target));
                }
            });
        });

        // ④ 每路的规格与耗时
        egui::Panel::top("table").show(ui, |ui| {
            for (i, s) in self.streams.iter().enumerate() {
                let (t50, t95) = p50_p95(&s.transfer);
                let (u50, _) = p50_p95(&s.upload);
                let (p50, p95) = p50_p95(&s.lat_pipe);
                let (e50, e95) = p50_p95(&s.lat_e2e);
                let (dev, codec, w, h) = match &s.ready {
                    Some(r) => (r.device.as_str(), r.codec.as_str(), r.width, r.height),
                    None => ("?", "?", 0, 0),
                };
                ui.monospace(format!(
                    "#{i:<2} {} {dev} {codec} {w}x{h}  fps {:5.1}  shown {}  drop {}  readback {t50:.1}/{t95:.1}  up {u50:.1}  lat {p50:.1}/{p95:.1}  e2e {e50:.1}/{e95:.1} ms  {}",
                    s.spec.label, s.fps, s.shown, s.dropped, s.status
                ));
            }
        });

        // ⑤ 底部：日志尾巴
        egui::Panel::bottom("log").show(ui, |ui| {
            for l in self.log.iter().rev().take(6).collect::<Vec<_>>().into_iter().rev() {
                ui.monospace(l);
            }
        });

        // ⑤b 保证周期性汇总一定会触发 —— 无帧可播时（到尾 / 等待中）UI 本来会睡下去，
        //     那样 STAT 永远不会打，**证据也就没了**。
        ui.ctx().request_repaint_after(REPORT_EVERY);

        // ⑥ 画面（只画前 DISPLAY_MAX 路；其余路照常跑完整链路，只是不上窗口）
        egui::CentralPanel::no_frame().show(ui, |ui| {
            let shown: Vec<(egui::TextureId, [f32; 2])> = self
                .streams
                .iter()
                .take(DISPLAY_MAX)
                .filter_map(|s| Some((s.tex_id?, s.target.as_ref()?.size())))
                .collect();
            if shown.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label("no stream yet - press +1");
                });
                return;
            }
            // 格子尺寸**必须在进入横向布局前**算好 —— 布局内部的 `available_height()`
            // 会退化成 0，`fit` 随之把画面压成一条线。
            let avail = ui.available_size();
            let cols = shown.len().min(2).max(1) as f32;
            let rows = (shown.len() as f32 / cols).ceil().max(1.0);
            let cell = egui::vec2(avail.x / cols - 6.0, avail.y / rows - 6.0);
            ui.horizontal_wrapped(|ui| {
                for (id, size) in shown {
                    ui.image((id, fit(size, cell)));
                }
            });
        });
    }
}

/// 入口。
fn main() -> eframe::Result {
    env_logger::init();
    let mut args = std::env::args().skip(1);
    let sample = args.next().unwrap_or_else(|| DEFAULT_SAMPLE.to_string());
    let dev_name = args.next().unwrap_or_else(|| DEFAULT_DEVICE.to_string());
    // 第三个参数：强制裸解复用器（`h264` / `hevc`）。省略 = 自动探测（本地 mp4）。
    let fmt_name = args.next().unwrap_or_default();

    // 验收 5：硬解设备探测（启动时一次；结论是"候选"，真实能力靠准入时试错）
    println!("=== 硬解设备（枚举 + 实试创建）===");
    for d in hw::probe_devices() {
        match (d.creatable, &d.error) {
            (true, _) => println!("  ✓ {:<12} 可创建", d.name),
            (false, Some(e)) => println!("  ✗ {:<12} {e}", d.name),
            _ => {}
        }
    }
    println!("=== 播放 {} （设备 {dev_name}）===", sample);

    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native(
        "WCReen PC spike",
        options,
        Box::new(move |cc| Ok(Box::new(SpikeApp::new(cc, PathBuf::from(sample), dev_name, fmt_name)))),
    )
}
