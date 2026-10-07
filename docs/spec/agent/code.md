# `:agent` · 代码形态

> 本文件是 [`AGENTS.md`](../../../AGENTS.md) 索引下的规范之一。
> **写 `agent/` 下任何代码之前必须读本文件**（与 [`agent/module.md`](module.md) 并列）。
> 本文件只讲**代码长什么样**；行为语义在各自子系统文档，此处不重复。
> **变更记录记在本文件 §12。**

**状态**：**已定**（2026-10-07，**用户已确认**）。

---

## 1. 为什么需要这份文件

既有规范覆盖了「**做什么**」（F1–F7 行为语义）、「**线程架构**」（[`agent/runtime-threads.md`](runtime-threads.md)）、
「**硬约束**」（[`agent/module.md`](module.md) §3）、「**注释**」（[`comments.md`](../comments.md)），
但**没有一份规定代码形态** ⇒ 直接开写会出现"一个文件一套风格"。

**包划分的唯一依据：与规范文档一一对应** —— 看代码能定位到文档，看文档能定位到包。

---

## 2. 包与文件

### 2.1 包结构（`com.xiaowine.wcreen.agent`）

| 包 | 放什么 | 对应规范 |
|---|---|---|
| （根） | **只有 `Main.kt`** —— 入口与编排 | [`agent/runtime-startup.md`](runtime-startup.md) |
| `runtime/` | 线程骨架、队列、收尾、失败通道、CLI 解析、日志 | [`agent/runtime.md`](runtime.md) + 6 子文档（**F6 + F7**） |
| `display/` | 虚拟屏：flags、建/拆/冻结、Context 引导 | [`agent/display.md`](display.md) + `display-flags` + `display-lifecycle`（**F1**） |
| `encode/` | 编码器：能力探测、参数、重建、零拷贝通路 | [`agent/encode.md`](encode.md) + 6 子文档（**F3**） |
| `proto/` | 线协议：帧头、控制消息 DTO、编解码 | [`agent/transport-protocol.md`](transport-protocol.md) + `transport-messages` |
| `transport/` | 传输接缝 + `AdbTransport` / `TcpTransport`、握手、背压 | [`agent/transport.md`](transport.md) + `transport-topology` / `-handshake` / `-backpressure` |
| `launch/` | 枚举可启动应用、拉起目标应用 | [`agent/launch.md`](launch.md) + `launch-enumerate`（**F2**） |
| `input/` | 触摸 / 按键 / 滚轮注入 | [`agent/input.md`](input.md)（**F5**） |
| `wrappers/` | **唯一允许反射**的包：隐藏 API 与版本差异 | [`agent/module.md`](module.md) §1、§8 |

**纪律**：新增包必须先改本表；一个包只对应上表某一行，**不出现"公共工具包"**。

### 2.2 文件粒度

- **一个顶层类一个文件**，文件名 = 类名；
- 强内聚的小类型（sealed 子类、DTO、枚举）**允许**与主体同文件；
- **不用** `Manager` / `Helper` / `Util` 后缀 —— 命名说清**角色**。

### 2.3 类命名：与线程表**逐行对应**

[`agent/runtime-threads.md`](runtime-threads.md) §2 的线程表就是命名依据，线程体一律 `<角色>Loop`：

| 文档里的线程 | 类 |
|---|---|
| `main` | 无类 —— 就是 `Main.kt` 里的 `Looper.loop()` |
| `accept` | `AcceptLoop` |
| `slow` | `SlowLoop` |
| `reader` | `ReaderLoop` |
| `link` | `LinkLoop` |
| `sender` | `SenderLoop` |
| `input` | `InputLoop` |

**每路**的对象由一个聚合类持有（`Route`），它创建并 join 自己的 `ReaderLoop` / `LinkLoop` / `SenderLoop` / `InputLoop`。

---

## 3. 失败与错误形态

**不用异常做控制流**。异常只在**跨平台边界**（Android API 抛出的）被捕获，并**立即转成**下面的类型。

