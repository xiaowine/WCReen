# 技术栈变更记录

> 本文件是 [`tech-stack/overview.md`](overview.md) 的**变更记录**，从该文件拆出（原文件超单文件上限）。
> 技术选型正文见 [`tech-stack/overview.md`](overview.md)；
> PC 解码链路见 [`tech-stack/pc.md`](pc.md)；UI 层见 [`tech-stack/pc-ui.md`](pc-ui.md)。
>
> 变更方式：先讨论 → **在本文件追加一条并说明理由**；**不删旧结论**。
> **2026-10-06 追加系列（初版–追加 11）已归档** ⇒ [`tech-stack/changelog-archive.md`](changelog-archive.md)。

---

- **2026-10-07 FFmpeg dev 包已获取并验证**：BtbN `ffmpeg-n9.0-latest-win64-lgpl-shared-9.0.zip`
  （**LGPL v3**、shared、n9.0 分支 = 与本机 9.0.2 同版本；含 `include/` + MSVC 导入库 `lib/` + `bin/` DLL）。
  **最小 Rust 程序链接 + 运行通过**（`avcodec_version()=63.1.102`）；`d3d11va` 等 hwaccel 齐备。
  ⇒ 未决项 **3 部分收口**（Windows 已定，Linux / macOS 未定）。
  细节与**环境坑**（`RUSTUP_HOME`/`CARGO_HOME` 未继承）见
  [`plans/2026-10-07-pc-spike-implementation.md`](../../plans/2026-10-07-pc-spike-implementation.md)。
- **2026-10-07 PC spike 方案已批准**：`docs/plans/2026-10-06-pc-spike.md` **§9 批准记录**已填 ——
  批准范围 = §3 默认选择 + §8 六项默认值（**仅 Windows** / **只做 T1** / **本地样片** /
  `pc/` + `pc/spike` / FFmpeg dev 包优先级 / 顺带给建议版本）。**闸门打开，可以开始实施**。
  连带：`code-map.md` 的状态与 `overview.md §5 #5` 同步。**方案内容不回头改**，偏差另建文档。
- **2026-10-07 未决项回填**：F4 定稿后回填 §5 —— **#4 / #5 / #6 / #7 / #8 收口**（能力协商、
  协议格式、编码参数对齐、容量模型、spike 方案），**#9 部分收口**（仍待定：多传输并存、网络侧认证）；
  **#1 / #2 / #3 仍待定**（PC 侧）。**编号不变**以保历史引用有效。
- **2026-10-07 输入上报**：新增 [`pc.md`](pc.md) §4「**输入上报：鼠标合并与限频**」——
  `move` **定频采样**（上限 = **屏刷新率**，它是 PC 自己选的参数且是"画面能体现的上限"）、
  **`scroll` 累加 `dx`/`dy`**（增量语义，**不能丢**）、`down`/`up`/`cancel`/`key` **立即发**；
  与 agent 侧队列合并**职责不同、两端都要做**（PC 省协议开销 / agent 是最后一道保险）。
- **2026-10-06 拆分**：**本变更记录整段提成独立文件**（原文件超单文件上限）；
  `overview.md` 只留一行指针。
- **2026-10-07 决定**：**删除容量处理** —— `pc.md §3` 由「容量模型（静态探测两层 + 准入控制 +
  预算账本 / 看门狗 / 结果缓存）」整段改为「**不做容量处理**」：**不设上限、不做准入、不做降级、
  不踢已有路**，容量由运行平台硬件决定，PC 侧只保留**硬解强校验**（保证「绝不软解」，与容量无关）。
  **依据**：spike 实测「加路**从不失败**」—— 一路加到 32 路（1080p）全部成功且全部通过硬解校验，
  超出时只有**每路 fps 按比例下降**（8 路 60fps → 16 路 32fps）⇒ **不存在可判定的"超容量"状态**，
  准入控制本就无从下手（[报告](../../plans/2026-10-07-pc-spike-report.md) §5）。
  连带更新：`overview.md`（摘要 + 未决项 4 由「容量模型：探测项与准入控制」改为「容量处理：**已定（不做）**」）、
  `pc-ui.md`（头部指针）、`pc.md §2`（延迟已量化：`lat ≈ 缓冲帧数 × 帧间隔`，见报告 §4）。
- **2026-10-07 决定**：**缓冲取尽可能小** —— 本应用是**远程操控**（人要看着画面操作手机），
  **延迟优先于抗抖动** ⇒ `pc.md §2` 记入取向与实测下限：**2 帧 = 38.6 ms**（1080p60），
  比 6 帧（105 ms）省 **66 ms**。⚠️ 早期报的"下限 3 帧"是 spike 的**漏唤醒死锁**造成的误判
  （已修，见报告 §8）。
