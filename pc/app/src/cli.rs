//! 命令行参数。
//!
//! 只用标准库手写解析（约 60 行），不引入 clap —— 依赖政策是「优先标准库与已定框架」，
//! 而这里要的只是 6 个可选开关；手写还能把**错误信息**与**帮助文本**写到完全贴合本项目
//! （例如直接告诉用户要做哪一条 `adb forward`）。

/// 命令行参数。
#[derive(Debug, Clone)]
pub struct Args {
    /// agent 监听端口（经 `adb forward` 暴露到本机）。
    pub port: u16,
    /// 虚拟屏宽；`None` = 由 agent 按设备推导。
    pub width: Option<u32>,
    /// 虚拟屏高；`None` = 由 agent 按设备推导。
    pub height: Option<u32>,
    /// 虚拟屏密度（dpi）；`None` = 由 agent 按设备推导（= 主屏 `densityDpi`）。
    pub dpi: Option<u32>,
    /// 刷新率 = 编码帧率；`None` = 由 agent 按设备推导。
    pub fps: Option<u32>,
    /// 码率系数；`None` = 由 agent 按设备推导。
    pub bitrate_bpp: Option<f64>,
    /// 硬解设备名。
    pub device: String,
    /// 设备侧抽象 socket 名（与 agent 的 `--name` 同名）。
    ///
    /// 用于在「本地端口连接被拒」时自动重建 `adb forward`。
    pub name: String,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            width: None,
            height: None,
            dpi: None,
            fps: None,
            bitrate_bpp: None,
            device: DEFAULT_DEVICE.into(),
            name: DEFAULT_NAME.into(),
        }
    }
}

/// 缺省端口（与 agent 的 `Options.DEFAULT_PORT` 一致）。
pub const DEFAULT_PORT: u16 = 27183;

/// 缺省硬解设备（Windows 上的 D3D11；spike 实测可用）。
pub const DEFAULT_DEVICE: &str = "d3d11va";

/// 缺省抽象 socket 名（与 agent 的 `Options.DEFAULT_NAME` 一致）。
pub const DEFAULT_NAME: &str = "wcreen";

/// 解析结果。
pub enum Parsed {
    /// 正常启动。
    Run(Box<Args>),
    /// 打印帮助后退出（退出码 0）。
    Help,
    /// 参数非法（打印原因 + 帮助，退出码 2）。
    Bad(String),
}

/// 解析命令行。
///
/// 支持 `--opt value` 与 `--opt=value` 两种写法。
///
/// @param argv 不含程序名的参数序列
/// @return 解析结果
pub fn parse(argv: impl IntoIterator<Item = String>) -> Parsed {
    let mut args = Args::default();
    let mut it = argv.into_iter();
    while let Some(arg) = it.next() {
        // 把 `--opt=value` 拆成名字与值。
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) => (n.to_string(), Some(v.to_string())),
            None => (arg.clone(), None),
        };
        let mut value = |name: &str| -> Result<String, Parsed> {
            if let Some(v) = inline.clone() {
                return Ok(v);
            }
            it.next().ok_or_else(|| Parsed::Bad(format!("{name} 缺少取值")))
        };

        match name.as_str() {
            "-h" | "--help" => return Parsed::Help,
            "--port" => match value("--port").and_then(|v| parse_num(&v, "--port", |s: &str| s.parse::<u16>())) {
                Ok(v) if v > 0 => args.port = v,
                Ok(_) => return Parsed::Bad("--port 必须在 1..=65535".into()),
                Err(bad) => return bad,
            },
            "--width" => match value("--width").and_then(|v| parse_num(&v, "--width", |s: &str| s.parse::<u32>())) {
                Ok(v) if v > 0 => args.width = Some(v),
                Ok(_) => return Parsed::Bad("--width 必须大于 0".into()),
                Err(bad) => return bad,
            },
            "--height" => match value("--height").and_then(|v| parse_num(&v, "--height", |s: &str| s.parse::<u32>()))
            {
                Ok(v) if v > 0 => args.height = Some(v),
                Ok(_) => return Parsed::Bad("--height 必须大于 0".into()),
                Err(bad) => return bad,
            },
            "--dpi" => match value("--dpi").and_then(|v| parse_num(&v, "--dpi", |s: &str| s.parse::<u32>()))
            {
                Ok(v) if v > 0 => args.dpi = Some(v),
                Ok(_) => return Parsed::Bad("--dpi 必须大于 0".into()),
                Err(bad) => return bad,
            },
            "--fps" => match value("--fps").and_then(|v| parse_num(&v, "--fps", |s: &str| s.parse::<u32>())) {
                Ok(v) if v > 0 => args.fps = Some(v),
                Ok(_) => return Parsed::Bad("--fps 必须大于 0".into()),
                Err(bad) => return bad,
            },
            "--bitrate-bpp" => {
                match value("--bitrate-bpp").and_then(|v| parse_num(&v, "--bitrate-bpp", |s: &str| s.parse::<f64>()))
                {
                    Ok(v) if v > 0.0 => args.bitrate_bpp = Some(v),
                    Ok(_) => return Parsed::Bad("--bitrate-bpp 必须大于 0".into()),
                    Err(bad) => return bad,
                }
            }
            "--device" => match value("--device") {
                Ok(v) if !v.is_empty() => args.device = v,
                Ok(_) => return Parsed::Bad("--device 不能为空".into()),
                Err(bad) => return bad,
            },
            "--name" => match value("--name") {
                Ok(v) if !v.is_empty() => args.name = v,
                Ok(_) => return Parsed::Bad("--name 不能为空".into()),
                Err(bad) => return bad,
            },
            other => return Parsed::Bad(format!("未知参数 `{other}`")),
        }
    }
    Parsed::Run(Box::new(args))
}

