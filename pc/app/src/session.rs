//! 会话：连接 → 握手 → 能力表 → 申请屏幕 → 收帧，以及判活 / 重连 / 收尾。
//!
//! # 时序（`docs/spec/agent/transport-handshake.md` §2）
//!
//! ```text
//! PC                                          agent
//!  │  connect                                  │
//!  │ ── 首包：协议版本 ──────────────────────►  │ 校验版本（不匹配 ⇒ 回报双方版本 + 断开）
//!  │ ◄── 握手应答：协议版本 + 自身状态 ───────  │
//!  │ ── 请求能力表 ────────────────────────►   │
//!  │ ◄── 能力表（编码器 / 范围步进 / 缺省）     │
//!  │  ← PC 求交集：自己的硬解 ∩ 能力表           │
//!  │ ── 申请屏幕（streamId? + 全部参数）─────►   │
//!  │ ◄── 回报：streamId + 每个参数的实际生效值 ── │
//!  │ ◄── 帧（首个是 IDR，自带 SPS/PPS）───────── │
//! ```
//!
//! # 线程结构
//!
//! **一个会话线程**包办「读 socket → 解码 → 放进最新帧槽」，UI 线程只做上传与绘制：
//!
//! - 读写同线程 ⇒ 满足协议「一条连接 = 一个写者」；心跳与收帧共用同一个循环，
//!   不需要额外的定时器线程；
//! - 解码就在读循环里做 ⇒ 天然反压（读一帧解一帧），**没有中间队列**，
//!   延迟 = 流水线成本本身（直播语义：到达即上屏，不节流）；
//! - 到 UI 只传**最新一帧**（一帧槽），装不下就丢旧的 —— 这是「缓冲尽可能小」的落点，
//!   打穿后按丢帧重对齐，**不是降级**。
//!
//! # 判活与重连
//!
//! 判据 = 「[`T_DEAD`] 内没收到 agent **任何**消息」（不是「没收到心跳」）。
//! 判死后**重连**：重连 = 首连 + 带个 `streamId`（协议就是这么设计的），
//! 带回去才能解冻 agent 侧那块冻结的屏。
//!
//! **硬解失败不重连**：`NotHardware` / 回读格式不符 / 建解码器失败都是**参数级**结论
//! （同样的参数重试一万次也一样），所以直接**明确失败并停住**，把原因留在界面上。

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use decode::hw::pix_fmt_name;
use decode::{
    DecodeError, HardwareCapability, HwDevice, Nv12Frame, Stage, VideoDecoder, VideoParams,
};
use proto::timing::{HEARTBEAT_PERIOD, T_DEAD};
use proto::wire::kind;
use proto::{
    decode_control, parse_video_payload, AppliedScreen, Caps, CapsRequest, Command, CommandAck,
    ErrorObj, Event, Heartbeat, Hello, HelloAck, ProtocolError, PROTOCOL_VERSION,
};
use serde::de::DeserializeOwned;

use crate::conn::Conn;
use crate::forward;
use crate::negotiate::{self, Request};

/// 连接超时。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// 重连前的等待（避免对端还没起来时打转）。
const RECONNECT_DELAY: Duration = Duration::from_millis(1000);

/// 自动重建 `adb forward` 的最小间隔。
///
/// 重连循环每秒转一次，硬性 ≥5s 限频才能保证「不得在重连循环里每秒调一次 adb」——
/// 每次 `repair` 都要创建一个进程，无节制地跑既是开销也是噪声。
const REPAIR_MIN_INTERVAL: Duration = Duration::from_secs(5);

/// 会话日志保留的条数（界面只显示尾部若干条）。
const LOG_CAP: usize = 200;

/// 待发输入队列的容量。
///
/// 正常永远碰不到上界：UI 已按**屏刷新率**采样（`tech-stack/pc.md` §4），
/// 而会话线程每 ≤[`POLL_INTERVAL`] 就排空一次。
const INPUT_QUEUE_CAP: usize = 64;

/// 「已发出、未收到拒绝」的输入关联号的环大小。
///
/// 输入**成功不回 ack** ⇒ 这个环只能靠条数淘汰，不能等回报。
const INPUT_ID_RING: usize = 64;

/// 周期性统计行的间隔。
///
/// 存在的理由与 spike 的 `STAT` 行相同：**证据要能留档**，不能只活在窗口里 ——
/// 出问题时把日志贴出来就能看出「到底有没有帧、丢了多少」。
const STAT_PERIOD: Duration = Duration::from_secs(10);

/// 会话阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// 正在收帧（会话健康）。
    Streaming,
    /// 连接掉了，正在重连。
    Reconnecting,
    /// **明确失败**（硬解不可用 / 版本不匹配之类），不再重试。
    Failed,
    /// 正常收尾（已发 `stopAgent`）。
    Stopped,
}

/// UI 可见的会话状态快照。
#[derive(Debug, Clone)]
pub struct Status {
    /// 当前阶段。
    pub phase: Phase,
    /// 当前认领 / 新建的屏 id（`0` = 还没有）。
    pub stream_id: i32,
    /// agent 回报的实际生效参数。
    pub applied: Option<AppliedScreen>,
    /// 已解出的帧数。
    pub frames: u64,
    /// 因来不及上屏而被替换掉的帧数。
    pub dropped: u64,
    /// 已发出的输入指令条数。
    pub input_sent: u64,
    /// 被 agent 拒掉的输入条数（成功**不回** ack，所以这个数只统计失败）。
    pub input_rejected: u64,
    /// 输入是否已被判定不可用（`inputDenied`，子系统级）—— 置位后不再生产新事件。
    pub input_blocked: bool,
    /// 最近一条诊断信息。
    pub message: String,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            phase: Phase::Reconnecting,
            stream_id: 0,
            applied: None,
            frames: 0,
            dropped: 0,
            input_sent: 0,
            input_rejected: 0,
            input_blocked: false,
            message: "尚未建立会话".into(),
        }
    }
}

