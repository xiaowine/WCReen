# PC spike · 实施记录

> 本文件是 [`docs/plans/2026-10-06-pc-spike.md`](2026-10-06-pc-spike.md)（**已批准**）的**实施记录**。
> 方案内容**不回头改**（[`docs/spec/docs.md`](../spec/docs.md)）—— 实际偏差与实施细节**记在这里**。
> **追加式记录**，不删历史条目。

---

## 1. 环境（本机，已实测）

| 项 | 值 |
|---|---|
| Rust | `rustc 1.97.1 (8bab26f4f 2026-07-14)` / `cargo 1.97.1`，`stable-x86_64-pc-windows-msvc` |
| 工具链根 | `D:\rust\`（`rustup\` = `RUSTUP_HOME`，`cargo\` = `CARGO_HOME`） |
| MSVC | `D:\rust\vs-buildtools` + Windows SDK（`C:\Program Files (x86)\Windows Kits\10`） |

⚠️ **坑**：`RUSTUP_HOME` / `CARGO_HOME` 是**用户级环境变量**，`PATH` 里的 `rustup.exe` 只是代理 ——
**新起的 shell / 子进程若没继承这两个变量，会报「no installed toolchains / no active toolchain」** ✗
（本记录即因此踩过一次）。脚本与工具里跑 Rust 前必须显式带上：

```
RUSTUP_HOME=D:\rust\rustup
CARGO_HOME=D:\rust\cargo
```

---

## 2. FFmpeg dev 包（**已获取并验证**）

| 项 | 值 |
|---|---|
| 来源 | **BtbN FFmpeg-Builds**（GitHub release tag `latest`） |
| 资产 | `ffmpeg-n9.0-latest-win64-lgpl-shared-9.0.zip`（77.2 MB） |
| 选择理由 | **LGPL**（`pc.md §6` 要求「LGPL 友好」）+ **shared**（动态链接）+ **n9.0 分支**（与本机 `ffmpeg 9.0.2` 同版本） |
| 本机位置 | `D:\ffmpeg-dev\n9.0\ffmpeg-n9.0-latest-win64-lgpl-shared-9.0\`（解压后 183 MB） |
| 版本 | `ffmpeg version n9.0.2-22-g46d8f462ee-20261006`；`avcodec 63.1.102` / `avutil 61.1.102` |

### 2.1 内容（**完整**）

| 目录 | 内容 |
|---|---|
| `include/` | 7 个模块头文件（`libavcodec` / `libavutil` / `libavformat` / `libavfilter` / `libavdevice` / `libswresample` / `libswscale`） |
| `lib/` | **MSVC 导入库**（`avcodec.lib` / `avutil.lib` / …）+ `.def` + MinGW `.dll.a` |
| `bin/` | DLL（`avcodec-63.dll` / `avutil-61.dll` / …）+ `ffmpeg.exe` / `ffprobe.exe` |

### 2.2 许可证合规（**已核实**）

- 配置含 **`--enable-version3`**、**`--disable-libx264`**、**`--disable-libx265`** ✓
- `ffmpeg -L` → **GNU Lesser General Public License v3** ✓
- `-encoders` 里**没有** x264 / x265 / xvid ✓
- ⇒ **符合 `pc.md §6`「LGPL 友好」** ✓（我们只解码，不需要这些编码器）

> ⚠️ 核查时的坑：`-encoders` 的**输出尾部会附带 configuration 行**，直接字符串搜 `libx264` 会
> 命中 `--disable-libx264` ⇒ **假阳性**。必须只搜编码器列表段，或查 `-buildconf`。

### 2.3 硬件加速能力（**已确认**）

`ffmpeg -hwaccels`：**`d3d11va`** ✓ · `dxva2` · `d3d12va` · `cuda` · `qsv` · `amf` · `vulkan` · `opencl` · `vaapi`

H.264 / HEVC 解码器齐备：native `h264` / `hevc` + `h264_cuvid` / `hevc_cuvid` / `_qsv` / `_amf`
（`d3d11va` 是**通用 hwaccel**，不是独立解码器条目 —— 正常）

### 2.4 链接与运行验证（**通过**）

最小 Rust 程序（`#[link(name = "avcodec")]` + `avcodec_version()` / `avcodec_configuration()`）：

