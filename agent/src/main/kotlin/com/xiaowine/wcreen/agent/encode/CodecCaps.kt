package com.xiaowine.wcreen.agent.encode

import android.media.MediaCodecInfo
import android.media.MediaCodecList
import android.media.MediaFormat
import com.xiaowine.wcreen.agent.proto.CapsMsg
import com.xiaowine.wcreen.agent.proto.CodecMsg
import com.xiaowine.wcreen.agent.proto.DefaultsMsg
import com.xiaowine.wcreen.agent.proto.RangeMsg
import com.xiaowine.wcreen.agent.runtime.Fail
import com.xiaowine.wcreen.agent.runtime.FailCode
import com.xiaowine.wcreen.agent.runtime.FailLevel
import com.xiaowine.wcreen.agent.runtime.Outcome

/**
 * 设备编码能力 —— **启动时探测一次**，全进程生命周期内不变。
 *
 * 只用**公开 API**（`MediaCodecList`），不需要隐藏 API、不需要建屏。
 * 表的内容与三条形态（枚举 / 范围+步进 / 整数）见 `docs/spec/agent/encode-probe.md` §4。
 *
 * ⚠️ **绝不可把本机实测数值硬编码**（`encode-capability.md` 的数值是某台设备的观测值）——
 * 一律读能力再判断。
 */
internal class CodecCaps private constructor(private val codecs: List<Codec>) {

    /**
     * 一个可用编码器的能力。
     *
     * @property name 编码器名
     * @property mime mime 类型
     * @property codec 归一化编码名（`H.264` / `HEVC`）
     * @property hardware 是否硬编（**如实上报**：`false` = 本机没有硬编时的软编回退）
     * @property profileIds 支持的 profile 常量值（**有序**，第一个是缺省）
     * @property colorStandards 支持的色彩标准（有序）
     * @property colorRanges 支持的色彩范围（有序）
     * @property colorTransfers 支持的色彩传递函数（有序）
     * @property widthMin 最小宽
     * @property widthMax 最大宽
     * @property widthStep 宽对齐
     * @property heightMin 最小高
     * @property heightMax 最大高
     * @property heightStep 高对齐
     * @property fpsMin 最小帧率
     * @property fpsMax 最大帧率
     * @property bitrateMin 最小码率（bit/s）
     * @property bitrateMax 最大码率（bit/s）
     * @property maxInstances 系统声明的并发实例数（**保证值，不是实测值**）
     * @property video 底层的 `VideoCapabilities`，供成对校验用
     */
    class Codec(
        val name: String,
        val mime: String,
        val codec: String,
        val hardware: Boolean,
        val profileIds: List<Int>,
        val colorStandards: List<Int>,
        val colorRanges: List<Int>,
        val colorTransfers: List<Int>,
        val widthMin: Int,
        val widthMax: Int,
        val widthStep: Int,
        val heightMin: Int,
        val heightMax: Int,
        val heightStep: Int,
        val fpsMin: Double,
        val fpsMax: Double,
        val bitrateMin: Int,
        val bitrateMax: Int,
        val maxInstances: Int,
        val video: MediaCodecInfo.VideoCapabilities,
    ) {
        /**
         * 优先 profile。
         *
         * 缺省规则（`encode-format.md` §1）：**优先默认**（H.264 → `High`、HEVC → `Main`），
         * 本机不支持才退到**第一个支持的**。不能直接取 `profileIds.first()` ——
         * `profileLevels` 的顺序是设备给的，实测第一个是 `Baseline`。
         *
         * @return profile 常量；一个都没有时为 -1
         */
        val preferredProfile: Int
            get() {
                val want = when (codec) {
                    CODEC_H264 -> MediaCodecInfo.CodecProfileLevel.AVCProfileHigh
                    CODEC_HEVC -> MediaCodecInfo.CodecProfileLevel.HEVCProfileMain
                    else -> -1
                }
                if (want >= 0 && profileIds.contains(want)) return want
                return profileIds.firstOrNull() ?: -1
            }
    }

    /**
     * 取缺省编码器 —— 规则是「优先 H.264，本机不支持则取第一个支持的」。
     *
     * @return 缺省编码器；一个可用的都没有时为 null
     */
    fun defaultCodec(): Codec? = codecs.firstOrNull { it.codec == CODEC_H264 } ?: codecs.firstOrNull()

    /**
     * 按名字取编码器。
     *
     * @param codec 归一化编码名（`H.264` / `HEVC`）；null 或空串表示取缺省
     * @return 编码器；没有可用的同类型编码器时为 null
     */
    fun pick(codec: String?): Codec? =
        if (codec.isNullOrEmpty()) defaultCodec() else codecs.firstOrNull { it.codec == codec }

