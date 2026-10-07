# `:agent` · 运行时（F6 生命周期与失败处理 / F7 可观测性）

> 本文件是 [`AGENTS.md`](../../../AGENTS.md) 索引下的规范之一，属 [`agent/module.md`](module.md) §2 的子系统 #1。
> 覆盖功能 **F6（生命周期与失败处理）** 与 **F7（可观测性：日志与心跳）**。
> 建屏引导链见 [`agent/display.md`](display.md) §2.2；屏与编码器的收尾顺序见
> [`agent/display-lifecycle.md`](display-lifecycle.md) §5。
> **变更记录记在本文件 §10。**

**状态**：**已定**（2026-10-07）—— 本文件是**入口**，细节在其 6 个子文档里。

---

## 1. 范围

| # | 功能 | 边界（只讲这些） |
|---|---|---|
| **F6** | 生命周期与失败处理 | **启动方式**、`main()` 编排顺序、进程与线程模型、退出与收尾顺序、错误策略 |
| **F7** | 可观测性 | **日志通道**、**心跳** —— 让各功能的验收都有可观测判据 |

**不管的**：协议字段与拓扑（F4）、屏与编码器的行为（F1 / F3）、输入注入（F5）。

---

## 2. 启动方式

### 2.1 必须脱离 adb 连接（已定）

启动命令**必须后台化**，让 agent 不随 adb shell 存活：

```sh
nohup setsid <agent 启动命令> </dev/null >/dev/null 2>&1 &
```

**依据（本机实测，实体机与 AVD 结论一致）**：

| 启动方式 | adb 连接断开后 |
|---|---|
| 前台（`adb shell <cmd>` 持有连接） | ❌ **死** |
| 前台 + `setsid` | ❌ **死** —— toybox 的 `setsid()` 在调用者已是进程组长时会失败并**静默继续** |
| **后台化（命令立即返回）** | ✅ **活**（且能扛过 `adb kill-server`） |

**为什么必须脱离**：不脱离时，断连 ⇒ agent 死 ⇒ **虚拟屏被系统自动回收**（已实测）
⇒ 屏上的 App 与状态全丢 ⇒ "重连"退化成冷重建。
脱离后 [`agent/display-lifecycle.md`](display-lifecycle.md) §2 的**冻结 + 重连模型才成立**。

#### 2.1.1 启动与通信是两件事

| | 走什么 | 可换吗 |
|---|---|---|
| **启动** | **只能走 adb** —— uid 2000 只能从 adb 起（不变量 1） | ❌ |
| **通信** | **可插拔**：adb（`localabstract` + `adb forward`）或**网络直连** | ✅（未决项 9） |

⇒ 脱离是**两种传输的共同前提**：adbd 被关 / USB 被拔时，agent 与流都要继续。
网络模式下收益最大 —— **拔掉 USB 后 agent 与视频流继续**。
⇒ 通信侧的一切（协议、线程划分）**必须传输无关**
（[`tech-stack/overview.md`](../tech-stack/overview.md) §5 未决项 8）。

**两个副作用**：

- **「结束会话」与「停止 agent」是两条不同的指令** —— 前者 = **拆屏但继续监听**（可再连），
  后者 = **触发全局收尾、进程退出**；脱离后不再受 adb 管理，必须有显式停止手段 ——
  **就是协议指令 `stopAgent`**（`pkill` 只是硬杀：虚拟屏会被系统回收，**安全但不走收尾**，
  见 [`agent/runtime-failure.md`](runtime-failure.md) §5）；
- **重复实例检测** —— 上次没死干净会占着屏；两种传输下**监听地址被占用**即可判出
  （`localabstract` 名 / TCP 端口）。

### 2.2 不做保活（已定）

**进程真崩了不自动拉起** —— 不做看门狗、不做自重启。
"活过断连"由 §2.1 的脱离解决，与"崩了重拉"是两件事，**不混为一谈**。

**依据（本机实测，实体机与 AVD 一致）**：`adb shell` 起的进程 **`oom_score_adj = -1000`**
（lmkd 的不可杀档），且 cgroup 归在 `system/uid_0/pid_<adbd>` ⇒ **连接期间不会被内存压力杀掉**；
真正杀它的是 **adbd**（连接断开）。

**待定**：谁把 agent 拉起来、崩了怎么办 —— 涉及 `:app` 的激活路线（见 [`app/module.md`](../app/module.md) §2）。
**未验风险**：小米等厂商的内存管理（如 `mimd` cgroup）是否会在**长跑**时绕过 `oom_score_adj` 杀 agent。

> **网络模式下的后果（须知道）**：agent 崩了**没有远程自救手段** ——
> `:app` 起不了它（uid 不是 2000 ⇒ 建屏 untrusted，不变量 1），USB 已拔时也无法再 `adb shell`
> ⇒ 只能**重新插线**。`:app` 的"看门狗"职责（[`app/module.md`](../app/module.md) §1）是**后续**的救援路径。

---

## 3. 启动编排

### 3.1 进程引导（已定）

