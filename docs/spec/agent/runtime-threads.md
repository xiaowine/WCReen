# `:agent` · 线程模型（F6）

> 本文件是 [`AGENTS.md`](../../../AGENTS.md) 索引下的规范之一，属 [`agent/runtime.md`](runtime.md)（F6）的**线程模型**部分。
> 启动方式与主线程骨架见 [`agent/runtime.md`](runtime.md) §2 / §3；收尾的硬顺序见 [`agent/display-lifecycle.md`](display-lifecycle.md) §5。
> **变更记录记在本文件 §10。**

**状态**：**已定**（2026-10-07）。§9 的待定值**不影响形状**。

---

## 1. 总则

**要定的不是"开几条线程"，而是"每个可变状态归谁独占"。**

| # | 规则 |
|---|---|
| 1 | **一个可变状态一个 owner 线程**；跨线程只传消息（`Handler.post` / 有界队列）⇒ **全模块不设业务锁**（唯一例外见 §6，靠 index 语义规避而非加锁） |
| 2 | **`main` 永不阻塞** —— 不碰 socket / `MediaCodec` / `VirtualDisplay` / 子进程，不 `sleep` |
| 3 | **socket 只有一个写者、一个读者** |
| 4 | **队列一律有界 + 非阻塞入队**；丢弃策略归队列 owner |
| 5 | **线程数固定、角色固定、命名固定** —— 日志必须带线程名（F7） |

**线程划分与传输无关**（§5）：传输只决定"连接从哪来"。

### 1.1 形状

```
agent 进程（一个实例）
├─ main（协调者）· accept（监听+分派）· slow（枚举/拉起）   ← 进程级单例
├─ 路 #1 ─ reader（唯一读者）· link（屏+编码器）· sender（唯一写者）
├─ 路 #2 ─ …
└─ 路 #N ─ …

每条连接 = 一对字节流；帧 {index,info} 由 link 入队 → sender 写完再 release
```

---

## 2. 线程表

| # | 线程 | 归属 | **独占拥有** | 循环干什么 | 硬性禁止 |
|---|---|---|---|---|---|
| 0 | `main`（Looper） | 进程级 | 组件完成计数、**按路各一份的接入失败计数**、DisplayListener 回调、心跳定时器、收尾编排 | `Looper.loop()` | 不碰 socket / `MediaCodec` / `VirtualDisplay` / 子进程；不 `sleep` |
| 1 | **`accept`** | 进程级 | **监听 socket** | 阻塞 `accept` → 为每条新连接起一条路 | 不做任何解析 |
| 2 | **`slow`** | 进程级 | 枚举 / 拉起 / 能力探测 | 串行执行慢指令 | 不碰链路与帧通路 |
| 3 | **`reader`** | **每路** | **socket 读侧（唯一读者）** | 判活 → 解析 → 分发 | 不执行慢指令；不写 socket |
| 4 | **`link`** | **每路** | **`VirtualDisplay` + `MediaCodec`**（除 §6 那一处） | 链路指令 → `dequeue` → 入队 | 不写 socket |
| 5 | **`sender`** | **每路** | **socket 写侧（唯一写者）**、帧队列、控制队列 | 先清控制队列 → 再取一帧写 | 不读 socket |
| 6 | **`input`** | **每路** | 注入队列（`move` 覆盖 / `scroll` 累加） | 注入 | 不与 `slow` 共用 |

**全进程只有一个 Looper（`main`）** —— 同步模式 `MediaCodec`、binder、子进程都不需要 Looper。
**不用 `MediaCodec` 异步回调模式**（会多一个 Looper 与回调线程）；**不用线程池**（`slow` 是单线程 executor）。

### 2.1 为什么不能更少

| 线程 | 不能省的理由 |
|---|---|
| `sender` | **心跳与控制应答也要写这个 socket** ⇒ 必须有**唯一写者**。否则心跳 / 应答只能排队等帧、或加锁阻塞帧 —— 两种都违反不变量 6。独立后网络抖动只波及它，**编码器不受牵连** |
| `link` 独占屏 + 编码器 | 不变量 3 的顺序与 [`agent/encode-lifecycle.md`](encode-lifecycle.md) §2.1 的重建五步**跨"屏"和"编码器"两个对象**。同一条线程 ⇒ 五步是普通顺序代码，**零协调**；分给两条线程则每次重建都要"暂停 / 确认已停 / 重启"握手 |
| `slow` | 拉起 = 子进程 + **轮询取判据**（[`agent/launch.md`](launch.md) §4.2 / §4.3），**秒级**。放 `reader` 会**阻塞读超时检测 ⇒ 误判 PC 掉线**；放 `main` 会阻塞 DisplayListener 与心跳定时器；放 `link` 会阻塞帧通路 |
| `input` 不与 `slow` 共用 | 注入高频且要求低延迟，`injectInputEvent` 是同步调用；与 `slow` 共用 ⇒ 一次枚举（百毫秒级）就让拖动卡住 |

