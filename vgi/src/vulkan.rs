use vulkano::{
    device::{DeviceExtensions, Features, Properties, Queue},
    format::{Format, FormatProperties},
    image::{
        ImageCreateFlags, ImageFormatProperties, ImageTiling, ImageType, ImageUsage,
    },
    instance::{
        ApplicationInfo, Instance, InstanceExtensions, LayerProperties, MemoryHeap,
        MemoryType, PhysicalDevice, QueueFamily, Version,
    },
};

use std::sync::Arc;

use crate::{Error, Result};

pub fn layers() -> Result<Vec<LayerProperties>> {
    Ok(err_at!(Vk, vulkano::instance::layers_list())?.collect())
}

/// Maps to VkQueueFlagBits.
#[derive(Clone)]
pub enum QueueCapability {
    Graphics,
    Compute,
    Transfer,
    SparseBinding,
}

/// Similar to VkDeviceQueueCreateInfo. A single instance of QueueCreateInfo shall create
/// as many VkQueue objects as then number of priorities, in other-words each item in
/// priorities vector specify the priority for queue-count-index.
///
/// By default [Builder] creates a single queue with Graphics capabilities with priority
/// `1.0`. Refer to [Builder::with_queues] to learn how to configure/create queues
/// for [Vulkan] instances.
#[derive(Clone)]
pub struct QueueCreateInfo {
    pub cap: QueueCapability,
    pub stages: Vec<vulkano::sync::PipelineStage>,
    pub priorities: Vec<f32>,
}

impl Default for QueueCreateInfo {
    fn default() -> Self {
        QueueCreateInfo {
            cap: QueueCapability::Graphics,
            stages: Vec::default(),
            priorities: vec![1.0],
        }
    }
}

fn make_queue_request<'a>(
    info: QueueCreateInfo,
    qfamilies: &[QueueFamily<'a>],
) -> Vec<(u32, f32)> {
    use std::cmp::min;

    for qf in qfamilies.iter() {
        let qf = match info.cap {
            QueueCapability::Graphics => {
                let ok1 = qf.supports_graphics();
                let ok2 = info
                    .stages
                    .clone()
                    .into_iter()
                    .all(|stage| qf.supports_stage(stage));

                if ok1 && ok2 {
                    qf
                } else {
                    continue;
                }
            }
            QueueCapability::Compute if qf.supports_compute() => qf,
            QueueCapability::Transfer if qf.explicitly_supports_transfers() => qf,
            QueueCapability::SparseBinding if qf.supports_sparse_binding() => qf,
            _ => continue,
        };
        return info.priorities
            [0..min(info.priorities.len(), qf.queues_count() as usize)]
            .to_vec()
            .into_iter()
            .map(|p| (qf.id(), p))
            .collect();
    }

    return vec![];
}

/// Return the vulkan implementation available through this package.
pub fn api_version() -> Result<Version> {
    use vulkano::instance::loader::auto_loader;

    let funcptrs = err_at!(Vk, auto_loader())?;
    err_at!(Vk, funcptrs.api_version())
}

pub struct Builder<'a> {
    // instance attributes
    app_info: ApplicationInfo<'a>,
    version: Version,
    layers: Vec<String>,
    iextns: InstanceExtensions,
    // device attributes
    device_id: usize,
    queue_infos: Vec<QueueCreateInfo>,
    dextns: Option<DeviceExtensions>,
    properties: Properties,
    features: Features,
}

impl<'a> Builder<'a> {
    /// Create new builder using cargo manifest for `application_info`, without enabling
    /// any of the instance-extensions and without enabling any of the layers. This
    /// method shall automatically detect the latest version from the driver's
    /// [FunctionPointers]. Later use one of the `with_*` methods to add more builder
    /// options.
    pub fn new() -> Result<Builder<'a>> {
        let builder = Builder {
            // instance attributes
            app_info: vulkano::app_info_from_cargo_toml!(),
            version: api_version()?,
            iextns: InstanceExtensions::none(),
            layers: Vec::default(),
            // device attributes
            device_id: 0,
            queue_infos: vec![QueueCreateInfo::default()],
            dextns: None,
            properties: Properties::default(),
            features: Features::none(),
        };

