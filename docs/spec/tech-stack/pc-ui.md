# 技术栈 · PC 端 UI 层

> 本文件是 [`AGENTS.md`](../../../AGENTS.md) 索引下的规范之一，属技术栈 PC 端的一部分。
> **解码 / 帧通路 / 容量**见 [`tech-stack/pc.md`](pc.md)；
> 总览与未决项见 [`tech-stack/overview.md`](overview.md)。

---

## 1. UI 层选型：egui + wgpu（已定）

理由：

- **视频纹理入口现成**：`egui_wgpu::Renderer::register_native_texture(device, texture_view, filter) -> TextureId`
  与 `egui_wgpu::Callback` 是官方 API，解码帧以 wgpu `Texture` 直接进 UI。
- **控件集齐全**：egui 自带按钮 / 输入框 / 列表 / 菜单，`egui_extras`、`egui_plot` 补表格与图表。
- **版本与生态稳定**：0.36.x 稳定序列，模板 / 示例 / 文档齐全，无需追 pre-1.0 主干。

**已接受的代价**：观感偏工具风；布局原语弱于 Flexbox（需要时用第三方 `egui_taffy`）；
立即模式每帧重建 —— 对「视频面 + 少量控件」的薄 UI 影响有限。

对比依据（保留）：

| 维度 | GPUI | egui |
|---|---|---|
| 编程模型 | 混合：retained（`Entity` / `Context` 状态图）+ 立即式元素树 | 纯立即模式，状态自持 |
| **控件集** | **不含控件库**（按钮 / 输入框 / 列表需自研；Zed 的 `ui` crate 不在 `gpui` 里） | 自带完整控件集（另有 `egui_extras`、`egui_plot`） |
| 布局 | flexbox（taffy）+ Tailwind 风格 API | 水平 / 垂直 / 网格为主，Flexbox 需第三方 `egui_taffy` |
| 文本 | cosmic-text，编辑器级排版与 IME | 基础排版，复杂脚本与富文本较弱 |
| 渲染后端 | Metal / D3D11 / Vulkan（blade）；主干迁 wgpu | wgpu（默认）或 glow / OpenGL |
| **视频纹理入口** | 只有 Apple 的 `CVPixelBuffer` surface，非 Apple 平台被 `cfg` 掉 | `register_native_texture` / `egui_wgpu::Callback` 官方现成 |
| 版本与生态 | pre-1.0；发布版与主干分叉；文档薄，API 跟着 Zed 需求走 | 0.36.x 稳定序列；模板 / 示例 / 文档齐全，社区大 |
| 已知短板 | 无跨平台纹理入口、需跟主干并维护 patch | 布局较简单、每帧重建成本、AccessKit 为外挂、动画原语有限 |

**结论**：客户端 UI 薄（视频网格 + 工具栏 + 设置 + 设备列表），
egui 的短板落在边缘需求，GPUI 的短板落在核心需求 —— 选 **egui + wgpu**。

## 2. 否决 GPUI 的依据（实查源码）

1. **pre-1.0，发布版与主干已分叉**：crates.io 是 `0.2.2`，Zed 主干已重组为
   `gpui_macos` / `gpui_windows` / `gpui_linux` / `gpui_wgpu`。
2. **视频帧进 GPUI 没有跨平台入口**（决定性问题）：
   - `SurfaceSource` 的唯一变体是 `CVPixelBuffer`，且 `pub fn surface(...)` 被
     `#[cfg(any(target_os = "macos", target_os = "ios"))]` 整体包住；非 Apple 平台上
     `Surface::paint()` 是空实现（图元存在但不显示）。
   - 主干 `gpui_wgpu` 已有 `SurfaceParams` + `surfaces` 渲染管线与绑定组，但 `SurfaceSource`
     仍只有 `CVPixelBuffer` —— **wgpu 后端是内部实现，不是对外的纹理入口**。
   - 发布版 `0.2.2` 的公开 API 里**没有任何自定义着色器入口**（整页索引 grep `shader` 零命中）；
     `ImageFormat` 只是图片文件格式，原始帧只能走 CPU 侧 BGRA 的 `RenderImage`。
   - 社区证据：`mgvs/gpui` 专门 fork 加 macOS/Metal IOSurface 外部纹理路径；
     `nestrilabs/gpui-wgpu`、`wgpui` 是把 GPUI 整体换成 wgpu 后端的 fork —— 都说明要动框架内部。
   → 与本项目核心需求（多路 GPU 纹理上屏）正面冲突，**否决**。

## 3. 被否的方案

| 方案 | 否掉的原因 |
|---|---|
| **GPUI**（含主干 wgpu 后端） | UI 精致度最好；但**没有跨平台的视频纹理入口**（`SurfaceSource` 仅 `CVPixelBuffer`，非 Apple 平台 API 被 `cfg` 掉；主干 `gpui_wgpu` 是内部实现）、发布版无自定义着色器入口、pre-1.0 且发布版与主干分叉、不含控件库 —— 与本项目核心需求（多路 GPU 纹理上屏）正面冲突。详见上方「否决 GPUI 的依据」 |
| Kotlin/JVM + Compose Desktop | 语言与 agent 一致、可共享协议代码；但硬解必须接 FFmpeg/JNI，且桌面端工具链与 Rust 相比在低延迟渲染上没有优势 |
| Web（WebCodecs）+ Electron | UI 迭代快、自带硬解；但引入 JS 生态，与移动端零共享，协议要写两份 |
| Python + PyAV | 最快跑通，但性能与分发都差 —— 正是本次重构要解决的问题 |
