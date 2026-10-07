# `:agent` 实施切片与推进顺序

> 本文件是 [`docs/spec/workflow.md`](../spec/workflow.md) §2 定义的**实现方案文档**。
> 范围：**怎么把已定稿的 F1–F7 规范切成可交付的片**，以及每片的验收方式。
> 代码形态（包结构 / 命名 / 失败形态 …）**不在本文件**，见 [`agent/code.md`](../spec/agent/code.md)。
> 写完即冻结；实施中的偏差另建文档。

**状态**：**已批准**（2026-10-07）。

---

## 1. 背景与目标

`:agent` 的 F1–F7 规范**全部定稿**，但 `agent/` 下**零行源码**（只有 `.gitkeep`）。
一次性写完的风险已被用户指出：**体量大、且极易偏离代码形态要求**。

**目标**：定出**逐片交付**的推进顺序，使每一片都是**可独立验收的完整功能**，
且**第一片就把代码形态钉死**，后续片按同一套形态填充。

**验收标准（本文件自身）**：每片都有**可观测、可复现**的里程碑（§6）。

---

## 2. 现状调研（事实依据）

| 事实 | 依据 |
|---|---|
| 两模块**零源码**，只有 `.gitkeep`；`agent/scripts/` **不存在** | `find agent/src app/src -name "*.kt" -o -name "*.java"` → 空 |
| 代码形态**原先无规范** ⇒ 已起草 [`agent/code.md`](../spec/agent/code.md)（草案） | 该文件 §1、§11 |
| **业务参数一律走协议**，CLI 只放进程级与调试；**不提供业务调试开关** | [`agent/runtime-startup.md`](../spec/agent/runtime-startup.md) §1.1 |
| **启动阶段不建屏**；agent **不自动建屏** | 同文件 §3；[`agent/display-lifecycle.md`](../spec/agent/display-lifecycle.md) §1 |
| 线程表是**固定 6 角色**（`main`/`accept`/`slow`/`reader`/`link`/`sender`，F5 再加 `input`） | [`agent/runtime-threads.md`](../spec/agent/runtime-threads.md) §2 |
| **不留半成品**（禁 TODO / 占位 / 假实现） | [`docs/spec/workflow.md`](../spec/workflow.md) §4 |
| 协议**版本精确匹配** ⇒ agent 与 PC **同步演进是合法状态** | [`agent/transport-handshake.md`](../spec/agent/transport-handshake.md) |
| **不探测编码器能力就 `configure()` 是禁忌** | [`agent/module.md`](../spec/agent/module.md) §8 |

### 2.1 由此推出的**实施顺序硬结论**

「业务参数走协议」+「不建屏在启动阶段」+「agent 不自动建屏」
⇒ **建屏只能由 PC 经协议触发** ⇒ **没有 F4 的连接层，F1 根本无法被触发**。

⇒ features.md §4 的 `F1 → F3 → F2 → F4 → F5` 是**讨论顺序**，
**实施顺序必须把 F4 的连接层提到最前**。

---

## 3. 方案与取舍

| 方案 | 结论 |
|---|---|
| **A. 按子系统顺序一次写完** | ❌ 与 §2.1 冲突（没连接层就触发不了建屏），且体量不可控 |
| **B. 按"半个功能"切**（如先不做能力探测、先不做静止刷帧） | ❌ 违反 `module.md §8` 禁忌与 `workflow.md §4`；半个功能无法验收 |
| **C. 按端到端完整功能子集切片**（**采纳**） | ✅ 每片都能独立演示与验收；协议版本精确匹配 ⇒ PC 侧同步只实现对应子集**不是半成品** |

**每片的定义**：一条**从 PC（或脚本）到设备副作用**的完整链路，链路两端同时只实现该子集。

---

## 4. 切片表

| 片 | 范围 | 里程碑（可验收） |
|---|---|---|
| **S0** | **代码形态定稿** —— 审 [`agent/code.md`](../spec/agent/code.md)（零业务码） | 用户逐条确认，状态转「已定」 |
| **S1** | **F6 运行时 + F7 可观测性 + F4 连接层**（拓扑 / 握手 / 协议骨架 / 心跳 / 停止） | 脚本连上 → 握手成功 → **每 3s 收到心跳** → 发停止 → **退出码 0**；**重复实例 → 退出码 2**；`kill -TERM` → 干净收尾 |
| **S2** | **F1 建屏 + F3 编码 + F4 帧路径**（能力探测 / 参数 / 重建 / 静止刷帧 / 背压与丢帧） | 发建屏指令 → 收到码流 → **ffmpeg 落盘出图**（首个真实画面）；PC 端同步实现真实解码渲染 |
| **S3** | **F2 枚举 + 拉起** | 枚举出应用列表；打开指令后 `dumpsys` 佐证目标 App **归属虚拟屏** |
| **S4** | **F5 输入注入** | 注入触点 → 虚拟屏内容按预期变化 |
| **S5** | **多路与生命周期语义**（`N` 计数、断开即释放、单路失败隔离） | 2 条连接 = 2 块屏，**互不影响**；其中一路断开不波及另一路 |