        Ok(builder)
    }

    /// Similar to [new] method, but supply the [ApplicationInfo] and [Version]. If
    /// requested [Version] is greater than the local vulkan version (driver), this
    /// call shall fail.
    pub fn with(
        app_info: ApplicationInfo<'a>,
        version: Option<Version>,
    ) -> Result<Builder<'a>> {
        let local_ver = api_version()?;
        let version = match version {
            Some(ver) if ver <= local_ver => ver,
            Some(ver) => err_at!(Vk, msg: "local_version {} < {}", local_ver, ver)?,
            None => local_ver,
        };

        Ok(Builder {
            // instance attributes
            app_info,
            version,
            iextns: InstanceExtensions::none(),
            layers: Vec::default(),
            // device attributes
            device_id: 0,
            queue_infos: vec![QueueCreateInfo::default()],
            dextns: None,
            properties: Properties::default(),
            features: Features::none(),
        })
    }

    /// Configure the [ApplicationInfo]
    pub fn with_app_info(mut self, app_info: ApplicationInfo<'a>) -> Self {
        self.app_info = app_info;
        self
    }

    /// List of layers to be enabled while creating vulkan-instance.
    pub fn with_layers<L>(mut self, layers: L) -> Self
    where
        L: IntoIterator<Item = &'a str>,
    {
        self.layers = layers.into_iter().map(|s| s.to_string()).collect();
        self
    }

    /// List of instance-extensions to enable while creating vulkan-instance. If
    /// `extensions` is None, then all supported core extensions shall be enabled.
    ///
    /// For screen rendering enable `khr_surface` extension and platform specific
    /// extensions like `khr_android_surface`, `khr_wayland_surface`,
    /// `khr_win32_surface`, `khr_xcb_surface`, `khr_xlib_surface`, `mvk_ios_surface`,
    /// `mvk_macos_surface`, `nn_vi_surface` in `InstanceExtensions`.
    pub fn with_extensions(mut self, extensions: Option<InstanceExtensions>) -> Self {
        self.iextns =
            extensions.unwrap_or(InstanceExtensions::supported_by_core().unwrap());
        self
    }

    /// Create VkDevice object using supplied parameters. At preset we don't have
    /// multi-device support. For requested [Features], device-extensions shall
    /// automatically be enabled event if they are not supplied in the `extensions` arg.
    ///
    /// By default if this method is not used, the the first available physical device
    /// shall be used with default properties and no-specific-feature requested and
    /// no-specific-device-extension enabled.
    ///
    /// If `extensions` is None, then all extensions required and supported by the
    /// physical device shall be enabled.
    ///
    /// For screen rendering enable `khr_swapchain` extension, also enable the
    /// `khr_surface` extension in `InstanceExtensions` refer to [with_extensions]
    /// method for details.
    pub fn with_device(
        mut self,
        id: usize,
        extensions: Option<DeviceExtensions>,
        properties: Properties,
        features: Features,
    ) -> Self {
        self.device_id = id;
        self.dextns = extensions;
        self.properties = properties;
        self.features = features;
        self
    }

    /// Create with queues. If not used a single graphics queue with priority 1.0 shall
    /// be used.
    pub fn with_queues(mut self, infos: Vec<QueueCreateInfo>) -> Self {
        self.queue_infos = infos;
        self
    }

    /// Finally call build, to obtain the [Vulkan] object. If surface and swapschain
    /// needs to be created for this Vulkan object, then pass valid set of instance
    /// extensions (for required surface).
    pub fn build(self, surface: Option<InstanceExtensions>) -> Result<Vulkan<'a>> {
        use vulkano::device::Device;
        use vulkano_win::VkSurfaceBuild;
        use winit::event_loop::EventLoop;
        use winit::window::WindowBuilder;

        let instance = {
            let iextns = match surface.clone() {
                Some(extens) => union_iextns(self.iextns.clone(), extens),
                None => self.iextns.clone(),
            };
            let layers = self.layers.iter().map(|s| s.as_str());

            let res = Instance::new(Some(&self.app_info), self.version, &iextns, layers);
            Box::new(err_at!(Vk, res)?)
        };

        let pds: Vec<PhysicalDevice> = unsafe {
            let inst = (instance.as_ref() as *const Arc<Instance>)
                .as_ref()
                .unwrap();
            PhysicalDevice::enumerate(inst).collect()
        };
        let pd = pds[self.device_id];
        confirm_properties(&self, pd.properties().clone())?;
        let qfamilies: Vec<QueueFamily> = pd.queue_families().collect();

        let dextns = match self.dextns {
            Some(extensions) => extensions,
            None => DeviceExtensions::required_extensions(pd),
        };
        let (dextns, device, queues) = {
            let qrs: Vec<(QueueFamily<'a>, f32)> = self
                .queue_infos
                .clone()
                .into_iter()
                .map(|info| make_queue_request(info, &qfamilies))
                .flatten()
                .map(|(id, p)| (pd.queue_family_by_id(id).unwrap(), p))
                .collect();
            let dextns = extensions_for_features(&self.features, dextns);
            let (device, queues) = err_at!(
                Vk,
                Device::new(pd, &self.features, &dextns, qrs.into_iter())
            )?;
            (dextns, device, queues.collect::<Vec<Arc<Queue>>>())
        };

        let event_loop = EventLoop::new();
        let surface = if surface.is_some() {
            let wb = WindowBuilder::new();
            Some(err_at!(
                Vk,
                wb.build_vk_surface(&event_loop, Arc::clone(&instance))
            )?)
        } else {
            None
        };

        let layers = layers()?
            .into_iter()
            .filter(|l| self.layers.contains(&l.name().to_string()))
            .collect();

        let val = Vulkan {
            // instance attribute
            layers,
            iextns: self.iextns,
            instance,
            phydevs: pds,
            // device attribute
            dextns,
            device,
            queues,
            // surface object
            event_loop,
            surface,
            swapchain: None,
            images: Some(Vec::default()),
        };

        Ok(val)
    }
}

pub struct SwapchainCreateInfo {
    // swapchain parameters
    num_images: u32,
    format: vulkano::format::Format,
    color_space: vulkano::swapchain::ColorSpace,
    dimensions: [u32; 2],
    layers: u32,
    usage: vulkano::image::ImageUsage,
    sharing_mode: vulkano::sync::SharingMode,
    transform: vulkano::swapchain::SurfaceTransform,
    composite_alpha: vulkano::swapchain::CompositeAlpha,
    present_mode: vulkano::swapchain::PresentMode,
    fullscreen_exclusive: vulkano::swapchain::FullscreenExclusive,
    clipped: bool,
}

/// Vulkan type roughly maps to instance/device object defined by the vulkan spec.
/// This type try to abstract the boiler plate code as much as possible by
/// providing convinient methods and related macros.
///
/// Note that this object shall be created in the main thread.
pub struct Vulkan<'a, W = winit::window::Window, T = ()>
where
    T: 'static,
{
    // instance objects
    layers: Vec<LayerProperties>,
    iextns: InstanceExtensions,
    instance: Box<Arc<Instance>>,
    phydevs: Vec<PhysicalDevice<'a>>,
    // device objects
    dextns: DeviceExtensions,
    device: Arc<vulkano::device::Device>,
    queues: Vec<Arc<Queue>>,
    // surface and swapchain objects
    event_loop: winit::event_loop::EventLoop<T>,
    surface: Option<Arc<vulkano::swapchain::Surface<W>>>,
    swapchain: Option<Arc<vulkano::swapchain::Swapchain<W>>>,
    images: Option<Vec<Arc<vulkano::image::swapchain::SwapchainImage<W>>>>,
}

impl<'a, W, T> Vulkan<'a, W, T>
where
    T: 'static,
{
    /// Return enabled layers for instance.
    pub fn enabled_layers(&self) -> Vec<LayerProperties> {
        self.layers.clone()
    }

    /// Return instance extensions that are enabled/disabled.
    pub fn instance_extensions(&self) -> InstanceExtensions {
        self.iextns.clone()
    }

    /// Return device extensions that are enabled/disabled.
    pub fn device_extensions(&self) -> DeviceExtensions {
        self.dextns.clone()
    }

    /// Return the instance api-version.
    pub fn api_version(&self) -> vulkano::instance::Version {
        self.instance.api_version()
    }

    /// Return the list of memory-heaps available for this device instance, depends
    /// on the physical-device used to create this device.
    pub fn memory_heaps(&self) -> Vec<MemoryHeap> {
        self.device.physical_device().memory_heaps().collect()
    }

    /// Return the list of memory-types available for this device instance, depends
    /// on the physical-device used to create this device.
    pub fn memory_types(&self) -> Vec<MemoryType> {
        self.device.physical_device().memory_types().collect()
    }

    /// Return the list of queue-families available for this device instance, depends
    /// on the physical-device used to create this device.
    pub fn queue_families(&self) -> Vec<QueueFamily> {
        self.device.physical_device().queue_families().collect()
    }

    /// Return the list of queue-families created for this device instance.
    pub fn active_queue_families(&self) -> Vec<QueueFamily> {
        self.device.active_queue_families().collect()
    }

    /// Return the properties of physical-device used to create this device.
    pub fn properties(&self) -> &Properties {
        self.device.physical_device().properties()
    }

    /// Return the features supported by physical-device used to create this device.
    pub fn supported_features(&self) -> &Features {
        self.device.physical_device().supported_features()
    }

    /// Return the format properties supported for this device.
    pub fn format_properties(&self, format: Format) -> Result<FormatProperties> {
        Ok(format.properties(self.device.physical_device()))
    }

    /// Return the image format properties supported for this device.
    pub fn image_format_properties(
        &self,
        format: Format,
        ty: ImageType,
        tiling: ImageTiling,
        usage: ImageUsage,
        create_flags: ImageCreateFlags,
    ) -> Result<ImageFormatProperties> {
        err_at!(
            Vk,
            self.device
                .image_format_properties(format, ty, tiling, usage, create_flags)
        )
    }

    /// Return the physical device used to create the device instance.
    pub fn to_physical_device(&'a self) -> PhysicalDevice<'a> {
        self.device.physical_device()
    }

    /// Return the instance object used to create this device.
    pub fn to_instance(&self) -> Arc<Instance> {
        Arc::clone(&self.instance)
    }

    /// Return the physical-device used to create this device.
    pub fn to_physical_devices(&self) -> Vec<PhysicalDevice<'a>> {
        self.phydevs.clone()
    }

    /// Return the queue objects created for this device
    pub fn to_queues(&self) -> Vec<Arc<Queue>> {
        self.queues.clone()
    }

    /// Return reference to surface object
    pub fn as_surface(&self) -> Option<&Arc<vulkano::swapchain::Surface<W>>> {
        self.surface.as_ref()
    }
}

