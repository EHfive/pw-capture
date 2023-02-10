mod utils;
use utils::*;

use pw_capture_client as client;

use core::ffi::{c_char, CStr};
use core::mem;
use core::ptr;
use core::result::Result::{Err, Ok};
use core::slice;
use std::collections::HashSet;
use std::ffi::CString;
use std::time::Instant;

use anyhow::{anyhow, Result};
use ash::extensions::khr;
use ash::vk;
use ash_layer::*;
use dashmap::DashMap;
use libc;
use once_cell::sync::{Lazy, OnceCell};
use tracing::{debug, error, info, instrument, span, trace, warn, Level};
use tracing_subscriber::FmtSubscriber;

struct LayerInstanceValid {
    // khr_surface: khr::Surface,
    khr_phy_props2: khr::GetPhysicalDeviceProperties2,
}

struct LayerInstance {
    ash_instance: ash::Instance,
    #[allow(unused)]
    valid: Option<LayerInstanceValid>,
}

struct LayerDeviceValid {
    khr_memfd: khr::ExternalMemoryFd,
    // ext_modifier: ext::ImageDrmFormatModifier,
}

struct LayerDevice {
    instance: vk::Instance,
    phy_device: vk::PhysicalDevice,
    ash_device: ash::Device,
    khr_swapchain: khr::Swapchain,
    queues: Vec<vk::Queue>,
    valid: Option<LayerDeviceValid>,
}

#[allow(unused)]
struct LayerQueue {
    device: vk::Device,
    family_index: u32,
    family_props: vk::QueueFamilyProperties,
    index: u32,
}

struct ImageData {
    semaphores: Vec<vk::Semaphore>,
    fence: FenceState,
    seq: usize,
}

struct ExportImage {
    format: vk::Format,
    image: vk::Image,
    memory: vk::DeviceMemory,
    fds: Vec<(i32, vk::SubresourceLayout)>,
    src_image: (vk::Image, usize),
}

#[derive(Default)]
struct ExportData {
    format: vk::Format,
    queue: vk::Queue,
    queue_family_index: u32,
    command_pool: vk::CommandPool,
    command_buffers: Vec<vk::CommandBuffer>,
    modifier: Option<u64>,
    planes: u32,
}

struct LayerSwapchain {
    #[allow(unused)]
    device: vk::Device,
    #[allow(unused)]
    surface: vk::SurfaceKHR,
    format: vk::Format,
    extent: vk::Extent2D,
    images: Vec<vk::Image>,
    stream: Option<client::Stream>,
    image_datas: DashMap<vk::Image, ImageData>,
    export_images: DashMap<vk::Image, ExportImage>,
    export_data: Option<ExportData>,
}

static TRACING: Lazy<()> = Lazy::new(|| {
    let subscriber = FmtSubscriber::builder()
        .with_ansi(true)
        .without_time()
        .with_max_level(Level::DEBUG)
        .with_file(true)
        .with_line_number(true)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
});

static mut CLIENT: Lazy<Option<client::Client>> = Lazy::new(|| {
    client::Client::new()
        .map_err(|e| error!("failed to create client: {e:?}"))
        .ok()
});

static GIPA: OnceCell<vk::PFN_vkGetInstanceProcAddr> = OnceCell::new();
static ENTRY: OnceCell<ash::Entry> = OnceCell::new();

// DashMap ensures thread-safely
static INSTANCE_MAP: Lazy<DashMap<vk::Instance, LayerInstance>> = Lazy::new(|| DashMap::new());
static PHY_TO_INSTANCE_MAP: Lazy<DashMap<vk::PhysicalDevice, vk::Instance>> =
    Lazy::new(|| DashMap::new());
static GDPA_MAP: Lazy<DashMap<vk::Device, vk::PFN_vkGetDeviceProcAddr>> =
    Lazy::new(|| DashMap::new());
static DEVICE_MAP: Lazy<DashMap<vk::Device, LayerDevice>> = Lazy::new(|| DashMap::new());
static QUEUE_MAP: Lazy<DashMap<vk::Queue, LayerQueue>> = Lazy::new(|| DashMap::new());
static SWAPCHAIN_MAP: Lazy<DashMap<vk::SwapchainKHR, LayerSwapchain>> =
    Lazy::new(|| DashMap::new());

macro_rules! map_err {
    ($e:expr) => {{
        error!("{:?}", $e);
        match $e.downcast_ref::<vk::Result>() {
            Some(&v) => v,
            None => vk::Result::ERROR_UNKNOWN,
        }
    }};
}

macro_rules! map_result {
    ($res:expr) => {
        match $res {
            Ok(()) => vk::Result::SUCCESS,
            Err(e) => map_err!(e),
        }
    };
}

#[no_mangle]
#[doc = "https://vulkan.lunarg.com/doc/view/1.3.236.0/linux/LoaderLayerInterface.html#user-content-layer-interface-version-2"]
#[instrument]
pub unsafe extern "system" fn vkNegotiateLoaderLayerInterfaceVersion(
    p_version_struct: *mut NegotiateLayerInterface,
) -> vk::Result {
    Lazy::force(&TRACING);

    let version_struct = &mut *p_version_struct;
    debug!(
        "loader LayerInterfaceVersion: {}",
        version_struct.loader_layer_interface_version,
    );
    version_struct.loader_layer_interface_version = 2;

    // Only vkGetInstanceProcAddr and vkCreateInstance are mandatory to intercept
    version_struct.pfn_get_instance_proc_addr = Some(pwcap_vkGetInstanceProcAddr);

    // pfn_get_device_proc_addr and pfn_get_physical_device_proc_addr are optional
    // and can be cleared with related functions & match branches if not needed
    version_struct.pfn_get_device_proc_addr = Some(pwcap_vkGetDeviceProcAddr);
    version_struct.pfn_get_physical_device_proc_addr = None;

    vk::Result::SUCCESS
}
const _: PFN_vkNegotiateLoaderLayerInterfaceVersion = vkNegotiateLoaderLayerInterfaceVersion;

