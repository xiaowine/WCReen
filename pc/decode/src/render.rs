//! NV12 → RGB 的 wgpu 渲染（T1 帧通路的落点）。
//!
//! # 色彩链路是这里最容易错的地方
//!
//! - 离屏目标用 **`Rgba8Unorm`（非 sRGB）** —— egui-wgpu 的着色器注释明确写着
//!   `We expect "normal" textures that are NOT sRGB-aware`，它把采样值当 **gamma 值**用；
//!   若用 `Rgba8UnormSrgb`，GPU 会再编码一次 ⇒ **画面发白**；
//! - 所以本着色器**直接输出 gamma 编码的 RGB**；
//! - 色彩矩阵用 **BT.709 全范围** —— 与 agent 侧缺省的 `BT709 + FULL + SDR_VIDEO` 一致。
//!   ⚠️ 全范围**不做** 16/255 的抬升；按有限范围写会导致画面偏暗、对比度失真。
//!
//! 这两条都是 **spike 实测**得出的（`docs/plans/2026-10-07-pc-spike-report.md` §3）。

use wgpu::{
    AddressMode, BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingResource, BindingType, ColorTargetState, ColorWrites,
    CommandEncoderDescriptor, Device, Extent3d, FilterMode, FragmentState, LoadOp,
    MultisampleState, Operations, Origin3d, PipelineLayoutDescriptor, PrimitiveState, Queue,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor,
    Sampler, SamplerBindingType, SamplerDescriptor, ShaderModuleDescriptor, ShaderSource,
    ShaderStages, StoreOp, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureAspect,
    TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType, TextureUsages,
    TextureView, TextureViewDimension, VertexState,
};

use crate::decoder::Nv12Frame;

/// 离屏目标格式。**不要改成 `Rgba8UnormSrgb`**（见模块头注释）。
pub const TARGET_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;

/// NV12 → RGB 的 WGSL。
const SHADER: &str = r#"
@group(0) @binding(0) var t_y:  texture_2d<f32>;
@group(0) @binding(1) var t_uv: texture_2d<f32>;
@group(0) @binding(2) var s:    sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0)       uv:  vec2<f32>,
};

// 全屏三角形：比全屏四边形少一个顶点，且没有对角接缝
@vertex
fn vs(@builtin(vertex_index) i: u32) -> VsOut {
    var p = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    var o: VsOut;
    o.pos = vec4<f32>(p[i], 0.0, 1.0);
    // NDC 是 y 向上，视频是 y 向下 ⇒ 翻转
    o.uv  = vec2<f32>((p[i].x + 1.0) * 0.5, 1.0 - (p[i].y + 1.0) * 0.5);
    return o;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let y  = textureSample(t_y,  s, in.uv).r;
    let uv = textureSample(t_uv, s, in.uv).rg;
    // BT.709 **全范围**（color_range=pc）：Y 不再做 16/255 抬升，色度以 0.5 为中心
    let u = uv.r - 0.5;
    let v = uv.g - 0.5;
    return vec4<f32>(
        y + 1.5748 * v,
        y - 0.1873 * u - 0.4681 * v,
        y + 1.8556 * u,
        1.0,
    );
}
"#;

/// NV12 → RGB 的**共享**渲染管线（全进程一份，多路复用）。
pub struct Nv12Pipeline {
    /// 渲染管线。
    pipeline: RenderPipeline,
    /// 绑定组布局（建 [`StreamTarget`] 时要用）。
    layout: wgpu::BindGroupLayout,
    /// 采样器（线性过滤）。
    sampler: Sampler,
}

impl Nv12Pipeline {
    /// 建管线。
    ///
    /// @param device wgpu 设备（来自 egui 的渲染状态）
    pub fn new(device: &Device) -> Self {
        let sm = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("nv12→rgb"),
            source: ShaderSource::Wgsl(SHADER.into()),
        });
        let tex_entry = |binding: u32| BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::FRAGMENT,
            ty: BindingType::Texture {
                sample_type: TextureSampleType::Float { filterable: true },
                view_dimension: TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("nv12"),
            entries: &[
                tex_entry(0),
                tex_entry(1),
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pl = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("nv12"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("nv12→rgb"),
            layout: Some(&pl),
            vertex: VertexState {
                module: &sm,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &sm,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format: TARGET_FORMAT,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("nv12"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..Default::default()
        });
        Self { pipeline, layout, sampler }
    }
}

/// 构造「整张纹理、全部 mip、全部 aspect」的拷贝目标。
fn tex_dst(tex: &Texture) -> TexelCopyTextureInfo<'_> {
    TexelCopyTextureInfo {
        texture: tex,
        mip_level: 0,
        origin: Origin3d::ZERO,
        aspect: TextureAspect::All,
    }
}