三条硬性要求见 [`agent/display.md`](display.md) §2.2。要点：
`Looper.prepare()` + 反射把 `Looper.sMainLooper` 设为它 → 反射构造 `ActivityThread` → 伪装 `com.android.shell` Context。
**不要用 `Looper.prepareMainLooper()`** —— 它 `quitAllowed = false` ⇒ 主线程**无法退出**（§3.2 的骨架依赖这一点）。

### 3.2 主线程骨架（已定）

```
main():
  ① 装 Thread.setDefaultUncaughtExceptionHandler    ← 最先，保证任何线程崩溃都有日志
  ② 进程引导（§3.1）
  ③ 解析参数
  ④ 启动各组件线程（每个组件结束时回报）
  ⑤ Looper.loop()                                   ← 被「全部结束」或「任一 fatal」打断
  finally:
    ⑥ 通知各组件停止 → join → 收尾（§6 / runtime-threads §8）
    ⑦ System.exit(status)                           ← 必须显式退出
```

**为什么 `finally` 里要 `System.exit()`**：Android SDK 内部可能起非 daemon 线程，
不显式退出会让 JVM 挂住。

### 3.3 具体顺序（已定）

**见 [`agent/runtime-startup.md`](runtime-startup.md)** —— 六步：①异常兜底 → ②进程引导 →
③解析 CLI 参数 → 建队列 → ④起进程级线程（`slow` → **`accept` 最后**）→ ⑤`Looper.loop()`；
**启动阶段不建屏**、**不做残留清理**；启动失败 ⇒ fatal ⇒ `System.exit(非 0)`。

---

## 4. 日志（F7）

**见 [`agent/runtime-observability.md`](runtime-observability.md)** §3 ——
**通道 = 复用 adb logcat（`android.util.Log`）**（**已实测**：能写 / 脱离启动也能写 / 不阻塞）；
三项待定**全部收口**（连接前日志天然解决、等级与限流交给 Android 原生、**不落盘**）。

---

## 5. 心跳（已定）

**见 [`agent/runtime-observability.md`](runtime-observability.md)** §1 / §2 ——
**判活看心跳不看帧**（静止屏不出帧）；**周期固定 3s**；**心跳是独立消息、不承载协议内容**；
超时判据 = **`T_dead`（10s）内没有收到对端任何消息**（机制按传输分两种，见
[`agent/runtime-observability.md`](runtime-observability.md) §2.1）。

---

## 6. 退出与收尾

**触发集合（已定）**：上游显式结束 / 连续 `N` 次接入失败 / 致命错误 / 进程退出。
（**终止信号不在其列** —— 实测 `SIGTERM` / `SIGINT` 不触发 shutdown hook，见
[`agent/runtime-failure.md`](runtime-failure.md) §5。）

**硬顺序（已定）**：**先 `release()` 虚拟屏 → 再 `release()` 编码器**
（反了会让编码器的输入面在屏仍渲染时失效，见 [`agent/display-lifecycle.md`](display-lifecycle.md) §5）。

**完整的动作序列与 join 顺序**见 [`agent/runtime-threads.md`](runtime-threads.md) §8；
**退出码（`0`/`1`/`2`）、终止信号、收尾超时值**见 [`agent/runtime-failure.md`](runtime-failure.md) §4–§6。

---

## 7. 线程模型（已定）

**见 [`agent/runtime-threads.md`](runtime-threads.md)** —— **单进程多路**（一路 = 一块屏 = 一条连接）：
每路 `reader` / `link` / `sender`，进程级 `main` / `accept` / `slow`（F5 再加 `input`），
**每个可变状态一个 owner、全模块不设业务锁**；含消息路由表、各线程循环、
传输接缝、零拷贝与背压、收尾与 join 顺序。

一句话：**帧通路（`link`）与控制消息不共享线程**，每条连接 **只有一个写者（`sender`）
与一个读者（`reader`）**，`main` 只做编排且**永不阻塞**。

---

## 8. 生命周期状态与协调（已定）

**见 [`agent/runtime-lifecycle.md`](runtime-lifecycle.md)** —— **不搞全局状态机**（两个生命周期正交）；
**显示链路（屏 + 编码器）合成一个 3 态状态机**（无屏 / 就绪 / 冻结）+ 合法组合表与 4 条不变量；
协调者 `main` **只持会话级策略与终局、不镜像子系统状态**，其唯一编排的序列是**收尾序列**；
仲裁由**各 owner 的串行队列**完成；两条硬规则：**命令幂等**、**线程意外退出 = fatal**。

---

## 9. 错误策略（已定）

**见 [`agent/runtime-failure.md`](runtime-failure.md)** —— **四级按「谁负责善后」划分**
（致命 / 子系统 / 单路 / 可恢复）；**致命来源只有三类**，运行期不会因指令失败退出；
**子系统级判据 = 能力缺失**；**可恢复的重试由 PC 负责**；**不做自动升级**。

---

## 10. 变更记录

**见 [`agent/runtime-changelog.md`](runtime-changelog.md)**（本记录因超出单文件上限已提成独立文件）。
