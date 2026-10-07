package com.xiaowine.wcreen.agent.proto

/**
 * 协议时间常量。
 *
 * 心跳周期是**协议约定的一部分**（PC 侧要用同一个值做对等判据），所以放在 `proto/`
 * 而不是某一侧的运行时代码里。
 */
internal object Timing {

    /** 心跳周期：agent → PC 固定每 3s 一条。 */
    const val HEARTBEAT_PERIOD_MS: Long = 3_000

    /**
     * 判活超时 `T_dead`：`T_dead` 内**没有收到对端任何消息**即判死（不是"没收到心跳"）。
     *
     * 它必须 **> 心跳周期**，否则正常的心跳间隔本身就会被判死；
     * 它还兼职兜住「PC 不读」—— PC 挂死时写者会卡在 `write`，判死后拆掉写侧即可唤醒它
     * ⇒ 因此它必须够短，不按传输参数化（3s 对 adb 与 Wi-Fi 都安全）。
     */
    const val T_DEAD_MS: Long = 10_000
}