/// 解析一个数值并给出统一格式的错误。
fn parse_num<T, E: std::fmt::Display>(
    text: &str,
    flag: &str,
    f: impl FnOnce(&str) -> Result<T, E>,
) -> Result<T, Parsed> {
    f(text).map_err(|e| Parsed::Bad(format!("{flag} 的取值 `{text}` 非法：{e}")))
}

/// 帮助文本。
pub fn usage() -> &'static str {
    "\
WCReen PC 客户端 —— 把 Android 独立虚拟屏串到 PC 并实时显示

用法：
  app [选项]

选项：
  --port <PORT>           agent 的监听端口（默认 27183）
  --width <PIXELS>        虚拟屏宽（缺省：由 agent 按设备推导）
  --height <PIXELS>       虚拟屏高（缺省：由 agent 按设备推导）
  --dpi <DPI>             虚拟屏密度（缺省：由 agent 按设备推导 = 主屏密度）
  --fps <HZ>              刷新率 = 编码帧率（缺省：由 agent 按设备推导）
  --bitrate-bpp <F>       码率系数 bits/pixel/frame（缺省：由 agent 按设备推导）
  --device <NAME>         硬解设备名（默认 d3d11va）
  --name <NAME>           设备侧抽象 socket 名（默认 wcreen，与 agent 同名）
                          连接被拒时会用它自动重建 adb 转发
  -h, --help              显示本帮助

连接前必须先做端口转发（agent 在设备上监听抽象 socket `wcreen`）：
  adb forward tcp:27183 localabstract:wcreen

示例：
  adb forward tcp:27183 localabstract:wcreen
  app --width 1920 --height 1080 --fps 60

退出码：
  0  正常退出        1  连不上 / 硬解不可用等运行期失败        2  命令行参数非法
"
}

#[cfg(test)]
mod tests {
    //! 命令行解析单测 —— 覆盖 `--name`（自动重建转发的目标抽象名）与 `--dpi`（虚拟屏密度）。

    use super::*;

    /// 把参数序列解析成 `Args`（断言解析成功）。
    fn run(argv: &[&str]) -> Args {
        match parse(argv.iter().map(|s| s.to_string())) {
            Parsed::Run(a) => *a,
            _ => panic!("期望正常解析：{argv:?}"),
        }
    }

    /// 不给 `--name` 时缺省为 `wcreen`（与 agent 同名）。
    #[test]
    fn name_defaults_to_wcreen() {
        assert_eq!(DEFAULT_NAME, "wcreen");
        assert_eq!(run(&[]).name, "wcreen");
    }

    /// `--name` 的两种写法都要支持。
    #[test]
    fn name_both_writing_styles() {
        assert_eq!(run(&["--name", "custom"]).name, "custom");
        assert_eq!(run(&["--name=custom"]).name, "custom");
    }

    /// 空名字 / 缺取值都是非法参数。
    #[test]
    fn name_empty_or_missing_is_bad() {
        assert!(matches!(parse(["--name".to_string()]), Parsed::Bad(_)));
        assert!(matches!(parse(["--name=".to_string()]), Parsed::Bad(_)));
    }

    /// `--name` 不影响其它选项的解析。
    #[test]
    fn name_coexists_with_others() {
        let a = run(&["--port=1234", "--name=foo", "--fps=30"]);
        assert_eq!(a.port, 1234);
        assert_eq!(a.name, "foo");
        assert_eq!(a.fps, Some(30));
        assert_eq!(a.device, DEFAULT_DEVICE);
    }

    /// `--dpi` 缺省为 `None`（= 由 agent 按设备推导 = 主屏密度）。
    #[test]
    fn dpi_defaults_to_none() {
        assert_eq!(run(&[]).dpi, None);
    }

    /// `--dpi` 的两种写法都要支持，且与其它选项共存。
    #[test]
    fn dpi_both_writing_styles() {
        assert_eq!(run(&["--dpi", "240"]).dpi, Some(240));
        assert_eq!(run(&["--dpi=480"]).dpi, Some(480));
        let a = run(&["--dpi=320", "--fps=30"]);
        assert_eq!(a.dpi, Some(320));
        assert_eq!(a.fps, Some(30));
    }

    /// `--dpi` 的 0、缺取值、非法值都是非法参数。
    #[test]
    fn dpi_zero_or_bad_is_rejected() {
        assert!(matches!(parse(["--dpi".to_string()]), Parsed::Bad(_)));
        assert!(matches!(parse(["--dpi=0".to_string()]), Parsed::Bad(_)));
        assert!(matches!(parse(["--dpi=abc".to_string()]), Parsed::Bad(_)));
    }
}
