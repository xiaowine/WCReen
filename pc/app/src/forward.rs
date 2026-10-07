//! adb 端口转发的自愈与诊断。
//!
//! # 为什么需要这个模块
//!
//! `adb forward` 是**挂在 adb 传输连接上**的：USB 一掉线、传输消失，转发会被 adb
//! 一并清掉，插回来后**不会自动恢复**。而 PC 侧连的是本地端口 `127.0.0.1:<port>`，
//! 于是 `TcpStream::connect` 一直被拒，重连循环每秒重试、**永远失败**。
//!
//! 本模块只负责两件事：在**确定信号（连接被拒）**下重建转发（[`repair`]），
//! 以及失败时拼出一份可照抄的诊断（[`diagnose`]）。**只有在确认「连接被拒」时**
//! 才会被调用 —— 转发本来就正常时，不产生任何 adb 子进程（零额外开销、零风险）。

use std::process::Command;

/// 重建 adb 转发：`adb forward tcp:<port> localabstract:<name>`。
///
/// 只在「本地端口连接被拒」（转发多半已随 USB 掉线丢失）时调用。
///
/// @param port 本机监听端口
/// @param name 设备侧抽象 socket 名（与 agent 的 `--name` 同名）
/// @return 成功给出 `Ok(())`；失败给出**含 stdout + stderr** 的说明。
///   `adb` 不存在（未安装 / 不在 `PATH`）同样返回 `Err`，**绝不 panic**。
pub fn repair(port: u16, name: &str) -> Result<(), String> {
    let mut cmd = Command::new("adb");
    cmd.arg("forward")
        .arg(format!("tcp:{port}"))
        .arg(format!("localabstract:{name}"));
    match cmd.output() {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => Err(format!(
            "`adb forward tcp:{port} localabstract:{name}` 失败（退出码 {:?}）：\n  stdout: {}\n  stderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stdout).trim(),
            String::from_utf8_lossy(&out.stderr).trim(),
        )),
        Err(e) => Err(format!(
            "无法执行 `adb`：{e}（adb 未安装，或不在 PATH 中；装好 adb 平台工具后重试）"
        )),
    }
}

/// 拼装「转发缺失」的诊断文案。
///
/// 不武断下结论：**并列**列出两种可能（① 设备离线 / USB 掉线后转发被 adb 清掉；
/// ② agent 没在监听这个抽象名 / 根本没启动），并给出**可直接复制**的排查命令。
/// `adb_error` 非空时（自动 `repair` 失败）把它的原文一并附上。
///
/// @param port 本机监听端口
/// @param name 设备侧抽象 socket 名
/// @param adb_error 自动重建转发失败时的错误说明（成功 / 未尝试时为 `None`）
/// @return 中文诊断文案（可直接写进日志与状态栏）
pub fn diagnose(port: u16, name: &str, adb_error: Option<&str>) -> String {
    let mut text = format!(
        "本地端口 tcp:{port} 连接被拒 —— 没有进程在监听。两种可能：\n\
         ① 设备离线 / USB 掉线后 adb 把转发清掉了（转发挂在 adb 传输连接上，掉线即消失）；\n\
         ② 设备上 agent 没启动，或没在监听抽象 socket `{name}`。\n\
         可照抄的排查命令：\n\
         \x20 adb devices\n\
         \x20 adb forward tcp:{port} localabstract:{name}\n\
         \x20 adb forward --list"
    );
    if let Some(err) = adb_error {
        text.push_str(&format!("\n自动重建转发失败：{err}"));
    }
    text
}

#[cfg(test)]
mod tests {
    //! `diagnose` 的文案要点单测。
    //!
    //! 这几条文案是「**失败时给用户可照抄的出路**」这条验收标准的落点，
    //! 不能因为改写措辞就把命令串弄丢。

    use super::*;

    /// 不管有没有 adb 错误，都必须带上三条可照抄的命令与抽象名。
    #[test]
    fn diagnose_contains_commands() {
        let text = diagnose(27183, "wcreen", None);
        assert!(text.contains("adb devices"), "缺 `adb devices`：{text}");
        assert!(
            text.contains("adb forward tcp:27183 localabstract:wcreen"),
            "缺转发命令：{text}"
        );
        assert!(
            text.contains("adb forward --list"),
            "缺 `adb forward --list`：{text}"
        );
        assert!(text.contains("wcreen"), "缺抽象名：{text}");
        // 两种可能必须并列出现，不武断下结论。
        assert!(text.contains("设备离线"), "缺「设备离线」可能：{text}");
        assert!(text.contains("agent"), "缺「agent 未监听」可能：{text}");
        // 没有 adb 错误时不该出现失败行。
        assert!(!text.contains("自动重建转发失败"), "多出失败行：{text}");
    }

    /// 传入 `adb_error` 时，原文必须被附上。
    #[test]
    fn diagnose_appends_adb_error() {
        let text = diagnose(27183, "wcreen", Some("adb: no devices/emulators found"));
        assert!(
            text.contains("自动重建转发失败"),
            "缺失败标记：{text}"
        );
        assert!(
            text.contains("adb: no devices/emulators found"),
            "缺 adb 原文：{text}"
        );
    }

    /// `diagnose` 的端口 / 名字必须原样反映到命令串里。
    #[test]
    fn diagnose_reflects_port_and_name() {
        let text = diagnose(12345, "custom_name", None);
        assert!(
            text.contains("adb forward tcp:12345 localabstract:custom_name"),
            "端口/名字没反映：{text}"
        );
    }
}
