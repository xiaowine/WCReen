# `:agent` 模块规范

> 本文件是 [`AGENTS.md`](../../../AGENTS.md) 索引下的规范之一。
> **改动 `agent/` 下任何内容之前必须读本文件。**

---

## 1. 模块定位

独立 jar，跑在 `app_process` 里，进程身份是 **shell（uid 2000）**。

- **不是可安装的 App**：产物是 APK（内含 `classes.dex`），改名 `.jar` 后由 `app_process` 加载执行。
- **不依赖 App 进程，甚至不依赖 App 已安装**：杀 App / `force-stop` / Doze 都不影响它。
- **TRUSTED 只能由系统或 shell 设置**：普通 App 设该 flag 会被 `DisplayManagerService` 拒绝，
  且非系统拥有的虚拟屏按 untrusted 处理，其上的 Activity 受 `allowEmbedded` 限制。
  这就是 `app_process` + uid 2000 这条路线的全部理由。
- **屏挂在虚拟设备上**（[`agent/display.md`](display.md) §3）：因此启动时还需要一条
  **self-managed 的 CDM 关联**（包名 `com.android.shell`，profile `…APP_STREAMING`）——
  agent 启动自检、缺则自建（`display/CompanionAssociation.kt`，走 `cmd companiondevice`，**不反射**）；
  用户可在系统设置里撤销 ⇒ 下次启动重建（幂等）。

| 项 | 值 | 位置 |
|---|---|---|
| 源码根 | `agent/src/main/kotlin/com/xiaowine/wcreen/agent/` | — |
| 隐藏 API 封装 | `agent/src/main/kotlin/com/xiaowine/wcreen/agent/wrappers/` | — |
| 设备端脚本 | `agent/scripts/` | — |
| Java 包名 | `com.xiaowine.wcreen.agent` | `agent/build.gradle.kts` |
| minSdk / targetSdk / compileSdk | 34 / 37 / 37 | 同上 |
| Java 级别 | 21 | `compileOptions` |

---

## 2. 子系统划分与推进顺序

`:agent` 内部按子系统拆分，**一个子系统一个文档**（见 [`docs.md`](../docs.md)）。
下表只做**结构映射**（子系统 → 文档）；**功能级状态**（哪个功能已定、哪个后置、哪个不做）
与推进顺序统一在 [`agent/features.md`](features.md) 维护 —— 本文件**不重复状态与顺序**，避免两处漂移。

| # | 子系统 | 边界（只讲这些） | 文档 |
|---|---|---|---|
| 1 | **运行时** | 启动方式（脱离 adb）、`main()` 编排顺序、进程与线程模型、退出与收尾、错误策略、**日志与心跳**（可观测性） | [`agent/runtime.md`](runtime.md)（入口，**F6 + F7**）及其 6 个子文档 |
| 2 | **虚拟屏与编码** | 建屏 flags / 尺寸 / DPI、damage 语义（含"静止无帧"）、MediaCodec 配置与能力探测、零拷贝通路 | [`agent/display.md`](display.md) + [`agent/display-lifecycle.md`](display-lifecycle.md) + **编码（F3）**：[`agent/encode.md`](encode.md)（入口）及其 6 个子文档 |
| 3 | **传输** | 连接生命周期、协议（握手 / 解码配置 / 控制消息）、背压与重连 | [`agent/transport.md`](transport.md)（入口）+ [`agent/transport-topology.md`](transport-topology.md)（拓扑与多路结构，**已定**） |
| 4 | **输入与拉起** | 触摸 / 按键 / 滚轮注入与坐标系；拉起 App 与失败分类 | [`agent/launch.md`](launch.md) + [`agent/input.md`](input.md)（**F5**，已定） |

**边界纪律**：

- 本文件只放**定位、子系统地图、约束、构建部署、契约索引、验证关注点、禁忌**，
  **不放子系统的行为细节**。
- 子系统文档之间**不复制内容**；跨界处只写「见 X 文档 §Y」。
- 子系统文档定稿后，该子系统的细节**以它为准**；本文件的约束与禁忌**全模块有效**。

---

## 3. 硬约束

> 违反即跑不起来 —— 不是风格问题。**「依据」列**区分来源：
>
> - `平台语义`：由 Android 平台 / 工具链本身决定，与具体实现无关；
> - `原型观测`：原型时代的实机结论，**重构后必须重新验证**（结论翻转或数值不同都很正常，禁止照抄）；
> - `本机实测`：本仓库真机验证过、可复现（见 [`agent/display.md`](display.md) §5、[`agent/launch-verified.md`](launch-verified.md)）。
>
> 任何一条在重构验证中不成立，都要回到 [`workflow.md`](../workflow.md) 重新讨论方案。
> 原型背景见 [`docs/prototype.md`](../../prototype.md)。