#[no_mangle]
#[instrument]
unsafe extern "system" fn pwcap_vkGetInstanceProcAddr(
    instance: vk::Instance,
    p_name: *const c_char,
) -> vk::PFN_vkVoidFunction {
    let name = CStr::from_ptr(p_name);
    loop {
        let pfn: *const () = match name.to_bytes() {
            b"vkGetInstanceProcAddr" => pwcap_vkGetInstanceProcAddr as _,
            b"vkCreateInstance" => pwcap_vkCreateInstance as _,
            b"vkDestroyInstance" => pwcap_vkDestroyInstance as _,
            b"vkGetDeviceProcAddr" => pwcap_vkGetDeviceProcAddr as _,
            b"vkCreateDevice" => pwcap_vkCreateDevice as _,
            b"vkDestroyDevice" => pwcap_vkDestroyDevice as _,
            _ => break,
        };
        debug!(
            name = name.to_string_lossy().as_ref(),
            ?pfn,
            "intercept instance function"
        );
        return mem::transmute(pfn);
    }
    let gipa = GIPA.get()?;
    gipa(instance, p_name)
}
const _: vk::PFN_vkGetInstanceProcAddr = pwcap_vkGetInstanceProcAddr;

#[no_mangle]
#[instrument]
unsafe extern "system" fn pwcap_vkGetDeviceProcAddr(
    device: vk::Device,
    p_name: *const c_char,
) -> vk::PFN_vkVoidFunction {
    let name = CStr::from_ptr(p_name);
    loop {
        let pfn: *const () = match name.to_bytes() {
            b"vkGetDeviceProcAddr" => pwcap_vkGetDeviceProcAddr as _,
            b"vkCreateDevice" => pwcap_vkCreateDevice as _,
            b"vkDestroyDevice" => pwcap_vkDestroyDevice as _,
            _ => break,
        };
        debug!(
            name = name.to_string_lossy().as_ref(),
            ?pfn,
            "intercept device function"
        );
        return mem::transmute(pfn);
    }

    let gdpa = GDPA_MAP.get(&device)?;
    let res = gdpa(device, p_name);
    loop {
        let pfn: *const () = match name.to_bytes() {
            b"vkCreateSwapchainKHR" => pwcap_vkCreateSwapchainKHR as _,
            b"vkDestroySwapchainKHR" => pwcap_vkDestroySwapchainKHR as _,
            b"vkAcquireNextImageKHR" => pwcap_vkAcquireNextImageKHR as _,
            b"vkAcquireNextImage2KHR" => pwcap_vkAcquireNextImage2KHR as _,
            b"vkQueuePresentKHR" => pwcap_vkQueuePresentKHR as _,
            _ => break,
        };
        if res.is_none() {
            // for extension command, return NULL if next layer does not support given command
            break;
        }
        debug!(
            name = name.to_string_lossy().as_ref(),
            ?pfn,
            "intercept device function"
        );
        return mem::transmute(pfn);
    }
    res
}

const _: vk::PFN_vkGetDeviceProcAddr = pwcap_vkGetDeviceProcAddr;

const LAYER_INSTANCE_EXTENSIONS: &[&'static CStr] = &[
    vk::KhrSurfaceFn::name(),
    vk::KhrExternalMemoryCapabilitiesFn::name(),
    vk::KhrGetPhysicalDeviceProperties2Fn::name(),
];

#[no_mangle]
#[instrument]
unsafe extern "system" fn pwcap_vkCreateInstance(
    p_create_info: *const vk::InstanceCreateInfo,
    p_allocator: *const vk::AllocationCallbacks,
    p_instance: *mut vk::Instance,
) -> vk::Result {
    let create_info = *p_create_info;
    let chain_info = get_instance_chain_info(&create_info, LayerFunction::LAYER_LINK_INFO);
    let chain_info = if let Some(mut v) = chain_info {
        v.as_mut()
    } else {
        error!("no chain info");
        return vk::Result::ERROR_INITIALIZATION_FAILED;
    };

    debug!("creating instance");

    let layer_info = chain_info.u.p_layer_info.read();
    chain_info.u.p_layer_info = layer_info.p_next;

    let gipa = layer_info
        .pfn_next_get_instance_proc_addr
        .expect("broken layer info");
    debug!(gipa = ?(gipa as *const ()));

    let name = CStr::from_bytes_with_nul_unchecked(b"vkCreateInstance\0");
    let create_instance: vk::PFN_vkCreateInstance =
        mem::transmute(gipa(vk::Instance::null(), name.as_ptr()));

    let mut extensions: HashSet<CString> = slice::from_raw_parts(
        create_info.pp_enabled_extension_names,
        create_info.enabled_extension_count as _,
    )
    .iter()
    .map(|&ptr| CStr::from_ptr(ptr).to_owned())
    .collect();
    // extra extensions used by layer
    for &name in LAYER_INSTANCE_EXTENSIONS {
        extensions.insert(name.to_owned());
    }
    debug!(?extensions, "instance extensions");
    let extensions_data: Vec<*const i8> = extensions.iter().map(|ext| ext.as_ptr()).collect();

    let mut create_info_ext = create_info.clone();
    create_info_ext.enabled_extension_count = extensions_data.len() as _;
    create_info_ext.pp_enabled_extension_names = extensions_data.as_ptr();

    let res = create_instance(&create_info_ext, p_allocator, p_instance);
    let valid = res == vk::Result::SUCCESS;
    if !valid {
        *p_instance = vk::Instance::null();
        let res = create_instance(&create_info, p_allocator, p_instance);
        if res != vk::Result::SUCCESS {
            return res;
        }
    }

    let instance = *p_instance;
    assert!(instance != vk::Instance::null());
    debug!(?instance, "created instance");

    // IMPORTANT: this should be put before any code executing dispatch_next_vkGetInstanceProcAddr
    //            i.e. ash::Instance::load and khr::Surface::new
    let _ = GIPA.set(gipa);

    let entry = ash::Entry::from_static_fn(vk::StaticFn {
        // IMPORTANT: this make sure the layer provided device specific vkGetDeviceProcAddr is used instead of
        //            the instance specific one get from vkGetInstanceProcAddr, as the later would somehow crashes on execution.
        get_instance_proc_addr: dispatch_next_vkGetInstanceProcAddr,
    });
    let _ = ENTRY.set(entry.clone());

    let ash_instance = ash::Instance::load(entry.static_fn(), instance);

    let phy_devices = ash_instance.enumerate_physical_devices().unwrap();
    for phy_device in phy_devices {
        PHY_TO_INSTANCE_MAP.insert(phy_device, instance);
    }

    let valid = if valid {
        // let khr_surface = khr::Surface::new(&entry, &ash_instance);
        let khr_phy_props2 = khr::GetPhysicalDeviceProperties2::new(&entry, &ash_instance);
        Some(LayerInstanceValid {
            // khr_surface,
            khr_phy_props2,
        })
    } else {
        None
    };

    INSTANCE_MAP.insert(
        instance,
        LayerInstance {
            ash_instance,
            valid,
        },
    );

    vk::Result::SUCCESS
}
const _: vk::PFN_vkCreateInstance = pwcap_vkCreateInstance;

