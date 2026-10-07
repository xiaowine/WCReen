# 技术栈

> 本文件是 [`AGENTS.md`](../../../AGENTS.md) 索引下的规范之一。
> 记录**已定的技术选择**、构建链、依赖与测试政策，以及**尚未决定**的条目。
> 变更方式：先讨论 → 在 [`tech-stack/changelog.md`](changelog.md) 追加一条并说明理由；**不删旧结论**。

---

## 1. 已定：三个部分

| 部分 | 语言 / 框架 | 形态与职责 | 状态 |
|---|---|---|---|
| `:agent` | **Kotlin / JVM** | APK 改名 `.jar`，由 `app_process` 加载；跑在 shell（uid 2000）；建虚拟屏、编码、传输、输入注入 | **S1 已实现**（运行时 + 可观测性 + 连接层）；其余按 [`plans/2026-10-07-agent-slicing.md`](../../plans/2026-10-07-agent-slicing.md) 分片推进 |
| `:app` | **Kotlin / JVM**（与 `:agent` 同工具链与版本） | 可安装 App；占位无源码。后续职责：**自激活、无线配置**、推送与拉起 agent、看门狗 | 占位 |
| PC 端 | **Rust**（UI：**egui** + **wgpu**；解码：**FFmpeg**） | **跨平台**桌面应用（Windows / macOS / Linux）；**多路合流**：同时接收多个源（多台手机 / 多块虚拟屏）并在同一客户端渲染、分发输入 | 未开始 |

### 为什么不共用一套语言

`:agent` 与 `:app` 同语言（Kotlin）是因为它们共用 Android 工具链与 SDK；
PC 端选 Rust 是要拿**最低延迟与可控资源占用**（含硬解与零拷贝路径），
且与移动端没有可共享的代码面（协议层按 §5 的传输无关原则各自实现）。


### PC 端：细节见独立文件

PC 端内容较多，按 [`docs.md`](../docs.md) 的单文件上限拆成两份：
[`tech-stack/pc.md`](pc.md)（跨平台后端 / 帧通路与硬解强校验 / 容量 / FFmpeg 接入）与
[`tech-stack/pc-ui.md`](pc-ui.md)（UI 层选型 / 否决 GPUI 的依据 / 被否方案）。

一句话摘要：PC 端 = **Rust + egui + wgpu + FFmpeg（动态链接、随包分发）**，
跨平台（Windows / macOS / Linux），**多路合流**，**只做硬解、不做软解回退**
（靠"逐路首帧校验帧是硬件帧"强制），容量**由运行平台硬件决定**且**不做任何容量处理**（[`pc.md`](pc.md) §3）。

---

## 2. 构建链（本仓库实测）

| 项 | 值 | 说明 |
|---|---|---|
| Gradle | **9.5.0**（wrapper） | `gradle/wrapper/gradle-wrapper.properties` |
| AGP | **9.3.3** | `gradle/libs.versions.toml` |
| Kotlin | **2.2.10**（AGP 内置） | AGP 9.3.3 的 POM 依赖 `kotlin-gradle-plugin:2.2.10`；本仓库**不单独声明** Kotlin 版本 |
| compileSdk / targetSdk | **37** | 本机已装 `platforms/android-37.0`、`build-tools/36.0.0` |
| minSdk | **34** | 两模块一致 |
| Java 级别 | **21** | 两模块 `compileOptions` 一致 |
| Gradle Daemon JVM | JetBrains **JDK 25**（`D:\Android\runtime\jbr`） | `gradlew :agent:buildEnvironment` |
| Gradle user home | `D:\gradle` | 缓存与 provisioned JDK 都在这里 |

**本机已知坑**：

- `JAVA_HOME` **未设置**，`gradlew` 起不来；需显式指定 JDK（`D:\Android\runtime\jbr`
  或 `D:\gradle\jdks\eclipse_adoptium-21-amd64-windows.2`）。
- 本机 shell 下 `./gradlew`（POSIX 脚本）不可用，用 **`gradlew.bat`**。

---

## 3. 依赖政策

- **默认零第三方依赖**。每引入一个库都必须先讨论，并说明：必要性、体积影响、
  在 `app_process` 环境下的初始化要求、R8 配置影响；结论记入 §6 变更记录。
