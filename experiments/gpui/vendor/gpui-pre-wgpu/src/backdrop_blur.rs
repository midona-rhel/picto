//! Backdrop blurs ([`gpui::BackdropBlur`]) for the wgpu renderer.
//!
//! A surface texture can only be drawn to, so while a blur is on screen the
//! frame is drawn into a texture of this module's instead, which each blur
//! reads, and is copied to the surface once it is finished. A frame without
//! blurs is drawn to the surface directly, as before.

use bytemuck::{Pod, Zeroable};
use gpui::{BACKDROP_BLUR_MAX_TAPS, BackdropBlur, BackdropBlurPlan, DevicePixels, Size};
use std::num::NonZeroU64;

pub(crate) const SHADERS: &str = include_str!("backdrop_blur.wgsl");

/// Mirrors `BlurParams` in backdrop_blur.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BackdropBlurParams {
    target_rect: [f32; 4],
    source_downsample_taps: [f32; 4],
    reduced_texture: [f32; 4],
    viewport_direction: [f32; 4],
    tap_offsets: [f32; BACKDROP_BLUR_MAX_TAPS],
    tap_weights: [f32; BACKDROP_BLUR_MAX_TAPS],
    bounds: [f32; 4],
    corner_radii: [f32; 4],
    clip: [f32; 4],
    filter: [f32; 4],
}

/// The passes one blur draws: shrink, blur across, blur down, composite.
const PASSES_PER_BLUR: u64 = 4;

pub(crate) struct BackdropBlurRenderer {
    downsample: wgpu::RenderPipeline,
    gaussian: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    present: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    format: wgpu::TextureFormat,
    params_stride: u64,
    params: Option<(wgpu::Buffer, u64)>,
    frame: Option<Target>,
    scratch: Option<[Target; 2]>,
}

struct Target {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl BackdropBlurRenderer {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("backdrop_blur_shaders"),
            source: wgpu::ShaderSource::Wgsl(SHADERS.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("backdrop_blur_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: NonZeroU64::new(
                            std::mem::size_of::<BackdropBlurParams>() as u64,
                        ),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("backdrop_blur_pipeline_layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |name: &str, vertex: &str, fragment: &str, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(name),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vertex),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fragment),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("backdrop_blur_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let alignment = u64::from(device.limits().min_uniform_buffer_offset_alignment);
        Self {
            downsample: pipeline(
                "backdrop_blur_downsample",
                "vs_blur_pass",
                "fs_blur_downsample",
                None,
            ),
            gaussian: pipeline(
                "backdrop_blur_gaussian",
                "vs_blur_pass",
                "fs_blur_gaussian",
                None,
            ),
            composite: pipeline(
                "backdrop_blur_composite",
                "vs_blur_composite",
                "fs_blur_composite",
                Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            ),
            present: pipeline(
                "backdrop_blur_present",
                "vs_blur_pass",
                "fs_blur_present",
                None,
            ),
            layout,
            sampler,
            format,
            params_stride: (std::mem::size_of::<BackdropBlurParams>() as u64)
                .next_multiple_of(alignment),
            params: None,
            frame: None,
            scratch: None,
        }
    }

    /// The texture a frame of `size` with blurs is drawn into. Its contents
    /// are left from the last frame; the frame clears it.
    pub(crate) fn begin_frame(
        &mut self,
        device: &wgpu::Device,
        size: Size<DevicePixels>,
        blur_count: usize,
    ) -> wgpu::TextureView {
        let (width, height) = (size.width.0.max(1) as u32, size.height.0.max(1) as u32);
        if self
            .frame
            .as_ref()
            .is_none_or(|frame| frame.texture.width() != width || frame.texture.height() != height)
        {
            self.frame = Some(self.target(device, "backdrop_blur_frame", width, height));
        }
        let needed = self.params_stride * PASSES_PER_BLUR * blur_count.max(1) as u64;
        if self.params.as_ref().is_none_or(|(_, size)| *size < needed) {
            let size = needed.next_power_of_two();
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("backdrop_blur_params"),
                size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.params = Some((buffer, size));
        }
        self.frame.as_ref().unwrap().view.clone()
    }

    /// Blurs what has been drawn into the frame texture around the `index`th
    /// blur of the frame and draws it back inside the blur's region.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        index: usize,
        blur: &BackdropBlur,
        plan: &BackdropBlurPlan,
        size: Size<DevicePixels>,
    ) {
        let reduced_width = plan.reduced_size.width.0.max(1) as u32;
        let reduced_height = plan.reduced_size.height.0.max(1) as u32;
        self.ensure_scratch(device, reduced_width, reduced_height);
        let (Some(frame), Some([reduced, across]), Some((params_buffer, _))) =
            (&self.frame, &self.scratch, &self.params)
        else {
            return;
        };

        let mut params = BackdropBlurParams::new(blur, plan, size, &reduced.texture);
        let base = self.params_stride * PASSES_PER_BLUR * index as u64;
        let directions = [[0., 0.], [1., 0.], [0., 1.], [0., 0.]];
        for (pass, direction) in directions.iter().enumerate() {
            params.viewport_direction[2] = direction[0];
            params.viewport_direction[3] = direction[1];
            queue.write_buffer(
                params_buffer,
                base + self.params_stride * pass as u64,
                bytemuck::bytes_of(&params),
            );
        }

        let bind_group = |source: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("backdrop_blur_bind_group"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: params_buffer,
                            offset: 0,
                            size: NonZeroU64::new(std::mem::size_of::<BackdropBlurParams>() as u64),
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(source),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            })
        };
        // Shrink the frame into `reduced`, blur it across into `across`,
        // then down back into `reduced`.
        let passes = [
            (&self.downsample, bind_group(&frame.view), &reduced.view),
            (&self.gaussian, bind_group(&reduced.view), &across.view),
            (&self.gaussian, bind_group(&across.view), &reduced.view),
        ];
        for (pass_index, (pipeline, bind_group, target)) in passes.iter().enumerate() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("backdrop_blur_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            pass.set_viewport(0., 0., reduced_width as f32, reduced_height as f32, 0., 1.);
            pass.set_pipeline(pipeline);
            let offset = base + self.params_stride * pass_index as u64;
            pass.set_bind_group(0, bind_group, &[offset as u32]);
            pass.draw(0..4, 0..1);
        }