unsafe fn destroy_instance(
    instance: vk::Instance,
    p_allocator: *const vk::AllocationCallbacks,
) -> Result<()> {
    debug!("destroying instance");
    let (_, ly_instance) = INSTANCE_MAP
        .remove(&instance)
        .ok_or(vk::Result::ERROR_UNKNOWN)?;

    let phy_devices = ly_instance
        .ash_instance
        .enumerate_physical_devices()
        .unwrap_or_default();
    for phy_device in phy_devices {
        PHY_TO_INSTANCE_MAP.remove(&phy_device);
    }

    (ly_instance.ash_instance.fp_v1_0().destroy_instance)(instance, p_allocator);
    Ok(())
}

#[no_mangle]
#[instrument]
unsafe extern "system" fn pwcap_vkDestroyInstance(
    instance: vk::Instance,
    p_allocator: *const vk::AllocationCallbacks,
) {
    let _ = map_result!(destroy_instance(instance, p_allocator));
}
const _: vk::PFN_vkDestroyInstance = pwcap_vkDestroyInstance;

const LAYER_DEVICE_EXTENSIONS: &[&'static CStr] = &[
    vk::KhrBindMemory2Fn::name(),
    vk::KhrImageFormatListFn::name(),
    vk::KhrMaintenance1Fn::name(),
    vk::KhrGetMemoryRequirements2Fn::name(),
    vk::KhrSamplerYcbcrConversionFn::name(),
    vk::ExtImageDrmFormatModifierFn::name(),
    vk::KhrExternalMemoryFn::name(),
    vk::KhrExternalMemoryFdFn::name(),
    vk::KhrSwapchainFn::name(),
];

#[no_mangle]
#[instrument]
unsafe extern "system" fn pwcap_vkCreateDevice(
    physical_device: vk::PhysicalDevice,
    p_create_info: *const vk::DeviceCreateInfo,
    p_allocator: *const vk::AllocationCallbacks,
    p_device: *mut vk::Device,
) -> vk::Result {
    debug!("creating device");

    let instance = *PHY_TO_INSTANCE_MAP.get(&physical_device).unwrap();
    let layer_instance = INSTANCE_MAP.get(&instance).unwrap();
    let ash_instance = &layer_instance.ash_instance;
    let instance_fn = ash_instance.fp_v1_0();

    let create_info = *p_create_info;
    let chain_info = get_device_chain_info(&create_info, LayerFunction::LAYER_LINK_INFO);
    let chain_info = if let Some(mut v) = chain_info {
        v.as_mut()
    } else {
        return vk::Result::ERROR_INITIALIZATION_FAILED;
    };

    let layer_info = chain_info.u.p_layer_info.read();
    chain_info.u.p_layer_info = layer_info.p_next;

    let gdpa = layer_info
        .pfn_next_get_device_proc_addr
        .expect("broken layer info");

    let mut extensions: HashSet<CString> = slice::from_raw_parts(
        create_info.pp_enabled_extension_names,
        create_info.enabled_extension_count as _,
    )
    .iter()
    .map(|&ptr| CStr::from_ptr(ptr).to_owned())
    .collect();
    // extra extensions used by layer
    for &name in LAYER_DEVICE_EXTENSIONS {
        extensions.insert(name.to_owned());
    }
    debug!("{:?}", extensions);
    let extensions_data: Vec<*const i8> = extensions.iter().map(|ext| ext.as_ptr()).collect();

    let mut create_info_ext = create_info.clone();
    create_info_ext.enabled_extension_count = extensions_data.len() as _;
    create_info_ext.pp_enabled_extension_names = extensions_data.as_ptr();

    let res = (instance_fn.create_device)(physical_device, &create_info_ext, p_allocator, p_device);
    let valid = res == vk::Result::SUCCESS;
    if !valid {
        *p_device = vk::Device::null();
        let res = (instance_fn.create_device)(physical_device, &create_info, p_allocator, p_device);
        if res != vk::Result::SUCCESS {
            return res;
        }
    }

    let device = *p_device;
    assert!(device != vk::Device::null());
    debug!(?device, "device created");

    // IMPORTANT: this should be put before any code executing dispatch_next_vkGetDeviceProcAddr,
    //            i.e. `ash::Device::load()` and `khr::Swapchain::new()`
    GDPA_MAP.insert(device, gdpa);

    let ash_device = ash::Device::load(&instance_fn, device);

    let khr_swapchain = khr::Swapchain::new(ash_instance, &ash_device);

    let valid = if valid {
        let khr_memfd = khr::ExternalMemoryFd::new(ash_instance, &ash_device);
        // let ext_modifier = ext::ImageDrmFormatModifier::new(ash_instance, &ash_device);
        Some(LayerDeviceValid {
            khr_memfd,
            // ext_modifier,
        })
    } else {
        None
    };

    let queue_family_properties =
        ash_instance.get_physical_device_queue_family_properties(physical_device);

    let queue_create_infos = core::slice::from_raw_parts(
        create_info.p_queue_create_infos,
        create_info.queue_create_info_count as _,
    );

    let mut queues = Vec::new();
    for queue_create_info in queue_create_infos {
        let &vk::DeviceQueueCreateInfo {
            queue_count,
            queue_family_index: family_index,
            ..
        } = queue_create_info;
        let family_props = queue_family_properties[family_index as usize];

        let span = span!(
            Level::DEBUG,
            "device queue family",
            family_index,
            queue_flags = ?family_props.queue_flags
        );
        let _enter = span.enter();

        for index in 0..queue_count {
            let queue = ash_device.get_device_queue(family_index, index);
            debug!(?index, ?queue, "device queue");
            queues.push(queue);
            QUEUE_MAP.insert(
                queue,
                LayerQueue {
                    device,
                    family_index,
                    family_props,
                    index,
                },
            );
        }
    }

    DEVICE_MAP.insert(
        device,
        LayerDevice {
            instance,
            phy_device: physical_device,
            ash_device,
            khr_swapchain,
            queues,
            valid,
        },
    );

    vk::Result::SUCCESS
}
const _: vk::PFN_vkCreateDevice = pwcap_vkCreateDevice;

