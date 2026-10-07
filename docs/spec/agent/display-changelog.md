# `:agent` · 显示源（F1）变更记录

> 本文件是 [`agent/display.md`](display.md) 的**变更记录**，从该文件拆出（原文件超单文件上限）。
> 规范正文见 [`agent/display.md`](display.md)；flags 见 [`agent/display-flags.md`](display-flags.md)；
> 生命周期见 [`agent/display-lifecycle.md`](display-lifecycle.md)。

---

- **2026-10-06 初版**：F1（显示源，扩展模式）定稿 —— 参数由上游决定、缺省按运行设备推导；
  镜像模式后置；flags 按语义位选择（默认组合待实机重测）。
- **2026-10-06**：flags 由"待定"改为**已定配置**（依据本机 SDK 源码的官方语义）：
  必须位 6 个、PC 可配 3 个、条件生效 1 个（`ALWAYS_UNLOCKED`）、明确不设 7 个；
  记录关键耦合（`ALWAYS_UNLOCKED` 仅在非默认显示组有效 ⇒ 依赖 `OWN_DISPLAY_GROUP`）
  与「**值一律用常量名拼，绝不硬编码**」（这 18 位全是 `@hide` 系统 API，跨版本可能变）。
- **2026-10-06**：`ALWAYS_UNLOCKED` 改为**由 PC 配置**（缺省开）；`OWN_DISPLAY_GROUP` 改为
  **派生位** —— 不再作为独立开关交给 PC，而是"有请求项需要它时自动设置"（当前唯一消费者是
  `ALWAYS_UNLOCKED`），并按参数模型的规则**回报实际结果**。flags 由此分三类来源：
  固定必须 / PC 直接可配 / **派生**。
- **2026-10-06**：生命周期展开为四节，**记录断开时的冻结候选 A/B**：
  A = `setSurface(null)`（停输出、编码器空转）；B = A + `release()` 编码器（更省，重连时用新
  `inputSurface` 重新 `setSurface`）。两者都保住"屏与 App 不重启"。同时记录被否的
  `PARAMETER_KEY_SUSPEND` 候选、三条待实机验证项、以及"先拆屏再拆编码器"的收尾顺序与异常残留处理。
  **A/B 最终取舍待实机验证后再定**。
- **2026-10-06**：按 [`docs.md`](../docs.md) 拆分（原 `agent-screen-encode.md` 已删除）——
  本文件保留 **F1 的参数模型 / flags / 验收**；生命周期移到 [`agent/display-lifecycle.md`](display-lifecycle.md)；
  编码移到 [`agent/encode.md`](encode.md)。
  尺寸耦合**定为硬约束**：**编码尺寸必须 = 屏尺寸，不允许 GPU 缩放（省性能）**；
  故尺寸变更 = `setSurface(null)` → `resize()` → 重建编码器 → `setSurface(新 inputSurface)` → 补关键帧（**仍不拆屏**）；
  DPI 变更只改屏、编码器不动。新增参数变更分类、验收指针、验收购据 5（无缩放）。
- **2026-10-06 实机验证完毕**：F1 **验收判据 1–5 全部成立**（逐条附实测证据）；新增
  **进程内 Context 引导的三条硬性要求**（主线程 Looper、隐藏构造器塞伪装 Context、
  包名伪装）—— 三条都是实测踩出来的失败模式。flags 因超出单文件上限**整段拆至
  [`agent/display-flags.md`](display-flags.md)**（含 `DESTROY_CONTENT_ON_REMOVAL` 的 2×2 对照实验
  与常量真实数值表）；原「待实机验证」中 4 项因需 F5 才能测，一并移入该文件 §6.4。
- **2026-10-06 决定**：`DESTROY_CONTENT_ON_REMOVAL` 由「PC 可配（缺省开）」**改为固定必须位** ——
  依据是实测的 2×2 对照实验（关掉后拆屏与崩溃都会污染本机屏）。参数表与 flags 速览同步更新：
  **必须位 7 个、PC 可配 2 个**。
