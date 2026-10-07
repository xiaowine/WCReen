package com.xiaowine.wcreen.agent.proto

/*
 * 能力表（`0x12` 请求 / `0x13` 应答）的 DTO。
 *
 * 形态纪律同 `Control.kt`：字段名与协议逐字一致、每个字段都有默认值、用 `var`、必须进 `-keep`。
 *
 * 结构依据 `docs/spec/agent/encode-probe.md` §4 —— 三类形式（枚举 / 范围+步进 / 整数）都要能表达，
 * 且**每一行都带缺省取值**、**逐编码器列出**、**枚举必须有序**（回退规则是"取第一个支持的"）。
 */

/** 请求能力表（PC→agent，`0x12`）。 */
internal class CapsRequestMsg(var requestId: Int = 0)

/**
 * 能力表（agent→PC，`0x13`）。
 *
 * **逐编码器列出**，不能只报一个 —— 同一 mime 下可能有多个编码器且能力差极大。
 */
internal class CapsMsg(
    var requestId: Int = 0,
    var codecs: List<CodecMsg> = emptyList(),
    var defaults: DefaultsMsg = DefaultsMsg(),
)

/**
 * 一个编码器的能力。
 *
 * @property name 编码器名（`MediaCodecInfo.getName()`）
 * @property mime mime 类型
 * @property codec 归一化后的编码名（`H.264` / `HEVC`）—— PC 侧按它做枚举
 * @property hardware 是否硬编；**如实上报** —— 本机没有硬编时给出的是软编回退（`false`），
 *   由 PC 侧决定是否接受（`encode.md` §1）
 * @property bitDepths 支持的位深（有序）
 * @property profiles 支持的 profile 名（**有序**，第一个是缺省）
 * @property colorStandards 支持的色彩标准（有序）
 * @property colorRanges 支持的色彩范围（有序）
 * @property colorTransfers 支持的色彩传递函数（有序）
 * @property width 宽度范围与对齐步进
 * @property height 高度范围与对齐步进
 * @property refreshRate 刷新率范围（= 编码帧率）
 * @property bitrate 码率范围
 * @property maxInstances 系统声明的并发实例数（**是"保证值"不是"实测值"**）
 */
internal class CodecMsg(
    var name: String = "",
    var mime: String = "",
    var codec: String = "",
    var hardware: Boolean = false,
    var bitDepths: List<Int> = emptyList(),
    var profiles: List<String> = emptyList(),
    var colorStandards: List<String> = emptyList(),
    var colorRanges: List<String> = emptyList(),
    var colorTransfers: List<String> = emptyList(),
    var width: RangeMsg = RangeMsg(),
    var height: RangeMsg = RangeMsg(),
    var refreshRate: RangeMsg = RangeMsg(),
    var bitrate: RangeMsg = RangeMsg(),
    var maxInstances: Int = 0,
)

/**
 * 一个数值范围。
 *
 * @property min 下界（含）
 * @property max 上界（含）
 * @property step 步进；1 表示无对齐约束
 */
internal class RangeMsg(
    var min: Int = 0,
    var max: Int = 0,
    var step: Int = 1,
)

/**
 * 缺省取值 —— PC 一个字段都不填时，也能**事先知道** agent 会选什么。
 *
 * @property codec 缺省编码名
 * @property bitDepth 缺省位深
 * @property profile 缺省 profile 名
 * @property colorStandard 缺省色彩标准
 * @property colorRange 缺省色彩范围
 * @property colorTransfer 缺省色彩传递函数
 * @property bitrateBpp 缺省码率系数（策略常量，PC 可覆盖）
 * @property bitrateMode 缺省码率模式（`VBR` / `CBR`）
 * @property refreshRate 缺省刷新率
 * @property iFrameInterval 关键帧间隔（秒）
 * @property showSystemDecorations 缺省是否显示系统装饰
 * @property alwaysUnlocked 缺省是否保持解锁态
 */
internal class DefaultsMsg(
    var codec: String = "",
    var bitDepth: Int = 0,
    var profile: String = "",
    var colorStandard: String = "",
    var colorRange: String = "",
    var colorTransfer: String = "",
    var bitrateBpp: Double = 0.0,
    var bitrateMode: String = "",
    var refreshRate: Int = 0,
    var iFrameInterval: Int = 0,
    var showSystemDecorations: Boolean = false,
    var alwaysUnlocked: Boolean = true,
)
