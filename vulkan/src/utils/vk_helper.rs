use crate::utils::*;

use anyhow::Result;
use ash::extensions::khr;
use ash::prelude::VkResult;
use ash::vk;
use function_name::named;

pub struct FenceState {
    fence: vk::Fence,
    busy: bool,
}

impl FenceState {
    pub unsafe fn new(device: &ash::Device) -> VkResult<Self> {
        let fence_info = vk::FenceCreateInfo::builder();
        let fence = device.create_fence(&fence_info, None)?;
        Ok(Self { fence, busy: false })
    }

    pub unsafe fn use_fence(&mut self) -> vk::Fence {
        assert!(!self.busy);
        self.busy = true;
        self.fence
    }

    pub unsafe fn wait_and_reset(&mut self, device: &ash::Device) -> VkResult<()> {
        if self.busy {
            device.wait_for_fences(&[self.fence], true, u64::MAX)?;
            device.reset_fences(&[self.fence])?;
            self.busy = false;
        }
        Ok(())
    }

    pub unsafe fn destroy(&self, device: &ash::Device) {
        device.destroy_fence(self.fence, None);
    }
}

#[named]
pub unsafe fn get_supported_modifiers(
    khr_phy_props2: &khr::GetPhysicalDeviceProperties2,
    phy_device: vk::PhysicalDevice,
    format: vk::Format,
    usage: vk::ImageUsageFlags,
    features: vk::FormatFeatureFlags,
) -> Result<Vec<vk::DrmFormatModifierPropertiesEXT>> {
    let mut modifier_props_list = vk::DrmFormatModifierPropertiesListEXT::builder().build();
    let mut props = vk::FormatProperties2KHR::builder()
        .push_next(&mut modifier_props_list)
        .build();
    khr_phy_props2.get_physical_device_format_properties2(phy_device, format, &mut props);

    let mut modifier_props: Vec<vk::DrmFormatModifierPropertiesEXT> = vec![
        vk::DrmFormatModifierPropertiesEXT::default();
        modifier_props_list.drm_format_modifier_count as _
    ];

    modifier_props_list.p_drm_format_modifier_properties = modifier_props.as_mut_ptr();
    khr_phy_props2.get_physical_device_format_properties2(phy_device, format, &mut props);

    let modifier_props: Vec<_> = modifier_props
        .into_iter()
        .filter(|props| {
            if !props.drm_format_modifier_tiling_features.contains(features) {
                return false;
            }
            let mut external_info = vk::PhysicalDeviceExternalImageFormatInfo::builder()
                .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT)
                .build();
            let mut modifier_info = vk::PhysicalDeviceImageDrmFormatModifierInfoEXT::builder()
                .drm_format_modifier(props.drm_format_modifier)
                .sharing_mode(vk::SharingMode::EXCLUSIVE)
                .build();
            let image_format_info = vk::PhysicalDeviceImageFormatInfo2KHR::builder()
                .push_next(&mut external_info)
                .push_next(&mut modifier_info)
                .format(format)
                .ty(vk::ImageType::TYPE_2D)
                .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
                .usage(usage)
                .flags(vk::ImageCreateFlags::empty());
            let mut image_format_props = vk::ImageFormatProperties2KHR::builder();
            khr_phy_props2
                .get_physical_device_image_format_properties2(
                    phy_device,
                    &image_format_info,
                    &mut image_format_props,
                )
                .is_ok()
        })
        .collect();

    for props in &modifier_props {
        trace!(
            "format:{:?} planes:{} {:?}",
            format,
            props.drm_format_modifier_plane_count,
            props.drm_format_modifier,
        );
    }

    Ok(modifier_props)
}

pub unsafe fn get_memory_type_indices(
    instance: &ash::Instance,
    phy_device: vk::PhysicalDevice,
    properties: vk::MemoryPropertyFlags,
    requirements: vk::MemoryRequirements,
) -> Vec<u32> {
    let memory = instance.get_physical_device_memory_properties(phy_device);
    (0..memory.memory_type_count)
        .filter(|i| {
            let suitable = (requirements.memory_type_bits & (1 << i)) != 0;
            let memory_type = memory.memory_types[*i as usize];
            suitable && memory_type.property_flags.contains(properties)
        })
        .collect()
}