/// 一路的**纹理与绑定组**。
///
/// 分辨率变化必须**重建**（并按需释放旧的 egui 纹理 id）—— 见 [`Self::matches`]。
pub struct StreamTarget {
    /// Y 平面纹理（`R8Unorm`）。
    y: Texture,
    /// UV 平面纹理（`Rg8Unorm`，宽高各半）。
    uv: Texture,
    /// 离屏目标的视图 —— 交给 egui 显示。
    ///
    /// 视图持有纹理的引用计数 ⇒ 不必再单独存 `wgpu::Texture`。
    pub view: TextureView,
    /// Y / UV / 采样器的绑定组。
    bind: BindGroup,
    /// 目标宽度。
    pub width: u32,
    /// 目标高度。
    pub height: u32,
}

impl StreamTarget {
    /// 按分辨率建纹理。
    ///
    /// @param device wgpu 设备
    /// @param pipe 共享管线（复用它的绑定组布局与采样器）
    /// @param width 视频宽
    /// @param height 视频高
    pub fn new(device: &Device, pipe: &Nv12Pipeline, width: u32, height: u32) -> Self {
        let make = |label: &str, format: TextureFormat, w: u32, h: u32| {
            device.create_texture(&TextureDescriptor {
                label: Some(label),
                size: Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format,
                // 平面纹理要能上传（COPY_DST）也要能被采样（TEXTURE_BINDING）
                usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let y = make("nv12.y", TextureFormat::R8Unorm, width, height);
        // NV12 的 UV 平面是 2×2 下采样、两通道交错 ⇒ 宽高各半、Rg8
        let uv = make("nv12.uv", TextureFormat::Rg8Unorm, width / 2, height / 2);
        let target = device.create_texture(&TextureDescriptor {
            label: Some("nv12.rgb"),
            size: Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let yv = y.create_view(&Default::default());
        let uvv = uv.create_view(&Default::default());
        let view = target.create_view(&Default::default());
        let bind = device.create_bind_group(&BindGroupDescriptor {
            label: Some("nv12"),
            layout: &pipe.layout,
            entries: &[
                BindGroupEntry { binding: 0, resource: BindingResource::TextureView(&yv) },
                BindGroupEntry { binding: 1, resource: BindingResource::TextureView(&uvv) },
                BindGroupEntry { binding: 2, resource: BindingResource::Sampler(&pipe.sampler) },
            ],
        });
        Self { y, uv, view, bind, width, height }
    }

    /// 这一路的纹理是否**恰好**装得下该帧（不匹配就要重建）。
    ///
    /// @param frame 刚解出的帧
    pub fn matches(&self, frame: &Nv12Frame) -> bool {
        self.width == frame.width && self.height == frame.height
    }

    /// 把一帧 NV12 上传到两个平面纹理。
    ///
    /// 尺寸必须与 [`Self::matches`] 判定的一致 —— 调用方负责在变化时重建。
    ///
    /// @param queue wgpu 队列
    /// @param frame 已回读的 NV12 帧
    pub fn upload(&self, queue: &Queue, frame: &Nv12Frame) {
        queue.write_texture(
            tex_dst(&self.y),
            &frame.y,
            TexelCopyBufferLayout {
                offset: 0,
                // 每行字节数就是宽度（R8，1 字节/像素）
                bytes_per_row: Some(frame.width),
                rows_per_image: Some(frame.height),
            },
            Extent3d { width: frame.width, height: frame.height, depth_or_array_layers: 1 },
        );
        queue.write_texture(
            tex_dst(&self.uv),
            &frame.uv,
            TexelCopyBufferLayout {
                offset: 0,
                // UV 平面每行 = 宽度个字节（Rg8，2 字节/像素 × 半宽）
                bytes_per_row: Some(frame.width),
                rows_per_image: Some(frame.height / 2),
            },
            Extent3d { width: frame.width / 2, height: frame.height / 2, depth_or_array_layers: 1 },
        );
    }

    /// 跑一遍 NV12 → RGB，结果落在离屏纹理上（供 egui 采样）。
    ///
    /// @param device wgpu 设备
    /// @param queue wgpu 队列
    /// @param pipe 共享管线
    pub fn render(&self, device: &Device, queue: &Queue, pipe: &Nv12Pipeline) {
        let mut enc = device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("nv12→rgb"),
        });
        {
            let mut rp = enc.begin_render_pass(&RenderPassDescriptor {
                label: Some("nv12→rgb"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &self.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations { load: LoadOp::Clear(wgpu::Color::BLACK), store: StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_pipeline(&pipe.pipeline);
            rp.set_bind_group(0, &self.bind, &[]);
            rp.draw(0..3, 0..1);
        }
        queue.submit([enc.finish()]);
    }

    /// 目标纹理的尺寸（与视频一致）—— 供 UI 计算等比缩放。
    pub fn size(&self) -> [f32; 2] {
        [self.width as f32, self.height as f32]
    }
}