- **可预期失败**（协议非法、参数非法、能力不支持、建屏失败…）⇒ 返回 **`sealed` 结果类型**；
- **级别随失败对象走**，取值沿用 [`agent/runtime-failure.md`](runtime-failure.md) §1 的四级
  （致命 / 子系统 / 单路 / 可恢复）⇒ **善后由收方按级别决定，不在抛出处决定**；
- **致命只有一条通道**汇到 `main`：`main` 收到 ⇒ 设停止标志 ⇒ 走收尾
  （[`agent/runtime-threads.md`](runtime-threads.md) §8）⇒ `System.exit`；
- **禁止** `runCatching{}` 吞掉异常后继续 —— 要么转成**带级别的失败对象**，要么让它成为**致命**。

> 具体类名与形状在第一片（S1）定稿，落定后回填本节。

### 3.1 形状（**S1 已定稿**，落在 `runtime/Failure.kt`）

```kotlin
/** 四级，与 runtime-failure.md §1 一致。 */
enum class FailLevel { FATAL, SUBSYSTEM, STREAM, RECOVERABLE }

/** 一次失败的描述：级别 + 机器可读的码 + 给人看的说明。 */
class Fail(val level: FailLevel, val code: String, val message: String, val cause: Throwable? = null)

/** 操作结果。 */
sealed class Outcome<out T> {
    class Ok<out T>(val value: T) : Outcome<T>()
    class Err(val fail: Fail) : Outcome<Nothing>()
}
```

- 取值一律用扩展函数 `valueOrNull()` / `failOrNull()`，不在调用处写 `when` 分支；
- `code` **分两处登记**：会出现在协议里的在 `proto/ErrorCode.kt`（PC 靠它分支），纯进程内的在
  `runtime/FailCode.kt`（只是日志里的定位线索）；退出码在 `runtime/ExitCode`（`0`/`1`/`2`）。

---

## 4. 并发原语

| 场景 | 用什么 |
|---|---|
| 有界队列（跨线程传消息 / 帧） | `ArrayBlockingQueue` + **`offer()` 入队** + **`poll(timeout)` 出队** |
| `main` ↔ 其它线程 | `main` 的 `Handler.post`（[`agent/runtime-threads.md`](runtime-threads.md) §1 规则 1） |
| **非 `main` 线程之间** | **只能用队列** —— 全进程只有一个 Looper |

- **入队一律非阻塞**（`offer` 失败 = 满 = 由 owner 决定丢弃）⇒ 消除死锁（同文档 §6）；
- **不加业务锁**（同文档 §1 规则 1）；
- 队列**容量必须是显式常量**并在注释里写依据（实测 / 协议规定），不写魔法数字。

---

## 5. 协议 DTO

- `data class`，**字段名与协议字段逐字一致**；
- **每个字段必须有默认值** —— Kotlin 在**主构造器参数全部有默认值**时会生成**无参构造**，
  Gson 于是走构造器路径 ⇒ **默认值生效、非空类型安全**。
  反之（有字段无默认值）Gson 会用 `Unsafe` 绕过构造器，非空字段在运行期就是 `null` ⇒ 迟早 NPE；
- 字段用 `var`（Gson 靠反射赋值）；
- 全部 DTO **必须进 `-keep`**（[`agent/module.md`](module.md) §4）—— R8 看不见反射访问。

---

## 6. 装配与可见性

- **`main()` 手工装配**：无框架、无 DI 容器、**无全局可变单例**；依赖走构造参数；
- 只有**不可变常量与纯函数**可以是顶层声明或 `object`；
- 可见性：**默认 `private`**；确实跨包才 `internal`；
  `public` 只给**入口类 `Main`**（`app_process` 按类名字符串加载，[`agent/module.md`](module.md) §4）。

---

## 7. 日志

- 统一走自建 `Ln`（在 `runtime/`）：**自动附线程名**（[`agent/runtime-threads.md`](runtime-threads.md) §1 规则 5）、
  等级运行期可改（启动期由 `--log`，连上后由 PC 配置）；
- **不直接调 `android.util.Log`** —— 否则既拿不到线程名、也改不了等级；
- **两条通道不要混**（[`agent/runtime-failure.md`](runtime-failure.md) §3）：
  **PC 要的走协议，给人看的走 logcat**。