pub unsafe fn create_target_image(
    ash_instance: &ash::Instance,
    ash_device: &ash::Device,
    khr_memfd: &khr::ExternalMemoryFd,
    // ext_modifier: &ext::ImageDrmFormatModifier,
    phy_device: vk::PhysicalDevice,
    format: vk::Format,
    width: u32,
    height: u32,
    modifier: u64,
    num_planes: u32,
) -> Result<(
    vk::Image,
    vk::DeviceMemory,
    Vec<(i32, vk::SubresourceLayout)>,
)> {
    let mut modidier_list = vk::ImageDrmFormatModifierListCreateInfoEXT::builder()
        .drm_format_modifiers(&[modifier])
        .build();
    let mut external_info = vk::ExternalMemoryImageCreateInfo::builder()
        .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT)
        .build();
    let image_info = vk::ImageCreateInfo::builder()
        .push_next(&mut external_info)
        .push_next(&mut modidier_list)
        .image_type(vk::ImageType::TYPE_2D)
        .format(format)
        .extent(vk::Extent3D {
            width,
            height,
            depth: 1,
        })
        .mip_levels(1)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
        .usage(vk::ImageUsageFlags::TRANSFER_DST)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);

    let image = ash_device.create_image(&image_info, None)?;

    let requirements = ash_device.get_image_memory_requirements(image);

    let indices = get_memory_type_indices(
        ash_instance,
        phy_device,
        // vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
        requirements,
    );

    let mut memory: VkResult<vk::DeviceMemory> = Err(vk::Result::ERROR_UNKNOWN);
    for i in indices {
        let memory_info = vk::MemoryAllocateInfo::builder()
            .allocation_size(requirements.size)
            .memory_type_index(i);
        memory = ash_device.allocate_memory(&memory_info, None);
        if memory.is_ok() {
            break;
        }
    }
    let memory = memory?;

    ash_device.bind_image_memory(image, memory, 0)?;

    // let mut props = vk::ImageDrmFormatModifierPropertiesEXT::builder().build();
    // ext_modifier.get_image_drm_format_modifier_properties(image, &mut props)?;
    // log!("modifier: {}", props.drm_format_modifier);

    let get_fd_info = vk::MemoryGetFdInfoKHR::builder()
        .memory(memory)
        .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);

    let dma_buf_fd = khr_memfd.get_memory_fd(&get_fd_info)?;
    // debug!("dma-buf fd: {}", dma_buf_fd);

    let fds = (0..num_planes.clamp(1, 4))
        .map(|i| {
            let subresource = vk::ImageSubresource::builder()
                .array_layer(0)
                .mip_level(0)
                .aspect_mask(vk::ImageAspectFlags::from_raw(
                    vk::ImageAspectFlags::MEMORY_PLANE_0_EXT.as_raw() << i,
                ))
                .build();
            let layout = ash_device.get_image_subresource_layout(image, subresource);
            let fd = if i == 0 {
                dma_buf_fd
            } else {
                libc::fcntl(dma_buf_fd, libc::F_DUPFD_CLOEXEC)
            };
            (fd, layout)
        })
        .collect::<Vec<_>>();

    Ok((image, memory, fds))
}

