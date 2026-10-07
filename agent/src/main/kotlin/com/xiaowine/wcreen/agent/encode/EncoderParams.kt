package com.xiaowine.wcreen.agent.encode

import android.media.MediaCodecInfo
import android.media.MediaFormat
import com.xiaowine.wcreen.agent.proto.ScreenParamsMsg
import com.xiaowine.wcreen.agent.runtime.Fail
import com.xiaowine.wcreen.agent.runtime.FailCode
import com.xiaowine.wcreen.agent.runtime.FailLevel
import com.xiaowine.wcreen.agent.runtime.Outcome

/**
 * 一次编码会话的**已解析、已校验**参数。
 *
 * 解析规则（`docs/spec/agent/encode-params.md` / `encode-format.md` / `encode-probe.md` §5）：
 * 每个字段**优先 PC 给的值**，缺省则按设备推导；**尺寸与帧率必须成对校验**；
 * PC 显式给的值不满足即**拒绝**（带具体哪一项不满足），**不静默修正、不静默降级**。
 *
 * @property codec 选中的编码器
 * @property width 编码宽 —— **必须等于屏宽**（不允许 GPU 缩放）
 * @property height 编码高
 * @property refreshRate 刷新率（Hz），同时是编码帧率
 * @property dpi 屏密度
 * @property bitrate 码率（bit/s），已按编码器能力夹过
 * @property bitrateMode `MediaCodecInfo.EncoderCapabilities.BITRATE_MODE_*`
 * @property profileId `CodecProfileLevel` 常量
 * @property colorStandard `MediaFormat.COLOR_STANDARD_*`
 * @property colorRange `MediaFormat.COLOR_RANGE_*`
 * @property colorTransfer `MediaFormat.COLOR_TRANSFER_*`
 * @property showSystemDecorations 屏是否显示系统装饰
 * @property alwaysUnlocked 屏是否保持解锁态
 * @property screenName 屏名
 */
