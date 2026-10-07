# `:agent` · 可观测性（F7）

> 本文件是 [`AGENTS.md`](../../../AGENTS.md) 索引下的规范之一，属 [`agent/runtime.md`](runtime.md)（**F7**）。
> 心跳的线程载体见 [`agent/runtime-threads.md`](runtime-threads.md) §7；错误分级的对外表现见
> [`agent/runtime-failure.md`](runtime-failure.md) §3。
> **变更记录记在本文件 §4。**

**状态**：**已定**（心跳 §2 + 日志 §3）。

---

## 1. 判活看心跳，不看帧（已定）

虚拟屏静止时**根本不出帧** ⇒ PC 会把"长时间无帧"误判成断流
（依据 [`agent/encode-lifecycle.md`](encode-lifecycle.md) §1）。
⇒ **保活（心跳）与帧解耦**。

## 2. 心跳（已定）

| 项 | 决定 |
|---|---|
| **周期** | **固定 3s**（agent → PC） |
| **消息形态** | **独立消息，不承载任何协议内容** —— agent 主动上报（如"屏已释放"）走**独立消息**，不搭心跳的便车 |
| **载体** | agent→PC：`main` 的 `Handler.postDelayed` → 投 `sender`；PC→agent：`reader` 的**读超时**（adb 走 `Os.poll`，tcp 走 `setSoTimeout`；[`agent/runtime-threads.md`](runtime-threads.md) §7） |
| **判据** | **`T_dead` 内没有任何消息 ⇒ 判 PC 掉线**（机制按传输分两种，见 §2.1） |

### 2.1 超时判据（已定）

**判据**：**`T_dead` 内没有收到 PC 的任意消息** ⇒ 判死。
**不是**"没收到心跳" —— PC 忙碌时**不必额外发心跳**，任何消息都算活着。

**机制按传输分两种**（**判据相同，谁在计时不同**）：

| 传输 | 机制 | 为什么 |
|---|---|---|
| adb（`LocalSocket`） | **`Os.poll(fd, T_dead)`**，返回 0 即判死 | `LocalSocket` 有 `getFileDescriptor()` |
| tcp（`java.net.Socket`） | **`setSoTimeout(T_dead)` + 读**，读超时抛 `SocketTimeoutException` 即判死 | `Socket` **没有** `getFileDescriptor()` |

```java
// adb 通道
StructPollfd[] p = { new StructPollfd() };
p[0].fd = conn.getFileDescriptor();
p[0].events = (short) OsConstants.POLLIN;
while (running) {
    if (Os.poll(p, 10_000) > 0) route(read());   // 有数据 ⇒ 活着
    else { onPcDead(); break; }                  // 10s 零字节 ⇒ 判死
}
```

`Os.poll` 是**公开 API**（`android.system`）。**已实测（`LocalSocket`）**：
无数据 ⇒ 返回 0、耗时 **701ms**（真阻塞，非忙轮询）；有数据 ⇒ 返回 1、耗时 **0ms**、`revents=POLLIN`。

⚠️ **`java.net.Socket` 没有 `getFileDescriptor()`** —— 已用 SDK 的 `android.jar` 核对
（`javap java.net.Socket` 只有 `getChannel` / `setSoTimeout` / `shutdown*` / `isInputShutdown` …）。
⇒ TCP 通道退化为 **`setSoTimeout(T_dead)` + 读**：超时抛的是**标准** `SocketTimeoutException`，
在 `Wire.readFrame` 里被翻成同一个「判对端掉线」失败。

⚠️ **adb 通道不用 `setSoTimeout`**：实测 `LocalSocket` 下它**有效**（808ms）但抛的是
**`IOException("Try again")`**（未文档化的 errno 文案），**不是 `SocketTimeoutException`** ——
靠文案匹配属于"依赖未文档化行为"（[`workflow.md`](../workflow.md) §4 全局规则）。
**这一条只针对 `LocalSocket`**，不影响 TCP 通道用 `setSoTimeout`。

`poll` 的超时**就是判据本身** ⇒ **不需要 `lastRecv` 记账、不需要额外定时器** ——
这正是心跳能"零新线程"的原因（[`agent/runtime-threads.md`](runtime-threads.md) §7）。

**判死后的动作（三步，都不是拆屏）**：

1. **冻结** `setSurface(null)`（屏停输出、**App 状态保住**）；
2. **断开当前连接**（`close()`）→ 回 `accept()` 等重连；
3. `main` 的**接入失败计数 +1** ⇒ **只有连续 `N` 次（缺省 5）才拆屏**。