unsafe fn destroy_device(
    device: vk::Device,
    p_allocator: *const vk::AllocationCallbacks,
) -> Result<()> {
    debug!("destroying device");
    GDPA_MAP.remove(&device);
    let (_, ly_device) = DEVICE_MAP
        .remove(&device)
        .ok_or(vk::Result::ERROR_DEVICE_LOST)?;

    for queue in ly_device.queues {
        QUEUE_MAP.remove(&queue);
    }

    (ly_device.ash_device.fp_v1_0().destroy_device)(device, p_allocator);
    return Ok(());
}

#[no_mangle]
#[instrument]
unsafe extern "system" fn pwcap_vkDestroyDevice(
    device: vk::Device,
    p_allocator: *const vk::AllocationCallbacks,
) {
    let _ = map_result!(destroy_device(device, p_allocator));
}
const _: vk::PFN_vkDestroyDevice = pwcap_vkDestroyDevice;

#[no_mangle]
unsafe extern "system" fn dispatch_next_vkGetInstanceProcAddr(
    instance: vk::Instance,
    p_name: *const c_char,
) -> vk::PFN_vkVoidFunction {
    let name = CStr::from_ptr(p_name);
    loop {
        let pfn: *const () = match name.to_bytes() {
            b"vkGetInstanceProcAddr" => dispatch_next_vkGetInstanceProcAddr as _,
            b"vkGetDeviceProcAddr" => dispatch_next_vkGetDeviceProcAddr as _,
            // These would cause some layer (i.e. VK_LAYER_KHRONOS_validation) crashes if called down via vkGetInstanceProcAddr.
            // But as ash::Entry loads all the global functions, we need to force return null to workaround it.
            // If you really need calling down these functions, follow
            // https://vulkan.lunarg.com/doc/view/1.3.236.0/linux/LoaderLayerInterface.html#user-content-pre-instance-functions
            b"vkEnumerateInstanceExtensionProperties" => ptr::null(),
            b"vkEnumerateInstanceLayerProperties" => ptr::null(),
            b"vkEnumerateInstanceVersion" => ptr::null(),
            _ => break,
        };
        return mem::transmute(pfn);
    }
    let gipa = GIPA.get()?;
    gipa(instance, p_name)
}
const _: vk::PFN_vkGetInstanceProcAddr = dispatch_next_vkGetInstanceProcAddr;

#[no_mangle]
unsafe extern "system" fn dispatch_next_vkGetDeviceProcAddr(
    device: vk::Device,
    p_name: *const c_char,
) -> vk::PFN_vkVoidFunction {
    let name = CStr::from_ptr(p_name);
    loop {
        let pfn: *const () = match name.to_bytes() {
            b"vkGetDeviceProcAddr" => dispatch_next_vkGetDeviceProcAddr as _,
            _ => break,
        };
        return mem::transmute(pfn);
    }
    let gdpa = GDPA_MAP.get(&device)?;
    gdpa(device, p_name)
}
const _: vk::PFN_vkGetDeviceProcAddr = dispatch_next_vkGetDeviceProcAddr;

