package com.xiaowine.wcreen.agent.runtime

import com.xiaowine.wcreen.agent.encode.CodecCaps
import com.xiaowine.wcreen.agent.wrappers.InputInjector
import com.xiaowine.wcreen.agent.wrappers.VirtualDevice
import com.xiaowine.wcreen.agent.wrappers.VirtualDisplayFlags

/**
 * 进程级共享的装配件 —— 由 `main` 建好一次，每路按它构造自己的屏 / 编码器 / 链路。
 *
 * 都是**设备级静态量**（全进程生命周期内不变）与**不可变值**，所以按构造参数传给每条路，
 * 不做全局单例。
 *
 * @property virtualDevice 虚拟设备句柄（**一个设备挂多块屏**：多路共用，屏天然在同一设备组里）
 * @property flags 已解析的虚拟屏 flag 常量
 * @property caps 设备编码能力（启动时探测一次）
 * @property defaultWidth 缺省宽 = 主屏分辨率
 * @property defaultHeight 缺省高 = 主屏分辨率
 * @property defaultDpi 缺省密度 = 主屏 densityDpi
 * @property inputInjector 输入注入器 —— 进程级共享（注入只看调用进程 uid，与路无关）
 */
internal class RouteDeps(
    val virtualDevice: VirtualDevice.Handle,
    val flags: VirtualDisplayFlags,
    val caps: CodecCaps,
    val defaultWidth: Int,
    val defaultHeight: Int,
    val defaultDpi: Int,
    val inputInjector: InputInjector.Injector,
)