impl<'a, W, T> Vulkan<'a, W, T> {
    fn default_swapchain_create_info(&self) -> Result<SwapchainCreateInfo> {
        use vulkano::{
            swapchain::{
                CompositeAlpha, FullscreenExclusive, PresentMode, SurfaceTransform,
            },
            sync::SharingMode,
        };

        let cap = match &self.surface {
            Some(srfc) => err_at!(Vk, srfc.capabilities(self.to_physical_device()))?,
            None => err_at!(Vk, msg: "surface not enabled")?,
        };
        let (format, color_space) = match cap.supported_formats.into_iter().next() {
            Some((Format::R8G8B8A8Unorm, cs)) => (Format::R8G8B8A8Unorm, cs),
            Some((Format::B8G8R8A8Unorm, cs)) => (Format::B8G8R8A8Unorm, cs),
            Some((format, cs)) => (format, cs),
            None => err_at!(Vk, msg: "no image-formats supported by surface")?,
        };

        Ok(SwapchainCreateInfo {
            num_images: cap.min_image_count,
            format,
            color_space,
            dimensions: Default::default(),
            layers: 1,
            usage: ImageUsage::none(),
            sharing_mode: SharingMode::Exclusive,
            transform: SurfaceTransform::Identity,
            composite_alpha: CompositeAlpha::Opaque,
            present_mode: PresentMode::Fifo,
            fullscreen_exclusive: FullscreenExclusive::Default,
            clipped: true,
        })
    }

    pub fn create_swapchain(&mut self, info: Option<SwapchainCreateInfo>) -> Result<()> {
        use std::cmp;
        use vulkano::swapchain::Swapchain;

        let info = match info {
            Some(info) => info,
            None => self.default_swapchain_create_info()?,
        };

        let (device, surface, cap) = match &self.surface {
            Some(surface) => {
                let cap = err_at!(Vk, surface.capabilities(self.to_physical_device()))?;
                (Arc::clone(&self.device), Arc::clone(surface), cap)
            }
            None => err_at!(Vk, msg: "surface not enabled")?,
        };

        let (swapchain, images) = {
            let max_image_count = cap.max_image_count.unwrap_or(info.num_images);
            err_at!(
                Vk,
                Swapchain::start(device, surface)
                    .num_images(cmp::min(info.num_images, max_image_count))
                    .format(info.format)
                    .color_space(info.color_space)
                    .dimensions(info.dimensions)
                    .layers(info.layers)
                    .usage(info.usage)
                    .sharing_mode(info.sharing_mode)
                    .transform(info.transform)
                    .composite_alpha(info.composite_alpha)
                    .present_mode(info.present_mode)
                    .fullscreen_exclusive(info.fullscreen_exclusive)
                    .clipped(info.clipped)
                    .build()
            )?
        };
        self.swapchain = Some(swapchain);
        self.images = Some(images);

        Ok(())
    }

    pub fn recreate_swapchain(&mut self, _info: SwapchainCreateInfo) {
        todo!()
    }

    pub unsafe fn wait(&self) -> Result<()> {
        err_at!(Vk, self.device.wait())
    }
}

//TODO
//fn enable_layers(layers: &[LayerProperties]) -> Vec<&'static str> {
//    layers
//        .iter()
//        .filter_map(|layer| match layer.name() {
//            "VK_LAYER_LUNARG_parameter_validation" => {
//                Some("VK_LAYER_LUNARG_parameter_validation")
//            }
//            "VK_LAYER_LUNARG_object_tracker" => Some("VK_LAYER_LUNARG_object_tracker"),
//            "VK_LAYER_LUNARG_standard_validation" => {
//                Some("VK_LAYER_LUNARG_standard_validation")
//            }
//            "VK_LAYER_LUNARG_core_validation" => Some("VK_LAYER_LUNARG_core_validation"),
//            "VK_LAYER_GOOGLE_threading" => Some("VK_LAYER_GOOGLE_threading"),
//            "VK_LAYER_GOOGLE_unique_objects" => Some("VK_LAYER_GOOGLE_unique_objects"),
//            _ => None,
//        })
//        .collect()
//}

//#[macro_export]
//macro_rules! feature_conflict {
//    ($features:ident, $field:ident, $($conflict:ident,)*) => {{
//        $(
//            if $features.$field && $features.$conflict {
//                let (field, conflict) = (stringify!($field), stringify!($conflict));
//                err_at!(Vk, msg: "{} conflict with {}", field, conflict)?
//            }
//        )*
//    }};
//}
//
//#[macro_export]
//macro_rules! device_extension_require_feature {
//    ($exten:expr, $features:ident, $field:ident) => {
//        if $exten {
//            $features.$field = true;
//        }
//    };
//}
//
//#[macro_export]
//macro_rules! feature_requires {
//    ($features:ident, $field:ident, $require:ident) => {
//        if $features.$field {
//            $features.$require = true;
//        }
//    };
//}
//
//pub fn dependency(
//    iextens: InstanceExtensions,
//    dextens: DeviceExtensions,
//    features: Features,
//) -> Result<(InstanceExtensions, DeviceExtensions, Features, Version)> {
//
//    // feature conflicts with other features.
//    feature_conflict!(
//        features, attachment_fragment_shading_rate,
//        shading_rate_image, fragment_density_map
//    );
//    feature_conflict!(
//        features, fragment_density_map,
//        pipeline_fragment_shading_rate, primitive_fragment_shading_rate,
//        attachment_fragment_shading_rate
//    );
//    feature_conflict!(
//        features, pipeline_fragment_shading_rate,
//        shading_rate_image, fragment_density_map
//    );
//    feature_conflict!(
//        features, primitive_fragment_shading_rate,
//        shading_rate_image, fragment_density_map
//    );
//    feature_conflict!(
//        features, shading_rate_image,
//        pipeline_fragment_shading_rate, primitive_fragment_shading_rate,
//        attachment_fragment_shading_rate
//    );
//    // feature required by device extension
//    device_extension_require_feature!(
//        dextens.ext_descriptor_indexing, features, descriptor_indexing,
//    );
//    device_extension_require_feature!(
//        dextens.khr_draw_indirect_count, features, draw_indirect_count,
//    );
//    device_extension_require_feature!(
//        dextens.ext_sampler_filter_minmax, features, sampler_filter_minmax,
//    );
//    device_extension_require_feature!(
//        dextens.khr_sampler_mirror_clamp_to_edge, features, sampler_mirror_clamp_to_edge,
//    );
//    device_extension_require_feature!(
//        dextens.khr_shader_draw_parameters, features, shader_draw_parameters,
//    );
//    device_extension_require_feature!(
//        dextens.ext_shader_viewport_index_layer, features, shader_output_layer,
//    );
//    device_extension_require_feature!(
//        dextens.ext_shader_viewport_index_layer, features, shader_output_viewport_index,
//    )
//    // feature requires other feature
//    feature_requires!(
//        features, sparse_image_float32_atomic_add, shader_image_float32_atomic_add
//    );
//    feature_requires!(
//        features, sparse_image_float32_atomics, shader_image_float32_atomics
//    );
//    feature_requires!(
//        features, sparse_image_int64_atomics, shader_image_int64_atomics
//    );
//
//    Ok((iextens, dextens, features))
//}

