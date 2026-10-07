//! 硬解设备与**能力探测**。
//!
//! 「探测给候选，试错给结论」：本模块报告的是**本机编译进来且真能创建**的设备，
//! 以及设备的帧约束（尺寸上下限 / 支持的软件像素格式）。
//! 真正的判据仍然是**逐路首帧硬校验**（见 [`crate::decoder`]）——
//! OS / 驱动广告的能力 ≠ FFmpeg 会使用（实测：不支持的 profile 会**静默软解**）。

use std::ffi::{c_char, c_int, CStr, CString};
use std::ptr;

use ffmpeg_next::sys;

use crate::decoder::{Codec, DecodeError, DecodeResult, Stage};

/// 把一个 FFmpeg 返回码转成可读文本。
///
/// @param ret FFmpeg 的负返回值
/// @return 可读说明（如 `Invalid argument`）
pub fn av_err(ret: c_int) -> String {
    let mut buf = [0 as c_char; 256];
    // `av_strerror` 只在 buf 里写 NUL 结尾的字符串；返回码本身不关心。
    unsafe { sys::av_strerror(ret, buf.as_mut_ptr(), buf.len()) };
    unsafe { CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned()
}

/// 像素格式的短名（`av_get_pix_fmt_name`）。
///
/// `AVFrame.format` 是 `c_int`，而 `av_get_pix_fmt_name` 收 enum ⇒ 需要转回。
/// **FFmpeg 返回的格式值都是合法枚举**，所以这里的 `transmute` 安全。
///
/// @param fmt 像素格式值
/// @return 短名；未知值给出 `(未知 N)` 而不是 panic
pub fn pix_fmt_name(fmt: i32) -> String {
    let f: sys::AVPixelFormat = unsafe { std::mem::transmute(fmt) };
    unsafe {
        let p = sys::av_get_pix_fmt_name(f);
        if p.is_null() {
            format!("(未知 {fmt})")
        } else {
            CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }
}

/// 把一个 FFmpeg 的 `char*` 转成 `String`（空指针给占位符）。
fn cstr_or(p: *const c_char, fallback: &str) -> String {
    if p.is_null() {
        fallback.to_string()
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

// ---------------------------------------------------------------- 设备枚举

/// 硬解设备的探测结果。
#[derive(Debug, Clone)]
pub struct DeviceProbe {
    /// 设备类型名（如 `d3d11va`），来自 `av_hwdevice_get_type_name`。
    pub name: String,
    /// 能否**真的**创建出来（`av_hwdevice_ctx_create` 是否成功）。
    pub creatable: bool,
    /// 创建失败时的可读原因。
    pub error: Option<String>,
}

/// 枚举本机**编译进来**的全部硬解设备，并逐个**实试创建**。
///
/// ⚠️ **「编译进来」≠「本机能用」** —— 所以 `creatable` 必须实试
/// （实测：本机 `vaapi` / `qsv` / `amf` 枚举得到却创建失败）。
///
/// @return 每个设备一条记录，顺序即 FFmpeg 的枚举顺序
pub fn probe_devices() -> Vec<DeviceProbe> {
    let mut out = Vec::new();
    let mut t = sys::AVHWDeviceType::AV_HWDEVICE_TYPE_NONE;
    loop {
        t = unsafe { sys::av_hwdevice_iterate_types(t) };
        if t == sys::AVHWDeviceType::AV_HWDEVICE_TYPE_NONE {
            break;
        }
        let name = cstr_or(unsafe { sys::av_hwdevice_get_type_name(t) }, "?");
        let mut ctx: *mut sys::AVBufferRef = ptr::null_mut();
        let ret =
            unsafe { sys::av_hwdevice_ctx_create(&mut ctx, t, ptr::null(), ptr::null_mut(), 0) };
        let (creatable, error) = if ret < 0 {
            (false, Some(av_err(ret)))
        } else {
            unsafe { sys::av_buffer_unref(&mut ctx) };
            (true, None)
        };
        out.push(DeviceProbe { name, creatable, error });
    }
    out
}

/// 按名字找硬解设备类型（`av_hwdevice_find_type_by_name`）。
///
/// @param name 设备类型名（如 `d3d11va`）
/// @return 成功给出枚举值；未知名字给出 [`Stage::Open`] 失败
pub fn device_by_name(name: &str) -> DecodeResult<sys::AVHWDeviceType> {
    let c = CString::new(name)
        .map_err(|_| DecodeError::new(Stage::Open, format!("设备名含 NUL: {name}")))?;
    let t = unsafe { sys::av_hwdevice_find_type_by_name(c.as_ptr()) };
    if t == sys::AVHWDeviceType::AV_HWDEVICE_TYPE_NONE {
        return Err(DecodeError::new(Stage::Open, format!("未知硬解设备: {name}")));
    }
    Ok(t)
}

// ---------------------------------------------------------------- 设备

/// 一个硬解设备（`AVBufferRef`）。
///
/// 产品里所有解码器都挂在**同一个**设备上（多路共享）：每路各建一个会虚高设备数，
/// 也会让 D3D11 设备对象重复创建。
pub struct HwDevice {
    /// FFmpeg 的设备引用（本结构持有，`Drop` 时释放）。
    pub(crate) ptr: *mut sys::AVBufferRef,
    /// 设备类型名（如 `d3d11va`）。
    pub name: String,
}

// `av_hwdevice_ctx_create` 返回的是**原子引用计数**的 `AVBufferRef`，
// 且 FFmpeg 建的 D3D11 设备默认 multithread-protected
// ⇒ 跨线程共享是 FFmpeg 的既定用法（`av_buffer_ref` 是原子的）。
unsafe impl Send for HwDevice {}
unsafe impl Sync for HwDevice {}

impl HwDevice {
    /// 建一个硬解设备。
    ///
    /// @param name 设备类型名（如 `d3d11va`）
    /// @return 成功给出设备；名字未知或创建失败给出 [`Stage::Open`] 失败
    pub fn create(name: &str) -> DecodeResult<Self> {
        let dev_type = device_by_name(name)?;
        let mut ptr: *mut sys::AVBufferRef = ptr::null_mut();
        let ret =
            unsafe { sys::av_hwdevice_ctx_create(&mut ptr, dev_type, ptr::null(), ptr::null_mut(), 0) };
        if ret < 0 {
            return Err(DecodeError::new(
                Stage::Open,
                format!("建硬解设备 {name} 失败: {}（{ret}）", av_err(ret)),
            ));
        }
        Ok(Self { ptr, name: name.to_string() })
    }
}

impl Drop for HwDevice {
    fn drop(&mut self) {
        unsafe { sys::av_buffer_unref(&mut self.ptr) };
    }
}

// ---------------------------------------------------------------- 能力探测

/// 本机**硬解能力**（探测结果，是「PC 侧能硬解什么」的答案）。
///
/// 这份结果是「PC 求交集」里的 **PC 侧输入**（交集由 PC 算 —— 只有它知道自己的解码器）。
#[derive(Debug, Clone)]
pub struct HardwareCapability {
    /// 设备类型名（如 `d3d11va`）。
    pub device: String,
    /// 本机能硬解的编码器类别（探测不到就是不支持，**绝不退软解**）。
    pub codecs: Vec<Codec>,
    /// 硬件帧的最大宽度；`0` = 设备未报告上限（此时只能靠首帧硬校验兜底）。
    pub max_width: u32,
    /// 硬件帧的最大高度；`0` = 设备未报告上限。
    pub max_height: u32,
    /// 硬件帧允许的**软件像素格式**（回读后落到 CPU 的格式）。
    ///
    /// 本项目只实现 `nv12`（8-bit 4:2:0）；`p010le` 之类意味着 10-bit —— 暂不做。
    pub sw_formats: Vec<String>,
}

impl HardwareCapability {
    /// 是否支持 8-bit 4:2:0 的回读（本 crate 唯一实现的格式）。
    pub fn supports_nv12(&self) -> bool {
        self.sw_formats.iter().any(|f| f == "nv12")
    }
}

/// 探测某个硬解设备的**帧约束**与可硬解的编码器。
///
/// 实现依据（都来自 FFmpeg 的公开 API，不是猜测）：
/// - `av_hwdevice_get_hwframe_constraints` 给出该设备的帧尺寸上下限与合法软件格式；
/// - `avcodec_get_hw_config` 逐个列出解码器支持的硬件配置，判据是
///   **device_type 匹配 + 支持 `HW_DEVICE_CTX` 装配方式**（我们用的就是这一种）。
///
/// @param device_name 设备类型名（如 `d3d11va`）
/// @return 成功给出能力；设备建不出来给出 [`Stage::Open`] 失败
pub fn probe_hardware_capability(device_name: &str) -> DecodeResult<HardwareCapability> {
    let device = HwDevice::create(device_name)?;
    let dev_type = device_by_name(device_name)?;

    let mut codecs = Vec::new();
    for (codec, id) in [
        (Codec::H264, sys::AVCodecID::AV_CODEC_ID_H264),
        (Codec::Hevc, sys::AVCodecID::AV_CODEC_ID_HEVC),
    ] {
        if decoder_supports_device(id, dev_type) {
            codecs.push(codec);
        }
    }

    // 约束里的 INT_MAX 表示「未知 / 无限制」，归一化成 0（不要假装它是一个真上限）。
    let mut max_width = 0u32;
    let mut max_height = 0u32;
    let mut sw_formats = Vec::new();
    let mut constraints = unsafe { sys::av_hwdevice_get_hwframe_constraints(device.ptr, ptr::null()) };
    if !constraints.is_null() {
        unsafe {
            let c = &*constraints;
            if c.max_width > 0 && c.max_width < c_int::MAX {
                max_width = c.max_width as u32;
            }
            if c.max_height > 0 && c.max_height < c_int::MAX {
                max_height = c.max_height as u32;
            }
            if !c.valid_sw_formats.is_null() {
                let mut p = c.valid_sw_formats;
                while *p != sys::AVPixelFormat::AV_PIX_FMT_NONE {
                    sw_formats.push(pix_fmt_name(*p as i32));
                    p = p.add(1);
                }
            }
            sys::av_hwframe_constraints_free(&mut constraints);
        }
    }

    Ok(HardwareCapability {
        device: device_name.to_string(),
        codecs,
        max_width,
        max_height,
        sw_formats,
    })
}

/// 某个解码器是否支持「用设备上下文装配」的该种硬解。
///
/// @param id 解码器 id（`AV_CODEC_ID_*`）
/// @param device_type 硬解设备类型
/// @return 支持为 true；解码器不存在或没有匹配的硬件配置为 false
fn decoder_supports_device(id: sys::AVCodecID, device_type: sys::AVHWDeviceType) -> bool {
    let codec = unsafe { sys::avcodec_find_decoder(id) };
    if codec.is_null() {
        return false;
    }
    let mut index = 0;
    loop {
        let cfg = unsafe { sys::avcodec_get_hw_config(codec, index) };
        if cfg.is_null() {
            return false;
        }
        let cfg = unsafe { &*cfg };
        let device_ctx_method =
            sys::AV_CODEC_HW_CONFIG_METHOD_HW_DEVICE_CTX as c_int;
        if cfg.device_type == device_type && (cfg.methods & device_ctx_method) != 0 {
            return true;
        }
        index += 1;
    }
}