**S1 为什么能单独成立**：它交付的是**一个可被 PC 连接与保活、可被干净停止的进程** ——
这是完整能力，不是"还没功能的骨架"；协议类型表只含握手 / 心跳 / 停止，**PC 侧也只实现这些**。

---

## 5. 影响面

| 区域 | 变化 |
|---|---|
| `agent/src/main/kotlin/com/xiaowine/wcreen/agent/` | 按 [`agent/code.md`](../spec/agent/code.md) §2.1 新增包与文件（根只有 `Main.kt`） |
| `agent/src/test/kotlin/` | **新增**（纯逻辑单测：协议编解码 / CLI 解析 / flag 位运算 / 状态机） |
| `agent/scripts/` | **新增**（`run-agent.sh`；目前该目录不存在，但 `module.md §5` 已引用它） |
| `agent/build.gradle.kts` | 可能需要加 `testImplementation`（[`tech-stack/overview.md`](../spec/tech-stack/overview.md) §4） |
| PC 端 `pc/` | **S2 起**新增产品 crate（现只有 `pc/spike`）；工程布局是 `tech-stack/overview.md` §5 的**未决项 #1** |
| 对外契约 | 线协议 / CLI / 部署路径**均已定**，本方案不改契约 |
| `docs/code-map.md` | 每片落地后同步（硬性要求） |

---

## 6. 验证方式

**混合**（已与用户确认）：**S1 用一次性脚本**；**S2 起同步做 PC**。

| 片 | 验证手段 |
|---|---|
| S1 | **一次性脚本**：`adb forward` + 连接 + 握手 + 收心跳 + 发停止；校验退出码。脚本验收后**丢弃** |
| S2 | **PC 端同步实现**真实解码渲染（避免重复造解码轮子）；同时用 `ffmpeg` 落盘出图作为**独立**证据 |
| S3 | `dumpsys` 佐证 + PC 侧列表/画面 |
| S4 | 注入后截图对比（虚拟屏内容变化） |
| S5 | 两条连接并行；逐路断开验证隔离 |

**每片都必须实跑**（`workflow.md §4` 交付前自验），单测通过不算功能可用
（[`tech-stack/overview.md`](../spec/tech-stack/overview.md) §4）。

---

## 7. 风险与回滚

| 风险 | 应对 |
|---|---|
| **S2 体量最大**（建屏 + 编码 + 帧路径三块耦合） | 它是**原子**的（`module.md §3` 不变量 3：屏输出面 = 编码器输入面）；如仍过大，**回到讨论**再切，不擅自缩范围 |
| **PC 工程布局未定**（`tech-stack/overview.md §5` #1） | S2 前必须先定，否则 S2 会边写边改目录 |
| 代码形态草案被推翻 ⇒ S1 重写 | S1 是**最小**的一片，重写代价可控；这也是先做 S0 的理由 |
| 实机与 AVD 行为不一致 | 沿用既有做法：**以实体机为准**，AVD 只作对照 |
| 回滚 | 每片独立提交粒度；任一片不通过 ⇒ 停在上一片，**不回退已通过的片** |

---

## 8. 未决问题

| # | 问题 | 阻塞哪一片 |
|---|---|---|
| 1 | PC 端 Rust 工程布局（workspace 位置与 crate 划分） | **S2** |
| 2 | egui / wgpu 版本锁定与升级策略 | S2 |
| 3 | Linux / macOS 的 FFmpeg dev 包获取方式 | S2（若三平台都做） |
| 4 | 多传输并存 + 网络侧认证 | S5 |
| 5 | 心跳与控制队列的确切容量值（`runtime-threads.md §9`） | S1 |

---

## 9. 变更记录

- **2026-10-07 起草**：定出 S0–S5 切片与推进顺序。**关键结论**：
  「业务参数走协议 + 启动不建屏 + agent 不自动建屏」⇒ **F4 连接层必须最先**，
  features.md 的 `F1→F3→F2→F4→F5` 只是**讨论顺序**。
  验证方式：**S1 用一次性脚本，S2 起同步做 PC**（用户已确认）。