#[instrument]
unsafe fn on_fixate_format(
    device: vk::Device,
    swapchain: vk::SwapchainKHR,
    info: client::EnumFormatInfo,
) -> Result<client::FixateFormat> {
    debug!("on_fixate_format");
    let ly_device = DEVICE_MAP
        .get(&device)
        .ok_or(vk::Result::ERROR_DEVICE_LOST)?;
    let ly_instance = INSTANCE_MAP
        .get(&ly_device.instance)
        .ok_or(vk::Result::ERROR_DEVICE_LOST)?;
    let ly_instance_valid = ly_instance.valid.as_ref().unwrap();
    let mut ly_swapchain = SWAPCHAIN_MAP
        .get_mut(&swapchain)
        .ok_or(vk::Result::ERROR_DEVICE_LOST)?;

    let transfer = vk_format_get_transfer(ly_swapchain.format);
    let format_info = client_format_get_info(info.formats[0], transfer);
    if format_info.vk_format == vk::Format::UNDEFINED {
        return Err(anyhow!(
            "format not supported: {:?} {:?}",
            info.formats[0],
            transfer
        ));
    }

    let (modifier, planes) = if info.modifiers.len() > 0 {
        let modifiers = get_supported_modifiers(
            &ly_instance_valid.khr_phy_props2,
            ly_device.phy_device,
            format_info.vk_format,
            vk::ImageUsageFlags::empty(),
            vk::FormatFeatureFlags::TRANSFER_DST,
        )?;
        let modifiers = modifiers
            .into_iter()
            .filter(|props| info.modifiers.contains(&props.drm_format_modifier))
            .collect::<Vec<_>>();

        debug!(?modifiers);

        let modifier = modifiers
            .get(0)
            .ok_or(anyhow!("modifiers {:?} not compatible", info.modifiers))?;

        (
            Some(modifier.drm_format_modifier),
            modifier.drm_format_modifier_plane_count,
        )
    } else {
        todo!("memfd")
    };

    let need_graphics = format_info.vk_format != ly_swapchain.format;
    let mut command_queue: Option<(vk::Queue, u32)> = None;

    for queue in &ly_device.queues {
        let ly_queue = if let Some(v) = QUEUE_MAP.get(queue) {
            v
        } else {
            continue;
        };
        if need_graphics {
            if ly_queue
                .family_props
                .queue_flags
                .contains(vk::QueueFlags::GRAPHICS)
            {
                command_queue = Some((*queue, ly_queue.family_index));
                break;
            }
        } else if ly_queue
            .family_props
            .queue_flags
            .contains(vk::QueueFlags::TRANSFER)
        {
            command_queue = Some((*queue, ly_queue.family_index));
            if !ly_queue
                .family_props
                .queue_flags
                .contains(vk::QueueFlags::GRAPHICS)
            {
                break;
            }
        }
    }
    let (queue, queue_family_index) = command_queue.ok_or(anyhow!("no compatible queue"))?;

    let (command_pool, command_buffers) = loop {
        if let Some(data) = ly_swapchain.export_data.take() {
            if data.queue == queue && data.command_buffers.len() >= ly_swapchain.images.len() {
                break (data.command_pool, data.command_buffers);
            }
            ly_device
                .ash_device
                .free_command_buffers(data.command_pool, &data.command_buffers);
            ly_device
                .ash_device
                .destroy_command_pool(data.command_pool, None);
        }
        let cmd_pool_info = vk::CommandPoolCreateInfo::builder()
            .queue_family_index(queue_family_index)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let cmd_pool = ly_device
            .ash_device
            .create_command_pool(&cmd_pool_info, None)?;
        let cmd_buffers_info = vk::CommandBufferAllocateInfo::builder()
            .command_pool(cmd_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(ly_swapchain.images.len() as _);
        let cmd_buffers = ly_device
            .ash_device
            .allocate_command_buffers(&cmd_buffers_info)?;
        break (cmd_pool, cmd_buffers);
    };

    info!(format = ?format_info, "stream format fixated");

    ly_swapchain.export_data = Some(ExportData {
        format: format_info.vk_format,
        queue,
        queue_family_index,
        command_pool,
        command_buffers,
        modifier,
        planes,
    });

    Ok(client::FixateFormat { modifier, planes })
}

#[instrument]
unsafe fn on_add_buffer(
    device: vk::Device,
    swapchain: vk::SwapchainKHR,
) -> Result<client::BufferInfo> {
    debug!("on_add_buffer");
    let ly_device = DEVICE_MAP
        .get(&device)
        .ok_or(vk::Result::ERROR_DEVICE_LOST)?;
    let ly_device_valid = ly_device.valid.as_ref().unwrap();
    let ly_instance = INSTANCE_MAP
        .get(&ly_device.instance)
        .ok_or(vk::Result::ERROR_DEVICE_LOST)?;
    let ly_swapchain = SWAPCHAIN_MAP
        .get(&swapchain)
        .ok_or(vk::Result::ERROR_DEVICE_LOST)?;

    let export_data = ly_swapchain
        .export_data
        .as_ref()
        .ok_or(anyhow!("no format fixated"))?;
    let export_format = export_data.format;

    if let Some(modifier) = export_data.modifier {
        let (image, memory, fds) = create_target_image(
            &ly_instance.ash_instance,
            &ly_device.ash_device,
            &ly_device_valid.khr_memfd,
            ly_device.phy_device,
            export_format,
            ly_swapchain.extent.width,
            ly_swapchain.extent.height,
            modifier,
            export_data.planes,
        )?;

        let plane_size = fds[0].1.size;
        assert!(plane_size > 0);

        debug!(?modifier, ?fds, "fd infos");

        let planes = fds
            .iter()
            .map(|(fd, layout)| client::BufferPlaneInfo {
                fd: *fd as _,
                offset: layout.offset as _,
                size: layout.size as _,
                stride: layout.row_pitch as _,
            })
            .collect::<Vec<_>>();

        ly_swapchain.export_images.insert(
            image,
            ExportImage {
                format: export_format,
                image,
                memory,
                fds,
                src_image: (vk::Image::null(), 0),
            },
        );

        Ok(client::BufferInfo {
            is_dma_buf: true,
            planes,
            user_handle: client::BufferUserHandle::VkImage(image),
        })
    } else {
        todo!()
    }
}

#[instrument]
unsafe fn on_remove_buffer(
    device: vk::Device,
    swapchain: vk::SwapchainKHR,
    user_handle: client::BufferUserHandle,
) -> Result<()> {
    debug!("on_remove_buffer");
    let image = match user_handle {
        client::BufferUserHandle::VkImage(image) => image,
        _ => unreachable!(),
    };

    let ly_device = DEVICE_MAP
        .get(&device)
        .ok_or(vk::Result::ERROR_DEVICE_LOST)?;
    let ly_swapchain = SWAPCHAIN_MAP
        .get(&swapchain)
        .ok_or(vk::Result::ERROR_DEVICE_LOST)?;

    let ExportImage {
        image, memory, fds, ..
    } = ly_swapchain
        .export_images
        .remove(&image)
        .ok_or(vk::Result::ERROR_UNKNOWN)?
        .1;

    ly_device.ash_device.destroy_image(image, None);
    for (fd, _) in fds {
        libc::close(fd);
    }
    ly_device.ash_device.free_memory(memory, None);

    Ok(())
}

unsafe fn on_process_buffer(
    device: vk::Device,
    swapchain: vk::SwapchainKHR,
    user_handle: client::BufferUserHandle,
) -> Result<()> {
    let image = match user_handle {
        client::BufferUserHandle::VkImage(image) => image,
        _ => unreachable!(),
    };

    trace!("process {image:?}");

    let ly_device = DEVICE_MAP
        .get(&device)
        .ok_or(vk::Result::ERROR_DEVICE_LOST)?;
    let ly_swapchain = SWAPCHAIN_MAP
        .get(&swapchain)
        .ok_or(vk::Result::ERROR_DEVICE_LOST)?;

    let (src_image, seq) = {
        let export_image = ly_swapchain.export_images.get(&image);
        if let Some(v) = export_image {
            v.src_image
        } else {
            debug!("buffer already removed");
            return Ok(());
        }
    };

    let mut data = ly_swapchain
        .image_datas
        .get_mut(&src_image)
        .ok_or(anyhow!("src image removed"))?;

    trace!(seq, data_seq = data.seq);
    if seq == data.seq {
        data.fence.wait_and_reset(&ly_device.ash_device)?;
    }

    Ok(())
}

#[instrument(skip(khr_phy_props2))]
unsafe fn create_stream(
    khr_phy_props2: &khr::GetPhysicalDeviceProperties2,
    phy_device: vk::PhysicalDevice,
    device: vk::Device,
    swapchain: vk::SwapchainKHR,
    swapchain_format: vk::Format,
    width: u32,
    height: u32,
) -> Result<client::Stream> {
    let src_format_info = vk_format_get_info(swapchain_format);
    // TODO: check if swapchain format is valid, e.g. supports TRANSFER_SRC

    info!(
        "creating stream:  width:{} height:{} format:{:?}",
        width, height, src_format_info
    );

    let formats: Vec<VkFormatInfo> = if src_format_info.format == client::Format::UNKNOWN {
        VK_FORMAT_INFO_TABLE
            .iter()
            .filter(|info| info.transfer == src_format_info.transfer)
            .cloned()
            .collect()
    } else {
        let it = VK_FORMAT_INFO_TABLE
            .iter()
            .filter(|info| {
                info.transfer == src_format_info.transfer
                    && !(info.vk_format == src_format_info.vk_format)
            })
            .cloned();
        core::iter::once(src_format_info).chain(it).collect()
    };

    // XXX: support for YUV formats with shader conversion?

    let mut enum_formats = Vec::<client::EnumFormatInfo>::new();

    'outer: for format_info in &formats {
        let (usage, features) = if src_format_info.vk_format == format_info.vk_format {
            (
                vk::ImageUsageFlags::TRANSFER_DST,
                vk::FormatFeatureFlags::TRANSFER_DST,
            )
        } else {
            (
                vk::ImageUsageFlags::TRANSFER_DST,
                vk::FormatFeatureFlags::BLIT_DST,
            )
        };
        let mut modifiers = get_supported_modifiers(
            khr_phy_props2,
            phy_device,
            format_info.vk_format,
            usage,
            features,
        )?
        .into_iter()
        .map(|props| props.drm_format_modifier)
        .collect::<Vec<_>>();

        if modifiers.is_empty() {
            debug!(?format_info, "does not support export modifier");
            continue;
        }

        let res = modifiers
            .iter()
            .enumerate()
            .find(|(_, &modifier)| modifier == 0);
        if let Some((idx, &default)) = res {
            modifiers.remove(idx);
            modifiers.insert(0, default);
        }

        for enum_format in &mut enum_formats {
            if enum_format.modifiers == modifiers {
                enum_format.formats.push(format_info.format);
                continue 'outer;
            }
        }

        let enum_format = client::EnumFormatInfo {
            formats: vec![format_info.format],
            modifiers,
        };
        enum_formats.push(enum_format);
    }

    for _format_info in &formats {
        // TODO: memfd or linear dma-buf
    }

    debug!(?enum_formats, "added formats");

    let stream_info = client::StreamInfo {
        width,
        height,
        enum_formats,
        fixate_format: Box::new(move |format| {
            on_fixate_format(device, swapchain, format)
                .map_err(|e| map_err!(e))
                .ok()
        }),
        add_buffer: Box::new(move || {
            on_add_buffer(device, swapchain)
                .map_err(|e| map_err!(e))
                .ok()
        }),
        remove_buffer: Box::new(move |user_handle| {
            let _ = on_remove_buffer(device, swapchain, user_handle).map_err(|e| map_err!(e));
        }),
        process_buffer: Box::new(move |user_handle| {
            let _ = on_process_buffer(device, swapchain, user_handle).map_err(|e| map_err!(e));
        }),
    };

    let stream = CLIENT
        .as_ref()
        .ok_or(anyhow!("failed to get client"))?
        .proxy()
        .try_create_stream(stream_info)???;

    Ok(stream)
}

