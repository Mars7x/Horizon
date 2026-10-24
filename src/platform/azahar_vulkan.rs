//! Azahar's libretro Vulkan v5 interface, on the existing emulation worker.
//! The core negotiates its device/features. Native-size images are copied into
//! the existing RGBA presentation boundary; no window surface or GL fallback.
//! ABI definitions follow libretro_vulkan.h (RetroArch team, MIT; see notices).
use ash::{Entry, vk};
use std::{
    ffi::c_void,
    ptr,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Context {
    gpu: vk::PhysicalDevice,
    device: vk::Device,
    queue: vk::Queue,
    family: u32,
    presentation_queue: vk::Queue,
    presentation_family: u32,
}
type CreateDevice = unsafe extern "C" fn(
    *mut Context,
    vk::Instance,
    vk::PhysicalDevice,
    vk::SurfaceKHR,
    vk::PFN_vkGetInstanceProcAddr,
    *const *const i8,
    u32,
    *const *const i8,
    u32,
    *const vk::PhysicalDeviceFeatures,
) -> bool;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Negotiation {
    pub kind: u32,
    pub version: u32,
    app: Option<unsafe extern "C" fn() -> *const vk::ApplicationInfo<'static>>,
    create_device: Option<CreateDevice>,
    destroy_device: Option<unsafe extern "C" fn()>,
    create_instance: *const c_void,
    create_device2: *const c_void,
}
impl Negotiation {
    pub fn supported(&self) -> bool {
        self.kind == 0
            && self.version == 2
            && self.create_instance.is_null()
            && self.create_device2.is_null()
    }
}
#[derive(Default)]
pub struct Backend {
    pub reset: Option<extern "C" fn()>,
    pub destroy: Option<extern "C" fn()>,
    pub negotiation: Option<Negotiation>,
    pub host: Option<Box<Host>>,
}
#[repr(C)]
struct Image {
    view: vk::ImageView,
    layout: vk::ImageLayout,
    info: vk::ImageViewCreateInfo<'static>,
}
#[repr(C)]
struct Interface {
    kind: u32,
    version: u32,
    handle: *mut c_void,
    instance: vk::Instance,
    gpu: vk::PhysicalDevice,
    device: vk::Device,
    get_device_proc_addr: vk::PFN_vkGetDeviceProcAddr,
    get_instance_proc_addr: vk::PFN_vkGetInstanceProcAddr,
    queue: vk::Queue,
    queue_index: u32,
    set_image: unsafe extern "C" fn(*mut c_void, *const Image, u32, *const vk::Semaphore, u32),
    sync_index: extern "C" fn(*mut c_void) -> u32,
    sync_mask: extern "C" fn(*mut c_void) -> u32,
    commands: unsafe extern "C" fn(*mut c_void, u32, *const vk::CommandBuffer),
    wait: extern "C" fn(*mut c_void),
    lock: unsafe extern "C" fn(*mut c_void),
    unlock: unsafe extern "C" fn(*mut c_void),
    signal: unsafe extern "C" fn(*mut c_void, vk::Semaphore),
}
#[derive(Clone, Copy)]
struct FrameImage {
    image: vk::Image,
    layout: vk::ImageLayout,
    format: vk::Format,
    family: u32,
}
#[derive(Default)]
struct Pending {
    image: Option<FrameImage>,
    waits: Vec<vk::Semaphore>,
    commands: Vec<vk::CommandBuffer>,
    signal: vk::Semaphore,
    error: Option<String>,
}
#[derive(Default)]
struct Shared {
    queue_locked: AtomicBool,
    pending: Mutex<Pending>,
}
struct QueueGuard<'a>(&'a AtomicBool);
impl Drop for QueueGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
fn acquire(lock: &AtomicBool) -> QueueGuard<'_> {
    while lock
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        std::thread::yield_now();
    }
    QueueGuard(lock)
}
// SAFETY: handles refer to the boxed Shared retained until after retro_deinit.
unsafe extern "C" fn set_image(
    handle: *mut c_void,
    image: *const Image,
    count: u32,
    waits: *const vk::Semaphore,
    family: u32,
) {
    if handle.is_null() {
        return;
    }
    let shared = unsafe { &*handle.cast::<Shared>() };
    let mut pending = shared
        .pending
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if image.is_null() || count > 16 || (count != 0 && waits.is_null()) {
        pending.error = Some("Azahar supplied an invalid Vulkan image".into());
        return;
    }
    let image = unsafe { &*image };
    if image.info.view_type != vk::ImageViewType::TYPE_2D
        || image.info.subresource_range.base_mip_level != 0
        || image.info.subresource_range.base_array_layer != 0
        || image.info.subresource_range.layer_count != 1
    {
        pending.error = Some("Azahar supplied an unsupported Vulkan image view".into());
        return;
    }
    pending.image = Some(FrameImage {
        image: image.info.image,
        layout: image.layout,
        format: image.info.format,
        family,
    });
    pending.waits.clear();
    if count != 0 {
        pending
            .waits
            .extend_from_slice(unsafe { std::slice::from_raw_parts(waits, count as usize) });
    }
}
extern "C" fn sync_index(_: *mut c_void) -> u32 {
    0
}
extern "C" fn sync_mask(_: *mut c_void) -> u32 {
    1
}
// Readback completes before video_refresh returns, so the one sync index is
// already released when the core asks for it again. No swapchain is involved.
extern "C" fn wait(_: *mut c_void) {}
unsafe extern "C" fn commands(handle: *mut c_void, count: u32, buffers: *const vk::CommandBuffer) {
    if handle.is_null() {
        return;
    }
    // SAFETY: retained handle; arrays are valid for the libretro callback.
    let shared = unsafe { &*handle.cast::<Shared>() };
    let mut pending = shared
        .pending
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if count > 32 || (count != 0 && buffers.is_null()) {
        pending.error = Some("Invalid Vulkan command submission".into());
        return;
    }
    pending.commands.clear();
    if count != 0 {
        pending
            .commands
            .extend_from_slice(unsafe { std::slice::from_raw_parts(buffers, count as usize) });
    }
}
unsafe extern "C" fn lock(handle: *mut c_void) {
    if handle.is_null() {
        return;
    }
    // SAFETY: retained boxed state, queue locking may come from core threads.
    let shared = unsafe { &*handle.cast::<Shared>() };
    std::mem::forget(acquire(&shared.queue_locked));
}
unsafe extern "C" fn unlock(handle: *mut c_void) {
    if !handle.is_null() {
        // SAFETY: same handle and queue lock acquired by the calling core.
        unsafe { &*handle.cast::<Shared>() }
            .queue_locked
            .store(false, Ordering::Release);
    }
}
unsafe extern "C" fn signal(handle: *mut c_void, semaphore: vk::Semaphore) {
    if !handle.is_null() {
        // SAFETY: retained boxed state for this core's device lifetime.
        unsafe { &*handle.cast::<Shared>() }
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .signal = semaphore;
    }
}