pub unsafe fn record_copy_image(
    ash_device: &ash::Device,
    command_buffer: vk::CommandBuffer,
    src_image: vk::Image,
    export_image: vk::Image,
    mut src_queue_family: u32,
    mut dst_queue_family: u32,
    width: u32,
    height: u32,
    need_blit: bool,
) -> VkResult<()> {
    if src_queue_family == dst_queue_family {
        src_queue_family = vk::QUEUE_FAMILY_IGNORED;
        dst_queue_family = vk::QUEUE_FAMILY_IGNORED;
    }

    let begin_info =
        vk::CommandBufferBeginInfo::builder().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
    ash_device.begin_command_buffer(command_buffer, &begin_info)?;

    let subresource = vk::ImageSubresourceRange::builder()
        .aspect_mask(vk::ImageAspectFlags::COLOR)
        .base_mip_level(0)
        .level_count(1)
        .base_array_layer(0)
        .layer_count(1)
        .build();

    let src_barrier = vk::ImageMemoryBarrier::builder()
        .old_layout(vk::ImageLayout::PRESENT_SRC_KHR)
        .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
        .src_queue_family_index(src_queue_family)
        .dst_queue_family_index(dst_queue_family)
        .image(src_image)
        .subresource_range(subresource)
        .src_access_mask(vk::AccessFlags::MEMORY_READ)
        .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
        .build();

    let dst_barrier = vk::ImageMemoryBarrier::builder()
        .old_layout(vk::ImageLayout::GENERAL)
        .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(export_image)
        .subresource_range(subresource)
        .src_access_mask(vk::AccessFlags::empty())
        .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
        .build();

    ash_device.cmd_pipeline_barrier(
        command_buffer,
        vk::PipelineStageFlags::BOTTOM_OF_PIPE,
        vk::PipelineStageFlags::TRANSFER,
        vk::DependencyFlags::empty(),
        &[],
        &[],
        &[src_barrier, dst_barrier],
    );

    let subresource_layer = vk::ImageSubresourceLayers::builder()
        .aspect_mask(vk::ImageAspectFlags::COLOR)
        .mip_level(0)
        .base_array_layer(0)
        .layer_count(1)
        .build();

    if need_blit {
        let src_subresource = vk::ImageSubresourceLayers::builder()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .mip_level(0)
            .base_array_layer(0)
            .layer_count(1)
            .build();

        let dst_subresource = vk::ImageSubresourceLayers::builder()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .mip_level(0)
            .base_array_layer(0)
            .layer_count(1)
            .build();

        let image_blit = vk::ImageBlit::builder()
            .src_offsets([
                vk::Offset3D { x: 0, y: 0, z: 0 },
                vk::Offset3D {
                    x: width as _,
                    y: height as _,
                    z: 1,
                },
            ])
            .src_subresource(src_subresource)
            .dst_offsets([
                vk::Offset3D { x: 0, y: 0, z: 0 },
                vk::Offset3D {
                    x: width as _,
                    y: height as _,
                    z: 1,
                },
            ])
            .dst_subresource(dst_subresource)
            .build();

        ash_device.cmd_blit_image(
            command_buffer,
            src_image,
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            export_image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &[image_blit],
            vk::Filter::NEAREST,
        )
    } else {
        let image_copy = vk::ImageCopy::builder()
            .extent(vk::Extent3D {
                width,
                height,
                depth: 1,
            })
            .src_subresource(subresource_layer)
            .dst_subresource(subresource_layer)
            .build();
        ash_device.cmd_copy_image(
            command_buffer,
            src_image,
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            export_image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &[image_copy],
        );
    }

    let src_barrier = vk::ImageMemoryBarrier::builder()
        .old_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
        .new_layout(vk::ImageLayout::PRESENT_SRC_KHR)
        .src_queue_family_index(dst_queue_family)
        .dst_queue_family_index(src_queue_family)
        .image(src_image)
        .subresource_range(subresource)
        .src_access_mask(vk::AccessFlags::TRANSFER_READ)
        .dst_access_mask(vk::AccessFlags::MEMORY_READ)
        .build();

    let dst_barrier = vk::ImageMemoryBarrier::builder()
        .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .new_layout(vk::ImageLayout::GENERAL)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(export_image)
        .subresource_range(subresource)
        .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
        .dst_access_mask(vk::AccessFlags::empty())
        .build();

    ash_device.cmd_pipeline_barrier(
        command_buffer,
        vk::PipelineStageFlags::TRANSFER,
        vk::PipelineStageFlags::BOTTOM_OF_PIPE,
        vk::DependencyFlags::empty(),
        &[],
        &[],
        &[src_barrier, dst_barrier],
    );

    ash_device.end_command_buffer(command_buffer)?;

    Ok(())
}