// TODO: why are we even doing this ? How can a device extension is enabled when a device
// feature is not available.
pub fn extensions_for_features(
    features: &Features,
    mut extensions: DeviceExtensions,
) -> DeviceExtensions {
    if !features.descriptor_indexing {
        extensions.ext_descriptor_indexing = false
    }
    if !features.draw_indirect_count {
        extensions.khr_draw_indirect_count = false
    }
    if !features.sampler_filter_minmax {
        extensions.ext_sampler_filter_minmax = false
    }
    if !features.sampler_mirror_clamp_to_edge {
        extensions.khr_sampler_mirror_clamp_to_edge = false
    }
    if !features.shader_output_layer {
        extensions.ext_shader_viewport_index_layer = false
    }
    extensions
}

// TODO: split this into properties, limits and more...
fn confirm_properties(val: &Builder, props: Properties) -> Result<()> {
    let p = val.properties.clone();

    if let Some(_val) = p.active_compute_unit_count {
        todo!()
    }
    if let Some(_val) = p.advanced_blend_all_operations {
        todo!()
    }
    if let Some(_val) = p.advanced_blend_correlated_overlap {
        todo!()
    }
    if let Some(_val) = p.advanced_blend_independent_blend {
        todo!()
    }
    if let Some(_val) = p.advanced_blend_max_color_attachments {
        todo!()
    }
    if let Some(_val) = p.advanced_blend_non_premultiplied_dst_color {
        todo!()
    }
    if let Some(_val) = p.advanced_blend_non_premultiplied_src_color {
        todo!()
    }
    if let Some(_val) = p.allow_command_buffer_query_copies {
        todo!()
    }
    if let Some(val) = p.api_version {
        if props.api_version.unwrap().lt(&val) {
            err_at!(Vk, msg: "api_version: {}", props.api_version.unwrap())?;
        }
    }
    if let Some(_val) = p.buffer_image_granularity {
        todo!()
    }
    if let Some(_val) = p.compute_units_per_shader_array {
        todo!()
    }
    if let Some(_val) = p.conformance_version {
        todo!()
    }
    if let Some(_val) = p.conservative_point_and_line_rasterization {
        todo!()
    }
    if let Some(_val) = p.conservative_rasterization_post_depth_coverage {
        todo!()
    }
    if let Some(_val) = p.cooperative_matrix_supported_stages {
        todo!()
    }
    if let Some(_val) = p.degenerate_lines_rasterized {
        todo!()
    }
    if let Some(_val) = p.degenerate_triangles_rasterized {
        todo!()
    }
    if let Some(_val) = p.denorm_behavior_independence {
        todo!()
    }
    if let Some(_val) = p.device_id {
        todo!()
    }
    if let Some(_val) = p.device_luid {
        todo!()
    }
    if let Some(_val) = p.device_luid_valid {
        todo!()
    }
    if let Some(_val) = p.device_name {
        todo!()
    }
    if let Some(_val) = p.device_node_mask {
        todo!()
    }
    if let Some(_val) = p.device_type {
        todo!()
    }
    if let Some(_val) = p.device_uuid {
        todo!()
    }
    if let Some(_val) = p.discrete_queue_priorities {
        todo!()
    }
    if let Some(_val) = p.driver_id {
        todo!()
    }
    if let Some(_val) = p.driver_info {
        todo!()
    }
    if let Some(_val) = p.driver_name {
        todo!()
    }
    if let Some(_val) = p.driver_uuid {
        todo!()
    }
    if let Some(_val) = p.driver_version {
        todo!()
    }
    if let Some(_val) = p.extra_primitive_overestimation_size_granularity {
        todo!()
    }
    if let Some(_val) = p.filter_minmax_image_component_mapping {
        todo!()
    }
    if let Some(_val) = p.filter_minmax_single_component_formats {
        todo!()
    }
    if let Some(_val) = p.fragment_density_invocations {
        todo!()
    }
    if let Some(_val) = p.fragment_shading_rate_non_trivial_combiner_ops {
        todo!()
    }
    if let Some(_val) = p.fragment_shading_rate_strict_multiply_combiner {
        todo!()
    }
    if let Some(_val) = p.fragment_shading_rate_with_conservative_rasterization {
        todo!()
    }
    if let Some(_val) = p.fragment_shading_rate_with_custom_sample_locations {
        todo!()
    }
    if let Some(_val) = p.fragment_shading_rate_with_fragment_shader_interlock {
        todo!()
    }
    if let Some(_val) = p.fragment_shading_rate_with_sample_mask {
        todo!()
    }
    if let Some(_val) = p.fragment_shading_rate_with_shader_depth_stencil_writes {
        todo!()
    }
    if let Some(_val) = p.fragment_shading_rate_with_shader_sample_mask {
        todo!()
    }
    if let Some(_val) = p.framebuffer_color_sample_counts {
        todo!()
    }
    if let Some(_val) = p.framebuffer_depth_sample_counts {
        todo!()
    }
    if let Some(_val) = p.framebuffer_integer_color_sample_counts {
        todo!()
    }
    if let Some(_val) = p.framebuffer_no_attachments_sample_counts {
        todo!()
    }
    if let Some(_val) = p.framebuffer_stencil_sample_counts {
        todo!()
    }
    if let Some(_val) = p.fully_covered_fragment_shader_input_variable {
        todo!()
    }
    if let Some(_val) = p.independent_resolve {
        todo!()
    }
    if let Some(_val) = p.independent_resolve_none {
        todo!()
    }
    if let Some(_val) = p.layered_shading_rate_attachments {
        todo!()
    }
    if let Some(_val) = p.line_sub_pixel_precision_bits {
        todo!()
    }
    if let Some(_val) = p.line_width_granularity {
        todo!()
    }
    if let Some(_val) = p.line_width_range {
        todo!()
    }
    if let Some(_val) = p.max_bound_descriptor_sets {
        todo!()
    }
    if let Some(_val) = p.max_clip_distances {
        todo!()
    }
    if let Some(_val) = p.max_color_attachments {
        todo!()
    }
    if let Some(_val) = p.max_combined_clip_and_cull_distances {
        todo!()
    }
    if let Some(_val) = p.max_compute_shared_memory_size {
        todo!()
    }
    if let Some(_val) = p.max_compute_work_group_count {
        todo!()
    }
    if let Some(_val) = p.max_compute_work_group_invocations {
        todo!()
    }
    if let Some(_val) = p.max_compute_work_group_size {
        todo!()
    }
    if let Some(_val) = p.max_compute_workgroup_subgroups {
        todo!()
    }
    if let Some(_val) = p.max_cull_distances {
        todo!()
    }
    if let Some(_val) = p.max_custom_border_color_samplers {
        todo!()
    }
    if let Some(_val) = p.max_descriptor_set_acceleration_structures {
        todo!()
    }
    if let Some(_val) = p.max_descriptor_set_inline_uniform_blocks {
        todo!()
    }
    if let Some(_val) = p.max_descriptor_set_input_attachments {
        todo!()
    }
    if let Some(_val) = p.max_descriptor_set_sampled_images {
        todo!()
    }
    if let Some(_val) = p.max_descriptor_set_samplers {
        todo!()
    }
    if let Some(_val) = p.max_descriptor_set_storage_buffers {
        todo!()
    }
    if let Some(_val) = p.max_descriptor_set_storage_buffers_dynamic {
        todo!()
    }
    if let Some(_val) = p.max_descriptor_set_storage_images {
        todo!()
    }
    if let Some(_val) = p.max_descriptor_set_subsampled_samplers {
        todo!()
    }
    if let Some(_val) = p.max_descriptor_set_uniform_buffers {
        todo!()
    }
    if let Some(_val) = p.max_descriptor_set_uniform_buffers_dynamic {
        todo!()
    }
    if let Some(_val) = p.max_descriptor_set_update_after_bind_acceleration_structures {
        todo!()
    }
    if let Some(_val) = p.max_descriptor_set_update_after_bind_inline_uniform_blocks {
        todo!()
    }
    if let Some(_val) = p.max_descriptor_set_update_after_bind_input_attachments {
        todo!()
    }
    if let Some(_val) = p.max_descriptor_set_update_after_bind_sampled_images {
        todo!()
    }
    if let Some(_val) = p.max_descriptor_set_update_after_bind_samplers {
        todo!()
    }
    if let Some(_val) = p.max_descriptor_set_update_after_bind_storage_buffers {
        todo!()
    }
    if let Some(_val) = p.max_descriptor_set_update_after_bind_storage_buffers_dynamic {
        todo!()
    }
    if let Some(_val) = p.max_descriptor_set_update_after_bind_storage_images {
        todo!()
    }
    if let Some(_val) = p.max_descriptor_set_update_after_bind_uniform_buffers {
        todo!()
    }
    if let Some(_val) = p.max_descriptor_set_update_after_bind_uniform_buffers_dynamic {
        todo!()
    }
    if let Some(_val) = p.max_discard_rectangles {
        todo!()
    }
    if let Some(_val) = p.max_draw_indexed_index_value {
        todo!()
    }
    if let Some(_val) = p.max_draw_indirect_count {
        todo!()
    }
    if let Some(_val) = p.max_draw_mesh_tasks_count {
        todo!()
    }
    if let Some(_val) = p.max_extra_primitive_overestimation_size {
        todo!()
    }
    if let Some(_val) = p.max_fragment_combined_output_resources {
        todo!()
    }
    if let Some(_val) = p.max_fragment_density_texel_size {
        todo!()
    }
    if let Some(_val) = p.max_fragment_dual_src_attachments {
        todo!()
    }
    if let Some(_val) = p.max_fragment_input_components {
        todo!()
    }
    if let Some(_val) = p.max_fragment_output_attachments {
        todo!()
    }
    if let Some(_val) = p.max_fragment_shading_rate_attachment_texel_size {
        todo!()
    }
    if let Some(_val) = p.max_fragment_shading_rate_attachment_texel_size_aspect_ratio {
        todo!()
    }
    if let Some(_val) = p.max_fragment_shading_rate_coverage_samples {
        todo!()
    }
    if let Some(_val) = p.max_fragment_shading_rate_invocation_count {
        todo!()
    }
    if let Some(_val) = p.max_fragment_shading_rate_rasterization_samples {
        todo!()
    }
    if let Some(_val) = p.max_fragment_size {
        todo!()
    }
    if let Some(_val) = p.max_fragment_size_aspect_ratio {
        todo!()
    }
    if let Some(_val) = p.max_framebuffer_height {
        todo!()
    }
    if let Some(_val) = p.max_framebuffer_layers {
        todo!()
    }
    if let Some(_val) = p.max_framebuffer_width {
        todo!()
    }
    if let Some(_val) = p.max_geometry_count {
        todo!()
    }
    if let Some(_val) = p.max_geometry_input_components {
        todo!()
    }
    if let Some(_val) = p.max_geometry_output_components {
        todo!()
    }
    if let Some(_val) = p.max_geometry_output_vertices {
        todo!()
    }
    if let Some(_val) = p.max_geometry_shader_invocations {
        todo!()
    }
    if let Some(_val) = p.max_geometry_total_output_components {
        todo!()
    }
    if let Some(_val) = p.max_graphics_shader_group_count {
        todo!()
    }
    if let Some(_val) = p.max_image_array_layers {
        todo!()
    }
    if let Some(_val) = p.max_image_dimension1_d {
        todo!()
    }
    if let Some(_val) = p.max_image_dimension2_d {
        todo!()
    }
    if let Some(_val) = p.max_image_dimension3_d {
        todo!()
    }
    if let Some(_val) = p.max_image_dimension_cube {
        todo!()
    }
    if let Some(_val) = p.max_indirect_commands_stream_count {
        todo!()
    }
    if let Some(_val) = p.max_indirect_commands_stream_stride {
        todo!()
    }
    if let Some(_val) = p.max_indirect_commands_token_count {
        todo!()
    }
    if let Some(_val) = p.max_indirect_commands_token_offset {
        todo!()
    }
    if let Some(_val) = p.max_indirect_sequence_count {
        todo!()
    }
    if let Some(_val) = p.max_inline_uniform_block_size {
        todo!()
    }
    if let Some(_val) = p.max_instance_count {
        todo!()
    }
    if let Some(_val) = p.max_interpolation_offset {
        todo!()
    }
    if let Some(_val) = p.max_memory_allocation_count {
        todo!()
    }
    if let Some(_val) = p.max_memory_allocation_size {
        todo!()
    }
    if let Some(_val) = p.max_mesh_multiview_view_count {
        todo!()
    }
    if let Some(_val) = p.max_mesh_output_primitives {
        todo!()
    }
    if let Some(_val) = p.max_mesh_output_vertices {
        todo!()
    }
    if let Some(_val) = p.max_mesh_total_memory_size {
        todo!()
    }
    if let Some(_val) = p.max_mesh_work_group_invocations {
        todo!()
    }
    if let Some(_val) = p.max_mesh_work_group_size {
        todo!()
    }
    if let Some(_val) = p.max_multiview_instance_index {
        todo!()
    }
    if let Some(_val) = p.max_multiview_view_count {
        todo!()
    }
    if let Some(_val) = p.max_per_set_descriptors {
        todo!()
    }
    if let Some(_val) = p.max_per_stage_descriptor_acceleration_structures {
        todo!()
    }
    if let Some(_val) = p.max_per_stage_descriptor_inline_uniform_blocks {
        todo!()
    }
    if let Some(_val) = p.max_per_stage_descriptor_input_attachments {
        todo!()
    }
    if let Some(_val) = p.max_per_stage_descriptor_sampled_images {
        todo!()
    }
    if let Some(_val) = p.max_per_stage_descriptor_samplers {
        todo!()
    }
    if let Some(_val) = p.max_per_stage_descriptor_storage_buffers {
        todo!()
    }
    if let Some(_val) = p.max_per_stage_descriptor_storage_images {
        todo!()
    }
    if let Some(_val) = p.max_per_stage_descriptor_uniform_buffers {
        todo!()
    }
    if let Some(_val) =
        p.max_per_stage_descriptor_update_after_bind_acceleration_structures
    {
        todo!()
    }
    if let Some(_val) = p.max_per_stage_descriptor_update_after_bind_inline_uniform_blocks
    {
        todo!()
    }
    if let Some(_val) = p.max_per_stage_descriptor_update_after_bind_input_attachments {
        todo!()
    }
    if let Some(_val) = p.max_per_stage_descriptor_update_after_bind_sampled_images {
        todo!()
    }
    if let Some(_val) = p.max_per_stage_descriptor_update_after_bind_samplers {
        todo!()
    }
    if let Some(_val) = p.max_per_stage_descriptor_update_after_bind_storage_buffers {
        todo!()
    }
    if let Some(_val) = p.max_per_stage_descriptor_update_after_bind_storage_images {
        todo!()
    }
    if let Some(_val) = p.max_per_stage_descriptor_update_after_bind_uniform_buffers {
        todo!()
    }
    if let Some(_val) = p.max_per_stage_resources {
        todo!()
    }
    if let Some(_val) = p.max_per_stage_update_after_bind_resources {
        todo!()
    }
    if let Some(_val) = p.max_primitive_count {
        todo!()
    }
    if let Some(_val) = p.max_push_constants_size {
        todo!()
    }
    if let Some(_val) = p.max_push_descriptors {
        todo!()
    }
    if let Some(_val) = p.max_ray_dispatch_invocation_count {
        todo!()
    }
    if let Some(_val) = p.max_ray_hit_attribute_size {
        todo!()
    }
    if let Some(_val) = p.max_ray_recursion_depth {
        todo!()
    }
    if let Some(_val) = p.max_recursion_depth {
        todo!()
    }
    if let Some(_val) = p.max_sample_location_grid_size {
        todo!()
    }
    if let Some(_val) = p.max_sample_mask_words {
        todo!()
    }
    if let Some(_val) = p.max_sampler_allocation_count {
        todo!()
    }
    if let Some(_val) = p.max_sampler_anisotropy {
        todo!()
    }
    if let Some(_val) = p.max_sampler_lod_bias {
        todo!()
    }
    if let Some(_val) = p.max_sgpr_allocation {
        todo!()
    }
    if let Some(_val) = p.max_shader_group_stride {
        todo!()
    }
    if let Some(_val) = p.max_storage_buffer_range {
        todo!()
    }
    if let Some(_val) = p.max_subgroup_size {
        todo!()
    }
    if let Some(_val) = p.max_subsampled_array_layers {
        todo!()
    }
    if let Some(_val) = p.max_task_output_count {
        todo!()
    }
    if let Some(_val) = p.max_task_total_memory_size {
        todo!()
    }
    if let Some(_val) = p.max_task_work_group_invocations {
        todo!()
    }
    if let Some(_val) = p.max_task_work_group_size {
        todo!()
    }
    if let Some(_val) = p.max_tessellation_control_per_patch_output_components {
        todo!()
    }
    if let Some(_val) = p.max_tessellation_control_per_vertex_input_components {
        todo!()
    }
    if let Some(_val) = p.max_tessellation_control_per_vertex_output_components {
        todo!()
    }
    if let Some(_val) = p.max_tessellation_control_total_output_components {
        todo!()
    }
    if let Some(_val) = p.max_tessellation_evaluation_input_components {
        todo!()
    }
    if let Some(_val) = p.max_tessellation_evaluation_output_components {
        todo!()
    }
    if let Some(_val) = p.max_tessellation_generation_level {
        todo!()
    }
    if let Some(_val) = p.max_tessellation_patch_size {
        todo!()
    }
    if let Some(_val) = p.max_texel_buffer_elements {
        todo!()
    }
    if let Some(_val) = p.max_texel_gather_offset {
        todo!()
    }
    if let Some(_val) = p.max_texel_offset {
        todo!()
    }
    if let Some(_val) = p.max_timeline_semaphore_value_difference {
        todo!()
    }
    if let Some(_val) = p.max_transform_feedback_buffer_data_size {
        todo!()
    }
    if let Some(_val) = p.max_transform_feedback_buffer_data_stride {
        todo!()
    }
    if let Some(_val) = p.max_transform_feedback_buffer_size {
        todo!()
    }
    if let Some(_val) = p.max_transform_feedback_buffers {
        todo!()
    }
    if let Some(_val) = p.max_transform_feedback_stream_data_size {
        todo!()
    }
    if let Some(_val) = p.max_transform_feedback_streams {
        todo!()
    }
    if let Some(_val) = p.max_triangle_count {
        todo!()
    }
    if let Some(_val) = p.max_uniform_buffer_range {
        todo!()
    }
    if let Some(_val) = p.max_update_after_bind_descriptors_in_all_pools {
        todo!()
    }
    if let Some(_val) = p.max_vertex_attrib_divisor {
        todo!()
    }
    if let Some(_val) = p.max_vertex_input_attribute_offset {
        todo!()
    }
    if let Some(_val) = p.max_vertex_input_attributes {
        todo!()
    }
    if let Some(_val) = p.max_vertex_input_binding_stride {
        todo!()
    }
    if let Some(_val) = p.max_vertex_input_bindings {
        todo!()
    }
    if let Some(_val) = p.max_vertex_output_components {
        todo!()
    }
    if let Some(_val) = p.max_vgpr_allocation {
        todo!()
    }
    if let Some(_val) = p.max_viewport_dimensions {
        todo!()
    }
    if let Some(_val) = p.max_viewports {
        todo!()
    }
    if let Some(_val) = p.mesh_output_per_primitive_granularity {
        todo!()
    }
    if let Some(_val) = p.mesh_output_per_vertex_granularity {
        todo!()
    }
    if let Some(_val) = p.min_acceleration_structure_scratch_offset_alignment {
        todo!()
    }
    if let Some(_val) = p.min_fragment_density_texel_size {
        todo!()
    }
    if let Some(_val) = p.min_fragment_shading_rate_attachment_texel_size {
        todo!()
    }
    if let Some(_val) = p.min_imported_host_pointer_alignment {
        todo!()
    }
    if let Some(_val) = p.min_indirect_commands_buffer_offset_alignment {
        todo!()
    }
    if let Some(_val) = p.min_interpolation_offset {
        todo!()
    }
    if let Some(_val) = p.min_memory_map_alignment {
        todo!()
    }
    if let Some(_val) = p.min_sequences_count_buffer_offset_alignment {
        todo!()
    }
    if let Some(_val) = p.min_sequences_index_buffer_offset_alignment {
        todo!()
    }
    if let Some(_val) = p.min_sgpr_allocation {
        todo!()
    }
    if let Some(_val) = p.min_storage_buffer_offset_alignment {
        todo!()
    }
    if let Some(_val) = p.min_subgroup_size {
        todo!()
    }
    if let Some(_val) = p.min_texel_buffer_offset_alignment {
        todo!()
    }
    if let Some(_val) = p.min_texel_gather_offset {
        todo!()
    }
    if let Some(_val) = p.min_texel_offset {
        todo!()
    }
    if let Some(_val) = p.min_uniform_buffer_offset_alignment {
        todo!()
    }
    if let Some(_val) = p.min_vertex_input_binding_stride_alignment {
        todo!()
    }
    if let Some(_val) = p.min_vgpr_allocation {
        todo!()
    }
    if let Some(_val) = p.mipmap_precision_bits {
        todo!()
    }
    if let Some(_val) = p.non_coherent_atom_size {
        todo!()
    }
    if let Some(_val) = p.optimal_buffer_copy_offset_alignment {
        todo!()
    }
    if let Some(_val) = p.optimal_buffer_copy_row_pitch_alignment {
        todo!()
    }
    if let Some(_val) = p.pci_bus {
        todo!()
    }
    if let Some(_val) = p.pci_device {
        todo!()
    }
    if let Some(_val) = p.pci_domain {
        todo!()
    }
    if let Some(_val) = p.pci_function {
        todo!()
    }
    if let Some(_val) = p.per_view_position_all_components {
        todo!()
    }
    if let Some(_val) = p.pipeline_cache_uuid {
        todo!()
    }
    if let Some(_val) = p.point_clipping_behavior {
        todo!()
    }
    if let Some(_val) = p.point_size_granularity {
        todo!()
    }
    if let Some(_val) = p.point_size_range {
        todo!()
    }
    if let Some(_val) = p.primitive_fragment_shading_rate_with_multiple_viewports {
        todo!()
    }
    if let Some(_val) = p.primitive_overestimation_size {
        todo!()
    }
    if let Some(_val) = p.primitive_underestimation {
        todo!()
    }
    if let Some(_val) = p.protected_no_fault {
        todo!()
    }
    if let Some(_val) = p.quad_divergent_implicit_lod {
        todo!()
    }
    if let Some(_val) = p.quad_operations_in_all_stages {
        todo!()
    }
    if let Some(_val) = p.required_subgroup_size_stages {
        todo!()
    }
    if let Some(_val) = p.residency_aligned_mip_size {
        todo!()
    }
    if let Some(_val) = p.residency_non_resident_strict {
        todo!()
    }
    if let Some(_val) = p.residency_standard2_d_block_shape {
        todo!()
    }
    if let Some(_val) = p.residency_standard2_d_multisample_block_shape {
        todo!()
    }
    if let Some(_val) = p.residency_standard3_d_block_shape {
        todo!()
    }
    if let Some(_val) = p.robust_buffer_access_update_after_bind {
        todo!()
    }
    if let Some(_val) = p.robust_storage_buffer_access_size_alignment {
        todo!()
    }
    if let Some(_val) = p.robust_uniform_buffer_access_size_alignment {
        todo!()
    }
    if let Some(_val) = p.rounding_mode_independence {
        todo!()
    }
    if let Some(_val) = p.sample_location_coordinate_range {
        todo!()
    }
    if let Some(_val) = p.sample_location_sample_counts {
        todo!()
    }
    if let Some(_val) = p.sample_location_sub_pixel_bits {
        todo!()
    }
    if let Some(_val) = p.sampled_image_color_sample_counts {
        todo!()
    }
    if let Some(_val) = p.sampled_image_depth_sample_counts {
        todo!()
    }
    if let Some(_val) = p.sampled_image_integer_sample_counts {
        todo!()
    }
    if let Some(_val) = p.sampled_image_stencil_sample_counts {
        todo!()
    }
    if let Some(_val) = p.sgpr_allocation_granularity {
        todo!()
    }
    if let Some(_val) = p.sgprs_per_simd {
        todo!()
    }
    if let Some(_val) = p.shader_arrays_per_engine_count {
        todo!()
    }
    if let Some(_val) = p.shader_core_features {
        todo!()
    }
    if let Some(_val) = p.shader_denorm_flush_to_zero_float16 {
        todo!()
    }
    if let Some(_val) = p.shader_denorm_flush_to_zero_float32 {
        todo!()
    }
    if let Some(_val) = p.shader_denorm_flush_to_zero_float64 {
        todo!()
    }
    if let Some(_val) = p.shader_denorm_preserve_float16 {
        todo!()
    }
    if let Some(_val) = p.shader_denorm_preserve_float32 {
        todo!()
    }
    if let Some(_val) = p.shader_denorm_preserve_float64 {
        todo!()
    }
    if let Some(_val) = p.shader_engine_count {
        todo!()
    }
    if let Some(_val) = p.shader_group_base_alignment {
        todo!()
    }
    if let Some(_val) = p.shader_group_handle_alignment {
        todo!()
    }
    if let Some(_val) = p.shader_group_handle_capture_replay_size {
        todo!()
    }
    if let Some(_val) = p.shader_group_handle_size {
        todo!()
    }
    if let Some(_val) = p.shader_input_attachment_array_non_uniform_indexing_native {
        todo!()
    }
    if let Some(_val) = p.shader_rounding_mode_rte_float16 {
        todo!()
    }
    if let Some(_val) = p.shader_rounding_mode_rte_float32 {
        todo!()
    }
    if let Some(_val) = p.shader_rounding_mode_rte_float64 {
        todo!()
    }
    if let Some(_val) = p.shader_rounding_mode_rtz_float16 {
        todo!()
    }
    if let Some(_val) = p.shader_rounding_mode_rtz_float32 {
        todo!()
    }
    if let Some(_val) = p.shader_rounding_mode_rtz_float64 {
        todo!()
    }
    if let Some(_val) = p.shader_sampled_image_array_non_uniform_indexing_native {
        todo!()
    }
    if let Some(_val) = p.shader_signed_zero_inf_nan_preserve_float16 {
        todo!()
    }
    if let Some(_val) = p.shader_signed_zero_inf_nan_preserve_float32 {
        todo!()
    }
    if let Some(_val) = p.shader_signed_zero_inf_nan_preserve_float64 {
        todo!()
    }
    if let Some(_val) = p.shader_sm_count {
        todo!()
    }
    if let Some(_val) = p.shader_storage_buffer_array_non_uniform_indexing_native {
        todo!()
    }
    if let Some(_val) = p.shader_storage_image_array_non_uniform_indexing_native {
        todo!()
    }
    if let Some(_val) = p.shader_uniform_buffer_array_non_uniform_indexing_native {
        todo!()
    }
    if let Some(_val) = p.shader_warps_per_sm {
        todo!()
    }
    if let Some(_val) = p.shading_rate_max_coarse_samples {
        todo!()
    }
    if let Some(_val) = p.shading_rate_palette_size {
        todo!()
    }
    if let Some(_val) = p.shading_rate_texel_size {
        todo!()
    }
    if let Some(_val) = p.simd_per_compute_unit {
        todo!()
    }
    if let Some(_val) = p.sparse_address_space_size {
        todo!()
    }
    if let Some(_val) = p.standard_sample_locations {
        todo!()
    }
    if let Some(_val) = p.storage_image_sample_counts {
        todo!()
    }
    if let Some(_val) = p.storage_texel_buffer_offset_alignment_bytes {
        todo!()
    }
    if let Some(_val) = p.storage_texel_buffer_offset_single_texel_alignment {
        todo!()
    }
    if let Some(_val) = p.strict_lines {
        todo!()
    }
    if let Some(_val) = p.sub_pixel_interpolation_offset_bits {
        todo!()
    }
    if let Some(_val) = p.sub_pixel_precision_bits {
        todo!()
    }
    if let Some(_val) = p.sub_texel_precision_bits {
        todo!()
    }
    if let Some(_val) = p.subgroup_quad_operations_in_all_stages {
        todo!()
    }
    if let Some(_val) = p.subgroup_size {
        todo!()
    }
    if let Some(_val) = p.subgroup_supported_operations {
        todo!()
    }
    if let Some(_val) = p.subgroup_supported_stages {
        todo!()
    }
    if let Some(_val) = p.subsampled_coarse_reconstruction_early_access {
        todo!()
    }
    if let Some(_val) = p.subsampled_loads {
        todo!()
    }
    if let Some(_val) = p.supported_depth_resolve_modes {
        todo!()
    }
    if let Some(_val) = p.supported_operations {
        todo!()
    }
    if let Some(_val) = p.supported_stages {
        todo!()
    }
    if let Some(_val) = p.supported_stencil_resolve_modes {
        todo!()
    }
    if let Some(_val) = p.timestamp_compute_and_graphics {
        todo!()
    }
    if let Some(_val) = p.timestamp_period {
        todo!()
    }
    if let Some(_val) = p.transform_feedback_draw {
        todo!()
    }
    if let Some(_val) = p.transform_feedback_queries {
        todo!()
    }
    if let Some(_val) = p.transform_feedback_rasterization_stream_select {
        todo!()
    }
    if let Some(_val) = p.transform_feedback_streams_lines_triangles {
        todo!()
    }
    if let Some(_val) = p.uniform_texel_buffer_offset_alignment_bytes {
        todo!()
    }
    if let Some(_val) = p.uniform_texel_buffer_offset_single_texel_alignment {
        todo!()
    }
    if let Some(_val) = p.variable_sample_locations {
        todo!()
    }
    if let Some(_val) = p.vendor_id {
        todo!()
    }
    if let Some(_val) = p.vgpr_allocation_granularity {
        todo!()
    }
    if let Some(_val) = p.vgprs_per_simd {
        todo!()
    }
    if let Some(_val) = p.viewport_bounds_range {
        todo!()
    }
    if let Some(_val) = p.viewport_sub_pixel_bits {
        todo!()
    }
    if let Some(_val) = p.wavefront_size {
        todo!()
    }
    if let Some(_val) = p.wavefronts_per_simd {
        todo!()
    }

    Ok(())
}