unsafe fn create_swapchain_khr(
    device: vk::Device,
    p_create_info: *const vk::SwapchainCreateInfoKHR,
    p_allocator: *const vk::AllocationCallbacks,
    p_swapchain: *mut vk::SwapchainKHR,
) -> Result<()> {
    let ly_device = DEVICE_MAP
        .get(&device)
        .ok_or(vk::Result::ERROR_DEVICE_LOST)?;
    let ly_instance = INSTANCE_MAP
        .get(&ly_device.instance)
        .ok_or(vk::Result::ERROR_DEVICE_LOST)?;

    let mut create_info = p_create_info.read();
    create_info.image_usage |= vk::ImageUsageFlags::TRANSFER_SRC;

    let vk::SwapchainCreateInfoKHR {
        image_format,
        image_extent,
        ..
    } = create_info;

    (ly_device.khr_swapchain.fp().create_swapchain_khr)(
        device,
        &create_info,
        p_allocator,
        p_swapchain,
    )
    .result()?;
    let swapchain = *p_swapchain;
    debug!(
        "created: {:?}, old: {:?}",
        swapchain, create_info.old_swapchain
    );

    let images = ly_device
        .khr_swapchain
        .get_swapchain_images(swapchain)
        .unwrap_or_default();

    let image_datas = DashMap::new();

    let stream = if let Some(valid) = &ly_instance.valid {
        if ly_device.valid.is_some() {
            for &image in images.iter() {
                let semaphore_info = vk::SemaphoreCreateInfo::builder();
                let semaphore = ly_device
                    .ash_device
                    .create_semaphore(&semaphore_info, None)?;
                let data = ImageData {
                    semaphores: vec![semaphore],
                    fence: FenceState::new(&ly_device.ash_device)?,
                    seq: 0,
                };

                image_datas.insert(image, data);
            }

            create_stream(
                &valid.khr_phy_props2,
                ly_device.phy_device,
                device,
                swapchain,
                image_format,
                image_extent.width,
                image_extent.height,
            )
            .map_err(|e| error!("failed to create stream: {e:?}"))
            .ok()
        } else {
            None
        }
    } else {
        None
    };

    SWAPCHAIN_MAP.insert(
        swapchain,
        LayerSwapchain {
            device,
            surface: create_info.surface,
            format: image_format,
            extent: image_extent,
            images,
            export_data: None,
            image_datas,
            stream,
            export_images: DashMap::new(),
        },
    );

    Ok(())
}

