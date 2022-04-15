use rand::random;
use structopt::StructOpt;
use winit::{
    event::{ElementState, Event, KeyboardInput, VirtualKeyCode, WindowEvent},
    event_loop::ControlFlow,
    window::Window,
};

use gpgpu::{niw, util, Config, Error, Render, Screen};

mod render;

#[derive(Clone, StructOpt)]
pub struct Opt {
    #[structopt(short = "bg")]
    bg: Option<String>,

    #[structopt(short = "fg")]
    fg: Option<String>,

    #[structopt(short = "n", default_value = "100")]
    n_points: u32,
}

struct State {
    bg: wgpu::Color,
    fg: wgpu::Color,
    n_points: u32,
    texture: wgpu::Texture,
}

fn main() {
    env_logger::init();

    let opts = Opt::from_args();

    let name = "example-points".to_string();
    let config = Config::default();

    let mut swin = {
        let wattrs = config.to_window_attributes().unwrap();
        niw::SingleWindow::<Render<State>, ()>::from_config(wattrs).unwrap()
    };

    swin.on_win_close_requested(Box::new(on_win_close_requested))
        .on_win_keyboard_input(Box::new(on_win_keyboard_input))
        .on_win_resized(Box::new(on_win_resized))
        .on_win_scale_factor_changed(Box::new(on_win_scale_factor_changed))
        .on_main_events_cleared(Box::new(on_main_events_cleared))
        .on_redraw_requested(Box::new(on_redraw_requested));

    let r = {
        let screen = pollster::block_on(Screen::new(
            name.clone(),
            swin.as_window(),
            Config::default(),
        ))
        .unwrap();
        let texture = {
            let size = screen.to_extent3d();
            let usage =
                wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::RENDER_ATTACHMENT;
            let desc = wgpu::TextureDescriptor {
                label: Some("point-render"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: screen.to_texture_format(),
                usage,
            };
            screen.device.create_texture(&desc)
        };
        let state = State {
            bg: util::html_to_color(&opts.bg.clone().unwrap_or("#000000".to_string()))
                .unwrap(),
            fg: util::html_to_color(&opts.fg.clone().unwrap_or("#123456".to_string()))
                .unwrap(),
            n_points: opts.n_points,
            texture,
        };
        Render::new(screen, state)
    };

    println!("Press Esc to exit");
    swin.run(r);
}

// RedrawRequested will only trigger once, unless we manually request it.
fn on_main_events_cleared(
    w: &Window,
    _r: &mut Render<State>,
    _event: &mut Event<()>,
) -> Option<ControlFlow> {
    w.request_redraw();
    None
}

fn on_redraw_requested(
    _: &Window,
    r: &mut Render<State>,
    _event: &mut Event<()>,
) -> Option<ControlFlow> {
    let state = r.as_state();

    let vertices: Vec<render::Vertex> = (0..state.n_points)
        .map(|_| {
            let wgpu::Color { r, g, b, .. } = state.fg;
            let x = ((random::<i32>() as f64) / (i32::MAX as f64)) as f32;
            let y = ((random::<i32>() as f64) / (i32::MAX as f64)) as f32;
            // println!("{} {}", x, y);
            render::Vertex {
                position: [x, y, 0.0],
                color: [r as f32, g as f32, b as f32],
            }
        })
        .collect();
    let vertex_buffer = render::Vertex::to_buffer(&r.screen.device, vertices.as_slice());
    let pipeline = render::render_pipeline(&r.screen);

    let surface_texture = r.screen.get_current_texture().ok()?;
    //let surface_view = {
    //    let desc = wgpu::TextureViewDescriptor::default();
    //    surface_texture.texture.create_view(&desc)
    //};
    let render_view = {
        let desc = wgpu::TextureViewDescriptor::default();
        state.texture.create_view(&desc)
    };

    let mut encoder = {
        let desc = wgpu::CommandEncoderDescriptor {
            label: Some("example-points"),
        };
        r.screen.device.create_command_encoder(&desc)
    };
    {
        let mut render_pass = {
            let ops = wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: true,
            };
            let desc = wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[
                    // This is what @location(0) in the fragment shader targets
                    wgpu::RenderPassColorAttachment {
                        view: &render_view,
                        resolve_target: None,
                        ops,
                    },
                ],
                depth_stencil_attachment: None,
            };
            encoder.begin_render_pass(&desc)
        };
        render_pass.set_pipeline(&pipeline);
        render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        render_pass.draw(0..state.n_points, 0..1);
    }
    {
        let src = state.texture.as_image_copy();
        let dst = surface_texture.texture.as_image_copy();
        encoder.copy_texture_to_texture(src, dst, r.screen.to_extent3d())
    }

    let cmd_buffers = vec![encoder.finish()];

    match r.screen.render(cmd_buffers, surface_texture) {
        Ok(_) => None,
        // Reconfigure the surface if lost
        Err(Error::SurfaceLost(_, _)) => {
            r.screen.resize(r.screen.to_physical_size());
            None
        }
        // The system is out of memory, we should probably quit
        Err(Error::SurfaceOutOfMemory(_, _)) => Some(ControlFlow::Exit),
        // All other errors (Outdated, Timeout) should be resolved by the next frame
        Err(e) => {
            eprintln!("{:?}", e);
            None
        }
    }
}

fn on_win_resized(
    _: &Window,
    r: &mut Render<State>,
    event: &mut Event<()>,
) -> Option<ControlFlow> {
    match event {
        Event::WindowEvent { event, .. } => match event {
            WindowEvent::Resized(size) => r.screen.resize(*size),
            _ => unreachable!(),
        },
        _ => unreachable!(),
    }

    None
}

fn on_win_scale_factor_changed(
    _: &Window,
    r: &mut Render<State>,
    event: &mut Event<()>,
) -> Option<ControlFlow> {
    match event {
        Event::WindowEvent { event, .. } => match event {
            WindowEvent::ScaleFactorChanged { new_inner_size, .. } => {
                // TODO Is this the right way to handle it, doc says the following:
                // After this event callback has been processed, the window will be
                // resized to whatever value is pointed to by the new_inner_size
                // reference. By default, this will contain the size suggested by the
                // OS, but it can be changed to any value.
                r.screen.resize(**new_inner_size)
            }
            _ => unreachable!(),
        },
        _ => unreachable!(),
    }

    None
}

fn on_win_close_requested(
    _: &Window,
    _r: &mut Render<State>,
    _: &mut Event<()>,
) -> Option<ControlFlow> {
    Some(ControlFlow::Exit)
}

fn on_win_keyboard_input(
    _: &Window,
    _r: &mut Render<State>,
    event: &mut Event<()>,
) -> Option<ControlFlow> {
    match event {
        Event::WindowEvent { event, .. } => match event {
            WindowEvent::KeyboardInput {
                input:
                    KeyboardInput {
                        state: ElementState::Pressed,
                        virtual_keycode: Some(VirtualKeyCode::Escape),
                        ..
                    },
                ..
            } => Some(ControlFlow::Exit),
            _ => None,
        },
        _ => None,
    }
}