- **2026-10-06 决定 + 实测**：新增**刷新率**参数（**PC 选择、缺省 60**、**会话内固定**）——
  实测 `setRequestedRefreshRate` **精确采纳**（30/24/90/120 全部生效），但运行期**无接口可改**，
  故与 flags 同类；它同时是"帧率"的唯一控制手段。变更分类已收录，
  并记下"**agent 侧运行期只能降码率**"这条对降级策略的推论（与 `tech-stack/pc.md` §3 待对齐）。
- **2026-10-06 决定**：新增 **§2.0 参数的载体** —— **建屏所需的全部参数**（画质 / 编码、
  屏行为 flags、DPI 与屏名）**集中在一条「申请屏幕」指令**里、由 **PC 选择**，
  **PC 拿这条指令申请屏幕**；**flags 并入同一条指令**（不另开）。
  由此确立：**建屏是显式指令、与连接的建立解耦**（连接可先存在而无屏），
  且该指令必须能作用于**已存在的屏**（可变参数直接应用、不可变的回报"需重开会话"）。
- **2026-10-06 决定**：**参数变更一律重建编码器，不存在热改** —— 参数变更分类表重写
  （原「码率热改」行删除，改为「**重建编码器**」类）；原则句改为"**绝不拆屏**"；
  降级策略推论同步更新（agent 侧运行期**没有可原地改的参数**）。
  详见 [`agent/encode-lifecycle.md`](encode-lifecycle.md) §2。
- **2026-10-06 决定**：**删除 PC 侧容量模型的降级机制**（`tech-stack/pc.md` §3 原「先降分辨率 / 帧率 →
  再降路数」）—— 改为**超出容量即拒绝新增路，不做任何降级**。§2.1 的推论注记随之由
  "与 PC 侧矛盾、待 F4 修正"改为**两侧口径已一致**（agent 侧运行期没有可原地改的参数 ⇒ 本就没有降级手段）。
- **2026-10-06 修正（本机实测）**：Context 引导第 1 条由 **`Looper.prepareMainLooper()`** 改为
  **`Looper.prepare()` + 反射设 `Looper.sMainLooper`**。两者都能完成引导与建屏，差别只在**能否退出**：
  前者 `quitAllowed = false` ⇒ `quitSafely()` 抛 `IllegalStateException: Main thread not allowed to quit.`、
  `Looper.loop()` **永不返回**，进程只能被强杀；后者 `quitSafely()` 成功、`loop()` **~300ms 返回**。
  实体机（Xiaomi / Android 16 / aarch64）与 AVD（AOSP / Android 16 / arm64）**结论一致**。
  依据：scrcpy 的 `Server.prepareMainLooper()` 就是这么做的（其注释明写"Like `Looper.prepareMainLooper()`,
  but with `quitAllowed` set to true"）。这是 F6「编排与收尾」的前提。
- **2026-10-06 拆分**：**本变更记录整段提成独立文件**（原文件超单文件上限）；
  `display.md` 只留一行指针。
- **2026-10-07 决定**：**PC 侧删除容量处理** ⇒ §2.1 的推论注记同步更新：
  「容量不足时只有**拒绝新增路**」改为「**没有「拒绝新增路」这回事** ——
  容量不足表现为**每路 fps 下降**，不拒绝、不降级、不踢路」。
  「不存在热改 ⇒ 不存在降级」这条**不变**，只是另一侧（PC）也不再做任何容量判断
  （[`tech-stack/pc.md`](../tech-stack/pc.md) §3）。