#[no_mangle]
unsafe extern "system" fn pwcap_vkCreateSwapchainKHR(
    device: vk::Device,
    p_create_info: *const vk::SwapchainCreateInfoKHR,
    p_allocator: *const vk::AllocationCallbacks,
    p_swapchain: *mut vk::SwapchainKHR,
) -> vk::Result {
    map_result!(create_swapchain_khr(
        device,
        p_create_info,
        p_allocator,
        p_swapchain,
    ))
}
const _: vk::PFN_vkCreateSwapchainKHR = pwcap_vkCreateSwapchainKHR;

unsafe fn destroy_swapchain_khr(
    device: vk::Device,
    swapchain: vk::SwapchainKHR,
    p_allocator: *const vk::AllocationCallbacks,
) -> Result<()> {
    debug!("destroying");

    if let Some(ly_swapchain) = SWAPCHAIN_MAP.get(&swapchain) {
        if let Some(stream) = &ly_swapchain.stream {
            let stream = stream.proxy();
            drop(ly_swapchain);
            let _ = stream.try_terminate().map_err(|e| map_err!(e));
        }
    }
    let ly_swapchain = SWAPCHAIN_MAP.remove(&swapchain);

    let ly_device = DEVICE_MAP
        .get(&device)
        .ok_or(vk::Result::ERROR_DEVICE_LOST)?;

    if let Some((_, ly_swapchain)) = ly_swapchain {
        for image_data in &ly_swapchain.image_datas {
            image_data.fence.destroy(&ly_device.ash_device);
            for &s in &image_data.semaphores {
                ly_device.ash_device.destroy_semaphore(s, None);
            }
        }
        if let Some(export_data) = ly_swapchain.export_data {
            ly_device
                .ash_device
                .free_command_buffers(export_data.command_pool, &export_data.command_buffers);
            ly_device
                .ash_device
                .destroy_command_pool(export_data.command_pool, None);
        }
    }

    (ly_device.khr_swapchain.fp().destroy_swapchain_khr)(device, swapchain, p_allocator);
    Ok(())
}

#[no_mangle]
#[instrument]
unsafe extern "system" fn pwcap_vkDestroySwapchainKHR(
    device: vk::Device,
    swapchain: vk::SwapchainKHR,
    p_allocator: *const vk::AllocationCallbacks,
) {
    let _ = map_result!(destroy_swapchain_khr(device, swapchain, p_allocator));
}
const _: vk::PFN_vkDestroySwapchainKHR = pwcap_vkDestroySwapchainKHR;

unsafe fn queue_present_khr(
    queue: vk::Queue,
    p_present_info: *const vk::PresentInfoKHR,
) -> Result<vk::Result> {
    trace!("present");
    let ly_queue = QUEUE_MAP.get(&queue).ok_or(vk::Result::ERROR_DEVICE_LOST)?;
    let ly_device = DEVICE_MAP
        .get(&ly_queue.device)
        .ok_or(vk::Result::ERROR_DEVICE_LOST)?;

    let mut present_info = p_present_info.read();

    let _wait_semaphores_new = if ly_device.valid.is_some() {
        let res = capture(&ly_device.ash_device, ly_queue.family_index, &present_info);
        if res.len() > 0 {
            present_info.wait_semaphore_count = res.len() as _;
            present_info.p_wait_semaphores = res.as_ptr();
        }
        Some(res)
    } else {
        None
    };

    let res = (ly_device.khr_swapchain.fp().queue_present_khr)(queue, &present_info);
    match res {
        vk::Result::SUCCESS | vk::Result::SUBOPTIMAL_KHR => Ok(res),
        _ => Err(anyhow!(res)),
    }
}

unsafe fn ly_swapchain_wait_image(
    ly_device: &LayerDevice,
    ly_swapchain: &LayerSwapchain,
    image_index: usize,
) -> Result<()> {
    let image = ly_swapchain.images[image_index];
    let mut data = ly_swapchain
        .image_datas
        .get_mut(&image)
        .ok_or(anyhow!("image removed"))?;
    data.fence.wait_and_reset(&ly_device.ash_device)?;
    Ok(())
}

unsafe fn acquire_next_image_khr(
    device: vk::Device,
    swapchain: vk::SwapchainKHR,
    timeout: u64,
    semaphore: vk::Semaphore,
    fence: vk::Fence,
    p_image_index: *mut u32,
) -> Result<vk::Result> {
    let ly_device = DEVICE_MAP
        .get(&device)
        .ok_or(vk::Result::ERROR_DEVICE_LOST)?;
    let ly_swapchain = SWAPCHAIN_MAP
        .get(&swapchain)
        .ok_or(vk::Result::ERROR_DEVICE_LOST)?;

    let res = (ly_device.khr_swapchain.fp().acquire_next_image_khr)(
        device,
        swapchain,
        timeout,
        semaphore,
        fence,
        p_image_index,
    );
    match res {
        vk::Result::SUCCESS | vk::Result::SUBOPTIMAL_KHR => (),
        _ => return Err(anyhow!(res)),
    };

    if ly_device.valid.is_some() {
        ly_swapchain_wait_image(&ly_device, &ly_swapchain, *p_image_index as _)?;
    }
    Ok(res)
}

unsafe fn acquire_next_image2_khr(
    device: vk::Device,
    p_acquire_info: *const vk::AcquireNextImageInfoKHR,
    p_image_index: *mut u32,
) -> Result<vk::Result> {
    let acquire_info = p_acquire_info.read();
    let ly_device = DEVICE_MAP
        .get(&device)
        .ok_or(vk::Result::ERROR_DEVICE_LOST)?;
    let ly_swapchain = SWAPCHAIN_MAP
        .get(&acquire_info.swapchain)
        .ok_or(vk::Result::ERROR_DEVICE_LOST)?;

    let res = (ly_device.khr_swapchain.fp().acquire_next_image2_khr)(
        device,
        p_acquire_info,
        p_image_index,
    );
    match res {
        vk::Result::SUCCESS | vk::Result::SUBOPTIMAL_KHR => (),
        _ => return Err(anyhow!(res)),
    };

    if ly_device.valid.is_some() {
        ly_swapchain_wait_image(&ly_device, &ly_swapchain, *p_image_index as _)?;
    }
    Ok(res)
}