internal class EncoderParams(
    val codec: CodecCaps.Codec,
    val width: Int,
    val height: Int,
    val refreshRate: Int,
    val dpi: Int,
    val bitrate: Int,
    val bitrateMode: Int,
    val profileId: Int,
    val colorStandard: Int,
    val colorRange: Int,
    val colorTransfer: Int,
    val showSystemDecorations: Boolean,
    val alwaysUnlocked: Boolean,
    val screenName: String,
) {

    /** 归一化编码名，回报用。 */
    val codecName: String get() = codec.codec

    /** 实际编码器名，回报用。 */
    val encoderName: String get() = codec.name

    /** 是否硬编，回报用（`false` = 本机无硬编时的软编回退）。 */
    val hardware: Boolean get() = codec.hardware

    /** profile 名，回报用。 */
    val profileName: String get() = CodecCaps.profileName(codec.codec, profileId)

    /** 码率模式名，回报用。 */
    val bitrateModeName: String
        get() = if (bitrateMode == MediaCodecInfo.EncoderCapabilities.BITRATE_MODE_CBR) {
            CodecCaps.BITRATE_MODE_CBR_NAME
        } else {
            CodecCaps.BITRATE_MODE_VBR_NAME
        }

    /** 解析与校验。 */
    companion object {

        /**
         * 把 PC 的请求解析成一份可直接 `configure` 的参数。
         *
         * @param params PC 的建屏参数（**每个字段都可缺省**）
         * @param caps 设备能力
         * @param screenWidth 缺省宽（主屏分辨率）
         * @param screenHeight 缺省高（主屏分辨率）
         * @param screenDpi 缺省密度（主屏 densityDpi）
         * @param screenName 缺省屏名
         * @return 成功，或可恢复级失败（**带具体哪一项不满足**）
         */
        fun resolve(
            params: ScreenParamsMsg,
            caps: CodecCaps,
            screenWidth: Int,
            screenHeight: Int,
            screenDpi: Int,
            screenName: String,
        ): Outcome<EncoderParams> {
            val codec = caps.pick(params.codec.ifEmpty { null })
                ?: return reject("设备没有可用的 '${params.codec}' 硬编编码器")

            val width = if (params.width > 0) params.width else screenWidth
            val height = if (params.height > 0) params.height else screenHeight
            val refreshRate = if (params.refreshRate > 0) params.refreshRate else CodecCaps.DEFAULT_REFRESH_RATE
            val dpi = if (params.dpi > 0) params.dpi else screenDpi

            // —— 尺寸与帧率成对校验（全部通过才应用）——
            if (width % codec.widthStep != 0 || height % codec.heightStep != 0) {
                return reject(
                    "尺寸 ${width}x$height 不满足对齐：宽需 $width step ${codec.widthStep}、高需 ${codec.heightStep}",
                )
            }
            if (!codec.video.isSizeSupported(width, height)) {
                return reject("编码器 ${codec.name} 不支持尺寸 ${width}x$height")
            }
            val widthsFor = try {
                codec.video.getSupportedWidthsFor(height)
            } catch (e: Exception) {
                null
            }
            if (widthsFor != null && !widthsFor.contains(width)) {
                return reject("高度 $height 下编码器只支持宽度 ${widthsFor.lower}..${widthsFor.upper}")
            }
            val heightsFor = try {
                codec.video.getSupportedHeightsFor(width)
            } catch (e: Exception) {
                null
            }
            if (heightsFor != null && !heightsFor.contains(height)) {
                return reject("宽度 $width 下编码器只支持高度 ${heightsFor.lower}..${heightsFor.upper}")
            }
            if (!codec.video.areSizeAndRateSupported(width, height, refreshRate.toDouble())) {
                val range = try {
                    codec.video.getSupportedFrameRatesFor(width, height)
                } catch (e: Exception) {
                    null
                }
                val hint = if (range != null) "（该尺寸下帧率范围 ${range.lower}..${range.upper}）" else ""
                return reject("尺寸 ${width}x$height 下不支持 ${refreshRate}Hz$hint")
            }

            // —— 码率：系数 > 固定值 > 缺省系数（系数是策略常量，与"绝不硬编码平台值"无关）——
            val bpp = if (params.bitrateBpp > 0.0) params.bitrateBpp else CodecCaps.DEFAULT_BITRATE_BPP
            val computed = if (params.bitrate > 0 && params.bitrateBpp <= 0.0) {
                params.bitrate.toLong()
            } else {
                (width.toLong() * height.toLong() * refreshRate.toLong() * bpp).toLong()
            }
            val bitrate = computed.coerceIn(codec.bitrateMin.toLong(), codec.bitrateMax.toLong()).toInt()

            // —— 格式组：PC 指定了就用它（不支持即拒绝）；没指定才按"优先默认 → 第一个支持的"回退 ——
            val profileId = if (params.profile.isNotEmpty()) {
                val requested = CodecCaps.profileId(codec.codec, params.profile)
                if (requested < 0 || !codec.profileIds.contains(requested)) {
                    return reject("编码器 ${codec.name} 不支持 profile '${params.profile}'")
                }
                requested
            } else {
                val preferred = codec.preferredProfile
                if (preferred < 0) return reject("编码器 ${codec.name} 没有可用 profile")
                preferred
            }

            val colorStandard = resolveColor(
                requested = params.colorStandard,
                supported = codec.colorStandards,
                toId = CodecCaps::colorStandardId,
                fallback = MediaFormat.COLOR_STANDARD_BT709,
                label = "色彩标准",
            ) ?: return reject("编码器不支持色彩标准 '${params.colorStandard}'")

            val colorRange = resolveColor(
                requested = params.colorRange,
                supported = codec.colorRanges,
                toId = CodecCaps::colorRangeId,
                fallback = MediaFormat.COLOR_RANGE_FULL,
                label = "色彩范围",
            ) ?: return reject("编码器不支持色彩范围 '${params.colorRange}'")

            val colorTransfer = resolveColor(
                requested = params.colorTransfer,
                supported = codec.colorTransfers,
                toId = CodecCaps::colorTransferId,
                fallback = MediaFormat.COLOR_TRANSFER_SDR_VIDEO,
                label = "色彩传递函数",
            ) ?: return reject("编码器不支持色彩传递函数 '${params.colorTransfer}'")

            val bitrateMode = when (params.bitrateMode) {
                CodecCaps.BITRATE_MODE_CBR_NAME -> MediaCodecInfo.EncoderCapabilities.BITRATE_MODE_CBR
                else -> MediaCodecInfo.EncoderCapabilities.BITRATE_MODE_VBR
            }

            return Outcome.Ok(
                EncoderParams(
                    codec = codec,
                    width = width,
                    height = height,
                    refreshRate = refreshRate,
                    dpi = dpi,
                    bitrate = bitrate,
                    bitrateMode = bitrateMode,
                    profileId = profileId,
                    colorStandard = colorStandard,
                    colorRange = colorRange,
                    colorTransfer = colorTransfer,
                    // 两个可配位：PC 没给就用缺省（装饰关、解锁开）。
                    showSystemDecorations = params.showSystemDecorations,
                    alwaysUnlocked = if (params.alwaysUnlocked) true else DEFAULT_ALWAYS_UNLOCKED,
                    screenName = params.screenName.ifEmpty { screenName },
                ),
            )
        }

        /**
         * 缺省解锁态 —— **缺省开**（屏上不显示 keyguard，且主屏锁屏 / 息屏都不接管本屏）。
         *
         * 产品取向（2026-10-07 定）：虚拟屏与**主屏的锁态解耦** —— 已像素级取证：主屏
         * `mWakefulness=Dozing`（物理屏 OFF）与 `mKeyguardShowing=true` 两档下，本屏照常出画
         * （见 `docs/plans/2026-10-07-vdm-mechanism-corrected.md` §2 #3/#4）。
         *
         * 代价：**已不存在**。屏挂在虚拟设备上（`wrappers/VirtualDevice.kt`），解锁位不再与"铺满"
         * 冲突：Android 16 起由 `setIgnoreActivitySizeRestrictions` 取消**活动级 letterbox**；
         * 14/15 没有该开关 ⇒ 这类 App 可能被裁（产品决定不为此另做动作，见
         * `docs/spec/agent/display-letterbox.md` §6）。
         */
        private const val DEFAULT_ALWAYS_UNLOCKED: Boolean = true

        /**
         * 解析一个色彩字段：PC 给了就用它（不支持则拒绝），没给就按"优先默认 → 第一个支持的"。
         *
         * @param requested PC 给的名字（空串表示没给）
         * @param supported 编码器支持的取值
         * @param toId 名字 → 常量
         * @param fallback 优先默认
         * @param label 字段名，用于报错
         * @return 常量；无法满足时 null
         */
        private fun resolveColor(
            requested: String,
            supported: List<Int>,
            toId: (String) -> Int,
            fallback: Int,
            label: String,
        ): Int? {
            if (requested.isEmpty()) {
                return if (supported.contains(fallback)) fallback else supported.firstOrNull()
            }
            val id = toId(requested)
            if (id < 0) return null
            return if (supported.contains(id)) id else null
        }

        /**
         * 造一个可恢复级失败 —— 建屏失败是资源类，PC 可稍后重试。
         *
         * @param message 具体哪一项不满足
         * @return 恒为失败
         */
        private fun reject(message: String): Outcome<EncoderParams> =
            Outcome.Err(Fail(FailLevel.RECOVERABLE, FailCode.SCREEN_CREATE, message))
    }
}