pub struct Host {
    entry: Entry,
    instance: Option<ash::Instance>,
    device: Option<ash::Device>,
    interface: Option<Box<Interface>>,
    shared: Box<Shared>,
    destroy_device: Option<unsafe extern "C" fn()>,
    gpu: vk::PhysicalDevice,
    queue: vk::Queue,
    family: u32,
    pool: vk::CommandPool,
    command: vk::CommandBuffer,
    fence: vk::Fence,
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    capacity: u64,
    coherent: bool,
}
fn error(operation: &str, value: vk::Result) -> String {
    format!("Vulkan {operation} failed ({value:?})")
}
impl Host {
    pub fn new(negotiation: Negotiation) -> Result<Box<Self>, String> {
        if negotiation.kind != 0
            || negotiation.version != 2
            || !negotiation.create_instance.is_null()
            || !negotiation.create_device2.is_null()
        {
            return Err("Unsupported Azahar Vulkan negotiation interface".into());
        }
        // SAFETY: loads the system Vulkan loader; Entry retains its library.
        let entry = unsafe { Entry::load() }.map_err(|_| "The Vulkan driver is unavailable")?;
        let mut host = Box::new(Self {
            entry,
            instance: None,
            device: None,
            interface: None,
            shared: Box::default(),
            destroy_device: None,
            gpu: vk::PhysicalDevice::null(),
            queue: vk::Queue::null(),
            family: 0,
            pool: vk::CommandPool::null(),
            command: vk::CommandBuffer::null(),
            fence: vk::Fence::null(),
            buffer: vk::Buffer::null(),
            memory: vk::DeviceMemory::null(),
            capacity: 0,
            coherent: false,
        });
        host.initialize(negotiation)?;
        Ok(host)
    }
    fn initialize(&mut self, negotiation: Negotiation) -> Result<(), String> {
        let application = negotiation
            .app
            .ok_or("Azahar supplied no Vulkan application information")?;
        // SAFETY: callbacks belong to the retained, pinned libretro core.
        let application =
            unsafe { application().as_ref() }.ok_or("Invalid Vulkan application information")?;
        let create = negotiation
            .create_device
            .ok_or("Azahar supplied no Vulkan device negotiation")?;
        // SAFETY: loader operations do not outlive Entry, and returned handles
        // are retained by this host, including all initialization error paths.
        unsafe {
            let version = self
                .entry
                .try_enumerate_instance_version()
                .map_err(|e| error("loader query", e))?
                .unwrap_or(vk::API_VERSION_1_0);
            if version < vk::API_VERSION_1_1 {
                return Err("Azahar requires Vulkan 1.1 or newer".into());
            }
            let properties = self
                .entry
                .enumerate_instance_extension_properties(None)
                .map_err(|e| error("extension query", e))?;
            let extensions: Vec<_> = [ash::khr::surface::NAME]
                .into_iter()
                .filter(|name| {
                    properties.iter().any(|extension| {
                        std::ffi::CStr::from_ptr(extension.extension_name.as_ptr()) == *name
                    })
                })
                .map(|name| name.as_ptr())
                .collect();
            let info = vk::InstanceCreateInfo::default()
                .application_info(application)
                .enabled_extension_names(&extensions);
            self.instance = Some(
                self.entry
                    .create_instance(&info, None)
                    .map_err(|e| error("instance creation", e))?,
            );
            let instance = self
                .instance
                .as_ref()
                .ok_or("Vulkan instance is unavailable")?;
            let mut devices = instance
                .enumerate_physical_devices()
                .map_err(|e| error("GPU query", e))?;
            devices.sort_by_key(|&gpu| {
                match instance.get_physical_device_properties(gpu).device_type {
                    vk::PhysicalDeviceType::DISCRETE_GPU => 0,
                    vk::PhysicalDeviceType::INTEGRATED_GPU => 1,
                    _ => 2,
                }
            });
            let gpu = devices
                .into_iter()
                .find(|&gpu| {
                    instance
                        .get_physical_device_queue_family_properties(gpu)
                        .iter()
                        .any(|family| {
                            family.queue_count != 0
                                && family
                                    .queue_flags
                                    .contains(vk::QueueFlags::GRAPHICS | vk::QueueFlags::COMPUTE)
                        })
                })
                .ok_or("No suitable Vulkan graphics device is available")?;
            let mut context = Context {
                gpu,
                device: vk::Device::null(),
                queue: vk::Queue::null(),
                family: 0,
                presentation_queue: vk::Queue::null(),
                presentation_family: 0,
            };
            if !create(
                &mut context,
                instance.handle(),
                gpu,
                vk::SurfaceKHR::null(),
                self.entry.static_fn().get_instance_proc_addr,
                ptr::null(),
                0,
                ptr::null(),
                0,
                &vk::PhysicalDeviceFeatures::default(),
            ) {
                return Err("Azahar could not create its Vulkan device".into());
            }
            if context.device == vk::Device::null() {
                return Err("Azahar returned no Vulkan device".into());
            }
            self.destroy_device = negotiation.destroy_device;
            self.device = Some(ash::Device::load(instance.fp_v1_0(), context.device));
            self.gpu = context.gpu;
            self.queue = context.queue;
            self.family = context.family;
            if self.gpu != gpu || self.queue == vk::Queue::null() {
                return Err("Invalid negotiated Vulkan device".into());
            }
            let device = self.device.as_ref().ok_or("Vulkan device is unavailable")?;
            self.pool = device
                .create_command_pool(
                    &vk::CommandPoolCreateInfo::default()
                        .queue_family_index(self.family)
                        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                    None,
                )
                .map_err(|e| error("command pool", e))?;
            self.command = device
                .allocate_command_buffers(
                    &vk::CommandBufferAllocateInfo::default()
                        .command_pool(self.pool)
                        .level(vk::CommandBufferLevel::PRIMARY)
                        .command_buffer_count(1),
                )
                .map_err(|e| error("command buffer", e))?
                .into_iter()
                .next()
                .ok_or("No Vulkan command buffer was allocated")?;
            self.fence = device
                .create_fence(
                    &vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED),
                    None,
                )
                .map_err(|e| error("fence", e))?;
            self.interface = Some(Box::new(Interface {
                kind: 0,
                version: 5,
                handle: (&mut *self.shared as *mut Shared).cast(),
                instance: instance.handle(),
                gpu,
                device: device.handle(),
                get_device_proc_addr: instance.fp_v1_0().get_device_proc_addr,
                get_instance_proc_addr: self.entry.static_fn().get_instance_proc_addr,
                queue: self.queue,
                queue_index: self.family,
                set_image,
                sync_index,
                sync_mask,
                commands,
                wait,
                lock,
                unlock,
                signal,
            }));
        }
        tracing::info!("Azahar Vulkan device initialized");
        Ok(())
    }
    pub fn interface(&self) -> *const c_void {
        self.interface.as_ref().map_or(ptr::null(), |interface| {
            (&**interface as *const Interface).cast()
        })
    }
    fn prepare_buffer(&mut self, bytes: u64) -> Result<(), String> {
        if self.capacity >= bytes {
            return Ok(());
        }
        let device = self.device.as_ref().ok_or("Vulkan device is unavailable")?;
        let instance = self
            .instance
            .as_ref()
            .ok_or("Vulkan instance is unavailable")?;
        // SAFETY: prior frame copies are fenced before returning; the worker
        // exclusively owns the staging buffer, allocation and command pool.
        unsafe {
            device.destroy_buffer(self.buffer, None);
            device.free_memory(self.memory, None);
            self.buffer = vk::Buffer::null();
            self.memory = vk::DeviceMemory::null();
            self.capacity = 0;
            self.buffer = device
                .create_buffer(
                    &vk::BufferCreateInfo::default()
                        .size(bytes)
                        .usage(vk::BufferUsageFlags::TRANSFER_DST)
                        .sharing_mode(vk::SharingMode::EXCLUSIVE),
                    None,
                )
                .map_err(|e| error("readback buffer", e))?;
            let required = device.get_buffer_memory_requirements(self.buffer);
            let properties = instance.get_physical_device_memory_properties(self.gpu);
            let mut types: Vec<_> = (0..properties.memory_type_count)
                .filter(|&index| {
                    required.memory_type_bits & (1 << index) != 0
                        && properties.memory_types[index as usize]
                            .property_flags
                            .contains(vk::MemoryPropertyFlags::HOST_VISIBLE)
                })
                .collect();
            types.sort_by_key(|&index| {
                !properties.memory_types[index as usize]
                    .property_flags
                    .contains(vk::MemoryPropertyFlags::HOST_COHERENT)
            });
            let index = types
                .into_iter()
                .next()
                .ok_or("No host-visible Vulkan readback memory is available")?;
            self.coherent = properties.memory_types[index as usize]
                .property_flags
                .contains(vk::MemoryPropertyFlags::HOST_COHERENT);
            self.memory = device
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(required.size)
                        .memory_type_index(index),
                    None,
                )
                .map_err(|e| error("readback allocation", e))?;
            device
                .bind_buffer_memory(self.buffer, self.memory, 0)
                .map_err(|e| error("readback binding", e))?;
            self.capacity = bytes;
        }
        Ok(())
    }
    pub fn read_frame(
        &mut self,
        width: u32,
        height: u32,
        rgba: &mut Vec<u8>,
    ) -> Result<(), String> {
        if !(1..=4096).contains(&width) || !(1..=4096).contains(&height) {
            return Err("Invalid Vulkan frame dimensions".into());
        }
        let (image, waits, mut commands, signal) = {
            let mut pending = self
                .shared
                .pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if let Some(error) = pending.error.take() {
                return Err(error);
            }
            let image = pending
                .image
                .ok_or("Azahar did not supply a Vulkan image")?;
            (
                image,
                std::mem::take(&mut pending.waits),
                std::mem::take(&mut pending.commands),
                std::mem::take(&mut pending.signal),
            )
        };
        if image.family != vk::QUEUE_FAMILY_IGNORED && image.family != self.family {
            return Err("Unsupported Azahar Vulkan queue ownership".into());
        }
        if !matches!(
            image.format,
            vk::Format::R8G8B8A8_UNORM
                | vk::Format::R8G8B8A8_SRGB
                | vk::Format::B8G8R8A8_UNORM
                | vk::Format::B8G8R8A8_SRGB
        ) || !matches!(
            image.layout,
            vk::ImageLayout::GENERAL | vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
        ) {
            return Err("Unsupported Azahar Vulkan pixel format or layout".into());
        }
        let bytes = u64::from(width) * u64::from(height) * 4;
        self.prepare_buffer(bytes)?;
        let device = self.device.as_ref().ok_or("Vulkan device is unavailable")?;
        let layout = if image.layout == vk::ImageLayout::GENERAL {
            image.layout
        } else {
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL
        };
        let range = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .level_count(1)
            .layer_count(1);
        // SAFETY: image was submitted by the pinned core on this device;
        // queue ordering/barriers protect it, and copy size is bounded above.
        unsafe {
            device
                .reset_command_buffer(self.command, vk::CommandBufferResetFlags::empty())
                .map_err(|e| error("command reset", e))?;
            device
                .begin_command_buffer(
                    self.command,
                    &vk::CommandBufferBeginInfo::default()
                        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
                )
                .map_err(|e| error("command begin", e))?;
            let before = vk::ImageMemoryBarrier::default()
                .image(image.image)
                .subresource_range(range)
                .old_layout(image.layout)
                .new_layout(layout)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .src_access_mask(vk::AccessFlags::MEMORY_WRITE | vk::AccessFlags::SHADER_READ)
                .dst_access_mask(vk::AccessFlags::TRANSFER_READ);
            device.cmd_pipeline_barrier(
                self.command,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[before],
            );
            let region = vk::BufferImageCopy::default()
                .image_subresource(
                    vk::ImageSubresourceLayers::default()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .layer_count(1),
                )
                .image_extent(vk::Extent3D {
                    width,
                    height,
                    depth: 1,
                });
            device.cmd_copy_image_to_buffer(
                self.command,
                image.image,
                layout,
                self.buffer,
                &[region],
            );
            let after = vk::ImageMemoryBarrier::default()
                .image(image.image)
                .subresource_range(range)
                .old_layout(layout)
                .new_layout(image.layout)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .src_access_mask(vk::AccessFlags::TRANSFER_READ)
                .dst_access_mask(vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE);
            let host = vk::BufferMemoryBarrier::default()
                .buffer(self.buffer)
                .size(vk::WHOLE_SIZE)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::HOST_READ);
            device.cmd_pipeline_barrier(
                self.command,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::ALL_COMMANDS | vk::PipelineStageFlags::HOST,
                vk::DependencyFlags::empty(),
                &[],
                &[host],
                &[after],
            );
            device
                .end_command_buffer(self.command)
                .map_err(|e| error("command end", e))?;
            let waits = if commands.is_empty() {
                waits
            } else {
                Vec::new()
            };
            commands.push(self.command);
            let stages = vec![vk::PipelineStageFlags::ALL_COMMANDS; waits.len()];
            let signals: Vec<_> = (signal != vk::Semaphore::null())
                .then_some(signal)
                .into_iter()
                .collect();
            let submit = vk::SubmitInfo::default()
                .command_buffers(&commands)
                .wait_semaphores(&waits)
                .wait_dst_stage_mask(&stages)
                .signal_semaphores(&signals);
            device
                .reset_fences(&[self.fence])
                .map_err(|e| error("fence reset", e))?;
            {
                let _queue = acquire(&self.shared.queue_locked);
                device
                    .queue_submit(self.queue, &[submit], self.fence)
                    .map_err(|e| error("frame submission", e))?;
            }
            device
                .wait_for_fences(&[self.fence], true, 1_000_000_000)
                .map_err(|e| error("frame completion", e))?;
            let mapped = device
                .map_memory(self.memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())
                .map_err(|e| error("readback mapping", e))?;
            if !self.coherent
                && let Err(value) =
                    device.invalidate_mapped_memory_ranges(&[vk::MappedMemoryRange::default()
                        .memory(self.memory)
                        .size(vk::WHOLE_SIZE)])
            {
                device.unmap_memory(self.memory);
                return Err(error("readback visibility", value));
            }
            rgba.resize(bytes as usize, 0);
            rgba.copy_from_slice(std::slice::from_raw_parts(
                mapped.cast::<u8>(),
                bytes as usize,
            ));
            device.unmap_memory(self.memory);
        }
        for pixel in rgba.as_chunks_mut::<4>().0 {
            if matches!(
                image.format,
                vk::Format::B8G8R8A8_UNORM | vk::Format::B8G8R8A8_SRGB
            ) {
                pixel.swap(0, 2);
            }
            pixel[3] = 255;
        }
        Ok(())
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        // SAFETY: the core has released its renderer/threads before the host;
        // handles are created on this device and null handles may be destroyed.
        unsafe {
            if let Some(device) = &self.device {
                if let Err(value) = device.device_wait_idle() {
                    tracing::warn!(?value, "Vulkan shutdown wait failed");
                }
                device.destroy_buffer(self.buffer, None);
                device.free_memory(self.memory, None);
                device.destroy_fence(self.fence, None);
                device.destroy_command_pool(self.pool, None);
                if let Some(destroy) = self.destroy_device {
                    destroy();
                }
                device.destroy_device(None);
            }
            if let Some(instance) = &self.instance {
                instance.destroy_instance(None);
            }
        }
    }
}
