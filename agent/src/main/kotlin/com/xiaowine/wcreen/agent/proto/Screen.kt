package com.xiaowine.wcreen.agent.proto

/*
 * 「申请屏幕」指令（`0x20` 的 `applyScreen`）与其回报的 DTO。
 *
 * 形态纪律同 `Control.kt`。字段语义出处：`docs/spec/agent/display.md` §2.0、
 * `encode-params.md` §2 / §3、`encode-format.md`、`display-flags.md` §3。
 */

/**
 * 「申请屏幕」指令的**载荷**（`0x20` 的 `applyScreen`，落在信封的 `payload` 里）。
 *
 * 它不再是「消息」—— 不含 `cmd` / `requestId`（那两个在信封上）。
 *
 * @property streamId 重连时带回来认领冻结的屏；不带或为 0 表示"建新屏"
 * @property params 建屏参数，**全部可缺省**（缺省由 agent 按设备推导）
 */
internal class ApplyScreenArgs(
    var streamId: Int = 0,
    var params: ScreenParamsMsg = ScreenParamsMsg(),
)

/**
 * 建屏参数 —— **全部可缺省**，实际生效值必须回报。
 *
 * @property width 宽（像素）；缺省 = 主屏分辨率
 * @property height 高（像素）；缺省 = 主屏分辨率
 * @property refreshRate 刷新率（Hz）；缺省 60。**建屏期参数，会话内固定**，它同时是"帧率"的唯一控制手段
 * @property dpi 密度；缺省 = 主屏 densityDpi
 * @property bitrateBpp 码率系数（bpp）；与 [bitrate] 二选一，**系数优先**
 * @property bitrate 码率绝对值（bit/s）；缺省由 [bitrateBpp] 推导
 * @property bitrateMode 码率模式 `VBR`（缺省）/ `CBR`
 * @property codec 编码名 `H.264`（缺省）/ `HEVC`
 * @property bitDepth 位深；缺省 8
 * @property profile profile 名；缺省 H.264 → `High`，HEVC → `Main`
 * @property colorStandard 色彩标准；缺省 `BT709`
 * @property colorRange 色彩范围；缺省 `FULL`
 * @property colorTransfer 色彩传递函数；缺省 `SDR_VIDEO`
 * @property showSystemDecorations 是否显示系统装饰；缺省关
 * @property alwaysUnlocked 是否保持解锁态；**缺省开**（主屏锁屏 / 息屏不接管本屏；
 *   屏挂虚拟设备 ⇒ 该位不再与"铺满"冲突，见
 *   `docs/plans/2026-10-07-vdm-mechanism-corrected.md`）
 * @property screenName 屏名；缺省 `WCReen-<序号>`
 */
internal class ScreenParamsMsg(
    var width: Int = 0,
    var height: Int = 0,
    var refreshRate: Int = 0,
    var dpi: Int = 0,
    var bitrateBpp: Double = 0.0,
    var bitrate: Int = 0,
    var bitrateMode: String = "",
    var codec: String = "",
    var bitDepth: Int = 0,
    var profile: String = "",
    var colorStandard: String = "",
    var colorRange: String = "",
    var colorTransfer: String = "",
    var showSystemDecorations: Boolean = false,
    var alwaysUnlocked: Boolean = false,
    var screenName: String = "",
)

/**
 * 「申请屏幕」回报里 `applied` 部分 —— **每个参数的实际生效值**。
 *
 * ⚠️ `applyScreen` **允许部分成功**：例如码率生效了但尺寸不能原地改。
 * 此时回报 `ok: false` + `error` + `applied`（已生效的那部分），**不丢信息**。
 *
 * @property streamId agent 分配的屏标识
 * @property width 实际生效宽
 * @property height 实际生效高
 * @property refreshRate 实际生效刷新率
 * @property dpi 实际生效密度
 * @property bitrate 实际生效码率（bit/s，已按编码器能力夹过）
 * @property bitrateMode 实际生效码率模式
 * @property codec 实际生效编码名
 * @property codecName 实际使用的编码器名（诊断用）
 * @property hardware 实际使用的是否硬编（`false` = 软编回退）
 * @property bitDepth 实际生效位深
 * @property profile 实际生效 profile 名
 * @property colorStandard 实际生效色彩标准
 * @property colorRange 实际生效色彩范围
 * @property colorTransfer 实际生效色彩传递函数
 * @property showSystemDecorations 实际生效的装饰位
 * @property alwaysUnlocked 实际生效的解锁位
 * @property ownDisplayGroup 实际是否派生了独立显示组（**恒 false**：新路径不请求该位，且回读校验
 *   把该位写进掩码 —— 一旦平台置上就是回读不符 ⇒ 建屏失败）
 * @property screenName 实际屏名
 */
internal class AppliedMsg(
    var streamId: Int = 0,
    var width: Int = 0,
    var height: Int = 0,
    var refreshRate: Int = 0,
    var dpi: Int = 0,
    var bitrate: Int = 0,
    var bitrateMode: String = "",
    var codec: String = "",
    var codecName: String = "",
    var hardware: Boolean = false,
    var bitDepth: Int = 0,
    var profile: String = "",
    var colorStandard: String = "",
    var colorRange: String = "",
    var colorTransfer: String = "",
    var showSystemDecorations: Boolean = false,
    var alwaysUnlocked: Boolean = false,
    var ownDisplayGroup: Boolean = false,
    var screenName: String = "",
)