    /**
     * 组装上报给 PC 的能力表。
     *
     * @param requestId PC 给的关联号，原样带回
     * @return 能力表消息
     */
    fun message(requestId: Int): CapsMsg {
        val default = defaultCodec()
        val codecMsgs = codecs.map { c ->
            CodecMsg(
                name = c.name,
                mime = c.mime,
                codec = c.codec,
                hardware = c.hardware,
                bitDepths = listOf(BIT_DEPTH_8),
                profiles = c.profileIds.map { profileName(c.codec, it) }.filter { it.isNotEmpty() },
                colorStandards = c.colorStandards.map { colorStandardName(it) },
                colorRanges = c.colorRanges.map { colorRangeName(it) },
                colorTransfers = c.colorTransfers.map { colorTransferName(it) },
                width = RangeMsg(c.widthMin, c.widthMax, c.widthStep),
                height = RangeMsg(c.heightMin, c.heightMax, c.heightStep),
                refreshRate = RangeMsg(c.fpsMin.toInt().coerceAtLeast(1), c.fpsMax.toInt(), 1),
                bitrate = RangeMsg(c.bitrateMin, c.bitrateMax, 1),
                maxInstances = c.maxInstances,
            )
        }
        return CapsMsg(
            requestId = requestId,
            codecs = codecMsgs,
            defaults = DefaultsMsg(
                codec = default?.codec ?: "",
                bitDepth = BIT_DEPTH_8,
                profile = default?.let { profileName(it.codec, it.preferredProfile) } ?: "",
                colorStandard = COLOR_STANDARD_BT709_NAME,
                colorRange = COLOR_RANGE_FULL_NAME,
                colorTransfer = COLOR_TRANSFER_SDR_VIDEO_NAME,
                bitrateBpp = DEFAULT_BITRATE_BPP,
                bitrateMode = BITRATE_MODE_VBR_NAME,
                refreshRate = DEFAULT_REFRESH_RATE,
                iFrameInterval = I_FRAME_INTERVAL_SECONDS,
                showSystemDecorations = false,
                // 缺省开：保证"主屏锁屏时虚拟屏照常可用"；代价（固定方向的 App 被裁）用"**屏方向 =
                // App 请求方向**"规避 —— 见 `docs/spec/agent/display-letterbox.md`。
                alwaysUnlocked = true,
            ),
        )
    }

    /**
     * 能力表的一句话摘要（启动日志用）—— **硬编/软编如实分开计**。
     *
     * @return 形如 `7 个编码器（硬编 7 / 软编 0）`
     */
    fun summary(): String {
        val hw = codecs.count { it.hardware }
        return "${codecs.size} 个编码器（硬编 $hw / 软编 ${codecs.size - hw}）"
    }

    /**
     * 编码器的可读名字，供日志与回报。
     *
     * @param codec 归一化编码名
     * @return 实际编码器名；找不到时为空串
     */
    fun codecName(codec: String): String = codecs.firstOrNull { it.codec == codec }?.name ?: ""