---

## 8. 可测性分层（**架构约束，不是风格**）

[`tech-stack/overview.md`](../tech-stack/overview.md) §4 要求纯逻辑有 JVM 单测
⇒ **下列代码不得引用任何 Android 类型**，否则根本进不了 `agent/src/test/`：

- `proto/` 的编解码与 DTO；
- `runtime/` 的 CLI 解析；
- `display/` 的 **flag 位运算与组合**（**不含**任何读 `DisplayInfo` 的部分 —— 运行期**不读**它，
  见 [`agent/code-hiddenapi.md`](code-hiddenapi.md) §3）；
- 生命周期状态机。

⇒ **凡"能纯算"的部分都要从 Android 依赖里切出来**；切不动，说明分层错了。

---

## 9. 「不依赖默认值」的落地形态

[`workflow.md`](../workflow.md) §4 是**全局规则**，落到代码形态上：

- **每个参数设置点紧跟一次回读 + 校验**：读到的不等于写入的 ⇒ 按**致命**处理；
- 做成**统一小工具**（`setAndVerify(...)` 形态）供各子系统调用 —— 靠自觉必然漏；
- 参数值一律**显式常量**，并在注释里写**依据**（实测 / 协议规定 / 系统限制）。

---

## 10. `wrappers/` 形态（隐藏 API 的唯一出口）

**唯一允许出现隐藏 API 的包** —— 展开见 [`agent/code-hiddenapi.md`](code-hiddenapi.md)。

- **尽可能不用反射**：能用公开 API 就用公开 API；非用不可的**只在本包出现**；
- **一个隐藏 API 域一个对象**；对外只暴露普通 Kotlin 类型，不泄露 `Method` / `Object` / `Class`；
- **版本差异在此收敛** —— 调用方看不到分支（[`agent/module.md`](module.md) §8）；
- **运行期反射的 6 处清单、四条纪律、为什么不用「补全 jar」** ⇒ 全部在
  [`agent/code-hiddenapi.md`](code-hiddenapi.md)。

---

## 11. 与既有文档的关系

[`docs/code-map.md`](../../code-map.md) §4 的 `:agent` 文件清单原为**原型时代遗留**（`Streamer.kt`
的单路思维、`Ln.kt` 的"双通道日志"都与现行规范冲突）；**S1 落地时已按 §2 重写该表**，作废声明撤销。

---

## 12. 变更记录

- **2026-10-07 草案**：建立代码形态规范 —— 包划分**与规范文档一一对应**（§2.1）；
  线程体命名 `<角色>Loop` 与线程表逐行对应（§2.3）；失败用**带级别的 sealed 结果类型**、致命汇到 `main`
  一条通道（§3）；并发原语定死 `ArrayBlockingQueue` + `offer`/`poll(timeout)`、非 `main` 线程间只用队列（§4）；
  DTO **全字段带默认值**以走 Gson 构造器路径（§5）；`main()` 手工装配、无全局可变单例（§6）；
  **可测性分层**（§8）与「不依赖默认值」的落地形态（§9）；声明 `code-map.md §4` 的 `:agent` 清单作废（§11）。
- **2026-10-07 §10 展开**：隐藏 API 的访问形态**提成独立文件**
  [`agent/code-hiddenapi.md`](code-hiddenapi.md) —— 原则「**尽可能不用反射**」、走公开 API 的 6 个域、
  **运行期反射只保留 6 处**（其中第 6 处可降级）、反射四条纪律、以及**否决「补全 jar」**的四条实测依据。
  本节改为指针；§8 的 `DisplayInfo` 注记同步（运行期不读）。
- **2026-10-07 确认**：用户确认全部条目 ⇒ 状态转「**已定**」。唯一保留的实施期待定项：**§3 失败形态的具体类名与形状**，由 S1 定稿后回填。
- **2026-10-07 S1 回填**：§3 补 §3.1「形状」（`FailLevel` / `Fail` / `Outcome` 落在 `runtime/Failure.kt`）；§11 的「作废声明」撤销 —— `code-map.md §4` 已按 §2 重写。
