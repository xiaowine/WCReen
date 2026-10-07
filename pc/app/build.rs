//! 构建脚本：把 FFmpeg 的**共享库**拷到可执行文件旁边。
//!
//! 依据 [`docs/spec/tech-stack/pc.md`](../../../docs/spec/tech-stack/pc.md) §5：
//! FFmpeg 以**动态链接共享库 + 随包分发**接入。
//!
//! 没有这一步，`cargo build` 出来的 `app.exe` 在本机**根本起不来** —— Windows 找不到
//! `avcodec-*.dll` 时进程直接以异常码退出，连 `--help` 都打不出来。
//! 那正是本项目最忌讳的失败形态：**不可诊断**（只有一个莫名其妙的退出码）。
//!
//! 只依赖 `FFMPEG_DIR`（构建期 `ffmpeg-sys-next` 本来就要求它）；缺失时静默跳过 ——
//! 那种情况下链接阶段自己会失败，不需要这里再补一句。

use std::path::{Path, PathBuf};

/// 入口。
fn main() {
    println!("cargo:rerun-if-env-changed=FFMPEG_DIR");
    println!("cargo:rerun-if-changed=build.rs");

    let Ok(ffmpeg_dir) = std::env::var("FFMPEG_DIR") else {
        return;
    };
    let Ok(out_dir) = std::env::var("OUT_DIR") else {
        return;
    };
    // `OUT_DIR` = `<target>/<profile>/build/app-<hash>/out` ⇒ 上溯三级就是 `<target>/<profile>`。
    let Some(profile_dir) = Path::new(&out_dir).ancestors().nth(3) else {
        return;
    };

    // FFmpeg 的发行包在 Windows 上把 DLL 放在 `bin/`，类 Unix 上把共享库放在 `lib/`。
    for sub in ["bin", "lib"] {
        let src = PathBuf::from(&ffmpeg_dir).join(sub);
        let Ok(entries) = std::fs::read_dir(&src) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !is_shared_lib(&path) {
                continue;
            }
            let Some(name) = path.file_name() else {
                continue;
            };
            // 覆盖式拷贝：`FFMPEG_DIR` 换了版本时不能留下旧的库。
            // 失败（例如正被运行中的进程占用）忽略 —— 不该因为打包步骤让构建失败。
            let _ = std::fs::copy(&path, profile_dir.join(name));
        }
    }
}

/// 是否是本平台的可执行文件需要的共享库。
fn is_shared_lib(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default();
    match std::env::consts::OS {
        "windows" => ext.eq_ignore_ascii_case("dll"),
        "macos" => ext.eq_ignore_ascii_case("dylib"),
        // Linux 的版本化库形如 `libavcodec.so.63`，扩展名是 `63` ⇒ 只能按名字判。
        _ => name.contains(".so"),
    }
}
