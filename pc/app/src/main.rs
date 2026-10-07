//! WCReen PC 客户端入口。
//!
//! 编排（`docs/spec/agent/transport-handshake.md` §2）：
//! 连接 → 握手（版本精确匹配）→ 请求能力表 → **PC 求交集选参数** → 申请屏幕 →
//! 收帧渲染；双向心跳每 3s 一条，`T_dead` 10s 无消息即判掉线并重连；退出时发 `stopAgent`。
//!
//! # 启动顺序里的一个刻意选择
//!
//! **先建会话、再开窗口**：连不上、版本不匹配、硬解不可用都必须在窗口出现之前
//! 就给出可诊断的错误并以非 0 退出码结束 —— 否则用户面对的只是一个空窗口。
//! 稳态之后的掉线才交给会话线程在后台重连。
//!
//! # 退出码
//!
//! `0` 正常退出 / `1` 运行期失败（连不上、硬解不可用等）/ `2` 命令行参数非法。

mod cli;
mod conn;
mod forward;
mod input;
mod negotiate;
mod session;
mod ui;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::process::ExitCode;
use std::sync::Arc;

use eframe::egui;

use crate::cli::{Args, Parsed};
use crate::negotiate::Request;
use crate::session::{Session, SessionConfig};

/// 入口。
fn main() -> ExitCode {
    env_logger::init();
    match cli::parse(std::env::args().skip(1)) {
        Parsed::Help => {
            println!("{}", cli::usage());
            ExitCode::SUCCESS
        }
        Parsed::Bad(msg) => {
            eprintln!("参数错误：{msg}\n\n{}", cli::usage());
            ExitCode::from(2)
        }
        Parsed::Run(args) => run(*args),
    }
}

/// 建会话并开窗口。
///
/// @param args 命令行参数
/// @return 进程退出码
fn run(args: Args) -> ExitCode {
    // ① 硬解设备：枚举是**候选**，逐个实试创建才是结论（实测有枚举得到却创建失败的设备）。
    println!("=== 硬解设备（枚举 + 实试创建）===");
    for probe in decode::probe_devices() {
        match (probe.creatable, &probe.error) {
            (true, _) => println!("  ✓ {:<12} 可创建", probe.name),
            (false, Some(e)) => println!("  ✗ {:<12} {e}", probe.name),
            (false, None) => println!("  ✗ {:<12} 创建失败", probe.name),
        }
    }

    // ② 建共享硬解设备（所有解码器共用同一个）。
    let device = match decode::HwDevice::create(&args.device) {
        Ok(d) => Arc::new(d),
        Err(e) => {
            eprintln!("**建硬解设备失败**：{e}");
            eprintln!("提示：换一个设备名试试（上面的列表里带 ✓ 的都可以）");
            return ExitCode::FAILURE;
        }
    };

    // ③ 本机硬解能力 —— 「PC 求交集」里的 PC 侧输入。
    let pc = match decode::probe_hardware_capability(&args.device) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("**探测本机硬解能力失败**：{e}");
            return ExitCode::FAILURE;
        }
    };
    println!("=== 本机硬解能力（{}）===", pc.device);
    println!(
        "  可硬解编码器：{}",
        if pc.codecs.is_empty() {
            "（无）".to_string()
        } else {
            pc.codecs.iter().map(|c| c.name()).collect::<Vec<_>>().join(", ")
        }
    );
    println!(
        "  尺寸上限：{}x{}（0 = 设备未报告，此时以首帧硬校验为准）",
        pc.max_width, pc.max_height
    );
    println!("  回读格式：{:?}（本实现只做 nv12 = 8-bit 4:2:0）", pc.sw_formats);

    // ④ 建会话（同步完成首轮建链；失败即明确报错退出）。
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), args.port);
    println!(
        "=== 连接 {addr}（设备侧抽象 socket `wcreen` 经 adb forward 暴露）==="
    );
    let cfg = SessionConfig {
        addr,
        device,
        pc,
        request: Request {
            width: args.width,
            height: args.height,
            dpi: args.dpi,
            fps: args.fps,
            bitrate_bpp: args.bitrate_bpp,
        },
        socket_name: args.name,
    };
    let session = match Session::start(cfg) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("**会话建立失败**\n{e}");
            return ExitCode::FAILURE;
        }
    };

    // ⑤ 开窗口。窗口尺寸只是初值 —— 画面按解码出来的实际尺寸等比缩放。
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_title("WCReen"),
        ..Default::default()
    };
    match eframe::run_native(
        "WCReen",
        options,
        Box::new(move |cc| Ok(Box::new(ui::App::new(cc, session)))),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("**窗口启动失败**：{e}");
            ExitCode::FAILURE
        }
    }
}
