package com.xiaowine.wcreen.agent.transport

import com.xiaowine.wcreen.agent.proto.ErrorCode
import com.xiaowine.wcreen.agent.runtime.Fail
import com.xiaowine.wcreen.agent.runtime.FailLevel
import com.xiaowine.wcreen.agent.runtime.Outcome

/**
 * 握手 —— 只做**版本校验**。
 *
 * 版本必须**精确相等**：agent 是推到设备上的 jar，很容易残留旧版 —— 精确匹配能立刻
 * 发现「设备上跑的是旧 jar」，而不是产生一堆诡异行为。不匹配时回报双方版本再断开。
 *
 * 只依赖 JDK 类型 ⇒ 可进 JVM 单测。
 */
internal object Handshake {

    /** 协议版本 —— 协议变更即 +1，与产品版本分开。 */
    const val PROTOCOL_VERSION: Int = 1

    /**
     * 校验对端版本。
     *
     * @param peerVersion 对端在握手首包里给的版本号
     * @return 相等则成功；否则**单路级**失败（只断这一条连接，其余路不受影响）
     */
    fun checkVersion(peerVersion: Int): Outcome<Unit> =
        if (peerVersion == PROTOCOL_VERSION) {
            Outcome.Ok(Unit)
        } else {
            Outcome.Err(
                Fail(
                    FailLevel.STREAM,
                    ErrorCode.VERSION_MISMATCH,
                    "协议版本不匹配：本机 $PROTOCOL_VERSION，对端 $peerVersion —— 请重推 agent",
                ),
            )
        }
}
