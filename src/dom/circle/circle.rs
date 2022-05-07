use bytemuck::{Pod, Zeroable};

use crate::{
    dom, BoxLayout, BoxVertex, ColorTarget, Context, Location, Result, Size, State,
    Style, Transform2D, Transforms,
};

pub struct Circle {
    state: State<Attributes>,
    // wgpu items
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    transform_buffer: wgpu::Buffer,
    style_buffer: wgpu::Buffer,
    uniform_buffer: wgpu::Buffer,
}

/// measurements are in pixels.
#[derive(Copy, Clone, Debug)]
pub struct Attributes {
    pub radius: f32,
    pub fill: bool,
}

impl Default for Attributes {
    fn default() -> Attributes {
        Attributes {
            radius: 1.0,
            fill: false,
        }
    }
}

impl Transform2D for Attributes {
    fn transform2d(&self, _offset: Location, scale_factor: f32) -> Attributes {
        Attributes {
            radius: self.radius * scale_factor,
            ..*self
        }
    }
}

#[repr(C)]
#[derive(Default, Copy, Clone, Debug, Pod, Zeroable)]
struct UniformBuffer {
    center: [f32; 2],
    radius: f32,
    fill: u32,
}

impl UniformBuffer {
    const SIZE: usize = 8 + 4 + 4;
}

impl Circle {
    pub fn new(
        attrs: Attributes,
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
    ) -> Circle {
        use std::borrow::Cow;

        let bind_group_layout = Self::to_bind_group_layout(device);

        let pipeline_layout = {
            let desc = wgpu::PipelineLayoutDescriptor {
                label: Some("dom/circle:pipeline-layout"),
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            };
            device.create_pipeline_layout(&desc)
        };

        let module = {
            let text = Cow::Borrowed(include_str!("circle.wgsl"));
            let desc = wgpu::ShaderModuleDescriptor {
                label: Some("dom/circle:shader"),
                source: wgpu::ShaderSource::Wgsl(text.into()),
            };
            device.create_shader_module(&desc)
        };

        let vertex = wgpu::VertexState {
            module: &module,
            entry_point: "vs_main",
            buffers: &[BoxVertex::to_vertex_buffer_layout()],
        };

        let primitive_state = wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        };

        let multisample = wgpu::MultisampleState {
            count: 1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        };

        let fragment = wgpu::FragmentState {
            module: &module,
            entry_point: "fs_main",
            targets: &[wgpu::ColorTargetState {
                format: target_format,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            }],
        };

        let pipeline = {
            let desc = wgpu::RenderPipelineDescriptor {
                label: Some("dom/circle:pipeline"),
                layout: Some(&pipeline_layout),
                vertex,
                primitive: primitive_state,
                depth_stencil: None,
                multisample,
                fragment: Some(fragment),
                multiview: None,
            };
            device.create_render_pipeline(&desc)
        };

        let transform_buffer = Self::to_transform_buffer(device);
        let style_buffer = Self::to_style_buffer(device);
        let uniform_buffer = Self::to_uniform_buffer(device);

        let bind_group = {
            let desc = wgpu::BindGroupDescriptor {
                label: Some("dom/circle:bind-group"),
                layout: &bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: transform_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: style_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: uniform_buffer.as_entire_binding(),
                    },
                ],
            };
            device.create_bind_group(&desc)
        };

        let state = {
            let mut state = State {
                attrs,
                computed_attrs: attrs,
                ..State::default()
            };
            let diameter = state.as_computed_attrs().radius * 2.0;
            let size = Size {
                width: diameter,
                height: diameter,
            };
            state.style.set_size(size);
            state
        };
        Circle {
            state,
            // wgpu items
            pipeline,
            bind_group,
            transform_buffer,
            style_buffer,
            uniform_buffer,
        }
    }

    pub fn print(&self, prefix: &str) {
        println!("{}node.Circle @ {}", prefix, self.state.box_layout);
    }
}

impl Circle {
    pub fn as_state(&self) -> &State<Attributes> {
        &self.state
    }

    pub fn as_mut_state(&mut self) -> &mut State<Attributes> {
        &mut self.state
    }

    pub fn to_mut_children(&mut self) -> Option<&mut Vec<dom::Node>> {
        None
    }

    pub fn to_extent(&self) -> Size {
        let diameter = self.state.as_computed_attrs().radius * 2.0;
        let size = Size {
            width: diameter,
            height: diameter,
        };
        size
    }

    pub fn transform(&mut self, offset: Location, scale_factor: f32) {
        self.state.transform(offset, scale_factor);
    }