        let composite = bind_group(&reduced.view);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("backdrop_blur_composite"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &frame.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            ..Default::default()
        });
        pass.set_pipeline(&self.composite);
        let offset = base + self.params_stride * 3;
        pass.set_bind_group(0, &composite, &[offset as u32]);
        pass.draw(0..4, 0..1);
    }

    /// Copies the finished frame texture to `surface`.
    pub(crate) fn present(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        surface: &wgpu::TextureView,
    ) {
        let (Some(frame), Some((params_buffer, _))) = (&self.frame, &self.params) else {
            return;
        };
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("backdrop_blur_present_bind_group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: params_buffer,
                        offset: 0,
                        size: NonZeroU64::new(std::mem::size_of::<BackdropBlurParams>() as u64),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&frame.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("backdrop_blur_present"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: surface,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            ..Default::default()
        });
        pass.set_pipeline(&self.present);
        pass.set_bind_group(0, &bind_group, &[0]);
        pass.draw(0..4, 0..1);
    }

    fn ensure_scratch(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if let Some([reduced, _]) = &self.scratch
            && reduced.texture.width() >= width
            && reduced.texture.height() >= height
        {
            return;
        }
        // Grow in steps, so a panel that grows does not remake them each frame.
        let round_up = |length: u32| length.div_ceil(128) * 128;
        let (width, height) = match &self.scratch {
            Some([reduced, _]) => (
                round_up(width.max(reduced.texture.width())),
                round_up(height.max(reduced.texture.height())),
            ),
            None => (round_up(width), round_up(height)),
        };
        self.scratch = Some([
            self.target(device, "backdrop_blur_reduced", width, height),
            self.target(device, "backdrop_blur_across", width, height),
        ]);
    }

    fn target(&self, device: &wgpu::Device, label: &str, width: u32, height: u32) -> Target {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Target { texture, view }
    }
}

impl BackdropBlurParams {
    fn new(
        blur: &BackdropBlur,
        plan: &BackdropBlurPlan,
        size: Size<DevicePixels>,
        scratch: &wgpu::Texture,
    ) -> Self {
        let clip = blur.content_mask.bounds;
        Self {
            target_rect: [
                plan.target.origin.x.0 as f32,
                plan.target.origin.y.0 as f32,
                plan.target.size.width.0 as f32,
                plan.target.size.height.0 as f32,
            ],
            source_downsample_taps: [
                plan.source_origin.x.0 as f32,
                plan.source_origin.y.0 as f32,
                plan.downsample as f32,
                plan.kernel.tap_count as f32,
            ],
            reduced_texture: [
                plan.reduced_size.width.0 as f32,
                plan.reduced_size.height.0 as f32,
                scratch.width() as f32,
                scratch.height() as f32,
            ],
            viewport_direction: [size.width.0 as f32, size.height.0 as f32, 0., 0.],
            tap_offsets: plan.kernel.offsets,
            tap_weights: plan.kernel.weights,
            bounds: [
                blur.bounds.origin.x.0,
                blur.bounds.origin.y.0,
                blur.bounds.size.width.0,
                blur.bounds.size.height.0,
            ],
            corner_radii: [
                blur.corner_radii.top_left.0,
                blur.corner_radii.top_right.0,
                blur.corner_radii.bottom_right.0,
                blur.corner_radii.bottom_left.0,
            ],
            clip: [
                clip.origin.x.0,
                clip.origin.y.0,
                clip.size.width.0,
                clip.size.height.0,
            ],
            filter: [blur.saturation, blur.brightness, blur.opacity, 0.],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BackdropBlurParams;

    #[test]
    fn backdrop_blur_params_match_the_shader_struct() {
        // Fifteen vec4s.
        assert_eq!(std::mem::size_of::<BackdropBlurParams>(), 15 * 16);
    }
}