- **硬禁**：任何需要 Android 组件初始化 / Application context 的库（androidx 及同类）
  在 `:agent` 中不可用 —— 该进程没有真正的 Application，引入会在运行时炸掉。
- **Kotlin 版本跟随 AGP 内置版本**，不单独声明，避免版本分叉。
- PC 端同理：优先标准库与已定框架；**FFmpeg 是唯一的重量级外部依赖**。

---

## 4. 测试政策

- 引入 **JVM 单元测试，只覆盖纯逻辑**：协议编解码、参数解析、flag 位运算、状态机等。
  这些逻辑不依赖 Android 运行时，可在 JVM 上直接测。
- 单测依赖属 `testImplementation`，**不进产物**（用 Kotlin 标准测试库 + JUnit）。
- **不引入** Android instrumentation 测试：agent 跑在 `app_process`，无法常规测试。
- 设备相关行为**只能实机验证**；单测通过 ≠ 功能可用，实机验证仍是硬要求。

---

## 5. 未决项

> **已收口的条目留在表里**（标结论 + 指向定稿文档），编号不变以保历史引用有效。

| # | 未决项 | 状态 |
|---|---|---|
| 1 | PC 端 Rust 工程布局 | **已定**（2026-10-07，S2 开工时定）—— `pc/` 作 **workspace**，三个产品 crate：`pc/proto`（线协议，**纯逻辑可单测**）/ `pc/decode`（FFmpeg 硬解 + 帧通路，**unsafe 隔离在此**）/ `pc/app`（egui + wgpu + 编排）；`pc/spike` 保留作历史证据 |
| 2 | egui / wgpu 版本锁定 | **待定** —— 具体版本与升级策略（风险远低于原 GPUI 分叉问题） |
| 3 | FFmpeg dev 包获取方式 | **部分** —— **Windows 已获取并验证**：BtbN `ffmpeg-n9.0-latest-win64-lgpl-shared-9.0.zip`（**LGPL v3** + shared；含头文件 / MSVC 导入库 / DLL），**最小程序链接 + 运行通过**（见 [`plans/2026-10-07-pc-spike-implementation.md`](../../plans/2026-10-07-pc-spike-implementation.md) §2）。**Linux / macOS 仍未定** |
| 4 | 容量处理 | **已定（不做）** ⇒ [`pc.md`](pc.md) §3：**不做任何容量处理**（无上限 / 无准入 / 无降级 / 不踢已有路）。依据：实测加路**从不失败**，只有 fps 劣化 ⇒ 不存在可判定的"超容量"状态（[报告](../../plans/2026-10-07-pc-spike-report.md) §5） |
| 5 | PC spike 范围与验收 | **已批准** ⇒ [`docs/plans/2026-10-06-pc-spike.md`](../../plans/2026-10-06-pc-spike.md)（§9 批准记录含批准范围） |
| 6 | 编码参数与硬解能力对齐 | **已定** ⇒ 由 #7 的**能力协商**承担；agent 侧格式组规则见 [`agent/encode-format.md`](../agent/encode-format.md)（**绝不退软编**） |
| 7 | **能力协商（必需）** | **已定** ⇒ [`agent/transport-handshake.md`](../agent/transport-handshake.md)：**握手 = 版本校验 + 能力表协商**；**交集由 PC 算**（只有它知道自己的解码器）、**agent 只做可行性否决**；版本**精确匹配** |
| 8 | 协议格式 | **已定** ⇒ [`agent/transport-protocol.md`](../agent/transport-protocol.md) + [`agent/transport-messages.md`](../agent/transport-messages.md)：**传输无关性已落地**（`[类型][长度][负载]`、控制消息 CRC32 + JSON、**不做分片**、小端） |
| 9 | 传输通道 | **部分** —— **已定**：**一个监听地址**、**agent 监听 / PC 主动连**、**重复实例 = 监听地址被占用**（[`agent/transport-topology.md`](../agent/transport-topology.md) §6）；**仍待定**：**多传输并存**、**网络侧认证** |

---

## 6. 变更记录

**见 [`tech-stack/changelog.md`](changelog.md)**（本记录因超出单文件上限已提成独立文件）。