---

## 3. 消息路由（`reader` 收到消息后往哪发）

| 消息 | 去向 | 为什么 |
|---|---|---|
| 建屏 / 拆屏 / 冻结 / 解冻 / 参数变更 / 请求 IDR | → `link` | **只有 `link` 拥有屏与编码器** |
| 会话级 / 停止 / `N` 计数 | → `main` | `main` 是协调者 |
| 枚举 / 拉起 | → `slow` | 秒级，不能占别处 |
| 输入事件 | → `input` | 高频 |
| 心跳 / 握手应答 | 本线程处理，投 `sender` | 无状态、极快 |

`main` → `link` 也需要一条同款队列：`N` 次接入失败触发的拆屏由 `main` 决策、`link` 执行。

**分发一律非阻塞入队**，且**必须在 `accept()` 返回之前就绪**（不变量 5）；细化见 [`agent/runtime-lifecycle.md`](runtime-lifecycle.md) §5.3。

---

## 4. 各线程的循环

```
link:    while (running) {
           while (cmd = cmdQueue.poll()) exec(cmd)       // 建屏/拆屏/冻结/重建/请求IDR
           if (codec != null) {
             idx = codec.dequeueOutputBuffer(info, 10ms)  // 短超时 ⇒ 指令及时 + 不空转
             if (idx >= 0) frameQueue.offer({idx, info})  // 非阻塞；满则丢 + 立即 release
           }
         }

sender:  while (running) {
           while (msg = ctrlQueue.poll()) write(msg)      // 控制 / 心跳优先，饿不死
           if (frame = frameQueue.poll()) {
             write(frame); codec.releaseOutputBuffer(frame.idx, false)
           } else 短超时等
         }

accept:  while (running) {
           conn = transport.accept()                      // 阻塞；stop 时由 close() 打断
           spawnRoute(conn)                               // 为这条连接起 reader/link/sender
         }

reader:  while (running) {                             // 每路一条
           if (!awaitReadable()) { onPcDead(); break }   // F7 判活（adb: Os.poll；tcp: 读超时）
           route(parse(read()))
         }
```

`main` 没有自己的循环 —— 它就是 `Looper.loop()`（[`agent/runtime.md`](runtime.md) §3.2）。

---

## 5. 传输接缝（唯一与 adb / 网络有关的地方）

`sender` / `reader` **只依赖「一条已建立的连接 + 一对字节流」**，不依赖传输类型：

- 传输提供 `accept()`（阻塞，返回一条连接）/ **`wake()`** / `close()`；
- `AdbTransport` = `LocalServerSocket("wcreen_XXX")`；`TcpTransport` = `ServerSocket(PORT)`；
- **`wake()` 是必须的**：`LocalServerSocket.close()` **不会**让阻塞中的 `accept()` 返回
  （实测：收尾白等一个 3s join 超时）⇒ 收尾时先**自己连自己一次**叫醒它，再 join，最后才 `close()`
  （见 §8 ①）；`AcceptLoop` 拿到这次自连接后直接丢掉，不为它起线程；
- 两者都提供 `getInputStream` / `getOutputStream` / `shutdownInput` / `shutdownOutput`
  ⇒ §8 的收尾技巧通用（**已实测**：`shutdownInput` 读侧立刻 EOF 且**写侧仍可用**、`shutdownOutput` 令对端 EOF）；
- ⚠️ **`getFileDescriptor()` 只有 `LocalSocket` 有** —— `java.net.Socket` **没有**（已用 SDK 的
  `android.jar` 核对）⇒ **判活机制按传输分两种**，见
  [`agent/runtime-observability.md`](runtime-observability.md) §2.1。