- **2026-10-07 修正（letterbox 调查）**：`ALWAYS_UNLOCKED` 的**缺省由「开」改为「关」**。
  实机调查发现：请求该位会让平台**连带给出 `OWN_DISPLAY_GROUP`**，而**带显示组的屏
  `ignoreOrientationRequest` 恒为 `false` 且设不上去**（`wm set` 后立刻 `get` 仍是 `false`）
  ⇒ 锁定方向的应用（如 B 站）被系统裁进 **0.82 比例**的兼容窗口（1920×1800 下只占 77% 宽、
  两侧黑边、内容极少）。关掉它 ⇒ 显示组消失 ⇒ 系统忽略应用的固定方向请求 ⇒ **App 铺满**
  （与同类工具 AndroMeld 的行为**逐字一致**，已复现）。
  代价（**§6.4 早已实测**）：手机锁屏时虚拟屏会显示 keyguard（`mCurrentFocus` 变成
  `com.android.systemui`）。完整因果链与四轮实验见
  [`plans/2026-10-07-letterbox-investigation.md`](../../plans/2026-10-07-letterbox-investigation.md)。
  连带更新：`display-flags.md` §3/§4、`display.md` 参数表与 §8 速览、`transport-shapes.md` 三处示例 JSON。
- **2026-10-07 实验（未采纳，两轮）**：① 试过改走公开的 `VirtualDisplayConfig` 建屏路径，期望在请求
  `ALWAYS_UNLOCKED` 时不被强塞 `OWN_DISPLAY_GROUP` —— **证伪**（回读 `期望 0xe80 实际 0xc80`）；
  ② 又补齐了 AndroMeld 剩下的两处 flag（`PRESENTATION` + `ROTATES_WITH_CONTENT`）—— **同样无效**
  （两处都设上了，显示组仍在、锁定方向的 App 仍被裁；且此轮证明 **`ignoreOrientationRequest` 在有显示组时
  形同虚设**：`get` 报 `true` 而 App 照样被裁）。**已全部回滚**：解锁位缺省维持**关**（= 选"铺满"）。
  ⇒ 公开 API 下"解锁位"与"铺满"只能二选一；AndroMeld 同时拿到两者的手段仍未查明。
  （**该结论的适用范围见下一条"定案"**：只对横屏尺寸成立。）
  见 [`plans/2026-10-07-screen-create-via-config.md`](../../plans/2026-10-07-screen-create-via-config.md)。
- **2026-10-07 定案（letterbox 调查收尾）**：`ALWAYS_UNLOCKED` 的缺省**回到「开」**（= 保证
  "主屏锁屏/休眠时虚拟屏照常可用"，两轮实测 ✓）。与"铺满"的冲突**只是条件性的**：兼容窗口的宽度被
  **屏宽**框住 ⇒ 屏的 **高÷宽 ≥ 1.22** 时它依然是满屏（1080×1920 @560 实测：解锁位 + 显示组都在，
  B站 窗口 `Rect(0,0-1080,1920)` ✓）。因此产品取「**解锁位开 + 尺寸选够竖的**」；
  **只有横屏尺寸**（1920×1080 / 1920×1800）配锁定方向的 App 才留黑边。
- **2026-10-07 收尾（第 6 处反射）**：**代价取消** —— 建屏配置上反射
  `VirtualDisplayConfig.Builder#setIgnoreActivitySizeRestrictions(true)`（**Android 16 起才有**，
  [`code-hiddenapi.md`](code-hiddenapi.md) §3 第 6 处）即取消"活动的尺寸限制" ⇒
  **1920×1800 实测满屏** `Rect(0,0-1920,1800)` ✓（此前 77% ✗），解锁位照旧 ✓。
  取得依据：同类工具 AndroMeld 反编译（同路由直调该隐藏方法；它在 Android 16 上另走 VDM 虚拟设备路线）
  + 本机 `framework.jar` 存在性 + api-versions 缺位。**该处可降级**（Android 14/15 拿不到只告警一次，
  退回 §4 的"尺寸够竖"退路）。
