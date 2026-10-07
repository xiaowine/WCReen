# `:agent` · 传输与协议（F4）

> 本文件是 [`AGENTS.md`](../../../AGENTS.md) 索引下的规范之一，属 [`agent/module.md`](module.md) §2 的子系统 #3。
> 覆盖功能 **F4（把码流传到 PC）**。
> 硬约束见 [`agent/module.md`](module.md) §3；屏与编码见 [`agent/display.md`](display.md) / [`agent/encode.md`](encode.md)；
> 运行时的线程与生命周期见 [`agent/runtime.md`](runtime.md) 及其子文档。
> **变更记录记在本文件 §3。**

**状态**：**已定**（2026-10-07）—— 五个议题全部定稿，细节在 5 个子文档里。

---

## 1. 范围

| # | 主题 | 边界（只讲这些） |
|---|---|---|
| 1 | **拓扑与传输** | 多路结构、监听与地址、连接生命周期、**接入失败计数语义**、多传输并存、网络侧认证 |
| 2 | **握手与能力协商** | 版本号、参数下发、能力协商（方向 / 决定权 / 序列）、解码配置传递、首帧 IDR |
| 3 | **协议格式** | 帧封装、控制消息封装、`requestId`、消息类型表、分片 |
| 4 | **消息清单** | 把 F1–F7 产出的全部指令与上报汇总 |
| 5 | **背压与丢帧** | `sender` 的丢弃策略、队列深度、与容量处理的关系（**PC 侧不做容量处理**） |

**不管的**：屏与编码器的行为（F1 / F3）、输入注入（F5）、启动与退出（F6）、日志与心跳（F7）。

---

## 2. 文档地图

| 文件 | 内容 |
|---|---|
| **本文件** | 入口：范围、文档地图、变更记录 |
| [`agent/transport-topology.md`](transport-topology.md) | **拓扑与传输**（**已定**）：单进程多路、`streamId`、线程归属、生命周期流转与计数语义、传输层 |
| [`agent/transport-handshake.md`](transport-handshake.md) | **握手与能力协商**（**已定**）：版本号、能力表、协商职责划分、`streamId` 位置、解码配置、握手超时 |
| [`agent/transport-protocol.md`](transport-protocol.md) | 协议格式（**已定**）：统一框架、帧负载、控制消息 CRC + JSON、类型表、不分片、字节序 |
| [`agent/transport-messages.md`](transport-messages.md) | **消息清单**（**已定**）：类型表、指令 `cmd` 清单、`applyScreen` 参数、回报与主动上报、错误对象 |
| [`agent/transport-shapes.md`](transport-shapes.md) | **报文形状**（**已定**）：能力表 / `applyScreen` / 指令回报 / 错误对象的**确切 JSON** + `error.code` 登记表 |
| [`agent/transport-backpressure.md`](transport-backpressure.md) | 背压与丢帧（**已定**）：两个机制分开、丢到 IDR、四个动作、主动请求 IDR、容量与 `T_drop` |

**推进顺序**：① 拓扑与传输 → ② 握手与能力协商 → ③ 协议格式 → ④ 消息清单 → ⑤ 背压与丢帧。
**① ② ③ ④ ⑤ 全部已定**。

---

## 3. 变更记录

- **2026-10-07 初版**：F4 入口落盘（范围、文档地图、推进顺序）。
  **议题 ① 拓扑与传输已定** ⇒ [`agent/transport-topology.md`](transport-topology.md)。
  **议题 ② 握手与能力协商已定** ⇒ [`agent/transport-handshake.md`](transport-handshake.md)。
  **议题 ③ 协议格式已定** ⇒ [`agent/transport-protocol.md`](transport-protocol.md)。
  **议题 ⑤ 背压与丢帧已定** ⇒ [`agent/transport-backpressure.md`](transport-backpressure.md)。
  **议题 ④ 消息清单已定** ⇒ [`agent/transport-messages.md`](transport-messages.md)。
  **F4 五个议题全部定稿**（④ 的 `input` 待 F5 补）。
