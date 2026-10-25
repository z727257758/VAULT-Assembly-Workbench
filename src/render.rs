use crate::{mesh::Vertex, scene::Scene};
use eframe::{
    egui,
    egui_wgpu::{self, wgpu},
};
use glam::{Mat4, Vec3};
use std::collections::HashMap;
use wgpu::util::DeviceExt;

#[derive(Clone)]
pub struct Camera {
    pub yaw: f32,
    pub pitch: f32,
    pub zoom: f32,
    pub pan: [f32; 2],
    pub center: Vec3,
    pub radius: f32,
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            yaw: 0.65,
            pitch: 0.5,
            zoom: 1.,
            pan: [0.; 2],
            center: Vec3::ZERO,
            radius: 100.,
        }
    }
}
impl Camera {
    pub fn fit(&mut self, scene: &Scene, explode: f32) {
        let (c, r) = scene.bounds(explode);
        self.center = c;
        self.radius = r;
        self.zoom = 1.;
        self.pan = [0.; 2];
    }
    pub fn matrix(&self, aspect: f32) -> Mat4 {
        let extent = self.radius * 1.10 / self.zoom;
        let direction = Vec3::new(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.cos() * self.pitch.cos(),
        );
        let eye = self.center + direction * self.radius * 4.;
        let view = Mat4::look_at_rh(eye, self.center, Vec3::Y);
        let halfx = extent * aspect.max(1.);
        let halfy = extent / aspect.clamp(0.01, 1.);
        let px = self.pan[0] * halfx;
        let py = self.pan[1] * halfy;
        Mat4::orthographic_rh(
            -halfx + px,
            halfx + px,
            -halfy + py,
            halfy + py,
            0.01,
            self.radius * 10. + 100.,
        ) * view
    }
    pub fn ray(&self, point: egui::Pos2, rect: egui::Rect) -> (Vec3, Vec3) {
        let x = (point.x - rect.left()) / rect.width() * 2. - 1.;
        let y = 1. - (point.y - rect.top()) / rect.height() * 2.;
        let inv = self.matrix(rect.aspect_ratio()).inverse();
        let near = inv.project_point3(Vec3::new(x, y, 0.));
        let far = inv.project_point3(Vec3::new(x, y, 1.));
        (near, (far - near).normalize())
    }
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ObjectUniform {
    model: [[f32; 4]; 4],
    color: [f32; 4],
    text_color: [f32; 4],
}
struct GpuMesh {
    source: std::sync::Arc<crate::mesh::Mesh>,
    buffer: wgpu::Buffer,
    count: u32,
}
struct GpuPart {
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
}
struct Target {
    size: [u32; 2],
    view: wgpu::TextureView,
    msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
    id: egui::TextureId,
}
#[derive(Default)]
pub struct ViewOptions {
    pub explosion: f32,
    pub selected: Option<usize>,
    pub ghost: bool,
    pub interior_only: bool,
    pub colliding: Vec<usize>,
}

pub struct Renderer {
    pub print_colors: Option<crate::print_plate::Config>,
    pub state: egui_wgpu::RenderState,
    pipeline: wgpu::RenderPipeline,
    ghost_pipeline: wgpu::RenderPipeline,
    camera: wgpu::Buffer,
    camera_bind: wgpu::BindGroup,
    object_layout: wgpu::BindGroupLayout,
    meshes: HashMap<(String, Option<u32>), GpuMesh>,
    parts: Vec<GpuPart>,
    targets: Vec<Option<Target>>,
}
impl Renderer {
    pub fn new(state: egui_wgpu::RenderState) -> Self {
        let device = &state.device;
        let layout = |label| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            })
        };
        let camera_layout = layout("Camera");
        let object_layout = layout("Object");
        let camera = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Camera"),
            contents: bytemuck::cast_slice(&Mat4::IDENTITY.to_cols_array()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let camera_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera.as_entire_binding(),
            }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("VAULT shaded models"),
            source: wgpu::ShaderSource::Wgsl(include_str!("model.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&camera_layout, &object_layout],
            push_constant_ranges: &[],
        });
        let make_pipeline = |ghost: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("VAULT scene"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32],
                    }],
                    compilation_options: Default::default(),
                },
                primitive: wgpu::PrimitiveState {
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: !ghost,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: 4,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                multiview: None,
                cache: None,
            })
        };
        let pipeline = make_pipeline(false);
        let ghost_pipeline = make_pipeline(true);
        Self {
            print_colors: None,
            state,
            pipeline,
            ghost_pipeline,
            camera,
            camera_bind,
            object_layout,
            meshes: HashMap::new(),
            parts: Vec::new(),
            targets: (0..4).map(|_| None).collect(),
        }
    }
    pub fn upload(&mut self, scene: &Scene) {
        self.meshes.retain(|key, old| {
            scene.parts.iter().any(|p| {
                &p.mesh_key() == key
                    && (std::sync::Arc::ptr_eq(&old.source, &p.mesh)
                        || bytemuck::cast_slice::<_, u8>(&old.source.vertices)
                            == bytemuck::cast_slice::<_, u8>(&p.mesh.vertices))
            })
        });
        self.parts.clear();
        for p in &scene.parts {
            self.meshes.entry(p.mesh_key()).or_insert_with(|| GpuMesh {
                source: p.mesh.clone(),
                buffer: self
                    .state
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some(&p.pose.path),
                        contents: bytemuck::cast_slice(&p.mesh.vertices),
                        usage: wgpu::BufferUsages::VERTEX,
                    }),
                count: p.mesh.vertices.len() as u32,
            });
            let uniform = self.state.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Part transform"),
                size: std::mem::size_of::<ObjectUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind = self
                .state
                .device
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &self.object_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    }],
                });
            self.parts.push(GpuPart { uniform, bind });
        }
    }
    fn target(&mut self, index: usize, size: [u32; 2]) {
        if self.targets[index].as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let texture = |format, samples, usage| {
            self.state
                .device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("VAULT viewport"),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let view = texture(
            wgpu::TextureFormat::Rgba8Unorm,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let msaa = texture(
            wgpu::TextureFormat::Rgba8Unorm,
            4,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let depth = texture(
            wgpu::TextureFormat::Depth32Float,
            4,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let mut r = self.state.renderer.write();
        let id = if let Some(old) = self.targets[index].take() {
            r.update_egui_texture_from_wgpu_texture(
                &self.state.device,
                &view,
                wgpu::FilterMode::Linear,
                old.id,
            );
            old.id
        } else {
            r.register_native_texture(&self.state.device, &view, wgpu::FilterMode::Linear)
        };
        self.targets[index] = Some(Target {
            size,
            view,
            msaa,
            depth,
            id,
        });
    }
    pub fn render(
        &mut self,
        index: usize,
        size: [u32; 2],
        scene: &Scene,
        camera: &Camera,
        options: ViewOptions,
    ) -> egui::TextureId {
        let ViewOptions {
            explosion,
            selected,
            ghost,
            interior_only,
            colliding,
        } = options;
        self.target(index, [size[0].clamp(1, 4096), size[1].clamp(1, 4096)]);
        let target = self.targets[index].as_ref().unwrap();
        self.state.queue.write_buffer(
            &self.camera,
            0,
            bytemuck::cast_slice(
                &camera
                    .matrix(target.size[0] as f32 / target.size[1] as f32)
                    .to_cols_array(),
            ),
        );
        for (i, p) in scene.parts.iter().enumerate() {
            let mut color = self
                .print_colors
                .as_ref()
                .map_or(p.color, |c| c.color(&p.pose.path));
            if colliding.contains(&i) {
                color = [0.95, 0.30, 0.32, 1.];
            }
            if selected == Some(i) && self.print_colors.is_none() {
                color = [0.98, 0.72, 0.30, 1.];
            }
            if ghost && p.pose.path.contains("/Modules/") {
                color[3] = 0.22;
            }
            let uniform = ObjectUniform {
                model: p.pose.matrix(explosion).to_cols_array_2d(),
                color,
                text_color: self.print_colors.as_ref().map_or([0.; 4], |c| {
                    let mut v = c.slot_color(c.text_slot(&p.pose.path));
                    v[3] = color[3];
                    v
                }),
            };
            self.state
                .queue
                .write_buffer(&self.parts[i].uniform, 0, bytemuck::bytes_of(&uniform));
        }
        let mut encoder = self
            .state
            .device
            .create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("VAULT viewport"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.msaa,
                    resolve_target: Some(&target.view),
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.055,
                            g: 0.071,
                            b: 0.09,
                            a: 1.,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_bind_group(0, &self.camera_bind, &[]);
            for transparent in [false, true] {
                pass.set_pipeline(if transparent {
                    &self.ghost_pipeline
                } else {
                    &self.pipeline
                });
                for (i, p) in scene.parts.iter().enumerate() {
                    let visible = if interior_only {
                        crate::dividers::interior_part(&p.pose.path)
                    } else {
                        p.visible
                    };
                    if !visible || (ghost && p.pose.path.contains("/Modules/")) != transparent {
                        continue;
                    }
                    let mesh = &self.meshes[&p.mesh_key()];
                    pass.set_bind_group(1, &self.parts[i].bind, &[]);
                    pass.set_vertex_buffer(0, mesh.buffer.slice(..));
                    pass.draw(0..mesh.count, 0..1);
                }
            }
        }
        self.state.queue.submit([encoder.finish()]);
        target.id
    }
}