**为什么 `T_dead = 3 × 周期 = 10s`**：

| 理由 | 说明 |
|---|---|
| **判据与实现同义** | 超时**就是判据本身** ⇒ 零记账（adb：`poll` 返回 0；tcp：读抛超时） |
| **误判代价低** | 误判只导致**冻结**（不丢数据），PC 恢复后重连即解冻（可反复冻结 / 解冻**已实测**，[`agent/display-lifecycle.md`](display-lifecycle.md) §3.1） |
| **单次误判不会拆屏** | 拆屏要**连续 `N = 5` 次**失败 ⇒ 对误判有**天然韧性** |
| **它还兜住「PC 不读」** | PC 挂死时 `sender` 会卡在 `write`；`reader` 判死后的收尾 `shutdownOutput()` **唤醒它** ⇒ **不必再加写看门狗**（[`agent/transport-backpressure.md`](transport-backpressure.md) §1.1）⇒ 所以它**必须够短** |

⇒ **不按传输参数化**（3s 对 adb 亚毫秒 RTT 与 Wi-Fi 毫秒级 + 抖动**都安全**）。
⚠️ **`T_dead` 与周期必须一起看** —— 它必须 **> 周期**，否则正常的心跳间隔本身就会被判死。

**边界情况**：

| # | 情况 | 处理 |
|---|---|---|
| 1 | **握手期**（连上但还没握手） | **沿用 `T_dead`** —— 两者都是"连上了但收不到消息"，语义与处置一致 ✓ |
| 2 | **反向**（agent→PC 心跳**写**失败） | **不单独处理** —— 写失败就是"连接坏了" ⇒ 走断开路径 |
| 3 | **PC 侧的对等判据** | PC 用同样的 **10s**（无 agent 消息 ⇒ 判 agent 死）；**心跳周期是协议常量**，须写进协议约定 |

**明确不做**：**`SO_KEEPALIVE`** —— 默认 **2 小时**才探测，比 3s 的应用层心跳**慢 2400 倍**，无用。

---

## 3. 日志（已定）

### 3.1 通道 = **复用 adb logcat**（`android.util.Log`）

**不自己造通道、不落盘、不依赖 stdout。**

**实测（本机实体机 / Android 16 / uid 2000 / 含脱离启动）**：

| 结论 | 证据 |
|---|---|
| ✅ **能写** | 5 个等级全部出现在 `logcat -d -s <TAG>` |
| ✅ **脱离启动也能写** | stdout 被丢弃**不影响** —— `Log` 走 logd socket，**不走 stdout** |
| ✅ **任意线程能写** | 子线程写成功（tid 不同） |
| ✅ **不阻塞** | 连写 20000 条 **59ms** ⇒ **约 34 万条/秒** |
| ⚠️ **`System.out` 不进 logcat** | `System.out.println` 只到 stdout ⇒ **必须显式用 `android.util.Log`** |
| ⚠️ **默认 `isLoggable(V/D) = false`** | 实测 V/D **仍**进了 logcat，但该行为**不保证** ⇒ 需要 DEBUG 级时用 **`setprop log.tag.<TAG> <LEVEL>`** 显式打开（实测设 `VERBOSE` 后 V/D/I/W/E 全 `true`） |
| ⚠️ **不能当档案** | 连写 20000 条只留下 **5836** 条（`main` 环形缓冲 2 MiB，logd 侧有丢弃 / 滚动） |

### 3.2 三项待定**全部收口**

| # | 原待定项 | 结论 |
|---|---|---|
| 1 | **连接建立之前的日志** | **天然解决** —— logcat 与进程 / 连接**无关**。**不需要**环形缓冲、**不需要**改启动脚本重定向 |
| 2 | **等级 + 限流 + 丢弃策略** | **等级用 Android 原生**（`V/D/I/W/E`；运行期用 `setprop log.tag.<TAG>` 改，**零代码**）；**限流与丢弃交给 logd**（不自己造） |
| 3 | **是否落盘** | **不做**；需要留存时由 **PC 侧 `adb logcat -f`** 负责（agent 不管） |

### 3.3 规则与残留缺口

- **Tag 固定**（如 `WCReen`）—— 便于 `logcat -s WCReen`；
- **帧通路热路径不打日志** —— `link` 的 dequeue 循环与 `sender` 的写循环里**只在异常 / 状态变化时**打
  （3µs/条虽便宜，但热路径上仍是纯开销）；
