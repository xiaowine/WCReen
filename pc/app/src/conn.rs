//! 连接：帧读写 + **带缓冲的帧解析**。
//!
//! # 为什么需要自己的缓冲
//!
//! 判活要求「读侧定期醒来检查心跳与死线」，所以 socket 带读超时。若直接对着 socket 读
//! 一帧（`read_frame`），**读到一半超时就会把半个帧丢掉** ⇒ 读循环失步，之后全是垃圾。
//!
//! 所以本模块把字节先收进缓冲、再从缓冲里切帧：超时只是「这一轮还没有完整的一帧」，
//! **缓冲里的字节一个都不丢**。这样既不空转（不忙轮询），也永不失步。
//!
//! 读写都在同一线程（协议规定「一条连接 = 一个写者」），所以 `Conn` 同时提供
//! [`Conn::send`] 与 [`Conn::try_read`]，不需要拆分 socket。

use std::io::{ErrorKind, Read};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use proto::{encode_control, parse_frame_header, write_frame, Frame, ProtocolError};
use serde::Serialize;

/// 一轮「没有数据」的等待时长。
///
/// 它同时是**三件事**的粒度：
/// - 心跳与死线的检查粒度（`T_dead` = 10s ⇒ 8ms 迟得可以忽略）；
/// - **输入上报的延迟上限** —— 会话线程是唯一写者，UI 只能把输入投进队列，
///   而队列要等这一轮读超时才被排空 ⇒ 它就是「鼠标动了 → 消息上线路」的最坏延迟。
///   输入是**交互**，人眼对 >20ms 就有感觉 ⇒ 取 8ms（约半帧 @60fps）；
/// - 队列排空的节奏（同上）。
///
/// ⚠️ 原值是 500ms —— 那时循环只负责收帧，判活粒度粗一点无所谓；
/// 有输入之后它成了延迟瓶颈 ✗
///
/// 代价只是每秒 ~125 次 `read` 超时返回，相对一次 JSON + CRC + socket 写可以忽略。
pub const POLL_INTERVAL: Duration = Duration::from_millis(8);

/// 缓冲压缩的阈值：消费掉这么多字节后就把前缀挪掉，避免缓冲无限增长。
const COMPACT_THRESHOLD: usize = 1 << 20;

/// 一条连接：读缓冲 + 写。
pub struct Conn {
    /// 底层 socket（读写共用；同一线程顺序使用）。
    stream: TcpStream,
    /// 已收到但还没被切走的字节。
    buf: Vec<u8>,
    /// [`Self::buf`] 里已经消费掉的前缀长度。
    start: usize,
}

impl Conn {
    /// 建连接并配好超时。
    ///
    /// @param addr 目标地址（`adb forward` 之后就是 `127.0.0.1:<port>`）
    /// @param connect_timeout 连接超时
    /// @return 成功给出连接；连接失败给出 IO 错误（由上层翻译成可诊断的提示）
    pub fn connect(addr: SocketAddr, connect_timeout: Duration) -> std::io::Result<Self> {
        let stream = TcpStream::connect_timeout(&addr, connect_timeout)?;
        // 实时画面：小包不能被 Nagle 攒着。
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(POLL_INTERVAL))?;
        Ok(Self { stream, buf: Vec::with_capacity(64 * 1024), start: 0 })
    }

    /// 发一条控制消息（`[类型][长度][CRC32 + JSON]`）。
    ///
    /// @param kind 帧类型
    /// @param msg 消息 DTO
    /// @return 成功；编码或写失败给出对应错误
    pub fn send<T: Serialize>(&mut self, kind: u8, msg: &T) -> Result<(), ProtocolError> {
        let payload = encode_control(kind, msg)?;
        write_frame(&mut self.stream, kind, &payload)
    }

    /// 试着取下一帧。
    ///
    /// @return `Ok(None)` = 这一轮还没有完整的一帧（读超时，缓冲里的字节**原样保留**）；
    ///   `Ok(Some)` = 完整的一帧；`Err` = 对端关闭 / 协议错误 / 长度越界
    pub fn try_read(&mut self) -> Result<Option<Frame>, ProtocolError> {
        loop {
            if let Some(frame) = self.take_buffered()? {
                return Ok(Some(frame));
            }
            let mut chunk = [0u8; 64 * 1024];
            match self.stream.read(&mut chunk) {
                Ok(0) => {
                    return Err(ProtocolError::Closed { pending: self.buf.len() - self.start });
                }
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                // 读超时：两种错误码都要认（Windows 给 `TimedOut`，类 Unix 给 `WouldBlock`）。
                Err(e)
                    if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    return Ok(None);
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => return Err(ProtocolError::Io(e)),
            }
        }
    }

    /// 从缓冲里切出一帧（不够就返回 `None`，不动缓冲）。
    fn take_buffered(&mut self) -> Result<Option<Frame>, ProtocolError> {
        let available = &self.buf[self.start..];
        if available.len() < proto::HEADER_SIZE {
            return Ok(None);
        }
        let header = parse_frame_header(available)?;
        let total = proto::HEADER_SIZE + header.length as usize;
        if available.len() < total {
            return Ok(None);
        }
        let payload = available[proto::HEADER_SIZE..total].to_vec();
        self.start += total;
        if self.start >= COMPACT_THRESHOLD && self.start * 2 >= self.buf.len() {
            self.buf.drain(..self.start);
            self.start = 0;
        }
        Ok(Some(Frame { kind: header.kind, payload }))
    }
}