    /** 探测与命名。 */
    companion object {

        /** 归一化编码名：H.264。 */
        const val CODEC_H264: String = "H.264"

        /** 归一化编码名：HEVC。 */
        const val CODEC_HEVC: String = "HEVC"

        /** 位深（本片只做 8-bit；10-bit 与 HDR 留作后续特性）。 */
        const val BIT_DEPTH_8: Int = 8

        /** 缺省码率系数（**策略常量**，PC 可覆盖）。 */
        const val DEFAULT_BITRATE_BPP: Double = 0.1

        /** 缺省刷新率（Hz）。 */
        const val DEFAULT_REFRESH_RATE: Int = 60

        /** 关键帧间隔（秒）—— 不能长（恢复时间）也不能短（平均码率）。 */
        const val I_FRAME_INTERVAL_SECONDS: Int = 1

        /** 码率模式名：VBR（缺省）。 */
        const val BITRATE_MODE_VBR_NAME: String = "VBR"

        /** 码率模式名：CBR。 */
        const val BITRATE_MODE_CBR_NAME: String = "CBR"

        /** 色彩标准名：BT.709。 */
        const val COLOR_STANDARD_BT709_NAME: String = "BT709"

        /** 色彩范围名：FULL。 */
        const val COLOR_RANGE_FULL_NAME: String = "FULL"

        /** 色彩传递函数名：SDR_VIDEO。 */
        const val COLOR_TRANSFER_SDR_VIDEO_NAME: String = "SDR_VIDEO"

        /** mime → 归一化编码名；不支持的 mime 返回空串。 */
        private fun normalize(mime: String): String = when (mime) {
            MediaFormat.MIMETYPE_VIDEO_AVC -> CODEC_H264
            MediaFormat.MIMETYPE_VIDEO_HEVC -> CODEC_HEVC
            else -> ""
        }

        /**
         * profile 常量 → 名字。
         *
         * @param codec 归一化编码名
         * @param profile `CodecProfileLevel` 常量
         * @return 名字；不认识的取值返回空串（**不猜**）
         */
        fun profileName(codec: String, profile: Int): String = when (codec) {
            CODEC_H264 -> when (profile) {
                MediaCodecInfo.CodecProfileLevel.AVCProfileBaseline -> "Baseline"
                MediaCodecInfo.CodecProfileLevel.AVCProfileMain -> "Main"
                MediaCodecInfo.CodecProfileLevel.AVCProfileHigh -> "High"
                else -> ""
            }

            CODEC_HEVC -> when (profile) {
                MediaCodecInfo.CodecProfileLevel.HEVCProfileMain -> "Main"
                MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10 -> "Main10"
                MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10HDR10 -> "Main10HDR10"
                else -> ""
            }

            else -> ""
        }

        /**
         * profile 名字 → 常量。
         *
         * @param codec 归一化编码名
         * @param name profile 名
         * @return 常量；不认识返回 -1
         */
        fun profileId(codec: String, name: String): Int = when (codec) {
            CODEC_H264 -> when (name) {
                "Baseline" -> MediaCodecInfo.CodecProfileLevel.AVCProfileBaseline
                "Main" -> MediaCodecInfo.CodecProfileLevel.AVCProfileMain
                "High" -> MediaCodecInfo.CodecProfileLevel.AVCProfileHigh
                else -> -1
            }

            CODEC_HEVC -> when (name) {
                "Main" -> MediaCodecInfo.CodecProfileLevel.HEVCProfileMain
                "Main10" -> MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10
                "Main10HDR10" -> MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10HDR10
                else -> -1
            }

            else -> -1
        }

        /**
         * 色彩标准常量 → 名字。
         *
         * @param standard `MediaFormat.COLOR_STANDARD_*`
         * @return 名字；不认识返回空串
         */
        fun colorStandardName(standard: Int): String = when (standard) {
            MediaFormat.COLOR_STANDARD_BT709 -> COLOR_STANDARD_BT709_NAME
            MediaFormat.COLOR_STANDARD_BT601_PAL -> "BT601_PAL"
            MediaFormat.COLOR_STANDARD_BT601_NTSC -> "BT601_NTSC"
            MediaFormat.COLOR_STANDARD_BT2020 -> "BT2020"
            else -> ""
        }

        /**
         * 色彩标准名字 → 常量。
         *
         * @param name 名字
         * @return 常量；不认识返回 -1
         */
        fun colorStandardId(name: String): Int = when (name) {
            COLOR_STANDARD_BT709_NAME -> MediaFormat.COLOR_STANDARD_BT709
            "BT601_PAL" -> MediaFormat.COLOR_STANDARD_BT601_PAL
            "BT601_NTSC" -> MediaFormat.COLOR_STANDARD_BT601_NTSC
            "BT2020" -> MediaFormat.COLOR_STANDARD_BT2020
            else -> -1
        }

        /**
         * 色彩范围常量 → 名字。
         *
         * @param range `MediaFormat.COLOR_RANGE_*`
         * @return 名字；不认识返回空串
         */
        fun colorRangeName(range: Int): String = when (range) {
            MediaFormat.COLOR_RANGE_FULL -> COLOR_RANGE_FULL_NAME
            MediaFormat.COLOR_RANGE_LIMITED -> "LIMITED"
            else -> ""
        }

        /**
         * 色彩范围名字 → 常量。
         *
         * @param name 名字
         * @return 常量；不认识返回 -1
         */
        fun colorRangeId(name: String): Int = when (name) {
            COLOR_RANGE_FULL_NAME -> MediaFormat.COLOR_RANGE_FULL
            "LIMITED" -> MediaFormat.COLOR_RANGE_LIMITED
            else -> -1
        }

        /**
         * 色彩传递函数常量 → 名字。
         *
         * @param transfer `MediaFormat.COLOR_TRANSFER_*`
         * @return 名字；不认识返回空串
         */
        fun colorTransferName(transfer: Int): String = when (transfer) {
            MediaFormat.COLOR_TRANSFER_SDR_VIDEO -> COLOR_TRANSFER_SDR_VIDEO_NAME
            MediaFormat.COLOR_TRANSFER_LINEAR -> "LINEAR"
            MediaFormat.COLOR_TRANSFER_ST2084 -> "ST2084"
            MediaFormat.COLOR_TRANSFER_HLG -> "HLG"
            else -> ""
        }

        /**
         * 色彩传递函数名字 → 常量。
         *
         * @param name 名字
         * @return 常量；不认识返回 -1
         */
        fun colorTransferId(name: String): Int = when (name) {
            COLOR_TRANSFER_SDR_VIDEO_NAME -> MediaFormat.COLOR_TRANSFER_SDR_VIDEO
            "LINEAR" -> MediaFormat.COLOR_TRANSFER_LINEAR
            "ST2084" -> MediaFormat.COLOR_TRANSFER_ST2084
            "HLG" -> MediaFormat.COLOR_TRANSFER_HLG
            else -> -1
        }

        /**
         * 探测本机的编码能力。
         *
         * **优先硬编**：先只收 `isHardwareAccelerated()` 为 true 的编码器；
         * **一个硬编都没有时退软编**（用户取向 2026-10-07：模拟器等无硬编环境也要能跑），
         * 并在能力表里把 `hardware=false` **如实上报** —— 由 PC 侧决定是否接受。
         * 每个 mime 下可能有多个编码器且能力差极大，所以**逐个列出**，不合并。
         *
         * @return 能力集合；**硬编与软编都没有**时返回子系统级失败
         */
        fun probe(): Outcome<CodecCaps> {
            val list = MediaCodecList(MediaCodecList.REGULAR_CODECS)
            val hardware = collect(list, requireHardware = true)
            if (hardware.isNotEmpty()) return Outcome.Ok(CodecCaps(hardware))

            val software = collect(list, requireHardware = false)
            if (software.isEmpty()) {
                return Outcome.Err(
                    Fail(
                        FailLevel.SUBSYSTEM,
                        FailCode.NO_HW_ENCODER,
                        "本机没有任何可用编码器（硬编与软编都没有）",
                    ),
                )
            }
            return Outcome.Ok(CodecCaps(software))
        }

        /**
         * 扫一遍编码器清单，收下符合"是否硬编"这一条的那批。
         *
         * @param list 编码器清单
         * @param requireHardware true = 只要硬编；false = 只要软编
         * @return 编码器能力集合（可能为空）
         */
        private fun collect(list: MediaCodecList, requireHardware: Boolean): List<Codec> {
            val found = ArrayList<Codec>()

            for (info in list.codecInfos) {
                if (!info.isEncoder || info.isHardwareAccelerated != requireHardware) continue
                for (mime in info.supportedTypes) {
                    val codec = normalize(mime)
                    if (codec.isEmpty()) continue
                    // 非视频编码器没有 VideoCapabilities；取不到就跳过这个 mime。
                    val caps = try {
                        info.getCapabilitiesForType(mime)
                    } catch (e: Exception) {
                        continue
                    }
                    val video = try {
                        caps.videoCapabilities
                    } catch (e: Exception) {
                        continue
                    } ?: continue

                    val profiles = caps.profileLevels
                        .map { it.profile }
                        .filter { profileName(codec, it).isNotEmpty() }
                        .distinct()
                    val widths = video.supportedWidths
                    val heights = video.supportedHeights
                    val fps = video.supportedFrameRates

                    found += Codec(
                        name = info.name,
                        mime = mime,
                        codec = codec,
                        hardware = requireHardware,
                        profileIds = profiles,
                        // 色彩标准不是"能力枚举"而是"可设置的取值"，编码器不逐个上报；
                        // 这里给出协议允许且实测可用的那几项，实际生效值靠回读确认。
                        colorStandards = listOf(
                            MediaFormat.COLOR_STANDARD_BT709,
                            MediaFormat.COLOR_STANDARD_BT601_NTSC,
                            MediaFormat.COLOR_STANDARD_BT601_PAL,
                        ),
                        colorRanges = listOf(MediaFormat.COLOR_RANGE_LIMITED, MediaFormat.COLOR_RANGE_FULL),
                        colorTransfers = listOf(MediaFormat.COLOR_TRANSFER_SDR_VIDEO),
                        widthMin = widths.lower,
                        widthMax = widths.upper,
                        widthStep = video.widthAlignment,
                        heightMin = heights.lower,
                        heightMax = heights.upper,
                        heightStep = video.heightAlignment,
                        fpsMin = fps.lower.toDouble(),
                        fpsMax = fps.upper.toDouble(),
                        bitrateMin = video.bitrateRange.lower,
                        bitrateMax = video.bitrateRange.upper,
                        maxInstances = caps.maxSupportedInstances,
                        video = video,
                    )
                }
            }
            return found
        }
    }
}