- **2026-10-07 VDM 实施（双端实测，取代上两条的机制结论）**：建屏改走**虚拟设备**
  （`wrappers/VirtualDevice.kt`：CDM 关联 → 建虚拟设备（`LOCK_STATE_ALWAYS_UNLOCKED`）→ 挂屏）。
  **机制纠正**：`ALWAYS_UNLOCKED` 与"铺满"的冲突**不是**因为"平台强塞 `OWN_DISPLAY_GROUP`"，
  而是因为**解锁位只对非默认组的屏有效**（默认组里平台会**直接丢弃**该位，AOSP
  `DisplayManagerService.createVirtualDisplayInternal`），而非默认组的屏必然采纳应用的固定方向请求；
  早期回读里的 `FLAG_OWN_DISPLAY_GROUP` 一直是 `DisplayFlags` **自己派生**的。
  **后果**：`DisplayFlags` 删除该派生；显示组交由虚拟设备（平台自动 `DEVICE_DISPLAY_GROUP`）；
  铺满由第 6 处开关（16+）或"尺寸够竖"（14/15）保证。
  **实测**：真机 16 建屏回读通过（`0xe80`，无 `FLAG_OWN_DISPLAY_GROUP`）、1920×1800 满屏、
  **锁屏与息屏两档照常出画**（像素取证）、PC 端到端出画（373 帧解出、0 丢弃）；
  AVD 15 同路径建屏成功（尺寸开关缺席 ⇒ 兜底生效）、1080×1920 满屏。
  ⚠️ 同时否证：`wm set-ignore-orientation-request -d <屏> true` **静默无效**（连主屏也改不动）。
  依据：[`plans/2026-10-07-vdm-mechanism-corrected.md`](../../plans/2026-10-07-vdm-mechanism-corrected.md)。
- **2026-10-07 裁切机制纠正（源码级复核；取代上面两条里的"尺寸规则 / 1.22"表述）**：
  裁切 = **fixed-orientation letterbox**（`dumpsys activity activities` 直接给
  `letterboxReason=FIXED_ORIENTATION`），**三条同时成立才裁**：① App 不可缩放且固定方向
  （`shouldCreateCompatDisplayInsets()`）；② 请求方向 ≠ 父（屏）方向（父方向按 `高 ≥ 宽 ⇒ PORTRAIT`）；
  ③ **屏不会为它旋转** —— VDM 建的屏实测 `mFixedToUserRotation=true`，`handlesOrientationChangeFromDescendant()`
  恒 false。窗口比例取**分屏比例**（AOSP 15 默认；模拟器实测：横屏 `宽 = (屏宽 − 30)/2`、高 = 屏高）
  或**配置值** `config_fixedOrientationLetterboxAspectRatio`（真机 16 = **1.22** ⇒ 窗口恒 `1/1.22 ≈ 0.819`，
  与屏尺寸无关）。**作废两条旧结论**：「14/15 只能靠 **高÷宽 ≥ 1.22**」（反例：模拟器 1024×768 = 1.333
  仍被裁成 497×768）、「屏 `ignoreOrientationRequest=false` ⇒ 被裁」（因果反了，见上）。
  实测：模拟器 A15 六档矩阵（1080×1080 / 1080×1920 **满屏**；四个横屏尺寸全部 = `(屏宽−30)/2 × 屏高`）、
  真机 16 复核 `setIgnoreActivitySizeRestrictions(true)` ⇒ 1920×1800 满屏。
  细节提成 [`agent/display-letterbox.md`](display-letterbox.md)（`display-flags.md` §4 只留摘要 + 链接）。
  **同批同步了代码里的旧说法**（用户批准；仅注释与告警文案，逻辑未动）：
  `display/Screen.kt`、`encode/CodecCaps.kt`、`encode/EncoderParams.kt`、`Main.kt`（含那条
  "尺寸够竖"的运行期告警文案）、`wrappers/ActivitySizeRestrictions.kt`、`wrappers/VirtualDevice.kt`
  —— 统一改为"活动级 letterbox（fixed-orientation）/ 屏方向 = App 请求方向"，并指向本文件。
  验证：`:agent:test` + `:agent:assembleRelease` 通过；模拟器实跑日志已只出新文案。