```
rustc t.rs -L native=<lib 目录> -o t.exe     → rc = 0
运行（PATH 加 <bin 目录>）                     → rc = 0
  avcodec_version = 63.1.102
  --disable-libx264 present: true
```

⇒ **导入库可被 MSVC 链接、DLL 可加载、符号可解析** ✓（脚手架已删除）

---

## 3. `pc/` 骨架与依赖（**已建成并跑通**）

### 3.1 布局

```
pc/
├─ Cargo.toml        # workspace（resolver 3 / edition 2024 / rust-version 1.97）
└─ spike/            # spike crate（一次性验证代码，不产出产品功能）
   ├─ Cargo.toml
   └─ src/main.rs
```

### 3.2 依赖版本（`cargo add` 解析结果）

| crate | 版本 |
|---|---|
| `eframe` / `egui` | **0.36.2** |
| `wgpu` | **30.0.1** |
| `ffmpeg-next` / `ffmpeg-sys-next` | **9.0.0** |
| `anyhow` / `log` / `env_logger` | 1.0.104 / 0.4.34 / 0.11.11 |

（`bindgen` 0.72.1 由 `ffmpeg-sys-next` 传递引入）

**`pc/Cargo.lock` 入库** —— 二进制项目应锁定依赖树；**它本身就是「版本锁定」的载体**
（技术栈未决项 2），改版本走 `cargo update -p <crate>`，不在 `Cargo.toml` 里写死补丁号。

### 3.3 构建环境（**三个必需的环境变量**）

| 变量 | 值 | 为什么必需 |
|---|---|---|
| `RUSTUP_HOME` | `D:\rust\rustup` | **不继承** ⇒ 否则报「no installed toolchains」 |
| `CARGO_HOME` | `D:\rust\cargo` | 同上 |
| `FFMPEG_DIR` | `D:\ffmpeg-dev\n9.0\ffmpeg-n9.0-latest-win64-lgpl-shared-9.0` | `ffmpeg-sys-next` 靠它找头文件与导入库（Windows 无 pkg-config） |
| `LIBCLANG_PATH` | `D:\rust\libclang` | ⚠️ **`ffmpeg-sys-next` 的 `build.rs` 无条件用 bindgen** ⇒ 必须有 `libclang.dll` |

