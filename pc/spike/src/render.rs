//! NV12 → RGB 的 wgpu 渲染（T1 帧通路的落点）。
//!
//! **色彩链路是这里最容易错的地方**，所以先把约定写清楚：
//!
//! - 离屏目标用 **`Rgba8Unorm`（非 sRGB）** —— egui-wgpu 的着色器注释明确写着
//!   `We expect "normal" textures that are NOT sRGB-aware`，它把采样值当 **gamma 值**用；
//!   若用 `Rgba8UnormSrgb`，GPU 会再编码一次 ⇒ 画面发白。
//! - 所以本着色器**直接输出 gamma 编码的 RGB**。
//! - 色彩矩阵用 **BT.709 全范围**（`color_range=pc`）—— 与样片、以及 agent 侧编码器
//!   已定的参数一致（见 `docs/plans/2026-10-07-pc-spike-implementation.md` §4）。
//!   ⚠️ 全范围**不做** 16/255 的抬升；按有限范围写会导致画面偏暗、对比度失真。

use eframe::wgpu;

use crate::hw::Nv12Frame;

/// 离屏目标格式。**不要改成 `Rgba8UnormSrgb`**（见模块头注释）。
pub const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

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
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl Nv12Pipeline {
    /// 建管线。
    pub fn new(device: &wgpu::Device) -> Self {
        let sm = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("nv12→rgb"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let tex_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("nv12"),
            entries: &[
                tex_entry(0),
                tex_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("nv12"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("nv12→rgb"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &sm,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &sm,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: TARGET_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nv12"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self { pipeline, layout, sampler }
    }
}

/// 构造「整张纹理、全部 mip、全部 aspect」的拷贝目标。
fn tex_dst(tex: &wgpu::Texture) -> wgpu::TexelCopyTextureInfo<'_> {
    wgpu::TexelCopyTextureInfo {
        texture: tex,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
    }
}

/// 一路的**纹理与绑定组**。分辨率变化必须**重建**（并按需释放旧的 egui 纹理 id）。
pub struct StreamTarget {
    y: wgpu::Texture,
    uv: wgpu::Texture,
    /// 离屏目标的视图 —— 交给 egui 显示。
    ///
    /// 视图持有纹理的引用计数 ⇒ 不必再单独存 `wgpu::Texture`。
    pub view: wgpu::TextureView,
    bind: wgpu::BindGroup,
    pub width: u32,
    pub height: u32,
}

impl StreamTarget {
    /// 按分辨率建纹理。
    pub fn new(device: &wgpu::Device, pipe: &Nv12Pipeline, width: u32, height: u32) -> Self {
        let make = |label: &str, format: wgpu::TextureFormat, w: u32, h: u32| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                // 平面纹理要能上传（COPY_DST）也要能被采样（TEXTURE_BINDING）
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let y = make("nv12.y", wgpu::TextureFormat::R8Unorm, width, height);
        // NV12 的 UV 平面是 2×2 下采样、两通道交错 ⇒ 宽高各半、Rg8
        let uv = make("nv12.uv", wgpu::TextureFormat::Rg8Unorm, width / 2, height / 2);
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("nv12.rgb"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let yv = y.create_view(&Default::default());
        let uvv = uv.create_view(&Default::default());
        let view = target.create_view(&Default::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nv12"),
            layout: &pipe.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&yv) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&uvv) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&pipe.sampler) },
            ],
        });
        Self { y, uv, view, bind, width, height }
    }

    /// 把一帧 NV12 上传到两个平面纹理。
    pub fn upload(&self, queue: &wgpu::Queue, f: &Nv12Frame) {
        queue.write_texture(
            tex_dst(&self.y),
            &f.y,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                // 每行字节数就是宽度（R8，1 字节/像素）
                bytes_per_row: Some(f.width),
                rows_per_image: Some(f.height),
            },
            wgpu::Extent3d { width: f.width, height: f.height, depth_or_array_layers: 1 },
        );
        queue.write_texture(
            tex_dst(&self.uv),
            &f.uv,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                // UV 平面每行 = 宽度个字节（Rg8，2 字节/像素 × 半宽）
                bytes_per_row: Some(f.width),
                rows_per_image: Some(f.height / 2),
            },
            wgpu::Extent3d {
                width: f.width / 2,
                height: f.height / 2,
                depth_or_array_layers: 1,
            },
        );
    }

    /// 跑一遍 NV12 → RGB，结果落在离屏纹理上（供 egui 采样）。
    pub fn render(&self, device: &wgpu::Device, queue: &wgpu::Queue, pipe: &Nv12Pipeline) {
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("nv12→rgb"),
        });
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("nv12→rgb"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
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

    /// 目标纹理的尺寸（与视频一致）。
    pub fn size(&self) -> [f32; 2] {
        [self.width as f32, self.height as f32]
    }
}