- **2026-10-07 交付**：**PC 侧输入上报（F5）落地并实机验收** —— `pc.md §4` 由「已定」转「**已实现**」。

  **代码**：新增 `pc/app/src/input.rs`（采样器 + 键位映射，纯逻辑）与 2 项协议契约测试 +
  11 项采样 / 映射单测；`pc/proto` 新增 `Command::Input` 与 `InputAction` / `InputPointer` /
  `InputScroll` / `InputKey`；`pc/app/src/session.rs` 加输入队列（UI 生产、会话线程消费 —— 协议要求
  **唯一写者**）、`Status` 加输入计数、失败回报识别为「输入被拒」、STAT 行带输入条数；
  `conn.rs` 的 `POLL_INTERVAL` 由 500ms 改 **8ms**。

  **两条规格级修正**：
  1. `POLL_INTERVAL` 原值是**判活粒度**（`T_dead`=10s 下 500ms 足够），但会话线程是唯一写者 ⇒
     它同时是「鼠标动了 → 消息上线路」的**延迟上限** ✗ ⇒ 收到 **8ms**（约半帧 @60fps）。
     代价只是每秒 ~125 次读超时返回。
  2. **滚轮符号取负**：egui 的 `smooth_scroll_delta` 正 = 内容向下移，而 Android 的 `AXIS_VSCROLL`
     经 `View.scrollBy(0, +)` 把**视口**往下移 ⇒ 两者**同向异号**。

  **实现决定**：单指模型（左键 = 触点，**右键 = BACK、中键 = HOME、Esc = BACK**）；
  键盘按 `egui::Key` → Android `keyCode` 表 + `metaState`，长按 = 连续 `keyDown`（agent 侧 `repeat` 恒 0）；
  **失焦 / 鼠标在窗外松开必须发 `cancel` / 补 `up`**（否则 agent 侧触点永远按着）；
  画面矩形**自己算**（不依赖 `centered_and_justified` 的隐式偏移）；
  **子系统级被拒**（`inputDenied`）后停掉生产端。

  **实测**（`cargo build --release --workspace` 零告警、`cargo test --workspace` **33 项全绿**；
  验收用**真实鼠标 / 键盘**驱动窗口，不是脚本发协议）：

  | 操作 | 证据 |
  |---|---|
  | 真实拖动（13 个采样点的路径） | PC 侧 STAT `输入 11 条（被拒 0）` —— 中间点被**定频采样**合并掉 ✓；虚拟屏原始截图 **96.0%** 行变化 ✓ |
  | 按一次 Escape | PC 侧 `输入 13 条`（`keyDown`+`keyUp` **立即发** ✓）；该屏前台 `com.android.settings/.MiuiSettings` → **（无前台）** ✓ |
  | 全程 | 被拒 **0**；硬解回读 `nv12` ✓；帧数 47 → 355（画面真在动 ✓） |

  **修正 3（同日，用户报「点击没反应」）**：触点按下 / 抬起原挂 `drag_started` / `drag_stopped` ✗ ——
  egui 的判定是「按下够久或移动够远才算拖」（*鼠标按下 ≠ 正在拖动，可能只是点击的开头*），
  纯点击一路都不触发 ⇒ 症状正是「**拖动有效、点击完全没反应**」。
  改为**事件驱动**：`pointer.primary_pressed()` + 指针在画面上 ⇒ `down`；`pointer.primary_released()`
  + 本端正按着 ⇒ `up`（抬在哪里都算）；只用主键 ⇒ 右键 / 中键（BACK / HOME）不产生触点。
  按新规范**先查文档**确认语义（`ctx7 docs /websites/rs_egui`）后再改，未凭猜测动手。
- **2026-10-07 修复**：**PC 侧 adb 转发丢失自愈** —— 新增 [`pc.md`](pc.md) §5「连接与 adb 转发」。

  **现象**：USB 掉线重插后 PC 一直连不上 —— 连的是本地端口 `127.0.0.1:27183`，`TcpStream::connect`
  一直**被拒**，重连循环每秒重试但**永远失败**（实测：`adb forward --list` 为空、端口拒连、**设备在线**）。

  **根因**：`adb forward` 挂在 **adb 传输连接**上 —— USB 一掉、传输消失，转发被 adb **一并清掉**，
  插回来**不会自动恢复**。

  **修法**（新增 `pc/app/src/forward.rs`）：① **只在连接被拒**（`ErrorKind::ConnectionRefused`）时补发
  `adb forward tcp:<port> localabstract:<name>`；② 补发**限频 ≥5s**；③ 补不好则给**并列诊断**
  （设备离线 / agent 没在下这个名 / 没装 adb）+ **可照抄的命令**；**转发正常时不产生任何 adb 子进程**。

  **CLI**：新增 `--name <NAME>`（缺省 `wcreen`，与 agent 的 `--name` 同名），用于自动重建转发。

  **边界**：「断开重连**不保屏**」—— agent 侧「断开即拆屏」是 **S2 语义**；「断开冻结 + 重连认领」
  属 **S5**（未做）。

  **验证**：`cargo build --release --workspace` 零告警、`cargo test --workspace` 全绿；
  实机以 `adb kill-server` 模拟掉线（等价触发条件），转发被自动补回、端口重新可连。
  **连带**：`pc.md` 的「FFmpeg 接入」由 §5 顺延为 §6（§5 让给「连接与 adb 转发」）；
  `code-map.md` 登记 `pc/app/src/forward.rs`。
- **2026-10-07 指针更正**：`plans/2026-10-07-pc-spike-implementation.md` 中 3 处指 FFmpeg 的引用
  「`pc.md §5`」随上一批 「FFmpeg 接入」由 §5 顺延为 §6 一并更正为 **§6**（L34 / L51 / L148）；
  **仅改编号引用，方案内容不动**（方案文档冻结，此为事实性更正）。