**libclang 的获取**（本机原先没有）：VS Build Tools 当初装的是**最小集**（只有 `VC.Tools.x86.x64`
+ `Windows11SDK`）⇒ `VC\Tools\` 下**只有 `MSVC`，没有 `Llvm`** ✗
⇒ 取 **NuGet `libclang.runtime.win-x64` 22.1.8**（31.5 MB 包 / 内含 85 MB `libclang.dll`），
解到 `D:\rust\libclang\` —— **零 UAC**，且符合「工具链集中在 `D:\rust\`」的既有约定。

> 备选（未采用）：给 VS Build Tools 增量装 `Microsoft.VisualStudio.Component.VC.Llvm.Clang`
> —— 更"正统"但要管理员权限。

### 3.4 验证结果（**全部通过**）

| 项 | 结果 |
|---|---|
| `cargo build` | ✅ rc=0（首次 37.7s） |
| **链接** | ✅ `ffmpeg-next 9.0.0` 编译并链接成功 |
| **运行期 DLL 加载** | ✅ `cargo run` 打印 `libavutil 61.1.102` / `libavcodec 63.1.102` / `libavformat 63.1.102` |
| **「随包分发」形态** | ✅ **PATH 只留 system32**、DLL 全在 exe 同目录 ⇒ 照样跑通 ✓ |

> 排障记录：中途曾以为"运行无输出"，实为**我自己的 Python 脚本 bug**（`r.stdout` 为 `None`）
> —— 程序本身一直正常。**用 `cargo run` 复现即可**。

### 3.5 硬解走哪一层：**`sys` 是设计内的出口，不是绕过**

- `ffmpeg-next 9.0.0` 的 `src/` 里**没有任何** `hwaccel` / `hw_device` / `HWDeviceType` ✗
- **`ffmpeg-the-third 6.0.0`（另一主流 fork）同样没有** ✗ ⇒ 这是**业界常态**，不是某个 crate 的缺陷
- **`ffmpeg-next` 官方暴露 FFI 层**：`pub extern crate ffmpeg_sys_next as sys;` +
  `pub use crate::sys as ffi;`（`src/lib.rs:8` / `:13`）⇒ **作者预期用户下沉到 FFI** ✓
- **`ffmpeg-sys-next` 生成的 bindings 硬件加速 API 齐全** ✓：
  `av_hwdevice_iterate_types` · `av_hwdevice_ctx_create` · `av_hwdevice_get_hwframe_constraints` ·
  `av_hwframe_transfer_data` · `av_hwframe_transfer_get_formats` · **`AV_PIX_FMT_D3D11`** ·
  `AV_HWDEVICE_TYPE_{D3D11VA, D3D12VA, DXVA2, CUDA, QSV, AMF, VULKAN, …}`

⇒ **分工**：高层（`ffmpeg-next`）做 demux / decode / frame / scaler 骨架；
**硬解配置与帧回读走 `sys`（`ffmpeg_next::sys`，unsafe FFI）** —— 与方案 §3.3 列的手段一致 ✓

**被排除的替代**：`rsmpeg`（唯一有 hwaccel 高层封装的）—— 它**要求自建 FFmpeg** ✗，
与 [`tech-stack/pc.md`](../spec/tech-stack/pc.md) §6 已定的「**动态链接 + 随包分发、不自建**」冲突 ⇒ 排除
（**与它有没有 hwaccel 无关**）。

> ⚠️ **仍未验证**：`sys` 层的硬解路径**尚未跑通**（还没写代码）—— 这是下一步的核心验收项。

---

## 4. 样片（**已生成并验证**）

**位置**：`D:\wcreen-samples\`（**仓库外** —— 测试数据不入库）

| 文件 | codec / profile | 分辨率 | fps | 大小 |
|---|---|---|---|---|
| `t-1440x3200-h264.mp4` | H.264 / High | **1440×3200**（纵向超长，关键） | 60 | 3.00 MB |
| `t-1440x3200-hevc.mp4` | HEVC / Main | 1440×3200 | 60 | 2.52 MB |
| `t-1920x1080-h264.mp4` | H.264 / High | 1920×1080 | 60 | 1.35 MB |
| `t-1280x720-h264.mp4` | H.264 / High | 1280×720 | 60 | 1.11 MB |

**共同参数**（ffprobe 已核实）：600 帧 / 10 秒 · `yuvj420p`（**full range**）· BT.709 ·
**`bf=0`（无 B 帧）** · GOP = 60（**关键帧间隔 1 秒**）· `-tune zerolatency` · `+faststart`

> **与 agent 侧编码参数对齐**：profile（High/Main）· 位深（8-bit）· 色彩（BT709 + **FULL**）·
> 关键帧间隔（1s）· **无 B 帧**（对应 `max-bframes=0`）—— 见
> [`tech-stack/pc.md`](../spec/tech-stack/pc.md) 与 `agent/encode-params.md`。
> 码率用 **CRF 18 / 22**（而非 agent 缺省的 bpp 0.1）—— 因为样片要**读得出帧号**。

### 4.1 帧号内嵌（**验收 #8 的前提**）

**做法**：`testsrc` + `drawtext` 叠加 `%{n}`（**帧号**），白字黑底、字号 = 高度/12。

⚠️ **不能用 `testsrc` 自带的那个白数字** —— 实测它是**秒计数**（帧 0 显示 `0`、帧 599 显示 `9`），
**只有 1 秒分辨率**，无法量化端到端延迟 ✗
改用 `drawtext` 后实测：**帧 301 显示 `301`、帧 599 显示 `599`** ✓ 逐帧递增、清晰可读 ✓

### 4.2 可复现命令

```bash
ffmpeg -f lavfi -i "testsrc=size=<W>x<H>:rate=60:duration=10" \
  -vf "drawtext=fontfile='C\:/Windows/Fonts/arialbd.ttf':text='%{n}':fontcolor=white:fontsize=<H/12>:box=1:boxcolor=black@0.75:boxborderw=<H/72>:x=(w-text_w)/2:y=h*0.03" \
  -c:v libx264 -profile:v high -pix_fmt yuv420p -bf 0 -g 60 -keyint_min 60 -sc_threshold 0 \
  -tune zerolatency -crf 18 \
  -color_primaries bt709 -color_trc bt709 -colorspace bt709 -color_range pc \
  -movflags +faststart -y t-<W>x<H>-h264.mp4