**连接方向（已定）**：两种传输都 **agent 监听、PC 主动连** ⇒ `accept` 线程**一字不改**；
**一个监听地址服务全部路**（**已实测**：设备内 3/3、经 `adb forward` 3/3）。
反方向（agent 外连 PC）要多一套"连接建立 + 重试"逻辑，且要配 PC 的 IP；而 PC 输**设备 IP** 更自然。

**待定**：多传输**并存**（同时监听 adb 与 TCP）会让 `accept` 侧需要 poll 两个监听 socket ——
倾向**一次只启用一种**（启动参数选），并存留后续。协议与认证归 **F4**。

---

## 6. 零拷贝与背压（**全模型唯一的共享点**）

- `link` `dequeueOutputBuffer` 拿到 index 后**不 release**，把 `{index, bufferInfo}` 入队；
- `sender` 写完再 `releaseOutputBuffer(index, false)`；**丢帧 = 立即 release**（否则输出池泄漏）；
- **`MediaCodec` 输出池 = 天然有界队列**；池满 ⇒ `link` 拿 `INFO_TRY_AGAIN_LATER` ⇒ 这就是背压；
- **入队永不阻塞** ⇒ 消除"`sender` 已退出、`link` 卡在入队"这类死锁。

⚠️ **`MediaCodec` 是唯一被两条线程共享的对象**（`link` dequeue / `sender` release），不加锁。
✅ **已实测（本机实体机 / Android 16 / `c2.qti.avc.encoder` HW）—— 跨线程 release 安全**：三组配置
（EGL 60fps 立即释放 **785** 帧 / 释放延时 20ms **786** 帧 / **真实链路**（虚拟屏零拷贝 + DPI 泵帧）**110** 帧）
全部 **0 异常、0 泄漏（dequeue−release=0）、0 卡死**；release 发起时 dequeue 正阻塞在 codec 内 **415 / 689 / 89** 次 ⇒ 并发确凿。
丢帧路径（入队满 ⇒ 立即 release）实测 78 + 4 次且**输出池未耗尽** ⇒ **退路不需要**；结论基于本机编码器，**换设备需重测**。

**丢弃策略**（丢非关键帧 / 丢到下一个 IDR）**归 `sender`**（release 权在它手上）；
**策略本身是 F4 的**，本文件只提供落点。

---

## 7. 心跳：**零新线程**

| 方向 | 载体 |
|---|---|
| agent → PC | `main` 的 `Handler.postDelayed`（`main` 永不阻塞 ⇒ 定时器可靠）→ 投 `sender` |
| PC → agent | **`reader` 的读超时**（adb 用 `Os.poll`，tcp 用 `setSoTimeout`）—— 本质就是"读超时" |

**周期固定 3s、超时判据固定 10s**（**不按传输参数化** —— 见 [`agent/runtime-observability.md`](runtime-observability.md) §2）。

---

## 8. 收尾与 join 顺序

`main` 的 `finally`（[`agent/runtime.md`](runtime.md) §3.2 的 ⑥）：

```
① 设全局停止标志 + **`wake()` 叫醒** `accept` → join → 关监听 socket
② 对**每一路**：shutdownInput() → reader 退出（读侧关掉，写侧还能用）→ join
③ 通知 slow（进程级）+ kill 子进程 → join；通知 input → join
④ 对**每一路**：通知 link（dequeue 短超时保证及时返回）→ join
⑤ 对**每一路**：release 虚拟屏 → release 编码器   ← 硬顺序，必须在 ④ 之后
⑥ 对**每一路**：sender 排空队列（带超时）→ shutdownOutput() → join
⑦ transport.close()
⑧ System.exit(status)
```

- **④ 必须在 ⑤ 之前**：否则 `link` 还在 `dequeue`，编码器已被 `release`；
- **⑤ 的顺序**（先屏后编码器）来自 [`agent/display-lifecycle.md`](display-lifecycle.md) §5；
- **`sender` 放在最后停** —— 收尾消息还能发出去；否则 socket 一关，全丢；
- `shutdownInput()` / `shutdownOutput()` 分别拆掉读侧与写侧，让阻塞的读 / 写立刻返回，
  **同时另一侧还能用**。

---

## 9. 待定值（不影响形状）

**仅一项**：`dequeue` 短超时（建议 5–10ms）。
**控制队列深度已定 = 64**（依据写在 `runtime/Route.kt` 的 KDoc 里）。

---

## 10. 变更记录

**见 [`agent/runtime-changelog.md`](runtime-changelog.md)**（本记录因超出单文件上限已提成独立文件）。
