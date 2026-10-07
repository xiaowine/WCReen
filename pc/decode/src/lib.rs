//! WCReen PC 端解码与帧通路。
//!
//! 规范：`docs/spec/tech-stack/pc.md`（硬解强校验、T1 帧通路、最小缓冲）、
//! `docs/plans/2026-10-07-pc-spike-report.md`（全部实测结论与踩过的坑）。
//!
//! # 三个模块
//!
//! | 模块 | 负责 |
//! |---|---|
//! | [`hw`] | 硬解设备（`d3d11va` 等）的创建与**能力探测** |
//! | [`decoder`] | 裸 Annex-B（H.264 / HEVC）→ 硬解 → 回读成 NV12 |
//! | [`render`] | NV12 → RGB 的 wgpu 管线（离屏纹理交给 egui 采样） |
//!
//! # 本 crate 是 FFI 的隔离层
//!
//! **所有 `unsafe` / FFI 都关在这里**，对外只暴露安全 API。上层（`app`）拿到的是
//! 可跨线程传递的 [`Nv12Frame`] 与 wgpu 资源，看不到任何裸指针。
//!
//! # 三条不可动摇的约束
//!
//! 1. **只硬解，绝不软解** —— 用 `get_format` 回调只接受 `AV_PIX_FMT_D3D11` 做**结构性保证**
//!    （拿不到硬解 ⇒ `avcodec_open2` 直接失败），再叠加**逐路首帧硬校验**
//!    （FFmpeg 对不支持的 profile / 尺寸**不报错，而是静默回落软解**，退出码仍是 0）；
//! 2. **不得从裸流推导帧率** —— 裸 Annex-B 没有容器时间戳，FFmpeg 对裸流猜的帧率是错的
//!    （实测 `r_frame_rate=120/1`、`avg_frame_rate=25/1`，真实 60）
//!    ⇒ 尺寸 / 帧率一律由**调用方从协商参数传入**（[`decoder::VideoParams`]）；
//! 3. **离屏纹理必须是 `Rgba8Unorm`（不是 sRGB）**、色彩矩阵 **BT.709 全范围** ——
//!    egui-wgpu 把采样值当 gamma 值用，用 sRGB 纹理会让 GPU 再编码一次（画面发白）；
//!    全范围**不做** 16/255 抬升（按有限范围写会导致画面偏暗）。
//!
//! # 输入形态是「一帧一帧喂」
//!
//! 调用方拿到的是 `0x01` 帧的 Annex-B 数据 + flags + pts_us，不是文件 ⇒
//! 本 crate **不碰任何解复用器**（没有 `avformat`），也**没有 EOF 概念**：
//! 直播语义是「到达即上屏」，不节流。

#![deny(missing_docs)]

pub mod decoder;
pub mod hw;
pub mod render;

pub use decoder::{Codec, DecodeError, DecodeResult, Nv12Frame, Stage, VideoDecoder, VideoParams};
pub use hw::{probe_devices, probe_hardware_capability, DeviceProbe, HardwareCapability, HwDevice};
pub use render::{Nv12Pipeline, StreamTarget, TARGET_FORMAT};