/// 与 UI 共享的句柄（`Session` 与 UI 都持有一份克隆）。
#[derive(Clone, Default)]
pub struct Handles {
    /// 最新一帧（只有一帧 ⇒ 缓冲最小）。
    slot: Arc<Mutex<Option<Nv12Frame>>>,
    /// 状态快照。
    status: Arc<Mutex<Status>>,
    /// 日志尾部。
    log: Arc<Mutex<VecDeque<String>>>,
    /// 待发的输入指令 —— UI **生产**、会话线程**消费**（协议规定「一条连接 = 一个写者」，
    /// 所以 UI 不能自己写 socket）。
    input: Arc<Mutex<VecDeque<Command>>>,
}

impl Handles {
    /// 取走最新一帧（取走即清空；没有新帧时返回 `None`）。
    pub fn take_frame(&self) -> Option<Nv12Frame> {
        self.slot.lock().unwrap_or_else(|e| e.into_inner()).take()
    }

    /// 取状态快照。
    pub fn status(&self) -> Status {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// 取日志尾部（最旧的在前）。
    pub fn log_lines(&self) -> Vec<String> {
        self.log
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .cloned()
            .collect()
    }

    /// 投一批输入指令（UI 线程调用）。
    ///
    /// 队列满了丢**最旧**的一条：正常永远碰不到（UI 已按上限频率采样，
    /// 而会话线程每 ≤[`POLL_INTERVAL`] 就排空一次）。
    ///
    /// @param cmds 待投的指令
    pub fn push_input(&self, cmds: Vec<Command>) {
        if cmds.is_empty() {
            return;
        }
        let mut q = self.input.lock().unwrap_or_else(|e| e.into_inner());
        for cmd in cmds {
            if q.len() >= INPUT_QUEUE_CAP {
                q.pop_front();
            }
            q.push_back(cmd);
        }
    }

    /// 取走全部待发输入（会话线程调用）。
    ///
    /// @return 指令列表（可能为空）
    fn take_inputs(&self) -> Vec<Command> {
        let mut q = self.input.lock().unwrap_or_else(|e| e.into_inner());
        q.drain(..).collect()
    }

    /// 放下一帧（丢掉旧的），并返回是否丢掉了一帧。
    fn put_frame(&self, frame: Nv12Frame) -> bool {
        let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        let dropped = slot.is_some();
        *slot = Some(frame);
        dropped
    }

    /// 改状态。
    fn update(&self, f: impl FnOnce(&mut Status)) {
        f(&mut self.status.lock().unwrap_or_else(|e| e.into_inner()));
    }

    /// 追加一条日志（同时打给 `log` crate）。
    fn note(&self, msg: impl Into<String>) {
        let msg = msg.into();
        log::info!("{msg}");
        let mut log = self.log.lock().unwrap_or_else(|e| e.into_inner());
        if log.len() >= LOG_CAP {
            log.pop_front();
        }
        log.push_back(msg);
    }
}

/// UI 唤醒器。
///
/// 会话线程每放下一帧就调用它 —— 否则 UI 睡下去就再没人叫醒（spike 踩过的坑）。
/// 它在会话建立之后才被装上（UI 还没创建时无从得到 `egui::Context`），
/// 所以是「可后设的回调」而不是构造参数。
#[derive(Clone, Default)]
pub struct Waker {
    /// 回调本体（`None` = UI 还没准备好）。
    inner: Arc<Mutex<Option<Arc<dyn Fn() + Send + Sync>>>>,
}

impl Waker {
    /// 装上回调（重复装会替换）。
    pub fn set(&self, f: impl Fn() + Send + Sync + 'static) {
        *self.inner.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(f));
    }

    /// 叫醒 UI（没装回调时是空操作）。
    fn wake(&self) {
        let cb = self.inner.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(cb) = cb {
            cb();
        }
    }
}

/// 建会话所需的全部输入。
pub struct SessionConfig {
    /// 目标地址（`adb forward` 之后就是 `127.0.0.1:<port>`）。
    pub addr: SocketAddr,
    /// 共享的硬解设备。
    pub device: Arc<HwDevice>,
    /// 本机硬解能力（交集的一半）。
    pub pc: HardwareCapability,
    /// PC 侧对建屏的显式要求。
    pub request: Request,
    /// 设备侧抽象 socket 名（与 agent 的 `--name` 同名）。
    ///
    /// 连接被拒时用它重建 `adb forward tcp:<port> localabstract:<name>`。
    pub socket_name: String,
}

/// 会话失败。
#[derive(Debug)]
pub enum SessionError {
    /// 连不上（最常见的原因就是没做 `adb forward`）。
    Connect {
        /// 目标地址。
        addr: SocketAddr,
        /// 是否**连接被拒** —— 本地端口没有监听，转发多半已随 USB 掉线丢失。
        /// 这是唯一会触发自动重建转发的信号；其它 IO 错误（超时 / 未达网络等）不触发。
        refused: bool,
        /// 底层原因。
        detail: String,
    },
    /// 握手失败（版本不匹配 / 对端状态不可用 / 首包不是握手）。
    Handshake {
        /// 可诊断说明。
        detail: String,
    },
    /// 能力表请求失败。
    Caps {
        /// 可诊断说明。
        detail: String,
    },
    /// 交集为空或参数不可满足。
    Negotiate {
        /// 可诊断说明。
        detail: String,
    },
    /// 「申请屏幕」被拒（可能带**部分生效**的结果）。
    ApplyScreen {
        /// 可诊断说明。
        detail: String,
        /// 已生效的那部分参数（部分成功时非空）。
        applied: Option<AppliedScreen>,
    },
    /// 解码链路失败（带阶段）。
    Decode(DecodeError),
    /// 协议级失败。
    Protocol(ProtocolError),
}