```

> 编码用**本机 gyan.dev essentials**（含 `libx264` / `libx265`）；**BtbN 那个 LGPL 版不能用来生成**
> —— 它 `--disable-libx264/x265` ✗

---

## 5. 下一步

| # | 步骤 | 状态 |
|---|---|---|
| 1 | **获取 FFmpeg dev 包** | ✅ **完成**（§2） |
| 2 | 建 `pc/` 骨架（Cargo workspace + `pc/spike`） | ✅ **完成**（§3） |
| 3 | 生成样片（本机 `ffmpeg.exe`，含 1440×3200 纵向超长） | ✅ **完成**（§4） |
| 4 | 按方案 §3 实现并逐项打 8 条验收 | ✅ **完成** —— 数据见 [`2026-10-07-pc-spike-report.md`](2026-10-07-pc-spike-report.md) |

---

## 6. 实施要点（代码落地时定下的，报告里只放数据）

| 项 | 决定 | 理由 |
|---|---|---|
| 硬解走哪一层 | **`ffmpeg_next::sys`（FFI）** | 高层无 hwaccel API；官方暴露 FFI 层（§3.5） |
| 「绝不软解」怎么落 | **`get_format` 回调只接受 `AV_PIX_FMT_D3D11`** | 找不到就返回 `AV_PIX_FMT_NONE` ⇒ `avcodec_open2` **明确失败**；否则 FFmpeg 会**静默软解**（退出码仍 0） |
| 硬解设备归属 | **全进程一个 `HwDevice`，各路共享**（`Arc`） | 产品里所有路挂在同一 D3D11 设备上；每路各建会虚高设备数 |
| 离屏纹理格式 | **`Rgba8Unorm`（非 sRGB）** | egui-wgpu 的着色器注明 *"expect 'normal' textures that are NOT sRGB-aware"*，它把采样值当 gamma 值用；用 sRGB 会二次编码 ⇒ 画面发白 |
| 色彩矩阵 | **BT.709 全范围**（`color_range=pc`） | 与 §4 样片及 agent 侧编码参数对齐 |
| 播放节流 | **按源时间轴 PTS 节流 + 展示队列反压** | 解码快于实时；不节流就变成"尽快解完"而非播放 |
| 延迟旋钮 | **缓冲深度（队列 + 通道）可配**（`SPIKE_BUFFER`） | 实测 `lat ≈ 缓冲帧数 × 帧间隔`（报告 §4） |

---

## 7. 变更记录

- **2026-10-07 初版**：环境实测（Rust 工具链 + `RUSTUP_HOME`/`CARGO_HOME` 的坑）；
  **FFmpeg dev 包已获取并验证**（BtbN LGPL shared n9.0，含头文件 + MSVC 导入库 + DLL；
  许可证与 hwaccel 已核实；**最小程序链接 + 运行通过**）。
- **2026-10-07 补 §3**：**`pc/` 骨架建成并跑通** —— workspace + `spike`；依赖解析到
  eframe/egui **0.36.2**、wgpu **30.0.1**、ffmpeg-next/sys **9.0.0**；
  **三个必需环境变量**（`RUSTUP_HOME` / `CARGO_HOME` / `FFMPEG_DIR` / `LIBCLANG_PATH`）；
  **libclang 经 NuGet 获取**（VS 最小集里没有 Llvm 组件）；
  **链接 + 运行期 DLL 加载 + 「随包分发」形态全部验证通过**；
  并核实 **硬解走 `sys` 层是设计内的出口**（`ffmpeg-next` 官方暴露 `ffmpeg_sys_next`；
  `ffmpeg-the-third` 同样没有 hwaccel 高层 API ⇒ 业界常态，不是缺陷）。
- **2026-10-07 补 §4**：**样片已生成并验证** —— `D:\wcreen-samples\`（仓库外）4 个文件
  （**1440×3200 纵向超长** h264 + hevc、1080p、720p）；共同参数 600 帧 / 60fps / **full range BT709** /
  **无 B 帧** / **GOP=1s**（**与 agent 侧编码参数对齐**）。
  ⚠️ 发现 `testsrc` 自带的数字是**秒计数**（1 秒分辨率 ✗）⇒ 改用 **`drawtext` 叠加帧号 `%{n}`**，
  实测逐帧递增、清晰可读 ✓（**验收 #8 端到端延迟量化的前提**）。
- **2026-10-07 补 §5 / §6 / §7**：**spike 实现完成，8 条验收逐项打过**（数据另见
  [`2026-10-07-pc-spike-report.md`](2026-10-07-pc-spike-report.md)）。补记代码落地时定下的
  实施要点（硬解走 `sys`、「绝不软解」用 `get_format` 强制、共享 `HwDevice`、
  **离屏纹理必须非 sRGB**、色彩用全范围、PTS 节流、延迟旋钮）。
  ⚠️ **样片回归事故（自己引入，已修复）**：曾按「真实投屏是有限范围」的推断把样片重生成成
  `testsrc2` + `tv` + 60 秒，**破坏了本节 §4 已定的三条**（`drawtext` 帧号水印 / 全范围 BT.709 /
  10 秒 600 帧）✗。已按 §4.2 原命令逐字节还原（四个文件大小与 §4 表格记录**完全一致**），
  shader 亦改为全范围系数。**教训：本文件的既有参数是已定结论，不得凭推断覆盖。**
- **2026-10-07 决策**：用户定 **不做容量处理** —— 并发上限、准入控制、降级策略一律**不进入设计**。
  spike 里的 `probe` / `admit` 仅作**测量夹具**保留，其产出的并发数据仅作事实记录
  （见报告 §5）。
- **2026-10-07 决策（追加）**：**并删除** `tech-stack/pc.md §3` 的容量/准入控制描述 ⇒
  §3 整段改为「**不做容量处理**」（不设上限 / 不做准入 / 不做降级 / 不踢已有路 / 无账本 / 无看门狗 /
  无缓存），仅保留**硬解强校验**（保证「绝不软解」，与容量无关）。
  **级联 10 处**：`tech-stack/{pc,overview,pc-ui}.md` + `agent/{display,encode,encode-params,encode-probe,transport,transport-handshake,transport-topology,transport-backpressure}.md` +
  索引 `AGENTS.md` / `code-map.md`；各文件的「变更记录」按追加式记录。
  ⚠️ **未动** `docs/plans/2026-10-06-pc-spike.md`（方案冻结，不回头改）—— 其中 §3.3 的准入控制设计
  与本次决策冲突，**以本文件与报告为准**。
- **2026-10-07 决策（追加）**：**缓冲帧数取尽可能小**（远程操控 ⇒ 延迟优先）。
  同时修掉一个 spike 的**漏唤醒死锁**：`fill_queue` 在展示队列满时不取通道，
  显示完最后一帧后队列空又**不排重绘**，而解码线程正阻塞在 `send` 上（唤醒标志已在阻塞前用掉）
  ⇒ 双方互等。**该 bug 曾把缓冲下限误报为 3 帧**；修后实测真值 **2 帧 = 38.6 ms**（1080p60，
  比 6 帧的 105 ms 省 66 ms）。已写入 `tech-stack/pc.md §2` 与报告 §4 / §8。
