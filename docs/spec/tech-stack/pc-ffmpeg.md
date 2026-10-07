# 技术栈 · PC 端 · FFmpeg 接入

> 本文件是 [`AGENTS.md`](../../../AGENTS.md) 索引下的规范之一，属 [`tech-stack/pc.md`](pc.md) 的
> **FFmpeg 接入**部分（2026-10-07 从该文件拆出 —— 原文件在补输入上报后超单文件上限）。
> 帧通路与硬解强校验见 [`tech-stack/pc.md`](pc.md) §2；变更记录见 [`tech-stack/changelog.md`](changelog.md)。

**状态**：**已定**（2026-10-06）。

---

## 接入方式

| 项 | 决定 |
|---|---|
| 接入方式 | **动态链接共享库 + 随包分发**（`.dll` / `.dylib` / `.so`） |
| 绑定 crate | `ffmpeg-next` 一类的高层绑定；crate 只负责接口与找库，**不替代 FFmpeg 本体** |
| 理由 | 构建简单、hwaccel 可控、**LGPL 友好** |

**为什么不用静态/自建**：FFmpeg 是 LGPL（部分构建 GPL），静态链接会触发「允许重新链接」的分发义务；
且 `rsmpeg` / vcpkg 构建期自建在 Windows 上需要 MSVC + nasm + git，构建慢且脆。

**被否的路径**：

| 路径 | 否掉原因 |
|---|---|
| 构建期自建 / 静态链接（`rsmpeg`、vcpkg） | 构建慢且脆，CI 成本高；静态链接另有 LGPL 分发义务 |
| 纯 Rust 解码（`openh264`、`dav1d-rs`） | 无 C 依赖，但都是软解 —— 与「PC 侧不做软解」政策冲突，且 H.264/H.265 覆盖面不足 |
| sidecar 子进程（`ffmpeg-sidecar`） | 镜像场景最差：每帧多一次进程间拷贝，拿不到 GPU surface |