impl SessionError {
    /// 是否属于**参数级**结论 —— 同样的参数重试没有意义，必须明确失败并停住。
    pub fn is_terminal(&self) -> bool {
        match self {
            Self::Decode(e) => matches!(e.stage, Stage::Open | Stage::NotHardware | Stage::Format),
            Self::Handshake { .. } | Self::Negotiate { .. } => true,
            Self::Connect { .. } | Self::Caps { .. } | Self::ApplyScreen { .. } => false,
            Self::Protocol(_) => false,
        }
    }
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connect { addr, refused, detail } => {
                write!(f, "无法连接 {addr}：{detail}")?;
                if *refused {
                    write!(
                        f,
                        "\n提示：本地端口 tcp:{} 没有进程在监听 —— 多半是 adb 转发随 USB \
                         掉线被清掉了。可用 `adb forward tcp:{} localabstract:<名字>` 重建\
                         （名字见 `--name`，默认 `wcreen`）",
                        addr.port(),
                        addr.port()
                    )?;
                }
                Ok(())
            }
            Self::Handshake { detail } => write!(f, "握手失败：{detail}"),
            Self::Caps { detail } => write!(f, "请求能力表失败：{detail}"),
            Self::Negotiate { detail } => write!(f, "参数协商失败：{detail}"),
            Self::ApplyScreen { detail, applied } => {
                write!(f, "「申请屏幕」被拒：{detail}")?;
                // 允许部分成功 —— 已生效的那部分必须让用户看见，不能丢信息。
                match applied {
                    Some(a) => write!(
                        f,
                        "\n（已生效的部分：streamId={} {}x{} @{}fps 码率={} 编码器={}）",
                        a.stream_id,
                        a.width.unwrap_or(0),
                        a.height.unwrap_or(0),
                        a.refresh_rate.unwrap_or(0.0),
                        a.bitrate
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "未回报".into()),
                        a.codec_name.clone().unwrap_or_else(|| "未回报".into())
                    ),
                    None => Ok(()),
                }
            }
            Self::Decode(e) => write!(f, "解码失败：{e}"),
            Self::Protocol(e) => write!(f, "协议错误：{e}"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<ProtocolError> for SessionError {
    fn from(e: ProtocolError) -> Self {
        Self::Protocol(e)
    }
}

impl From<DecodeError> for SessionError {
    fn from(e: DecodeError) -> Self {
        Self::Decode(e)
    }
}

/// 一轮已经建立好的会话（连接 + 解码器）。
struct Live {
    /// 连接。
    conn: Conn,
    /// 解码器。
    decoder: VideoDecoder,
    /// 本轮认领 / 新建的屏 id。
    stream_id: i32,
    /// agent 回报的实际生效参数。
    applied: AppliedScreen,
    /// 已发出、还没被拒的输入关联号。
    ///
    /// 输入**成功不回 ack** ⇒ 这个环只为识别**失败**回报，从而把「输入被拒」与
    /// 「收到一条不属于任何在途请求的回报」区分开（两者的处置完全不同）。
    input_ids: VecDeque<i32>,
}

impl Live {
    /// 记下一个已发出的输入关联号（有界环）。
    ///
    /// @param id 关联号
    fn note_input_id(&mut self, id: i32) {
        if self.input_ids.len() >= INPUT_ID_RING {
            self.input_ids.pop_front();
        }
        self.input_ids.push_back(id);
    }

    /// 这个关联号是不是一个已被拒的输入。
    ///
    /// @param id 回报里的关联号
    /// @return true = 是（并从环里移除）
    fn take_input_id(&mut self, id: i32) -> bool {
        match self.input_ids.iter().position(|&v| v == id) {
            Some(idx) => {
                self.input_ids.remove(idx);
                true
            }
            None => false,
        }
    }
}

/// 一个会话。
pub struct Session {
    /// 与 UI 共享的句柄。
    handles: Handles,
    /// UI 唤醒器。
    wake: Waker,
    /// 停止标志。
    stop: Arc<AtomicBool>,
    /// 会话线程。
    join: Option<JoinHandle<()>>,
}

impl Session {
    /// 建会话：**同步完成首轮建链**（连接 + 握手 + 能力表 + 申请屏幕 + 建解码器）。
    ///
    /// 之所以同步：连不上 / 版本不匹配 / 硬解不可用都必须在**开窗口之前**就报清楚，
    /// 否则用户看到的是一个空窗口，反而更难诊断。稳态之后的掉线才走线程内的重连。
    ///
    /// @param cfg 会话配置
    /// @return 成功给出已进入收帧状态的会话；任何一步失败给出**可诊断**的错误
    pub fn start(cfg: SessionConfig) -> Result<Self, SessionError> {
        let handles = Handles::default();
        let live = match bring_up(&cfg, 0, &handles) {
            Ok(live) => live,
            // 只有「连接被拒」（本地端口没人在听 —— 转发多半已随 USB 掉线丢失）才碰 adb：
            // 先自动重建一次转发，再重试一次；转发本来就正常的分支永远不会走到这里。
            Err(e @ SessionError::Connect { refused: true, .. }) => {
                handles.note(format!("连接被拒（{}）⇒ 尝试重建 adb 转发…", cfg.addr));
                match forward::repair(cfg.addr.port(), &cfg.socket_name) {
                    Ok(()) => {
                        handles.note(format!(
                            "转发缺失 ⇒ 已自动重建 tcp:{} → localabstract:{}",
                            cfg.addr.port(),
                            cfg.socket_name
                        ));
                        match bring_up(&cfg, 0, &handles) {
                            Ok(live) => live,
                            Err(retry) => {
                                return Err(with_connect_diagnosis(
                                    retry,
                                    cfg.addr.port(),
                                    &cfg.socket_name,
                                    None,
                                ))
                            }
                        }
                    }
                    Err(err) => {
                        handles.note(format!("自动重建转发失败：{err}"));
                        return Err(with_connect_diagnosis(
                            e,
                            cfg.addr.port(),
                            &cfg.socket_name,
                            Some(&err),
                        ));
                    }
                }
            }
            Err(e) => return Err(e),
        };
        handles.update(|s| {
            s.phase = Phase::Streaming;
            s.stream_id = live.stream_id;
            s.applied = Some(live.applied.clone());
            s.message = format!(
                "已建屏 streamId={} {}x{} @{}fps",
                live.stream_id,
                live.applied.width.unwrap_or(0),
                live.applied.height.unwrap_or(0),
                live.applied.refresh_rate.unwrap_or(0.0)
            );
        });
        handles.note(format!(
            "建屏成功：streamId={} 编码器={} {}x{} @{:.0}fps",
            live.stream_id,
            live.applied.codec_name.clone().unwrap_or_else(|| "?".into()),
            live.applied.width.unwrap_or(0),
            live.applied.height.unwrap_or(0),
            live.applied.refresh_rate.unwrap_or(0.0)
        ));

        let stop = Arc::new(AtomicBool::new(false));
        let wake = Waker::default();
        let thread_handles = handles.clone();
        let thread_stop = stop.clone();
        let thread_wake = wake.clone();
        let join = std::thread::Builder::new()
            .name("session".into())
            .spawn(move || run(cfg, thread_handles, thread_wake, thread_stop, live))
            .map_err(|e| SessionError::Protocol(ProtocolError::Malformed {
                detail: format!("起会话线程失败：{e}"),
            }))?;

        Ok(Self { handles, wake, stop, join: Some(join) })
    }

    /// 与 UI 共享的句柄。
    pub fn handles(&self) -> &Handles {
        &self.handles
    }

    /// UI 唤醒器（装上 `egui::Context::request_repaint`）。
    pub fn waker(&self) -> &Waker {
        &self.wake
    }

    /// 收尾：让会话线程发 `stopAgent` 并退出，然后等它结束。
    ///
    /// 幂等 —— `on_exit` 可能被调用多次。
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 会话线程主体：稳态收帧 → 掉线重连 → 直到停止或**明确失败**。
fn run(cfg: SessionConfig, handles: Handles, wake: Waker, stop: Arc<AtomicBool>, first: Live) {
    let mut live = first;
    let mut want_stream_id = live.stream_id;
    loop {
        match stream_loop(&handles, &wake, &stop, &mut live) {
            Exit::Stopped => {
                handles.update(|s| {
                    s.phase = Phase::Stopped;
                    s.message = "已发送 stopAgent，会话结束".into();
                });
                return;
            }
            Exit::Fatal(reason) => {
                handles.note(format!("**明确失败**：{reason}"));
                handles.update(|s| {
                    s.phase = Phase::Failed;
                    s.message = reason;
                });
                return;
            }
            Exit::Reconnect { reason, reclaim } => {
                handles.note(reason);
                if !reclaim {
                    want_stream_id = 0;
                }
                handles.update(|s| {
                    s.phase = Phase::Reconnecting;
                    s.message = "连接已断，正在重连…".into();
                });
                match reconnect(&cfg, &handles, &stop, want_stream_id) {
                    Some(next) => {
                        want_stream_id = next.stream_id;
                        handles.update(|s| {
                            s.phase = Phase::Streaming;
                            s.stream_id = next.stream_id;
                            s.applied = Some(next.applied.clone());
                            s.message = format!("已重连（streamId={}）", next.stream_id);
                        });
                        handles.note(format!("重连成功：streamId={}", next.stream_id));
                        live = next;
                    }
                    None => {
                        handles.update(|s| {
                            s.phase = Phase::Failed;
                            s.message = "重连被中止".into();
                        });
                        return;
                    }
                }
            }
        }
    }
}

/// 稳态循环的退出原因。
enum Exit {
    /// 收到停止请求（已发 `stopAgent`）。
    Stopped,
    /// 连接不可用，应当重连；`reclaim` 决定重连时是否带回 `streamId`。
    Reconnect {
        /// 可诊断的原因。
        reason: String,
        /// 是否认领原来那块屏（`screenReleased` 之后就不能再认领了）。
        reclaim: bool,
    },
    /// **明确失败**（参数级结论），不再重试。
    Fatal(String),
}

/// 稳态收帧循环。
fn stream_loop(handles: &Handles, wake: &Waker, stop: &AtomicBool, live: &mut Live) -> Exit {
    let mut last_recv = Instant::now();
    let mut last_send = Instant::now();
    let mut last_stat = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        // 周期性统计：只进日志（界面已有实时计数），让「有没有帧」这件事能留档。
        if last_stat.elapsed() >= STAT_PERIOD {
            last_stat = Instant::now();
            let status = handles.status();
            let first = live.decoder.first_frame();
            log::info!(
                "STAT 解出 {} 帧（丢弃 {}）输入 {} 条（被拒 {}）首帧硬校验={} 已解码总帧数={}",
                status.frames,
                status.dropped,
                status.input_sent,
                status.input_rejected,
                match first {
                    Some(f) if f.is_hardware() => format!(
                        "通过（format={} 回读={} {:.1}ms）",
                        pix_fmt_name(f.hw_format),
                        pix_fmt_name(f.sw_format),
                        f.transfer_ms
                    ),
                    Some(f) => format!("**未通过**（format={}）", pix_fmt_name(f.hw_format)),
                    None => "尚无首帧".to_string(),
                },
                live.decoder.decoded()
            );
        }

        // 心跳：每 3s 一条。agent 的判活是「10s 内收到任何消息」，所以它不能被
        // 收帧阻塞 —— 本循环在无帧时每 500ms 醒一次，最坏迟 500ms，远小于 10s。
        if last_send.elapsed() >= HEARTBEAT_PERIOD {
            if let Err(e) = live.conn.send(kind::HEARTBEAT, &Heartbeat {}) {
                return Exit::Reconnect { reason: format!("发心跳失败：{e}"), reclaim: true };
            }
            last_send = Instant::now();
        }

        // 输入：UI 只**生产**，发送一律在本线程（协议规定「一条连接 = 一个写者」）。
        // 循环因读超时每 ≤POLL_INTERVAL 转一圈 ⇒ 输入最坏迟这么久就上线路。
        for cmd in handles.take_inputs() {
            let id = cmd.request_id();
            if let Err(e) = live.conn.send(kind::COMMAND, &cmd) {
                return Exit::Reconnect { reason: format!("发输入失败：{e}"), reclaim: true };
            }
            live.note_input_id(id);
            handles.update(|s| s.input_sent += 1);
        }

        match live.conn.try_read() {
            Ok(Some(frame)) => {
                last_recv = Instant::now();
                match frame.kind {
                    kind::FRAME => match pump(handles, wake, live, &frame.payload) {
                        Ok(()) => {}
                        Err(PumpError::Decode(e)) => {
                            if matches!(e.stage, Stage::Open | Stage::NotHardware | Stage::Format) {
                                return Exit::Fatal(e.to_string());
                            }
                            return Exit::Reconnect {
                                reason: format!("解码出错：{e}"),
                                reclaim: true,
                            };
                        }
                        Err(PumpError::Protocol(e)) => {
                            return Exit::Reconnect {
                                reason: format!("帧负载非法：{e}"),
                                reclaim: true,
                            };
                        }
                    },
                    // 心跳也要过 CRC —— 它是「失步检测」里频率最高的锚点（视频帧不带 CRC）。
                    kind::HEARTBEAT => {
                        if let Err(e) = decode_control::<Heartbeat>(frame.kind, &frame.payload) {
                            return Exit::Reconnect { reason: format!("心跳非法：{e}"), reclaim: true };
                        }
                    }
                    kind::COMMAND_ACK => match decode_control::<CommandAck>(frame.kind, &frame.payload)
                    {
                        // 输入**成功不回 ack** ⇒ 这里来的回报只有两种：输入被拒，或异常回报。
                        Ok(ack) if live.take_input_id(ack.request_id) => {
                            let detail = describe_error(ack.error.as_ref());
                            let subsystem = ack
                                .error
                                .as_ref()
                                .is_some_and(|e| e.class == ErrorObj::CLASS_SUBSYSTEM);
                            handles.note(format!("输入被拒（requestId={}）：{detail}", ack.request_id));
                            handles.update(|s| {
                                s.input_rejected += 1;
                                // 子系统级（例如没开「USB 调试（安全设置）」）重试无意义 ⇒
                                // 停掉生产端，免得每秒白送一堆注定被拒的消息。
                                if subsystem {
                                    s.input_blocked = true;
                                }
                                s.message = format!("输入被拒：{detail}");
                            });
                        }
                        Ok(ack) => handles.note(format!(
                            "收到一条不属于任何在途请求的回报（requestId={}，ok={}）",
                            ack.request_id, ack.ok
                        )),
                        Err(e) => {
                            return Exit::Reconnect { reason: format!("回报非法：{e}"), reclaim: true };
                        }
                    },
                    kind::EVENT => match decode_control::<Event>(frame.kind, &frame.payload) {
                        Ok(event) => {
                            handles.note(format!("agent 上报：{} {:?}", event.event, event.reason));
                            if event.event == Event::SCREEN_RELEASED {
                                // agent 主动拆了屏 ⇒ 那个 streamId 已经没了，不能再认领。
                                // 重连走「首连」形态（streamId=0）重建一块屏 —— 复用同一条
                                // 已验证路径，比在连接内塞一套「重发 + 等回报」的状态机更可靠。
                                return Exit::Reconnect {
                                    reason: format!(
                                        "agent 主动拆屏（reason={:?}）⇒ 重连并重建",
                                        event.reason
                                    ),
                                    reclaim: false,
                                };
                            }
                        }
                        Err(e) => {
                            return Exit::Reconnect { reason: format!("上报非法：{e}"), reclaim: true };
                        }
                    },
                    other => {
                        return Exit::Reconnect {
                            reason: format!(
                                "收到不该出现的类型 0x{other:02x}（{}）⇒ 协议级错误",
                                proto::wire::kind_name(other)
                            ),
                            reclaim: true,
                        };
                    }
                }
            }
            // 这一轮没有完整的一帧：什么都不做，继续等（缓冲里的字节不丢）。
            Ok(None) => {}
            Err(e) => {
                return Exit::Reconnect { reason: format!("读连接失败：{e}"), reclaim: true };
            }
        }

        if last_recv.elapsed() >= T_DEAD {
            return Exit::Reconnect {
                reason: format!("{}s 内没有收到 agent 任何消息 ⇒ 判掉线", T_DEAD.as_secs()),
                reclaim: true,
            };
        }
    }

    // 停止请求：先把 stopAgent 发出去（agent 回报 ok 后关连接并退出），再收尾。
    send_stop_agent(handles, &mut live.conn);
    Exit::Stopped
}

/// 发 `stopAgent` 并尽量等到回报。
///
/// 尽力而为：等不到也不影响收尾（socket 一关，agent 侧的判活会接手）。
fn send_stop_agent(handles: &Handles, conn: &mut Conn) {
    let request_id = next_request_id();
    if let Err(e) = conn.send(
        kind::COMMAND,
        &Command::stop_agent(request_id),
    ) {
        handles.note(format!("发 stopAgent 失败：{e}"));
        return;
    }
    handles.note(format!("已发送 stopAgent（requestId={request_id}）"));
    let deadline = Instant::now() + Duration::from_millis(1000);
    while Instant::now() < deadline {
        match conn.try_read() {
            Ok(Some(frame)) if frame.kind == kind::COMMAND_ACK => {
                match decode_control::<CommandAck>(frame.kind, &frame.payload) {
                    Ok(ack) if ack.request_id == request_id => {
                        handles.note(format!("stopAgent 回报：ok={}", ack.ok));
                        return;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        handles.note(format!("stopAgent 回报非法：{e}"));
                        return;
                    }
                }
            }
            Ok(Some(_)) => {}
            Ok(None) => {}
            Err(e) => {
                handles.note(format!("等 stopAgent 回报时连接结束：{e}"));
                return;
            }
        }
    }
    handles.note("stopAgent 未收到回报（连接可能已被关闭）");
}

/// 把一帧视频负载喂给解码器，并把解出来的帧放进最新帧槽。
fn pump(
    handles: &Handles,
    wake: &Waker,
    live: &mut Live,
    payload: &[u8],
) -> Result<(), PumpError> {
    let (header, annex_b) = parse_video_payload(payload).map_err(PumpError::Protocol)?;
    live.decoder.feed(annex_b, header.pts_us).map_err(PumpError::Decode)?;
    loop {
        match live.decoder.receive().map_err(PumpError::Decode)? {
            Some(frame) => {
                let replaced = handles.put_frame(frame);
                handles.update(|s| {
                    s.frames += 1;
                    if replaced {
                        s.dropped += 1;
                    }
                });
                // 先放下帧、再叫醒 UI：一帧槽不会阻塞，所以不像 spike 的通道那样
                // 需要「先叫醒再送」的次序（那里的 send 可能阻塞，阻塞后没人叫醒 UI）。
                wake.wake();
            }
            None => return Ok(()),
        }
    }
}

/// `pump` 的失败分类。
enum PumpError {
    /// 帧负载结构非法。
    Protocol(ProtocolError),
    /// 解码链路失败。
    Decode(DecodeError),
}

/// 重连：反复尝试直到成功、被中止，或遇到**参数级**的明确失败。
///
/// 期间若连接被拒（本地端口无监听），按 [`REPAIR_MIN_INTERVAL`] 限频自动重建转发 ——
/// 只有 `refused` 分支会调用 [`forward::repair`]，转发正常时不会产生任何 adb 子进程。
fn reconnect(
    cfg: &SessionConfig,
    handles: &Handles,
    stop: &AtomicBool,
    want_stream_id: i32,
) -> Option<Live> {
    // 上次尝试修复的时刻（含失败）——用于 ≥5s 限频，避免每秒创建一个 adb 进程。
    let mut last_repair: Option<Instant> = None;
    loop {
        if stop.load(Ordering::SeqCst) {
            return None;
        }
        match bring_up(cfg, want_stream_id, handles) {
            Ok(live) => return Some(live),
            Err(e) => {
                handles.note(format!("重连失败：{e}"));
                if e.is_terminal() {
                    handles.update(|s| {
                        s.phase = Phase::Failed;
                        s.message = e.to_string();
                    });
                    return None;
                }
                // 仅「连接被拒」才怀疑转发缺失；其它错误碰 adb 只会掩盖真因。
                if matches!(e, SessionError::Connect { refused: true, .. }) {
                    let now = Instant::now();
                    let due = last_repair
                        .map_or(true, |t| now.duration_since(t) >= REPAIR_MIN_INTERVAL);
                    if due {
                        last_repair = Some(now);
                        match forward::repair(cfg.addr.port(), &cfg.socket_name) {
                            Ok(()) => {
                                handles.note(format!(
                                    "转发缺失 ⇒ 已自动重建 tcp:{} → localabstract:{}",
                                    cfg.addr.port(),
                                    cfg.socket_name
                                ));
                            }
                            Err(err) => {
                                handles.note(format!("自动重建转发失败：{err}"));
                                let diag =
                                    forward::diagnose(cfg.addr.port(), &cfg.socket_name, Some(&err));
                                // 与 repair 同步限频：诊断只随修复尝试一起刷新，不每秒刷屏。
                                handles.update(|s| s.message = diag);
                            }
                        }
                    }
                }
            }
        }
        // 退避 1s，期间可被停止请求打断。
        let until = Instant::now() + RECONNECT_DELAY;
        while Instant::now() < until {
            if stop.load(Ordering::SeqCst) {
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// 把首连的「连接被拒」翻成带诊断文案的错误（其余错误原样返回）。
///
/// `start` 的失败路径在重试仍未成功后调用：此时本地端口仍无监听，把可照抄的命令
/// （[`forward::diagnose`]）放进错误详情，用户/日志直接能看到出路。
///
/// @param e 首连（或重试）的错误
/// @param port 本机监听端口
/// @param socket_name 设备侧抽象 socket 名
/// @param adb_error 自动重建转发失败时的说明（成功 / 未尝试时为 `None`）
fn with_connect_diagnosis(
    e: SessionError,
    port: u16,
    socket_name: &str,
    adb_error: Option<&str>,
) -> SessionError {
    match e {
        SessionError::Connect { addr, refused: true, .. } => SessionError::Connect {
            addr,
            refused: true,
            detail: forward::diagnose(port, socket_name, adb_error),
        },
        other => other,
    }
}

/// 「连接被拒」判定 —— **只有** `ConnectionRefused` 才代表本地端口没有监听
/// （转发可能已随 USB 掉线丢失），从而触发自动重建转发。
///
/// 其它 IO 错误（超时 / 网络不可达 / 连接重置）都不触发：它们各有别的原因，
/// 无脑跑 adb 只会掩盖真因。
///
/// @param kind 连接失败的 IO 错误类别
/// @return true = 连接被拒
fn is_refused(kind: std::io::ErrorKind) -> bool {
    kind == std::io::ErrorKind::ConnectionRefused
}

/// 建一轮会话：连接 → 握手 → 能力表 → 选参数 → 申请屏幕 → 建解码器。
///
/// @param cfg 会话配置
/// @param want_stream_id 想认领的屏 id（`0` = 建新屏）
/// @param handles 日志出口
fn bring_up(cfg: &SessionConfig, want_stream_id: i32, handles: &Handles) -> Result<Live, SessionError> {
    // ① 连接
    let mut conn = Conn::connect(cfg.addr, CONNECT_TIMEOUT).map_err(|e| SessionError::Connect {
        addr: cfg.addr,
        refused: is_refused(e.kind()),
        detail: e.to_string(),
    })?;

    // ② 握手：版本精确匹配
    conn.send(kind::HELLO, &Hello { version: PROTOCOL_VERSION })?;
    let hello_ack: HelloAck = expect(&mut conn, kind::HELLO_ACK, handles)
        .map_err(|e| handshake_failure(cfg.addr, &e))?;
    if hello_ack.version != PROTOCOL_VERSION {
        return Err(SessionError::Handshake {
            detail: format!(
                "协议版本不匹配：本机 {PROTOCOL_VERSION}，对端 {} —— 请重推 agent",
                hello_ack.version
            ),
        });
    }
    if hello_ack.state != HelloAck::STATE_READY {
        return Err(SessionError::Handshake {
            detail: format!(
                "对端状态是 `{}`（{}）—— 设备上没有可用的硬件编码器，无法建屏",
                hello_ack.state,
                HelloAck::STATE_NO_HW_ENCODER
            ),
        });
    }
    handles.note(format!("握手完成（协议版本 {PROTOCOL_VERSION}）"));

    // ③ 能力表
    let caps_request_id = next_request_id();
    conn.send(kind::CAPS_REQUEST, &CapsRequest { request_id: caps_request_id })?;
    let caps: Caps = expect(&mut conn, kind::CAPS, handles)
        .map_err(|e| SessionError::Caps { detail: e.to_string() })?;
    if caps.request_id != caps_request_id {
        return Err(SessionError::Caps {
            detail: format!(
                "能力表的 requestId 对不上（收到 {}，期望 {caps_request_id}）",
                caps.request_id
            ),
        });
    }
    handles.note(format!(
        "收到能力表：{} 个编码器，缺省 {}/{}bit/{}",
        caps.codecs.len(),
        match caps.defaults.codec {
            proto::Codec::H264 => "H.264",
            proto::Codec::Hevc => "HEVC",
        },
        caps.defaults.bit_depth,
        caps.defaults.profile
    ));

    // ④ 求交集 + 选参数
    let choice = negotiate::choose(&caps, &cfg.pc, &cfg.request)
        .map_err(|detail| SessionError::Negotiate { detail })?;
    handles.note(format!(
        "选定参数：{} / {} / {}bit / {}x{} / {}fps / bpp={}",
        choice.encoder,
        choice.profile,
        choice.bit_depth,
        choice.width.map(|v| v.to_string()).unwrap_or_else(|| "agent推导".into()),
        choice.height.map(|v| v.to_string()).unwrap_or_else(|| "agent推导".into()),
        choice.fps,
        choice.params.bitrate_bpp.unwrap_or(0.0)
    ));

    // ⑤ 申请屏幕
    let apply_request_id = next_request_id();
    conn.send(
        kind::COMMAND,
        &Command::apply_screen(apply_request_id, want_stream_id, choice.params.clone()),
    )?;
    let ack: CommandAck = expect(&mut conn, kind::COMMAND_ACK, handles)
        .map_err(|e| SessionError::ApplyScreen { detail: e.to_string(), applied: None })?;
    if ack.request_id != apply_request_id {
        return Err(SessionError::ApplyScreen {
            detail: format!(
                "回报的 requestId 对不上（收到 {}，期望 {apply_request_id}）",
                ack.request_id
            ),
            applied: None,
        });
    }
    if !ack.ok {
        return Err(SessionError::ApplyScreen {
            detail: describe_error(ack.error.as_ref()),
            applied: ack.applied,
        });
    }
    let result = ack.result.ok_or_else(|| SessionError::ApplyScreen {
        detail: "回报 ok=true 但没有 result".into(),
        applied: None,
    })?;
    let applied = result.applied;

    // ⑥ 建解码器：帧率取 agent 回报的实际生效值（裸流没有时间戳，帧率只能来自协商）
    let fps = applied
        .refresh_rate
        .filter(|v| *v > 0.0)
        .map(|v| v.round() as u32)
        .unwrap_or(choice.fps);
    let decoder = VideoDecoder::open(
        &cfg.device,
        &VideoParams {
            codec: match choice.codec {
                proto::Codec::H264 => decode::Codec::H264,
                proto::Codec::Hevc => decode::Codec::Hevc,
            },
            fps,
            width: applied.width.or(choice.width).unwrap_or(0),
            height: applied.height.or(choice.height).unwrap_or(0),
        },
    )?;

    Ok(Live { conn, decoder, stream_id: result.stream_id, applied, input_ids: VecDeque::new() })
}

/// 把握手首包的失败翻成可诊断的错误。
///
/// 「连上了但立刻被关闭」这一类的成因高度集中（端口转发还在、设备上却没人在监听），
/// 值得单独说清楚 —— 否则用户只会看到一句「对端关闭了连接」而无从下手。
///
/// @param addr 目标地址
/// @param e 首包读 / 解析的失败
fn handshake_failure(addr: SocketAddr, e: &ProtocolError) -> SessionError {
    let detail = match e {
        ProtocolError::Closed { .. } => format!(
            "连上了 {addr}，但对端在握手完成前就关闭了连接。\n\
             最常见的原因：端口转发还在，而设备上**没有 agent 在监听** `localabstract:wcreen`\
             （跑的是旧实例 / agent 没启动 / 转发指向了别的服务）。\n\
             排查三步：`adb forward --list`、`adb shell ps -A | grep wcreen`、`adb logcat -s WCReen`"
        ),
        other => other.to_string(),
    };
    SessionError::Handshake { detail }
}

/// 等一条指定类型的消息（握手期与申请屏幕用）。
///
/// 期间容忍心跳（agent 每 3s 一条，可能与握手交错）；别的类型一律当协议错误。
/// 死线用 [`T_DEAD`] —— 协议规定握手期沿用同一个判活超时。
fn expect<T: DeserializeOwned>(
    conn: &mut Conn,
    want: u8,
    handles: &Handles,
) -> Result<T, ProtocolError> {
    let deadline = Instant::now() + T_DEAD;
    loop {
        if Instant::now() >= deadline {
            return Err(ProtocolError::Malformed {
                detail: format!(
                    "等 {}（0x{want:02x}）超时：{}s 内没有收到",
                    proto::wire::kind_name(want),
                    T_DEAD.as_secs()
                ),
            });
        }
        match conn.try_read()? {
            Some(frame) if frame.kind == want => {
                return decode_control::<T>(frame.kind, &frame.payload);
            }
            Some(frame) if frame.kind == kind::HEARTBEAT => {}
            Some(frame) => {
                return Err(ProtocolError::Malformed {
                    detail: format!(
                        "期望 {}（0x{want:02x}），收到 {}（0x{:02x}）",
                        proto::wire::kind_name(want),
                        proto::wire::kind_name(frame.kind),
                        frame.kind
                    ),
                });
            }
            None => {
                handles.update(|s| s.message = format!("等待 {}…", proto::wire::kind_name(want)));
            }
        }
    }
}

/// 把错误对象翻成一句可诊断的话。
fn describe_error(error: Option<&ErrorObj>) -> String {
    match error {
        Some(e) => format!("[{}] {} —— {}", e.class, e.code, e.message),
        None => "agent 没有给出错误对象".into(),
    }
}

/// 单调递增的关联号（协议要求 PC 生成、回报必须携带同一 id）。
///
/// 输入流水线（[`crate::input`]）也要用它 —— 它只被**生产**指令的那一侧调用，
/// 而生产只在 UI 线程发生 ⇒ 仍是单点分配。
pub(crate) fn next_request_id() -> i32 {
    use std::sync::atomic::AtomicI32;
    static NEXT: AtomicI32 = AtomicI32::new(1);
    NEXT.fetch_add(1, Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    //! 「连接被拒」判定与诊断翻装的单测。
    //!
    //! 这两条规则决定**何时**才去碰 adb：判错就会在转发正常时跑子进程，
    //! 或在该修复时错过修复。

    use super::*;
    use std::io::ErrorKind;

    /// `refused` 只对 `ConnectionRefused` 为真。
    #[test]
    fn refused_only_for_connection_refused() {
        assert!(is_refused(ErrorKind::ConnectionRefused));
        assert!(!is_refused(ErrorKind::TimedOut));
        assert!(!is_refused(ErrorKind::ConnectionReset));
        assert!(!is_refused(ErrorKind::ConnectionAborted));
        assert!(!is_refused(ErrorKind::NotFound));
        assert!(!is_refused(ErrorKind::PermissionDenied));
    }

    /// 非 `refused` 的连接错误**原样返回**，不附加转发诊断（避免掩盖真因）。
    #[test]
    fn with_connect_diagnosis_leaves_non_refused_untouched() {
        let err = SessionError::Connect {
            addr: "127.0.0.1:27183".parse().unwrap(),
            refused: false,
            detail: "超时".into(),
        };
        let out = with_connect_diagnosis(err, 27183, "wcreen", None);
        match out {
            SessionError::Connect { refused, detail, .. } => {
                assert!(!refused);
                assert_eq!(detail, "超时");
            }
            other => panic!("类型不应改变：{other:?}"),
        }
    }

    /// `refused` 的连接错误会被翻成带可照抄命令的诊断文案。
    #[test]
    fn with_connect_diagnosis_enriches_refused() {
        let err = SessionError::Connect {
            addr: "127.0.0.1:27183".parse().unwrap(),
            refused: true,
            detail: "os error 10061".into(),
        };
        let out = with_connect_diagnosis(err, 27183, "wcreen", Some("adb: no devices")).to_string();
        assert!(out.contains("adb forward tcp:27183 localabstract:wcreen"), "{out}");
        assert!(out.contains("自动重建转发失败：adb: no devices"), "{out}");
    }
}