fn union_iextns(a: InstanceExtensions, b: InstanceExtensions) -> InstanceExtensions {
    InstanceExtensions {
        khr_android_surface: a.khr_android_surface || b.khr_android_surface,
        khr_device_group_creation: a.khr_device_group_creation
            || b.khr_device_group_creation,
        khr_display: a.khr_display || b.khr_display,
        khr_external_fence_capabilities: a.khr_external_fence_capabilities
            || b.khr_external_fence_capabilities,
        khr_external_memory_capabilities: a.khr_external_memory_capabilities
            || b.khr_external_memory_capabilities,
        khr_external_semaphore_capabilities: a.khr_external_semaphore_capabilities
            || b.khr_external_semaphore_capabilities,
        khr_get_display_properties2: a.khr_get_display_properties2
            || b.khr_get_display_properties2,
        khr_get_physical_device_properties2: a.khr_get_physical_device_properties2
            || b.khr_get_physical_device_properties2,
        khr_get_surface_capabilities2: a.khr_get_surface_capabilities2
            || b.khr_get_surface_capabilities2,
        khr_surface: a.khr_surface || b.khr_surface,
        khr_surface_protected_capabilities: a.khr_surface_protected_capabilities
            || b.khr_surface_protected_capabilities,
        khr_wayland_surface: a.khr_wayland_surface || b.khr_wayland_surface,
        khr_win32_surface: a.khr_win32_surface || b.khr_win32_surface,
        khr_xcb_surface: a.khr_xcb_surface || b.khr_xcb_surface,
        khr_xlib_surface: a.khr_xlib_surface || b.khr_xlib_surface,
        ext_acquire_xlib_display: a.ext_acquire_xlib_display
            || b.ext_acquire_xlib_display,
        ext_debug_report: a.ext_debug_report || b.ext_debug_report,
        ext_debug_utils: a.ext_debug_utils || b.ext_debug_utils,
        ext_direct_mode_display: a.ext_direct_mode_display || b.ext_direct_mode_display,
        ext_directfb_surface: a.ext_directfb_surface || b.ext_directfb_surface,
        ext_display_surface_counter: a.ext_display_surface_counter
            || b.ext_display_surface_counter,
        ext_headless_surface: a.ext_headless_surface || b.ext_headless_surface,
        ext_metal_surface: a.ext_metal_surface || b.ext_metal_surface,
        ext_swapchain_colorspace: a.ext_swapchain_colorspace
            || b.ext_swapchain_colorspace,
        ext_validation_features: a.ext_validation_features || b.ext_validation_features,
        ext_validation_flags: a.ext_validation_flags || b.ext_validation_flags,
        fuchsia_imagepipe_surface: a.fuchsia_imagepipe_surface
            || b.fuchsia_imagepipe_surface,
        ggp_stream_descriptor_surface: a.ggp_stream_descriptor_surface
            || b.ggp_stream_descriptor_surface,
        mvk_ios_surface: a.mvk_ios_surface || b.mvk_ios_surface,
        mvk_macos_surface: a.mvk_macos_surface || b.mvk_macos_surface,
        nn_vi_surface: a.nn_vi_surface || b.nn_vi_surface,
        nv_external_memory_capabilities: a.nv_external_memory_capabilities
            || b.nv_external_memory_capabilities,
        _unbuildable: a._unbuildable,
    }
}