    pub fn redraw(
        &mut self,
        context: &Context,
        encoder: &mut wgpu::CommandEncoder,
        target: &mut ColorTarget,
    ) -> Result<()> {
        // debug!("Settings view port for circle {:?}", target.view_port);

        let vertex_buffer = self.to_vertex_buffer(&context.device);
        // overwrite the transform mvp buffer.
        {
            let content = context.transforms.to_bind_content();
            context
                .queue
                .write_buffer(&self.transform_buffer, 0, &content);
        }
        // overwrite the style buffer
        {
            let content = self.state.as_computed_style().to_bind_content();
            context.queue.write_buffer(&self.style_buffer, 0, &content);
        }
        // overwrite the uniform buffer
        {
            let attrs = self.state.as_computed_attrs();
            let blayt: &BoxLayout = self.state.as_ref();
            let ub = UniformBuffer {
                center: [blayt.x + attrs.radius, blayt.y + attrs.radius],
                radius: attrs.radius,
                fill: if attrs.fill { 1 } else { 0 },
            };
            let content: [u8; UniformBuffer::SIZE] = bytemuck::cast(ub);
            context
                .queue
                .write_buffer(&self.uniform_buffer, 0, &content.to_vec());
        }

        let mut render_pass = {
            let desc = wgpu::RenderPassDescriptor {
                label: Some("dom/circle:render-pass"),
                color_attachments: &[wgpu::RenderPassColorAttachment {
                    view: &target.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: true,
                    },
                }],
                depth_stencil_attachment: None,
            };
            encoder.begin_render_pass(&desc)
        };
        target.view_port.set_viewport(&mut render_pass);
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        render_pass.set_bind_group(0, &self.bind_group, &[]);
        render_pass.draw(0..6, 0..1);

        Ok(())
    }
}

impl Circle {
    fn to_transform_buffer(device: &wgpu::Device) -> wgpu::Buffer {
        use wgpu::{util::DeviceExt, BufferUsages};

        let content = Transforms::empty().to_bind_content();
        let desc = wgpu::util::BufferInitDescriptor {
            label: Some("transform-buffer"),
            contents: &content,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        };
        device.create_buffer_init(&desc)
    }

    fn to_style_buffer(device: &wgpu::Device) -> wgpu::Buffer {
        use wgpu::{util::DeviceExt, BufferUsages};

        // this style is not rendered, check render()  function
        let content = Style::default().to_bind_content();
        let desc = wgpu::util::BufferInitDescriptor {
            label: Some("style-buffer"),
            contents: &content,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        };
        device.create_buffer_init(&desc)
    }

    fn to_uniform_buffer(device: &wgpu::Device) -> wgpu::Buffer {
        use wgpu::{util::DeviceExt, BufferUsages};

        let contents = {
            let ub = UniformBuffer::default();
            let contents: [u8; UniformBuffer::SIZE] = bytemuck::cast(ub);
            contents.to_vec()
        };
        let desc = wgpu::util::BufferInitDescriptor {
            label: Some("dom/circle:uniform-buffer"),
            contents: &contents,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        };
        device.create_buffer_init(&desc)
    }

    fn to_vertex_buffer(&self, device: &wgpu::Device) -> wgpu::Buffer {
        use wgpu::{util::DeviceExt, BufferUsages};

        let vertices = [
            BoxVertex {
                position: [-1.0, 1.0, 0.0, 1.0],
            },
            BoxVertex {
                position: [-1.0, -1.0, 0.0, 1.0],
            },
            BoxVertex {
                position: [1.0, 1.0, 0.0, 1.0],
            },
            BoxVertex {
                position: [1.0, 1.0, 0.0, 1.0],
            },
            BoxVertex {
                position: [-1.0, -1.0, 0.0, 1.0],
            },
            BoxVertex {
                position: [1.0, -1.0, 0.0, 1.0],
            },
        ];
        let contents: &[u8] = bytemuck::cast_slice(&vertices);
        let desc = wgpu::util::BufferInitDescriptor {
            label: Some("dom/circle:vertex-buffer"),
            contents,
            usage: BufferUsages::VERTEX,
        };
        device.create_buffer_init(&desc)
    }

    fn to_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        use wgpu::ShaderStages;

        let entry_0 = Transforms::to_bind_group_layout_entry(0);
        let entry_1 = Style::to_bind_group_layout_entry(1);
        let desc = wgpu::BindGroupLayoutDescriptor {
            label: Some("dom/circle:bind-group-layout"),
            entries: &[
                entry_0,
                entry_1,
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        };
        device.create_bind_group_layout(&desc)
    }
}