- **2026-10-07 S2 交付（F1 建屏 + F3 编码 + F4 帧路径）**：实机 **28 项验收全过** ——
  握手 → 能力表（**7 个硬编编码器**，含尺寸范围与步进、缺省表）→ `applyScreen 1920×1080@60`
  （回报 `applied` 全部实际生效值，码率 **12,441,600 bit/s** = 1920×1080×60×0.1，`profile=High`，
  `ownDisplayGroup=true`）→ **`dumpsys display` 查到屏名 `WCReen-1`**（独立证据）→
  **收到 47 帧**（**首帧就是关键帧**、Annex-B 起始码、pts 单调）→ **ffmpeg 解出 PNG 且码流尺寸 = 1920×1080** →
  `stopAgent` 退出码 **0**。JVM 单测 **42 个全绿**，构建零警告。
  **PC 端同步交付**：`pc/` 三个产品 crate（`proto` / `decode` / `app`），`cargo build --release` 零 warning、
  `proto` 20 个单测全过；**真机联调通过** —— PC 应用渲染出虚拟屏上运行的 Android 设置页，
  状态栏 `streamId=1 c2.qti.avc.encoder 1920x1080 @60fps · 已上屏 49 解出 49 丢弃 0`，无解码错误。
  实施中的三处修正：
  ① 连接断开后 `link` 线程不退出（每条死连接留一条永久空转线程）⇒ 加 `linkStop` 信号；
  ② 缺省 profile 取成了 `profileLevels` 的第一个（实测是 `Baseline`）⇒ 改为按规范优先 `High`/`Main`；
  ③ **单独的参数集包（`BUFFER_FLAG_CODEC_CONFIG`）被当成视频帧发出去** ⇒ PC 侧解码器报
  `AVERROR_INVALIDDATA`；`PREPEND_HEADER_TO_SYNC_FRAMES = 1` 已让每个 IDR 自带参数集
  （协议里本就不需要 CSD 交换）⇒ 该缓冲改为**丢弃**。修正后首帧即关键帧。
  **PC 工程布局已定**（`pc/` workspace = `proto` + `decode` + `app`）并在 `tech-stack/overview.md §5 #1` 记录。
  **待收口**：`transport-messages.md §3.2` 的能力表 / `applyScreen` / 回报仍只有带 `…` 的草图，
  确切 JSON 形状尚未回填（两实现已按同一契约对齐）。
- **2026-10-07 批准**：用户批准本方案，S1 随即开工。**S1 交付与偏差**：
  - 已交付：F6 运行时 + F7 可观测性 + F4 连接层（拓扑 / 握手 / 协议骨架 / 心跳 / 停止），
    实机 19 项验收全过（握手 / 心跳周期 3.00s / CRC 失步断连 / 单路断开不杀进程 /
    `stopAgent` 退出码 0 / 重复实例退出码 2）；
  - **§8 #5 已定**：控制队列深度 **64**（依据写在 `runtime/Route.kt`）；
  - 实施中发现三处与规范不符，**已就地按实测修正**并另行报告：
    ① `java.net.Socket` **没有** `getFileDescriptor()` ⇒ TCP 的判活退化为 socket 读超时
    （`runtime-observability.md §2.1` / `runtime-threads.md §5` 的「两种传输同一实现」对 TCP 不成立）；
    ② `LocalServerSocket.close()` **不会**让阻塞中的 `accept()` 返回 ⇒ 收尾新增 `Transport.wake()`
    （否则每次收尾白等一个 3s join 超时；实测已从 3001ms 降到 101ms）；
    ③ SIGTERM / SIGINT **不触发** shutdown hook（实测退出码 143 / 130）⇒
    `runtime-failure.md §5`「接管 SIGTERM」在 `app_process` 下不成立，可靠停止手段是协议指令 `stopAgent`。
