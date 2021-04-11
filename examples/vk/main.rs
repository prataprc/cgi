use structopt::StructOpt;
use vulkano::app_info_from_cargo_toml;
use vulkano::instance::{Instance, InstanceExtensions, PhysicalDevice, QueueFamily};

mod info;
mod window;

/// Command line options.
#[derive(Debug, Clone, StructOpt)]
#[structopt(name = "vk", version = "0.0.1")]
pub struct Opt {
    #[structopt(long = "app-info")]
    app_info: bool,

    #[structopt(long = "layers")]
    layers: bool,

    #[structopt(long = "extensions")]
    extensions: bool,

    #[structopt(long = "pd")]
    pd: Option<usize>,

    #[structopt(long = "features")]
    features: bool,

    #[structopt(long = "monitors")]
    monitors: bool,

    #[structopt(long = "events")]
    events: bool,

    #[structopt(long = "limits")]
    limits: bool,
}

fn main() {
    let opts = Opt::from_args();

    let instance = Instance::new(None, &InstanceExtensions::none(), None)
        .expect("failed to create vulkan instance");

    let pd = match opts.pd {
        Some(index) => PhysicalDevice::from_index(&instance, index),
        None => None,
    };

    if opts.monitors {
        info::print_monitors()
    } else if opts.events {
        window::event_loop(opts)
    } else if opts.layers {
        info::print_layers()
    } else if opts.extensions {
        println!("Supported by core:");
        let ext = InstanceExtensions::supported_by_core().unwrap();
        info::print_extensions(ext);
        //println!("Supported by core (with loader):");
        //let ext = InstanceExtensions::supported_by_core_with_loader().unwrap();
        //info::print_extensions(ext)
    } else if opts.limits && pd.is_some() {
        // instance.limits()
    } else if opts.features && pd.is_some() {
        let pd = pd.as_ref().unwrap();
        info::print_features(pd, pd.supported_features());
    } else if pd.is_some() {
        info::print_pd(pd.as_ref().unwrap());
    } else if opts.app_info {
        let app_info = app_info_from_cargo_toml!();
        println!("Application-info");
        println!("  application_name: {:?}", app_info.application_name);
        println!("  application_vers: {:?}", app_info.application_version);
        println!("       engine_name: {:?}", app_info.engine_name);
        println!("       engine_vers: {:?}", app_info.engine_version);
    } else {
        println!("List of devices");
        for pd in PhysicalDevice::enumerate(&instance) {
            println!("{:4}: {}", pd.index(), pd.name())
        }
    }
}

#[allow(dead_code)]
fn get_graphics_qf<'a>(pd: &'a PhysicalDevice) -> QueueFamily<'a> {
    for qf in pd.queue_families() {
        if qf.supports_graphics() {
            return qf.clone();
        }
    }
    unreachable!()
}
