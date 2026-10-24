//! Azahar's libretro Vulkan v5 interface, on the existing emulation worker.
//! The core negotiates its device/features. Each frame is accepted when the
//! core presents it and read back only when needed: shown frames are first
//! shrunk on the GPU to the size each screen is shown at, and screenshots read
//! the full image. No window surface or GL fallback.
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
    /// The latest frame hasn't been read at full size yet.
    pub unread: bool,
    /// The shown frame, shrunk on the GPU, reused between frames.
    pub presentation: super::gba_core::Frame,
    /// Shrinking failed once: read full frames for the rest of the session.
    pub shrink_failed: bool,
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
    /// The core's latest presented frame and its size.
    current: Option<(FrameImage, u32, u32)>,
    /// GPU images the shown frame is shrunk through: halvings of the whole
    /// frame (mip levels), then the two screens at their shown sizes.
    halves: Scratch,
    shown: Scratch,
}
/// A reusable device-local image, recreated only when it must grow.
#[derive(Default)]
struct Scratch {
    image: vk::Image,
    memory: vk::DeviceMemory,
    format: vk::Format,
    extent: (u32, u32),
    levels: u32,
}
/// A rectangle of pixels: x, y, width, height.
pub type Region = (u32, u32, u32, u32);
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
            current: None,
            halves: Scratch::default(),
            shown: Scratch::default(),
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
            // The CPU reads every frame back: memory the CPU caches is far
            // faster to read than uncached (often device-local, read over
            // PCIe) memory. Prefer cached, then not device-local, then
            // coherent (which saves an invalidate).
            types.sort_by_key(|&index| {
                let flags = properties.memory_types[index as usize].property_flags;
                (
                    !flags.contains(vk::MemoryPropertyFlags::HOST_CACHED),
                    flags.contains(vk::MemoryPropertyFlags::DEVICE_LOCAL),
                    !flags.contains(vk::MemoryPropertyFlags::HOST_COHERENT),
                )
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
    /// The core presented a frame of `width`×`height`: remember its image,
    /// and submit any work it asked the frontend to submit. Nothing is read.
    pub fn accept_frame(&mut self, width: u32, height: u32) -> Result<(), String> {
        if !(1..=4096).contains(&width) || !(1..=4096).contains(&height) {
            return Err("Invalid Vulkan frame dimensions".into());
        }
        let (image, waits, commands, signal) = {
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
        if !commands.is_empty() || !waits.is_empty() || signal != vk::Semaphore::null() {
            let device = self.device.as_ref().ok_or("Vulkan device is unavailable")?;
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
            let _queue = acquire(&self.shared.queue_locked);
            // SAFETY: the core's own command buffers and semaphores, for its
            // device, submitted once on the queue it shares with us.
            unsafe { device.queue_submit(self.queue, &[submit], vk::Fence::null()) }
                .map_err(|e| error("core submission", e))?;
        }
        self.current = Some((image, width, height));
        Ok(())
    }

    /// The latest frame's size.
    pub fn current_size(&self) -> Option<(u32, u32)> {
        self.current.map(|(_, width, height)| (width, height))
    }

    /// The latest frame at full size, as opaque RGBA.
    pub fn read_frame(&mut self, rgba: &mut Vec<u8>) -> Result<(u32, u32), String> {
        let (image, width, height) = self.current.ok_or("No Azahar frame is available")?;
        self.read(image, None, (width, height), rgba)?;
        Ok((width, height))
    }

    /// Two `regions` of the latest frame, each shrunk on the GPU to its
    /// `target` size, the first above the second at the left (a buffer of
    /// the wider width), as opaque RGBA. Targets must not enlarge.
    pub fn read_shrunk(
        &mut self,
        regions: [Region; 2],
        targets: [(u32, u32); 2],
        rgba: &mut Vec<u8>,
    ) -> Result<(u32, u32), String> {
        let (image, width, height) = self.current.ok_or("No Azahar frame is available")?;
        if regions
            .iter()
            .zip(&targets)
            .any(|(&(x, y, w, h), &(tw, th))| {
                tw == 0 || th == 0 || tw > w || th > h || x + w > width || y + h > height
            })
        {
            return Err("Invalid Vulkan shrink request".into());
        }
        let size = (targets[0].0.max(targets[1].0), targets[0].1 + targets[1].1);
        self.read(image, Some((regions, targets, (width, height))), size, rgba)?;
        Ok(size)
    }

    /// Make the halving (`halves`) or shown-size scratch image at least
    /// `extent` with `levels` mip levels, recreating it only to grow.
    fn grow_image(
        &mut self,
        halves: bool,
        format: vk::Format,
        extent: (u32, u32),
        levels: u32,
    ) -> Result<(), String> {
        let scratch = if halves { &self.halves } else { &self.shown };
        if scratch.image != vk::Image::null()
            && scratch.format == format
            && scratch.extent.0 >= extent.0
            && scratch.extent.1 >= extent.1
            && scratch.levels >= levels
        {
            return Ok(());
        }
        let device = self.device.as_ref().ok_or("Vulkan device is unavailable")?;
        let instance = self
            .instance
            .as_ref()
            .ok_or("Vulkan instance is unavailable")?;
        // SAFETY: earlier reads are fenced before returning, so the old image
        // is idle; the new one is created and bound on this device.
        unsafe {
            let features = instance
                .get_physical_device_format_properties(self.gpu, format)
                .optimal_tiling_features;
            if !features.contains(
                vk::FormatFeatureFlags::BLIT_SRC
                    | vk::FormatFeatureFlags::BLIT_DST
                    | vk::FormatFeatureFlags::SAMPLED_IMAGE_FILTER_LINEAR,
            ) {
                return Err("The GPU cannot shrink Azahar's frames".into());
            }
            let scratch = if halves {
                &mut self.halves
            } else {
                &mut self.shown
            };
            device.destroy_image(scratch.image, None);
            device.free_memory(scratch.memory, None);
            *scratch = Scratch::default();
            let image = device
                .create_image(
                    &vk::ImageCreateInfo::default()
                        .image_type(vk::ImageType::TYPE_2D)
                        .format(format)
                        .extent(vk::Extent3D {
                            width: extent.0,
                            height: extent.1,
                            depth: 1,
                        })
                        .mip_levels(levels)
                        .array_layers(1)
                        .samples(vk::SampleCountFlags::TYPE_1)
                        .tiling(vk::ImageTiling::OPTIMAL)
                        .usage(
                            vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::TRANSFER_DST,
                        )
                        .sharing_mode(vk::SharingMode::EXCLUSIVE)
                        .initial_layout(vk::ImageLayout::UNDEFINED),
                    None,
                )
                .map_err(|e| error("shrink image", e))?;
            let required = device.get_image_memory_requirements(image);
            let properties = instance.get_physical_device_memory_properties(self.gpu);
            let Some(index) = (0..properties.memory_type_count)
                .filter(|&index| required.memory_type_bits & (1 << index) != 0)
                .min_by_key(|&index| {
                    !properties.memory_types[index as usize]
                        .property_flags
                        .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
                })
            else {
                device.destroy_image(image, None);
                return Err("No Vulkan memory for shrinking frames".into());
            };
            let memory = match device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(required.size)
                    .memory_type_index(index),
                None,
            ) {
                Ok(memory) => memory,
                Err(value) => {
                    device.destroy_image(image, None);
                    return Err(error("shrink allocation", value));
                }
            };
            if let Err(value) = device.bind_image_memory(image, memory, 0) {
                device.destroy_image(image, None);
                device.free_memory(memory, None);
                return Err(error("shrink binding", value));
            }
            *scratch = Scratch {
                image,
                memory,
                format,
                extent,
                levels,
            };
        }
        Ok(())
    }

    /// Copy `image` (or, with `shrink`, its two regions shrunk to their
    /// targets) into the readback buffer and then into `rgba`, waiting for
    /// the GPU.
    #[allow(clippy::type_complexity)]
    fn read(
        &mut self,
        image: FrameImage,
        shrink: Option<([Region; 2], [(u32, u32); 2], (u32, u32))>,
        (width, height): (u32, u32),
        rgba: &mut Vec<u8>,
    ) -> Result<(), String> {
        // How many halvings each screen needs before one linear blit (which
        // reads 2×2 pixels) reaches its target without skipping any.
        let halvings = |(_, _, w, h): Region, (tw, th): (u32, u32)| {
            let mut level = 0;
            while (w >> level) > 2 * tw || (h >> level) > 2 * th {
                level += 1;
            }
            level
        };
        let plan = shrink.map(|(regions, targets, source)| {
            let levels = [0, 1].map(|i| halvings(regions[i], targets[i]));
            (regions, targets, source, levels)
        });
        if let Some((_, _, source, levels)) = plan {
            let deepest = levels[0].max(levels[1]);
            if deepest > 0 {
                self.grow_image(
                    true,
                    image.format,
                    ((source.0 >> 1).max(1), (source.1 >> 1).max(1)),
                    deepest,
                )?;
            }
            self.grow_image(false, image.format, (width, height), 1)?;
        }
        let bytes = u64::from(width) * u64::from(height) * 4;
        self.prepare_buffer(bytes)?;
        let device = self.device.as_ref().ok_or("Vulkan device is unavailable")?;
        let layout = if image.layout == vk::ImageLayout::GENERAL {
            image.layout
        } else {
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL
        };
        let range = |level: u32, count: u32| {
            vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .base_mip_level(level)
                .level_count(count)
                .layer_count(1)
        };
        let layers = |level: u32| {
            vk::ImageSubresourceLayers::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .mip_level(level)
                .layer_count(1)
        };
        let barrier = |image: vk::Image,
                       range: vk::ImageSubresourceRange,
                       (old, new): (vk::ImageLayout, vk::ImageLayout),
                       (from, to): (vk::AccessFlags, vk::AccessFlags)| {
            vk::ImageMemoryBarrier::default()
                .image(image)
                .subresource_range(range)
                .old_layout(old)
                .new_layout(new)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .src_access_mask(from)
                .dst_access_mask(to)
        };
        let corner = |x: u32, y: u32| vk::Offset3D {
            x: x as i32,
            y: y as i32,
            z: 1,
        };
        let origin = |x: u32, y: u32| vk::Offset3D {
            x: x as i32,
            y: y as i32,
            z: 0,
        };
        let (halves, shown) = (self.halves.image, self.shown.image);
        // SAFETY: images were created on this device (the core's by the
        // pinned core); barriers order them against the core's work on the
        // same queue, and every size is bounded by the frame's.
        unsafe {
            let command = self.command;
            let record =
                |what: &str, value: Result<(), vk::Result>| value.map_err(|e| error(what, e));
            record(
                "command reset",
                device.reset_command_buffer(command, vk::CommandBufferResetFlags::empty()),
            )?;
            record(
                "command begin",
                device.begin_command_buffer(
                    command,
                    &vk::CommandBufferBeginInfo::default()
                        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
                ),
            )?;
            let transfer = vk::PipelineStageFlags::TRANSFER;
            device.cmd_pipeline_barrier(
                command,
                vk::PipelineStageFlags::ALL_COMMANDS,
                transfer,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier(
                    image.image,
                    range(0, 1),
                    (image.layout, layout),
                    (
                        vk::AccessFlags::MEMORY_WRITE | vk::AccessFlags::SHADER_READ,
                        vk::AccessFlags::TRANSFER_READ,
                    ),
                )],
            );
            // What the readback buffer is copied from.
            let mut copy_from = (image.image, layout, 0);
            if let Some((regions, targets, source, levels)) = plan {
                let deepest = levels[0].max(levels[1]);
                let write = (vk::AccessFlags::empty(), vk::AccessFlags::TRANSFER_WRITE);
                let to_read = (
                    vk::AccessFlags::TRANSFER_WRITE,
                    vk::AccessFlags::TRANSFER_READ,
                );
                let mut starts = vec![barrier(
                    shown,
                    range(0, 1),
                    (
                        vk::ImageLayout::UNDEFINED,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    ),
                    write,
                )];
                if deepest > 0 {
                    starts.push(barrier(
                        halves,
                        range(0, deepest),
                        (
                            vk::ImageLayout::UNDEFINED,
                            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        ),
                        write,
                    ));
                }
                device.cmd_pipeline_barrier(
                    command,
                    transfer,
                    transfer,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &starts,
                );
                // Halve the whole frame, level by level: each halving
                // averages 2×2 pixels exactly.
                for level in 1..=deepest {
                    let (from, from_layout, from_level) = if level == 1 {
                        (image.image, layout, 0)
                    } else {
                        (halves, vk::ImageLayout::TRANSFER_SRC_OPTIMAL, level - 2)
                    };
                    let from_size = (
                        (source.0 >> (level - 1)).max(1),
                        (source.1 >> (level - 1)).max(1),
                    );
                    let to_size = ((source.0 >> level).max(1), (source.1 >> level).max(1));
                    let blit = vk::ImageBlit::default()
                        .src_subresource(layers(from_level))
                        .src_offsets([origin(0, 0), corner(from_size.0, from_size.1)])
                        .dst_subresource(layers(level - 1))
                        .dst_offsets([origin(0, 0), corner(to_size.0, to_size.1)]);
                    device.cmd_blit_image(
                        command,
                        from,
                        from_layout,
                        halves,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &[blit],
                        vk::Filter::LINEAR,
                    );
                    device.cmd_pipeline_barrier(
                        command,
                        transfer,
                        transfer,
                        vk::DependencyFlags::empty(),
                        &[],
                        &[],
                        &[barrier(
                            halves,
                            range(level - 1, 1),
                            (
                                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                            ),
                            to_read,
                        )],
                    );
                }
                // Each screen from the level that is at most twice its size.
                let mut below = 0;
                for screen in 0..2 {
                    let (x, y, w, h) = regions[screen];
                    let level = levels[screen];
                    let (from, from_layout, from_level) = if level == 0 {
                        (image.image, layout, 0)
                    } else {
                        (halves, vk::ImageLayout::TRANSFER_SRC_OPTIMAL, level - 1)
                    };
                    let (tw, th) = targets[screen];
                    let blit = vk::ImageBlit::default()
                        .src_subresource(layers(from_level))
                        .src_offsets([
                            origin(x >> level, y >> level),
                            corner((x + w) >> level, (y + h) >> level),
                        ])
                        .dst_subresource(layers(0))
                        .dst_offsets([origin(0, below), corner(tw, below + th)]);
                    device.cmd_blit_image(
                        command,
                        from,
                        from_layout,
                        shown,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &[blit],
                        vk::Filter::LINEAR,
                    );
                    below += th;
                }
                device.cmd_pipeline_barrier(
                    command,
                    transfer,
                    transfer,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[barrier(
                        shown,
                        range(0, 1),
                        (
                            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                        ),
                        to_read,
                    )],
                );
                copy_from = (shown, vk::ImageLayout::TRANSFER_SRC_OPTIMAL, 0);
            }
            let region = vk::BufferImageCopy::default()
                .image_subresource(layers(copy_from.2))
                .image_extent(vk::Extent3D {
                    width,
                    height,
                    depth: 1,
                });
            device.cmd_copy_image_to_buffer(
                command,
                copy_from.0,
                copy_from.1,
                self.buffer,
                &[region],
            );
            let after = barrier(
                image.image,
                range(0, 1),
                (layout, image.layout),
                (
                    vk::AccessFlags::TRANSFER_READ,
                    vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
                ),
            );
            let host = vk::BufferMemoryBarrier::default()
                .buffer(self.buffer)
                .size(vk::WHOLE_SIZE)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::HOST_READ);
            device.cmd_pipeline_barrier(
                command,
                transfer,
                vk::PipelineStageFlags::ALL_COMMANDS | vk::PipelineStageFlags::HOST,
                vk::DependencyFlags::empty(),
                &[],
                &[host],
                &[after],
            );
            record("command end", device.end_command_buffer(command))?;
            let commands = [command];
            let submit = vk::SubmitInfo::default().command_buffers(&commands);
            record("fence reset", device.reset_fences(&[self.fence]))?;
            {
                let _queue = acquire(&self.shared.queue_locked);
                record(
                    "frame submission",
                    device.queue_submit(self.queue, &[submit], self.fence),
                )?;
            }
            record(
                "frame completion",
                device.wait_for_fences(&[self.fence], true, 1_000_000_000),
            )?;
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
            // One pass from the mapping: opaque RGBA, swizzled if needed.
            let source = std::slice::from_raw_parts(mapped.cast::<u8>(), bytes as usize);
            let bgra = matches!(
                image.format,
                vk::Format::B8G8R8A8_UNORM | vk::Format::B8G8R8A8_SRGB
            );
            let pixels = rgba.as_chunks_mut::<4>().0.iter_mut();
            if bgra {
                for (pixel, from) in pixels.zip(source.as_chunks::<4>().0) {
                    *pixel = [from[2], from[1], from[0], 255];
                }
            } else {
                for (pixel, from) in pixels.zip(source.as_chunks::<4>().0) {
                    *pixel = [from[0], from[1], from[2], 255];
                }
            }
            device.unmap_memory(self.memory);
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
                for scratch in [&self.halves, &self.shown] {
                    device.destroy_image(scratch.image, None);
                    device.free_memory(scratch.memory, None);
                }
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
