# 单一路径：用「虚拟设备」建屏（解锁位 + 任意尺寸铺满，无回退）

> 本文件是 [`docs/spec/workflow.md`](../spec/workflow.md) §2 定义的**实现方案文档**。
> 范围：把 `:agent` 的**建屏路径**整体换到「虚拟设备（VDM）」上；flags 组合规则与参数模型不变。
> 写完即冻结；后续结论另建文档。

**状态**：**草案（待批准）**（2026-10-07）。

**取代关系**（旧文档按惯例冻结不改，在此声明被本方案取代）：

- [`2026-10-07-screen-create-via-config.md`](2026-10-07-screen-create-via-config.md) §6 的
  "解锁位与铺满只能二选一 / Android 16 起才兼得" —— **被取代**：走虚拟设备后**任何版本**都兼得，
  且不再需要 `setIgnoreActivitySizeRestrictions`；
- [`agent/display-flags.md`](../spec/agent/display-flags.md) §4 的"Android 14/15 退路（高÷宽 ≥ 1.22）"
  与 §3 的"开解锁位会带来显示组代价" —— **被取代**：不再有版本退路，显示组代价消失。

---

## 1. 背景与目标

**问题**：在**默认设备**上建屏时，`ALWAYS_UNLOCKED`（主屏锁屏时不接管该屏）与"铺满"互斥 ——
请求解锁位 ⇒ 平台**强塞 `OWN_DISPLAY_GROUP`** ⇒ 该屏照常采纳应用的固定方向请求 ⇒
锁定方向的应用（B 站那类）被裁进 **0.82 比例**兼容窗口。两条旧建屏路径都试过，结论一致。

**目标（验收标准，可观测）**：

| # | 要求 | 判据 |
|---|---|---|
| ① | 多版本都不被锁屏接管 | 主屏锁屏/休眠时，该屏**照常显示 App**（原始帧缓冲可证） |
| ② | App 画面跟随分辨率 | 锁定方向 App 在**任意尺寸**上铺满（窗口 = 整屏） |
| ③ | 一条 API 覆盖多版本 | 同一份代码跑 Android 14/15/16 |
| ④ | 不要回退 | 无版本分支、无"尺寸够竖"降级 |

## 2. 现状调研（事实依据）

- 旧路径代码：`display/Screen.kt`（`createVirtualDisplay` 调用点 + 回读校验）、
  `display/DisplayFlags.kt`（`ALWAYS_UNLOCKED ⇒ OWN_DISPLAY_GROUP` 派生）、
  `wrappers/ActivitySizeRestrictions.kt`（第 6 处反射，**Android 16 起才有**）、
  `Main.kt`（拿不到它就告警降级）。
- 已实测结论（见 §1 提到的两份文档）：两条旧路径都被平台强塞显示组；`ignoreOrientationRequest`
  在有显示组时**形同虚设**；16 上隐藏开关能取消裁切，**15 上没有该开关**。

## 3. 关键事实：虚拟设备（本机双端实读 + AOSP 源码核对）

**一条 API（15/16 同签名）**：

```
new VirtualDeviceManager(IVirtualDeviceManager$Stub.asInterface(ServiceManager.getService("virtualdevice")), ctx)
  .createVirtualDevice(associationId, params)                    → VirtualDeviceManager$VirtualDevice
  .createVirtualDisplay(config, executor = null, callback = null) → VirtualDisplay
```

- **屏建在设备上** ⇒ 平台**不再**强塞 `OWN_DISPLAY_GROUP` ⇒ 忽略方向请求生效 ⇒ 任意尺寸铺满；
- 设备参数 `LOCK_STATE_ALWAYS_UNLOCKED = 1` ⇒ 主屏锁屏不接管该屏；
- 15 与 16 的**客户端层签名一致**（差异被包装层吸收：16 的 AIDL 是
  `IVirtualDevice.createVirtualDisplay`，15 是 `IVirtualDeviceManager.createVirtualDisplay`）；
- **两端实测**：真机 16 建屏回读 `0xe80`（`TRUSTED|ALWAYS_UNLOCKED|TOUCH_FEEDBACK_DISABLED|OWN_FOCUS`）、
  **无显示组** ✓、屏归属设备（`getDisplayIds()` 含新屏）✓；活样本对照：同类工具 AndroMeld 的
  设备（`mLockState=1`）+ 其屏（无显示组、`ignoreOrientationRequest=true`、京东**满屏**）✓。
- **两个前置条件**（都实测踩过）：
  1. **CDM 关联**：服务端要求"设备必须有对应关联"，按 **attribution 的包名**查
     （`cmd companiondevice associate 0 com.android.shell <mac>
     android.app.role.COMPANION_DEVICE_APP_STREAMING SELF_MANAGED`，两端均验证通过）；
  2. **Context**：`getApplicationContext()` 必须非空（包装层构造第一步就调它）且
     **attribution 包名 = `com.android.shell`**（系统 Context 报 `android`）⇒ 需要一小段伪装 + 反射改字段。

## 4. 方案与取舍

**采纳**：上节那条客户端包装层 API，**单一路径**（无版本分支）。理由：它是唯一在 14/15/16 上
**签名一致**、且不需要 `MediaProjection`、也不需要自己拼 AIDL/回调/Binder 的入口。

**被否方案**：

| 候选 | 否决原因 |
|---|---|
| 旧路（现状）：公开 `VirtualDisplayConfig` + 解锁位 | 不满足 ①②（二选一） |
| 旧路 + "尺寸 ≥1.22" 规避 | 用户明确**不要回退**；且无法用任意尺寸 |
| AIDL 直调（探针 v5 路线） | **15/16 的 AIDL 签名不同**（15 无 `IVirtualDevice.createVirtualDisplay`）⇒ 必须版本分支 ✗ |
| 镜像路线（设备自有屏 + 镜像到编码面） | 多一路合成与一路 `VirtualDisplay` 开销；设备自有屏还依赖 home 组件（我们无 APK） |
| 补全 hidden 的 `android.jar` 直调 | 既有规范已实测否决（[`code-hiddenapi.md`](../spec/agent/code-hiddenapi.md) §5） |