- **2026-10-07 补测：14/15 的"铺满"可行路与死路（模拟器 A15 实机，一次性探针）**：
  ① **死路**：关解锁位 / 留在默认显示组 / 置 `ignoreOrientationRequest=true` —— 四组组合**全部仍被裁**
  （1920×1080 上恒为 945×1080、`letterboxReason=FIXED_ORIENTATION`）⇒ round 4 那套**只在 Android 16 上成立**，
  **14/15 不可移植**（该标志在 15 的虚拟屏上默认 `false`，且设成 `true` 也无效）。
  ② **可行路**（任意尺寸都能铺满）：**让屏可旋转** —— 建屏后 `wm fixed-to-user-rotation -d <id> disabled`
  （= `cmd window fixed-to-user-rotation …`，shell 权限即可）⇒ 系统把屏转到 App 请求的方向，
  VDM 屏实测 `rot=3 cur=1080x1920`、ActivityRecord `[0,0][1080,1920]`、`areBoundsLetterboxed=false` ✓，
  **解锁位不受影响**；代价是屏逻辑方向翻转（帧缓冲仍 1920×1080，内容转 90°，PC 侧需按 rotation 旋转）。
  另一条老路：**按 App 方向建屏**（竖屏 App 配 `高 ≥ 宽`，1080×1080 / 1080×1920 满屏 ✓）。
  细节见 [`agent/display-letterbox.md`](display-letterbox.md) §6；探针用完即删。
- **2026-10-08 实施：屏可旋转（14/15 的"任意分辨率铺满"）**：新增 `display/DisplayRotation.kt` ——
  尺寸开关拿不到时（14/15）建屏后 exec `cmd window fixed-to-user-rotation -d <id> disabled` 并**回读确认**
  （可降级）；屏方向用公开 API `Display.getRotation()` 读，**变化**时经 `screenRotation` 主动上报
  （`DisplayListener` 回调在 `main`，去重状态只由回调持有，注册时不产生事件）。
  协议侧：`applied.rotation`（建屏那一刻，通常 0）+ `screenRotation` 事件（见 `transport-changelog.md`）。
  16+ 不设可旋转（尺寸开关已取消活动级 letterbox，再转只会让 PC 多转一次）。
  **实测**：AVD 15 上 1920×1080 / 1024×768 / 1600×900 **首拉即满屏**（可缩放 App）、PC 画面正立
  （`屏旋转 270°`、0 丢弃）；真机 16 `sizeRestrictionsIgnored=true` ⇒ 不转、1920×1800 满屏、1430 帧 0 丢弃
  ⇒ **无回归**。⚠️ 已知边界：**不可缩放**（`resizeableActivity=false`）App 首拉落 `SIZE_COMPAT_MODE`，
  重启一次即满屏（未决项见 [`plans/2026-10-08-screen-rotatable-implementation.md`](../../plans/2026-10-08-screen-rotatable-implementation.md) §4）。
- **2026-10-08 回退（产品决定：放弃"铺满"）**：上一条的整套机制**已撤销** —— `display/DisplayRotation.kt`
  与其单测删除，`Screen` / `Route` / `RouteDeps` / `Main` / `ReaderLoop` / `LinkLoop` / 两个 proto 的改动
  全部还原；协议里的 `applied.rotation` 与 `screenRotation` 事件同时撤销（见 `transport-changelog.md`）。
  **理由**：用户判断"允许屏旋转 ⇒ agent 还要检测 + 通知 PC"这条链路的**维护面不值**；B 方案（按 App 方向
  建屏）也不实现 ⇒ **产品直接放弃"铺满"这个目标**：14/15 上 App 被裁按平台行为接受。
  **保留**：Android 16+ 的尺寸开关（`setIgnoreActivitySizeRestrictions`）仍然生效（与本次机制无关）；
  机制与可选做法留档在 [`agent/display-letterbox.md`](display-letterbox.md) §6（标注"当前均未启用"）。
  验证：agent 单测回到 62 用例全绿、`:agent:assembleRelease` 通过；PC `cargo test` 24+23 通过、无警告。