| # | 约束 | 依据 | 说明 |
|---|---|---|---|
| 1 | 建屏进程必须是 **uid 2000**（或系统身份） | 平台语义 | 非系统拥有的虚拟屏按 untrusted 处理，其上的 Activity 受 `allowEmbedded` 限制；`TRUSTED` 只有系统或 shell 能设 |
| 2 | 虚拟屏 flag 必须包含 `TRUSTED` 位 | 平台语义 | 完整 flag 组合**待重新测定**，不要照抄旧数值 |
| 3 | 虚拟屏输出 Surface 就是编码器输入 Surface，且 `createInputSurface()` → `createVirtualDisplay()` → 编码器 `start()` 的顺序不可换 | 平台语义 | 零拷贝通路；顺序具体约束需在重构中复现确认 |
| 4 | 接收端拿到解码配置（SPS/PPS 等）之前，不能只发普通帧 | 平台语义 | 没有配置信息解不出帧；配置的发送时机与方式需自行设计 |
| 5 | 控制消息 handler 必须在**建立连接之前**就绪 | 原型观测 | 读线程在 `accept()` 返回那刻就开跑，晚挂一步先到的消息会被静默丢弃 |
| 6 | 实时流不得被输出缓冲扣住 | 平台语义 | 缓冲未刷新会把实时流卡死；具体缓冲策略自行设计并验证 |
| 7 | 静止画面下不会有新帧；**主动刷帧手段已找到** | 平台语义 + **本机实测** | 虚拟屏由 damage 驱动合成 ⇒ 静止无新帧；**换输出面**会触发一次渲染 ⇒ 静止态可解码性有解（见 [`agent/encode-lifecycle.md`](encode-lifecycle.md) §1） |
| 8 | 客户端第一帧必须是 I 帧 | 平台语义 | 无参考帧则解不出；取关键帧的时机与手段自行设计 |
| 9 | 建屏前置条件**已实测锁定**：Context 引导三条硬要求；`DESTROY_CONTENT_ON_REMOVAL` 为**固定必须位** | 本机实测 | 违反前者 ⇒ 以"看不懂的异常"失败；违反后者 ⇒ **拆屏或崩溃会把内容甩到本机屏**。详见 [`agent/display.md`](display.md) §2.2、[`agent/display-flags.md`](display-flags.md) §6.3 |

**另外三条容易反着做的**：

- `app_process` 的 `--nice-name` 这类参数必须放在类名分隔符 `/` **之后**；放错会**静默失败**
  （进程起来、退出码 0、零输出）。`原型观测`，重构时以实际参数解析行为为准。
- **不要用 `MediaProjection`**（`平台语义`）：全局单例且只能捕默认屏，与独立虚拟屏互斥。
- **不要指望用「切电源」强制虚拟屏重绘**（`原型观测`）：虚拟屏没有电源控制器，相应接口调用返回失败。

---

## 4. 依赖与构建约束

来自 `agent/build.gradle.kts`，**改动需先讨论**：

| 约束 | 原因 |
|---|---|
| **默认零第三方依赖**（唯一例外：**Gson**） | 每引入一个库都要先讨论并说明必要性（见 [`tech-stack/overview.md`](../tech-stack/overview.md) §3）。**androidx 及任何需要 Application context / Android 组件初始化的库一律不可用** —— agent 跑在 `app_process` 的 shell 进程里，没有真正的 Application，引入会在运行时炸掉 |
| **Gson**（**唯一例外**） | 协议序列化。**已实测**在 `app_process` 可用（含**反射 DTO**）；代价 **dex +约 200 KB**。**两条配套**：① `rules.keep` 必须 **`-keep` 全部协议 DTO** —— Gson 靠反射访问字段，R8 看不见会删掉；② **必须保持单 dex** —— `app_process` 只加载 `classes.dex`，**不能 multidex** |
| `kotlin-stdlib` 不可剔除 | AGP 内置 Kotlin 支持的运行时必需品（Intrinsics / Unit / 集合类） |
| R8 裁剪开启 + `rules.keep` 里 **`-dontobfuscate`** | `app_process` 按**类名字符串**定位入口，改名必然 `ClassNotFoundException`；关掉混淆后 R8 仍能安全裁剪与优化（具体体积以本仓库构建实测为准） |
| `keepRules/rules.keep` 的入口 `-keepclasseswithmembers` | R8 看不到调用方，不 keep 会把 `main` 当死代码删除 |
| `AndroidManifest.xml` 内容只有 `<manifest />` | 从不安装，manifest 里的任何声明都不生效；AGP 只要求文件存在 |
| `lint` 关掉 API 检查 | 大量隐藏 API 与高版本 API 调用，检查没有意义 |