- **残留缺口 1**：**"Java 起来之前"的失败**（`app_process` 参数解析 / 类加载，即
  [`agent/module.md`](module.md) §3 的"`--nice-name` 放错会**静默失败**"）**仍无日志** ——
  那时 `android.util.Log` 还不可用 ⇒ 这类失败只能靠**退出码 + 启动脚本**；
- **残留缺口 2**：**网络模式 + 拔线**时 logcat 只能经 adb 读 ⇒ 拿不到。

> **与"日志走协议"的关系**：协议通道**后置**（不删除）—— 等 **F4** 定完协议后再评估是否需要
> （场景：PC 侧 UI 直接展示 agent 日志，不用开 adb）。**当前主通道 = logcat**。

---

## 4. 变更记录

- **2026-10-07 心跳定稿**：**周期固定 10s**；**心跳是独立消息、不承载协议内容**；
  **超时判据定为「`T_dead` 内无任何消息 ⇒ 判死」**（实现方式见下条修正），
  判死后三步（冻结 / 断开 / 计数 +1，**都不是拆屏**）；三条理由（判据与实现同义 / 误判代价低 /
  单次误判不拆屏）并据此**撤销"按传输参数化"**；边界：握手期独立超时**归 F4**、
  反向写失败不单独处理、PC 侧对等判据；**明确不做 `SO_KEEPALIVE`**。
  **日志三项仍待定**。
- **2026-10-07 周期与超时调小**：心跳 **10s → 3s**、`T_dead` **30s → 10s**（≈3 个周期）——
  理由是 `T_dead` **还要兜住"PC 不读"**（PC 挂死时 `sender` 卡在 `write`，靠它判死后
  `shutdownOutput()` 唤醒）⇒ **不必再加写看门狗**（同作用，只保留一个）⇒ 它必须够短。
  **撤销「握手期独立超时」** —— 沿用 `T_dead`（语义与处置一致）。
  ⚠️ `T_dead` 必须 **> 周期**，两者要一起看。
- **2026-10-07 判活实现修正（实测）**：**`setSoTimeout` 改为 `Os.poll(fd, T_dead)`** ——
  实测 adb 通道（`LocalSocket`）下 `setSoTimeout` **有效但抛 `IOException("Try again")`**，
  不是 `SocketTimeoutException` ⇒ 靠文案匹配属"依赖未文档化行为"。
  `Os.poll` 是公开 API 且**两种传输同一实现**（实测无数据返回 0 / 701ms，有数据返回 1 / 0ms）。
  **判据与 `T_dead` 值不变**。
- **2026-10-07 传输 API 实测**：`LocalSocket` 的 `shutdownInput`（读侧立刻 EOF、**写侧仍可用**）与
  `shutdownOutput`（对端 EOF）**均可用** ⇒ §8 收尾技巧成立；**一个监听名可同时接受多条连接**
  （设备内 3/3、**经 `adb forward` 3/3**）。
- **2026-10-07 判活机制修正（S1 实测）**：§2.1 的「**两种传输同一个实现**」**不成立** ——
  `java.net.Socket` **没有** `getFileDescriptor()`（SDK 的 `android.jar` 已核对）⇒
  **判活按传输分两种机制**：adb 走 `Os.poll`，TCP 走 `setSoTimeout` + 读。**判据不变**（都是
  `T_dead` 内没有对端任何消息）。同时把「不用 `setSoTimeout`」限定为**只针对 `LocalSocket`**
  （它抛未文档化的 `IOException("Try again")`）—— `java.net.Socket` 抛的是标准 `SocketTimeoutException`。
- **2026-10-07 日志定稿**：**通道 = 复用 adb logcat（`android.util.Log`）** —— **实测**能写 /
  **脱离启动也能写**（走 logd socket，不走 stdout）/ 任意线程能写 / **不阻塞**（20000 条 59ms）；
  三项待定**全部收口**（连接前日志天然解决 / 等级与限流交给 Android 原生 + `setprop log.tag.<TAG>` /
  **不落盘**，留存由 PC 侧 `adb logcat -f` 负责）。记下两条坑（`System.out` 不进 logcat、
  默认 `isLoggable(V/D)=false`）与两个残留缺口（Java 起来之前的失败 / 网络模式拔线）。
  **协议日志通道后置**，等 F4 后评估。