**代码形态**（公开 API 优先，隐藏 API 收敛在 `wrappers/`）：

| 区域 | 变化 |
|---|---|
| `wrappers/VirtualDevice.kt`（新增，1 个反射域） | 解析并缓存：`IVirtualDeviceManager$Stub.asInterface`、`VirtualDeviceManager` 隐藏构造器、`createVirtualDevice`、嵌套 `createVirtualDisplay`、`VirtualDeviceParams$Builder`、伪装 Context（含 `getApplicationContext` 覆写与 attribution 反射）；对外只暴露普通 Kotlin 类型 |
| `display/Screen.kt` | 建屏改走虚拟设备（config 仍由**公开**的 `VirtualDisplayConfig.Builder` 装配）；`setSurface`/`resize`/`release` 语义不变（仍是同一个 `VirtualDisplay`） |
| `display/DisplayFlags.kt` | **删除** `ALWAYS_UNLOCKED ⇒ OWN_DISPLAY_GROUP` 派生（新路径平台不塞） |
| `encode/EncoderParams.kt` + `proto/Screen.kt` | `alwaysUnlocked` **缺省改 `true`**（旧缺省 `false` 是二选一的产物） |
| `Main.kt`（启动编排） | 新增"**确保 CDM 关联**（缺失则建 + 解析 id）+ **建虚拟设备**"两步，沿用"启动期一次解析、失败即退出码 2"的既有纪律 |
| `wrappers/ActivitySizeRestrictions.kt` | **整份删除**（连同 `Main.kt` 的降级告警与各处的尺寸规避文案） |

**失败形态**：关联拿不到 / 隐藏成员缺失 / 建屏被拒 ⇒ **启动失败（退出码 2）**，日志列出缺哪条。

## 5. 影响面

- 代码：上表 6 处；对外契约（协议 / 消息字段 / CLI）**无变化**；
- 部署：agent 启动时会**确保一条 self-managed 的 CDM 关联**（`com.android.shell`，
  profile = `COMPANION_DEVICE_APP_STREAMING`）；用户可在系统里撤销 ⇒ 下次启动自检重建 ✓；
- 文档：`code-hiddenapi.md` §3（第 6 处 → 新域）、`display-flags.md` §3/§4、`display.md` §2/§3、
  `display-changelog.md`、`code-map.md`（新增/删除文件）。

## 6. 实施步骤

1. 新增 `wrappers/VirtualDevice.kt`（含关联自检与建设备）；
2. `display/Screen.kt` 切换建屏调用；`DisplayFlags` 删派生；解锁位缺省改 `true`；
3. 删 `ActivitySizeRestrictions` 与全部退路文案；单测（flags 组合 / 回读期望 / 缺省值）随之更新；
4. 双端实跑 + 端到端（PC 出画）；
5. 同步文档与 `code-map.md`。

## 7. 验证方式

| 机型 | 项 | 期望 |
|---|---|---|
| 真机 Android 16 | 建屏回读 | 无 `FLAG_OWN_DISPLAY_GROUP`；屏出现在设备 `getDisplayIds()` |
| | 铺满 | 锁定方向 App 窗口 = `Rect(0,0-1920,1800)`、`letterBoxed=false` |
| | 锁屏不接管 | 按电源键（`mWakefulness=Dozing`）后该屏**仍显示 App**（原始帧取证） |
| | 端到端 | PC 收到并硬解出画 |
| AVD Android 15 | 建屏回读 / 铺满 | 同上（用竖屏锁定测试 App） |
| | 锁屏 | **不可验**（AVD 拒绝休眠）⇒ 记为待真机 |
| Android 14 | — | **无设备**：只能核"API 面同 15"，真机待补 |

## 8. 风险与回滚

| 风险 | 对策 |
|---|---|
| 厂商 ROM 与 AOSP 差异 | 真机（Xiaomi / Android 16）与 AOSP 15 双端已跑通整链 |
| 平台在后续版本收紧隐藏 API | 失败即启动失败并**列出缺哪条**（不静默降级） |
| 关联被用户/系统撤销 | 启动自检：缺失即重建（幂等） |
| 进程死亡后设备与屏的回收 | 由系统按 owner 死亡回收（`DESTROY_CONTENT_ON_REMOVAL` 一并保留） |
| `resize`/`setSurface` 在 VDM 屏上的行为 | 仍走 `VirtualDisplay` 公开方法（内部用我们的回调）⇒ 实施后照旧回读校验 |

**回滚**：恢复 `Screen.kt` 的旧建屏调用与 `ActivitySizeRestrictions`、解锁位缺省改回 `false`
（即回到今天的可用状态）；方案文档保留不动。

## 9. 未决问题

1. **Android 14 真机**未验（本机无设备）；
2. **Android 15 的锁屏行为**在 AVD 上不可验（模拟器拒绝休眠）⇒ 需一台 14/15 真机；
3. **多路（多屏）时**：一个进程用一个设备（多屏共用）还是每路一个设备 —— 倾向**一个设备**，
   实施时按会话模型定；
4. 设备名与屏名的命名口径（现为 `WCReen` / `WCReen-<n>`）是否要区分多路，实施时定。

## 10. 变更记录

- **2026-10-07 草案**：机制查明 + 单路径方案成形；真机 16 上整链实跑通过（无显示组、屏归属设备）。
  双端铺满/锁屏取证与 15 验证随批准后一并补录。