---

## 5. 构建与部署

```bash
# 构建（产物是 APK，改名 .jar 直接用）
./gradlew :agent:assembleRelease
# → agent/build/outputs/apk/release/agent-release-unsigned.apk

# 部署（路径属对外契约；日志走 logcat，**不落文件**）
adb push agent/build/outputs/apk/release/agent-release-unsigned.apk /data/local/tmp/wcreen-agent.jar
adb push agent/scripts/run-agent.sh /data/local/tmp/run-agent.sh
# 启动与接入：见 agent/runtime.md §2.1（脱离启动）、runtime-startup.md §1.1（CLI）
```

---

## 6. 对外契约（改了就废掉工具链）

下列契约**变更必须先讨论**（[`workflow.md`](../workflow.md)），并同步 PC 端实现。

| 契约 | 内容 | 状态 |
|---|---|---|
| 线协议 | 见 [`agent/transport.md`](transport.md) 及其子文档 | **已定** |
| 命令行 | 见 [`agent/runtime-startup.md`](runtime-startup.md) §1.1 | **已定** |
| 部署路径 | **`/data/local/tmp/`**：`wcreen-agent.jar`（产物）+ `run-agent.sh`（启动脚本）。**无日志文件** —— 日志走 logcat（[`agent/runtime-observability.md`](runtime-observability.md) §3） | **已定** |
| ~~控制脚本格式~~ | **撤销** —— 控制通道由**协议**承担（8 条指令），`run-agent.sh` 只负责**启动** | **撤销** |

---

## 7. 验证（须重新设计）

**原型的验证命令、日志格式与期望数值一律不适用**（见 [`docs/prototype.md`](../../prototype.md)）。
每个模块实施时按 [`workflow.md`](../workflow.md) 重新设计验证方式；未验证的结论不得写成约束。

纯逻辑部分（协议编解码、参数解析、flag 位运算等）必须有 JVM 单元测试，见
[`tech-stack/overview.md` §4](../tech-stack/overview.md)。设备相关行为只能实机验证 —— 单测通过 ≠ 功能可用。

可复用的只是**分层定位思路**（自上而下：先确认上游没问题，再看下游）：

1. 屏建起来了吗（能从系统侧查到这块虚拟屏）；
2. 编码器在出帧吗（有单调递增的出帧计数或等价可观测证据）；
3. 传输在发吗（发送侧计数与编码侧同步增长）；
4. 内容真的落在虚拟屏上吗（在虚拟屏上起一个 App，确认它归属该屏）；
5. 输入通了吗（注入成功计数与画面变化对应）。

**必须验证的关注点**（对应 §3 不变量；具体命令与期望值在方案定稿时填写）：

| # | 关注点 | 对应不变量 |
|---|---|---|
| 1 | 连接建立后控制通道立刻可用，首条消息不丢 | 5 |
| 2 | 实时流不被输出缓冲扣住 | 6 |
| 3 | 接收端在收到视频帧之前已获得解码配置 | 4 |
| 4 | 客户端接入后立即可见画面（首帧可独立解码） | 8 |
| 5 | 虚拟屏属性（含 flag 位）与设计一致 | 1、2 |
| 6 | 构建产物入口未被改名，可按类名加载 | §4 构建约束 |
| 7 | 零拷贝通路成立：帧数据不经过 agent 用户态内存 | 3 |

---

## 8. 禁忌清单

- ❌ 引入 androidx 或任何需要 Application context 的库；
- ❌ 关闭 `-dontobfuscate`，或改动入口 `-keepclasseswithmembers`；
- ❌ 在 manifest 里声明权限 / 组件（不生效，只会误导）；
- ❌ 用 `MediaProjection` 替代虚拟屏；
- ❌ 在 `configure()` 之前不探测编码器能力就假定尺寸可用（原型观测：失败时抛的是**空的** `IllegalArgumentException`，看不出原因；重构时以自己的实测为准）；
- ❌ 把反射调用散在各处 —— 版本差异集中到 `wrappers/`；
- ❌ 新增文件后不更新 [`docs/code-map.md`](../../code-map.md)。
- ❌ **依赖任何"默认值"** —— 设置参数必须显式写出取值并回读确认（**全局规则**，见
  [`workflow.md`](../workflow.md) §4）。实测踩过：色彩字段默认值与真值不符、
  `prepend-sps-pps-to-idr-frames` 默认 0、`color-standard` 默认 BT.601
  （见 [`agent/encode-format.md`](encode-format.md) §2）。

---

## 9. 变更记录

**见 [`agent/module-changelog.md`](module-changelog.md)**（本记录因超出单文件上限已提成独立文件）。