#[no_mangle]
#[instrument]
unsafe extern "system" fn pwcap_vkAcquireNextImageKHR(
    device: vk::Device,
    swapchain: vk::SwapchainKHR,
    timeout: u64,
    semaphore: vk::Semaphore,
    fence: vk::Fence,
    p_image_index: *mut u32,
) -> vk::Result {
    acquire_next_image_khr(device, swapchain, timeout, semaphore, fence, p_image_index)
        .unwrap_or_else(|e| map_err!(e))
}
const _: vk::PFN_vkAcquireNextImageKHR = pwcap_vkAcquireNextImageKHR;

#[no_mangle]
unsafe extern "system" fn pwcap_vkAcquireNextImage2KHR(
    device: vk::Device,
    p_acquire_info: *const vk::AcquireNextImageInfoKHR,
    p_image_index: *mut u32,
) -> vk::Result {
    acquire_next_image2_khr(device, p_acquire_info, p_image_index).unwrap_or_else(|e| map_err!(e))
}
const _: vk::PFN_vkAcquireNextImage2KHR = pwcap_vkAcquireNextImage2KHR;

#[no_mangle]
#[instrument]
unsafe extern "system" fn pwcap_vkQueuePresentKHR(
    queue: vk::Queue,
    p_present_info: *const vk::PresentInfoKHR,
) -> vk::Result {
    queue_present_khr(queue, p_present_info).unwrap_or_else(|e| map_err!(e))
}
const _: vk::PFN_vkQueuePresentKHR = pwcap_vkQueuePresentKHR;

unsafe fn capture_swapchain(
    ash_device: &ash::Device,
    swapchain: vk::SwapchainKHR,
    image_index: usize,
    src_queue_family_index: u32,
    wait_semaphores: &[vk::Semaphore],
) -> Result<Option<Vec<vk::Semaphore>>> {
    let stream = {
        let ly_swapchain = SWAPCHAIN_MAP
            .get(&swapchain)
            .ok_or(vk::Result::ERROR_UNKNOWN)?;
        match ly_swapchain.stream.as_ref() {
            Some(v) => v.proxy(),
            None => return Ok(None),
        }
    };

    let start = Instant::now();

    let (buffer, user_handle) = match stream.try_dequeue_buffer()?? {
        Some(v) => v,
        None => return Ok(None),
    };
    let export_image = match user_handle {
        client::BufferUserHandle::VkImage(image) => image,
        _ => unreachable!(),
    };
    let duration = start.elapsed();
    trace!(?duration, "dequeue");

    let ly_swapchain = SWAPCHAIN_MAP
        .get(&swapchain)
        .ok_or(vk::Result::ERROR_UNKNOWN)?;

    let export_data = ly_swapchain
        .export_data
        .as_ref()
        .ok_or(anyhow!("no format fixated"))?;

    let vk::Extent2D { width, height } = ly_swapchain.extent;
    let src_image = ly_swapchain.images[image_index as usize];

    let mut export_image_data = ly_swapchain
        .export_images
        .get_mut(&export_image)
        .ok_or(anyhow!("buffer image not found"))?;
    let export_format = export_image_data.format;

    let need_blit = export_format != ly_swapchain.format;

    let mut data = ly_swapchain
        .image_datas
        .get_mut(&src_image)
        .ok_or(anyhow!("src image data removed"))?;
    data.fence.wait_and_reset(&ash_device)?;

    let command_buffer = export_data.command_buffers[image_index as usize];
    ash_device.reset_command_buffer(command_buffer, vk::CommandBufferResetFlags::empty())?;

    record_copy_image(
        ash_device,
        command_buffer,
        src_image,
        export_image,
        src_queue_family_index,
        export_data.queue_family_index,
        width,
        height,
        need_blit,
    )?;

    let command_buffers = &[command_buffer];
    let wait_stages = &[vk::PipelineStageFlags::TRANSFER];
    let submit_info = vk::SubmitInfo::builder()
        .command_buffers(command_buffers)
        .wait_semaphores(wait_semaphores)
        .signal_semaphores(&data.semaphores)
        .wait_dst_stage_mask(wait_stages)
        .build();

    ash_device.queue_submit(export_data.queue, &[submit_info], data.fence.use_fence())?;
    data.seq += 1;
    export_image_data.src_image = (src_image, data.seq);

    let res = data.semaphores.clone();
    drop(data);
    drop(export_image_data);
    drop(ly_swapchain);

    let start = Instant::now();
    stream.try_queue_buffer_process(buffer)???;
    let duration = start.elapsed();
    trace!(?duration, "process");

    Ok(Some(res))
}

unsafe fn capture(
    ash_device: &ash::Device,
    src_queue_family_index: u32,
    present_info: &vk::PresentInfoKHR,
) -> Vec<vk::Semaphore> {
    let &vk::PresentInfoKHR {
        p_swapchains,
        p_image_indices,
        p_wait_semaphores,
        swapchain_count,
        wait_semaphore_count,
        ..
    } = present_info;

    let swapchains = slice::from_raw_parts(p_swapchains, swapchain_count as _);
    let image_indices = slice::from_raw_parts(p_image_indices, swapchain_count as _);
    let wait_semaphores_old = slice::from_raw_parts(p_wait_semaphores, wait_semaphore_count as _);

    let mut wait_semaphores_new = vec![];

    for i in 0..swapchains.len() {
        let res = capture_swapchain(
            ash_device,
            swapchains[i],
            image_indices[i] as _,
            src_queue_family_index,
            wait_semaphores_old,
        );
        match res {
            Ok(Some(v)) => wait_semaphores_new.extend(&v),
            Err(e) => {
                error!("failed to capture swapchain: {e:?}");
                continue;
            }
            _ => continue,
        }
    }

    wait_semaphores_new
}
