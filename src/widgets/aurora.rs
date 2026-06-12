//! The shader tier: a custom wgpu fragment shader painting an animated mesh
//! gradient ("aurora"). This is the real ceiling — arbitrary GPU shading
//! behind native libcosmic widgets.
//!
//! Structure follows iced's `custom_shader` example for this fork:
//!   - `Aurora`           — the `shader::Program`; produces a `Primitive` per frame.
//!   - `AuroraPrimitive`  — per-frame data (the uniforms); uploads + records draws.
//!   - `AuroraPipeline`   — created once, owns the wgpu pipeline + uniform buffer.

use cosmic::iced::wgpu;
use cosmic::iced::widget::shader::{self, Viewport};
use cosmic::iced::{Color, Rectangle, mouse};

/// std140-friendly: three vec4 colours then a trailing f32 time, padded to 64
/// bytes. All fields f32 so the layout has no implicit padding (Pod-safe).
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    c1: [f32; 4],
    c2: [f32; 4],
    c3: [f32; 4],
    time: f32,
    _pad: [f32; 3],
}

/// The `shader::Program`. Carries the live time base + three gradient colours.
pub struct Aurora {
    pub time: f32,
    pub colors: [Color; 3],
}

impl<M> shader::Program<M> for Aurora {
    type State = ();
    type Primitive = AuroraPrimitive;

    fn draw(&self, _state: &(), _cursor: mouse::Cursor, _bounds: Rectangle) -> AuroraPrimitive {
        let f = |c: Color| [c.r, c.g, c.b, c.a];
        AuroraPrimitive {
            uniforms: Uniforms {
                c1: f(self.colors[0]),
                c2: f(self.colors[1]),
                c3: f(self.colors[2]),
                time: self.time,
                _pad: [0.0; 3],
            },
        }
    }
}

#[derive(Debug)]
pub struct AuroraPrimitive {
    uniforms: Uniforms,
}

impl shader::Primitive for AuroraPrimitive {
    type Pipeline = AuroraPipeline;

    fn prepare(
        &self,
        pipeline: &mut AuroraPipeline,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _bounds: &Rectangle,
        _viewport: &Viewport,
    ) {
        queue.write_buffer(&pipeline.uniforms, 0, bytemuck::bytes_of(&self.uniforms));
    }

    // Reuse iced's render pass (already scissored to our widget bounds) and
    // just paint a fullscreen triangle. Returning true means "I drew here".
    fn draw(&self, pipeline: &AuroraPipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        render_pass.set_pipeline(&pipeline.pipeline);
        render_pass.set_bind_group(0, &pipeline.bind_group, &[]);
        render_pass.draw(0..3, 0..1);
        true
    }
}

#[derive(Debug)]
pub struct AuroraPipeline {
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl shader::Pipeline for AuroraPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("aurora uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aurora bind group layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("aurora bind group"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aurora shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(SHADER)),
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("aurora pipeline layout"),
            bind_group_layouts: &[&bgl],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("aurora pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });

        Self { pipeline, uniforms, bind_group }
    }
}

const SHADER: &str = r#"
struct Uniforms {
    c1: vec4<f32>,
    c2: vec4<f32>,
    c3: vec4<f32>,
    time: f32,
};
@group(0) @binding(0) var<uniform> u: Uniforms;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    // Oversized fullscreen triangle.
    var verts = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 3.0,  1.0),
    );
    let xy = verts[vi];
    var out: VsOut;
    out.pos = vec4<f32>(xy, 0.0, 1.0);
    out.uv = xy * 0.5 + vec2<f32>(0.5, 0.5);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let t = u.time;

    // Three drifting sine fields → smoothly migrating colour weights.
    let w1 = 0.5 + 0.5 * sin(t * 0.50 + uv.x * 3.0 + uv.y * 1.5);
    let w2 = 0.5 + 0.5 * sin(t * 0.37 + uv.y * 4.0 - uv.x * 2.0 + 2.094);
    let w3 = 0.5 + 0.5 * sin(t * 0.61 - uv.x * 2.5 - uv.y * 3.0 + 4.188);
    let sum = w1 + w2 + w3 + 0.0001;

    var col = (u.c1.rgb * w1 + u.c2.rgb * w2 + u.c3.rgb * w3) / sum;

    // Soft radial vignette for depth.
    let d = distance(uv, vec2<f32>(0.5, 0.5));
    col = col * (1.0 - 0.35 * d);

    return vec4<f32>(col, 1.0);
}
"#;