- **2026-10-07 S4 交付（F5 输入注入）**：**F5 转「已定·已实现」**，§4 切片表的 S4 里程碑达成。
  交付内容：新增 `input/` 包 4 文件（`InputCmd` / `InputQueue` / `PointerTracker` / `EventBuilder`）
  + `wrappers/InputInjector` + `runtime/InputLoop` + `proto/Input`；改动 `proto/{ErrorCode,Control}`、
  `runtime/{RouteDeps,Route,ReaderLoop,LinkLoop,Teardown}`、`Main`。协议上 `input` 指令字段 =
  `action` + `pointers[]` / `scroll` / `key`，新增错误码 `noScreen` / `badInput` / `inputDenied` /
  `inputFailed`，`EventMsg` 增加 `subsystem` 字段。线程：`input` 是每路第 **7** 条线程，**每路一条**、
  不与 `slow` 共用；收尾序列里排第 **③** 步（停 input → join），**早于** 第 ④ 步 link（否则可能往已拆的屏
  注入）；连接断开随 `inputStop` 一起停（防泄漏）。
  取法（实体机 Android 16 / SDK 36）：`getSystemService(Context.INPUT_SERVICE)` **零反射**取
  `InputManager`；反射**恰好 3 个成员** —— `InputManager.injectInputEvent(InputEvent,int)`、
  `InputEvent.setDisplayId(int)`、常量 `INJECT_INPUT_EVENT_MODE_ASYNC`；事件本体全走**公开 API**
  （`MotionEvent.obtain` 14 参公开重载、`KeyEvent` 公开构造器）。交叉验证：scrcpy 走同一条路。
  实机验收 **31 项全过**：注入滑动 ⇒ 虚拟屏原始截图 **91.9%** 行变化、滚轮 **91.8%**，
  **本机屏 0.000%**（基线 0.000%）；BACK ⇒ 画面 **100%** 变化且该屏前台由
  `com.android.settings/.MiuiSettings` 变为**无前台**；`input` 线程断开后退出（无泄漏）；
  无屏 ⇒ `noScreen`、坏载荷 ⇒ `badInput`、成功 ⇒ 静默；`stopAgent` 退出码 **0**。
  构建：源文件 **44 个 / 6216 行**（S4 前 37 / 5053），测试 **8 个 / 853 行**（S4 前 6 / 577）；
  **单测 60 项全绿**（新增 18 项），`:agent:assembleRelease` **零警告**。
  实施中的修正：① 真机注入模式常量实为 **`ASYNC=0` / `WAIT_FOR_RESULT=1` / `WAIT_FOR_FINISH=2`**，
  规范原表把 `WAIT_FOR_FINISH` 误记为 `1` ⇒ 已更正为 **2**；② 不可合并事件补充**硬上界 `HARD_LIMIT=64`**，
  防队列无界 ⇒ 内存无界；③ 协议约定**成功注入不回 ack**（高频、静默），只有失败回错误码。
- **2026-10-07 S4 续（PC 侧 F5：输入上报）**：**PC 侧 F5 已实现** —— PC 现在能从窗口采集鼠标 / 键盘并下发
  `input` 指令（agent 侧 S4 已在上一片交付，本片补齐 PC 端把用户操作变成指令的那一环）。
  交付内容：新增 `pc/app/src/input.rs`（输入采样器 + `egui` 键位 → Android keyCode 映射 + **11 项单测**）；
  `pc/proto/src/dto.rs` 新增 `Command::Input` 与 `InputAction` / `InputPointer` / `InputScroll` / `InputKey` 五个类型，
  由 `pc/proto/src/lib.rs` 导出；`pc/proto/tests/protocol.rs` 新增 **2 项契约测试**
  （`input_command_has_the_exact_wire_shape` / `every_input_action_has_its_protocol_spelling`）；
  `pc/app/src/session.rs`（`Handles` 加输入队列，`Status` 加 `input_sent` / `input_rejected` / `input_blocked`，
  `stream_loop` 排空输入并发送、失败回报识别输入被拒、STAT 行带输入计数）；`pc/app/src/ui.rs`（采集鼠标 / 键盘、
  画面矩形自算、失焦取消手势）；`pc/app/src/conn.rs`（`POLL_INTERVAL` **500ms → 8ms**）；`pc/app/src/main.rs`（加 `mod input`）。
  采样与映射规则（依据 [`tech-stack/pc.md`](../spec/tech-stack/pc.md) §4）：`down` / `up` / `key` **立即发**，
  `move` **定频采样**（中间点被定频合并）、`scroll` **累加**；上限 = **屏刷新率**。
  验证：`cargo build --release --workspace` **零告警**；`cargo test --workspace` **33 项全绿**
  （原 20 项协议 + 新增 11 项输入采样 / 映射 + 新增 2 项协议契约）。实机验收（**真实鼠标按键，非脚本发协议**）：
  真实拖动（13 个采样点路径）⇒ PC 侧 STAT `输入 11 条（被拒 0）`（中间点被**定频采样**合并 ✓）、虚拟屏原始截图
  **96.0%** 行变化 ✓；按一次 Escape ⇒ PC 侧 `输入 13 条`（`keyDown` + `keyUp` **立即发** ✓）、该虚拟屏前台由
  `com.android.settings/.MiuiSettings` 变为**（无前台）** ✓；全程**被拒 0**，硬解回读 `nv12` ✓，帧数 **47 → 355**。
  实施决定：① **滚轮符号取负** —— `egui` 的 `scroll_delta` 与 Android `MotionEvent` 的滚轮量**方向相反**，
  映射时必须取负，否则滚动反向；② **失焦必须 cancel** —— 窗口失焦时若已有触点按下，必须补发 `cancel`，
  否则虚拟屏上会**卡住一个按下的触点**；③ **会话线程唯一写者** —— 输入只经会话线程（`stream_loop`）排空并写出，
  采样器只入队，与「每条连接一个写者」一致、无需额外锁。
