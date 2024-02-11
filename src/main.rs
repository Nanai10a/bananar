#![feature(box_into_inner)]
#![feature(decl_macro)]
#![feature(error_in_core)]
#![feature(exclusive_wrapper)]
#![feature(iterator_try_collect)]
#![feature(never_type)]
#![feature(trait_alias)]
#![feature(slice_from_ptr_range)]
#![feature(slice_as_chunks)]
#![feature(stmt_expr_attributes)]

extern crate alloc;

use core::error::Error;
use wayland_client::Connection;

type Result<T = (), E = Box<dyn Error + Sync + Send>> = core::result::Result<T, E>;

// --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

slint::slint! {
    export component Root {
        Text {
            text: "Hello World!";
            font-size: 24px;
            horizontal-alignment: center;
        }
    }
}

// --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

mod mem;
mod out;
mod reg;
mod ui;

fn main() -> Result {
    let connection = Connection::connect_to_env()?;
    let display = connection.display();

    let mut req = connection.new_event_queue();
    let mut seq = connection.new_event_queue();

    let sqh = seq.handle();

    display.get_registry(&req.handle(), sqh.clone());

    let mut reg = reg::Registry::new();
    req.roundtrip(&mut reg).unwrap();

    let compositor = reg.pull_one::<WlCompositor>().unwrap();
    let output = reg.pull_one::<WlOutput>().unwrap();
    let shm = reg.pull_one::<WlShm>().unwrap();
    let layer_shell = reg.pull_one::<ZwlrLayerShellV1>().unwrap();

    let size = mem::Size::new(500, 500, 4);
    let mut mem = mem::Mem::new(shm, size, &sqh);

    let surface = {
        let (tx, rx) = std::sync::mpsc::channel();
        let p = compositor.create_surface(&sqh, tx);

        reg::WithRx::new(p, rx)
    };

    let layer_surface = {
        use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer;

        let (tx, rx) = std::sync::mpsc::channel();
        let layer = Layer::Bottom;
        let ns = "bananar".to_owned();
        let p = layer_shell.get_layer_surface(&*surface, Some(&*output), layer, ns, &sqh, tx);

        reg::WithRx::new(p, rx)
    };

    let mut out = out::Out::new(layer_surface, surface, output);

    std::thread::Builder::new()
        .name("wayland-worker".to_owned())
        .spawn(move || loop {
            req.flush().unwrap();
            req.blocking_dispatch(&mut reg).unwrap();
            req.dispatch_pending(&mut reg).unwrap();

            seq.flush().unwrap();
            seq.blocking_dispatch(&mut reg::Stub).unwrap();
            seq.dispatch_pending(&mut reg::Stub).unwrap();
            println!("rotate!");
        })?;

    let () = {
        let width = size.width as i32;
        let height = size.height as i32;
        let stride = (size.width * size.pixel_size) as i32;
        let format = wayland_client::protocol::wl_shm::Format::Argb8888;

        out.attach(mem.make_buffer(width, height, stride, format, &sqh));
        out.configure();
        out.wait_ack();
        out.commit();
        out.resize(size.width, size.height);
        out.redraw();
        out.commit();
    };

    let ui = ui::Ui::new();

    let shared = mem.share_allocation();
    ui.inner.write().unwrap().feed_shared(shared);

    slint::platform::set_platform(Box::new(ui)).map_err(PlatformError::SetPlatformError)?;
    Root::new()?.run()?;

    Ok(())

    // let mut queue = connection.new_event_queue();
    // let mut state = InitialGateState::default();

    // let _ = display.get_registry(&queue.handle(), ());

    // queue.roundtrip(&mut state)?;
    // queue.roundtrip(&mut state)?;

    // let mut queue = connection.new_event_queue();
    // let mut state = state.forward(&queue.handle())?;

    // queue.roundtrip(&mut state)?;

    // let state = state.forward(&connection)?;

    // let adapter = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);

    // slint::platform::set_platform(Box::new(MyPlatform::new(adapter.clone())?))
    //     .map_err(PlatformError::SetPlatformError)?;

    // let mut state = state.forward(adapter.clone())?;
    // state.instances[0].set_height(64)?;

    // loop {
    //     state.tick()?;
    // }
}

// --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

struct MyPlatform {
    adapter: Rc<dyn WindowAdapter>,
}

impl MyPlatform {
    fn new(adapter: Rc<dyn WindowAdapter>) -> Result<Self> {
        Ok(Self { adapter })
    }
}

use alloc::rc::Rc;
use slint::platform::Platform;
use slint::platform::PlatformError;
use slint::platform::WindowAdapter;

impl Platform for MyPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.adapter.clone())
    }
}

// --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

// use slint::platform::software_renderer::SoftwareRenderer;

// struct MyWindowAdapter {
//     window: SlintWindow,
//     renderer: SoftwareRenderer,
// }
//
// impl MyWindowAdapter {
//     fn new() -> Result<Rc<Self>> {
//         let renderer = SoftwareRenderer::new();
//
//         Ok(Rc::<Self>::new_cyclic(|weak| {
//             let window = SlintWindow::new(weak.clone());
//
//             Self { window, renderer }
//         }))
//     }
// }
//
// use slint::platform::Renderer;
// use slint::PhysicalSize;
// use slint::Window as SlintWindow;
//
// impl WindowAdapter for MyWindowAdapter {
//     fn window(&self) -> &SlintWindow {
//         &self.window
//     }
//
//     fn size(&self) -> PhysicalSize {
//         // HACK:
//         PhysicalSize {
//             width: 3456,
//             height: 2160 / 32,
//         }
//     }
//
//     fn renderer(&self) -> &dyn Renderer {
//         &self.renderer
//     }
// }

// --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

#[derive(Debug)]
struct MissingError(String);

impl MissingError {
    fn new(content: impl Into<String>) -> Self {
        Self(content.into())
    }
}

use core::fmt::Display;

impl Display for MissingError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Missing {}", self.0)
    }
}

impl Error for MissingError {}

// --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_shm::WlShm;
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::ZwlrLayerShellV1;

#[derive(Debug)]
struct InitialGateState {
    compositor: Option<WlCompositor>,
    shm: Option<WlShm>,
    layer_shell: Option<ZwlrLayerShellV1>,
    outputs: Vec<LazyBind<WlOutput>>,
    is_rgba8888_supported: bool,
}

impl Default for InitialGateState {
    fn default() -> Self {
        Self {
            compositor: None,
            shm: None,
            layer_shell: None,
            outputs: Vec::new(),
            is_rgba8888_supported: false,
        }
    }
}

impl InitialGateState {
    fn forward(self, handle: &QueueHandle<PrepareGateState>) -> Result<PrepareGateState> {
        let compositor = self
            .compositor
            .ok_or_else(|| MissingError::new("wl_compositor"))?;

        #[rustfmt::skip]
        let shm = self
            .shm
            .ok_or_else(|| MissingError::new("wl_shm"))?;

        let layer_shell = self
            .layer_shell
            .ok_or_else(|| MissingError::new("zwlr_layer_shell_v1"))?;

        if !self.is_rgba8888_supported {
            return Err(MissingError::new("support of format RGBA8888").into());
        }

        let outputs = self.outputs.iter().map(|lb| lb.bind(handle, ())).collect();

        Ok(PrepareGateState {
            compositor,
            shm,
            layer_shell,
            outputs,
            modes: HashMap::new(),
        })
    }
}

// --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

use core::marker::PhantomData;

#[derive(Debug)]
struct LazyBind<I> {
    registry: WlRegistry,
    name: u32,
    version: u32,
    _phantom: PhantomData<I>,
}

impl<I: Proxy + 'static> LazyBind<I> {
    fn new(registry: WlRegistry, name: u32, version: u32) -> Self {
        Self {
            registry,
            name,
            version,

            _phantom: PhantomData,
        }
    }

    fn bind<U: Send + Sync + 'static, D: Dispatch<I, U> + 'static>(
        &self,
        handle: &QueueHandle<D>,
        data: U,
    ) -> I {
        self.registry.bind(self.name, self.version, handle, data)
    }
}

// --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::Dispatch;
use wayland_client::Proxy;
use wayland_client::QueueHandle;

impl Dispatch<WlRegistry, ()> for InitialGateState {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: <WlRegistry as Proxy>::Event,
        (): &(),
        _: &Connection,
        handle: &QueueHandle<Self>,
    ) {
        type Event = <WlRegistry as Proxy>::Event;

        let Event::Global {
            name,
            version,
            interface,
        } = event
        else {
            unreachable!()
        };

        if <WlCompositor as Proxy>::interface().name == interface {
            let None = state
                .compositor
                .replace(registry.bind(name, version, handle, ()))
            else {
                unreachable!()
            };
        }

        if <WlShm as Proxy>::interface().name == interface {
            let None = state.shm.replace(registry.bind(name, version, handle, ())) else {
                unreachable!()
            };
        }

        if <ZwlrLayerShellV1 as Proxy>::interface().name == interface {
            let None = state
                .layer_shell
                .replace(registry.bind(name, version, handle, ()))
            else {
                unreachable!()
            };
        }

        if <WlOutput as Proxy>::interface().name == interface {
            state
                .outputs
                .push(LazyBind::new(registry.clone(), name, version));
        }
    }
}

// `WlCompositor` has no events
wayland_client::delegate_noop!(InitialGateState: WlCompositor);

impl Dispatch<WlShm, ()> for InitialGateState {
    fn event(
        state: &mut Self,
        _: &WlShm,
        event: <WlShm as Proxy>::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        type Event = <WlShm as Proxy>::Event;

        let Event::Format { format } = event else {
            unreachable!()
        };

        if let Ok(Format::Rgba8888) = format.into_result() {
            state.is_rgba8888_supported = true;
        }
    }
}

// `ZwlrLayerShellV1` has no events
wayland_client::delegate_noop!(InitialGateState: ZwlrLayerShellV1);

// unallow to receive events
wayland_client::delegate_noop!(InitialGateState: WlOutput);

// --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

use wayland_client::protocol::wl_shm_pool::WlShmPool;

#[derive(Debug)]
struct RenderingRegion {
    fd: OwnedFd,
    shm: WlShm,
    shm_pool: WlShmPool,
    buffer: WlBuffer,
    region: Region,
}

use std::os::fd::AsFd;
use wayland_client::protocol::wl_shm::Format;

impl RenderingRegion {
    // RGBA8888 : 4 [B]
    const PIXEL_SIZE: usize = 4;

    fn make(connection: &Connection, handle: &QueueHandle<Window>, shm: &WlShm) -> Result<Self> {
        const INITIAL_SIZE: usize = 1024;
        let fd = allocate_shm(INITIAL_SIZE)?;

        let shm_pool = {
            // correctness: `wl_shm_pool` has no events
            wayland_client::delegate_noop!(Stub: WlShmPool);
            struct Stub;

            let queue = connection.new_event_queue::<Stub>();
            shm.create_pool(fd.as_fd(), INITIAL_SIZE as i32, &queue.handle(), ())
        };

        let buffer = shm_pool.create_buffer(0, 0, 0, 0, Format::Rgba8888, handle, ());
        let region = Region::new(&fd, 0)?;

        Ok(Self {
            fd,
            shm: shm.clone(),
            shm_pool,
            buffer,
            region,
        })
    }

    fn resize(&mut self, mode: Mode, handle: &QueueHandle<Window>) -> Result {
        self.buffer.destroy();

        let size = mode.width * mode.height * Self::PIXEL_SIZE;

        self.region.remap(&self.fd, size)?;
        self.shm_pool.resize(size as i32);

        self.buffer = {
            let offset = 0;
            let width = mode.width as i32;
            let height = mode.height as i32;
            let stride = width * Self::PIXEL_SIZE as i32;
            let format = Format::Rgba8888;

            self.shm_pool
                .create_buffer(offset, width, height, stride, format, handle, ())
        };

        Ok(())
    }
}

// --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

#[derive(Debug)]
struct Region {
    ptr: *mut u8,
    size: usize,
}

use core::ffi::c_void;
use slint::platform::software_renderer::PremultipliedRgbaColor;
use slint::platform::software_renderer::TargetPixel;

impl Region {
    fn new(fd: impl AsFd, size: usize) -> Result<Self> {
        let ptr = if size == 0 {
            core::ptr::null_mut()
        } else {
            map_shm(&fd, size)?
        };

        Ok(Self { ptr, size })
    }

    fn remap(&mut self, fd: impl AsFd, size: usize) -> Result {
        if !self.ptr.is_null() {
            unsafe { nix::sys::mman::munmap(self.ptr as *mut c_void, self.size as usize)? }
        }

        self.ptr = if size == 0 {
            core::ptr::null_mut()
        } else {
            map_shm(&fd, size)?
        };

        self.size = size;

        Ok(())
    }

    fn as_pixels_mut(&mut self) -> &mut [impl TargetPixel] {
        unsafe {
            core::slice::from_raw_parts_mut(self.ptr as *mut PremultipliedRgbaColor, self.size / 4)
        }
    }
}

// --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

use std::collections::HashMap;
use wayland_client::backend::ObjectId;

#[derive(Debug)]
struct PrepareGateState {
    compositor: WlCompositor,
    shm: WlShm,
    layer_shell: ZwlrLayerShellV1,
    outputs: Vec<WlOutput>,
    modes: HashMap<ObjectId, Mode>,
}

impl PrepareGateState {
    fn forward(mut self, connection: &Connection) -> Result<ReadyGateState> {
        use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer;
        let layer = Layer::Overlay;

        let namespace = env!("CARGO_PKG_NAME");

        use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::Anchor;
        let anchor = Anchor::Top;

        let windows = self
            .outputs
            .into_iter()
            .map(|p| {
                self.modes
                    .remove(&p.id())
                    .ok_or_else(|| MissingError::new("mode of wl_output"))
                    .map(|m| (p, m))
            })
            .try_collect::<Vec<_>>()?
            .into_iter()
            .map(|(output, mode)| -> Result<_> {
                let mut queue = connection.new_event_queue();
                let handle = &queue.handle();

                let surface = self.compositor.create_surface(handle, ());
                let layer_surface = self.layer_shell.get_layer_surface(
                    &surface,
                    Some(&output),
                    layer,
                    namespace.to_owned(),
                    handle,
                    (),
                );

                layer_surface.set_anchor(anchor);
                surface.commit();

                let rr = RenderingRegion::make(&connection, handle, &self.shm)?;

                let mut window = Window {
                    output,
                    mode,
                    surface,
                    layer_surface,
                    rr,
                };

                queue.roundtrip(&mut window)?;

                window.surface.attach(Some(&window.rr.buffer), 0, 0);
                window.surface.commit();

                queue.roundtrip(&mut window)?;

                Ok((window, queue))
            })
            .try_collect()?;

        Ok(ReadyGateState { windows })
    }
}

// --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

#[derive(Debug, Clone, Copy)]
struct Mode {
    width: usize,
    height: usize,
}

use slint::LogicalSize;
use slint::WindowSize;

impl Into<LogicalSize> for Mode {
    fn into(self) -> LogicalSize {
        let Self { width, height } = self;

        LogicalSize {
            width: width as f32,
            height: height as f32,
        }
    }
}

impl Into<WindowSize> for Mode {
    fn into(self) -> WindowSize {
        Into::<LogicalSize>::into(self).into()
    }
}

// --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

// unallow to receive events
wayland_client::delegate_noop!(PrepareGateState: WlRegistry);
wayland_client::delegate_noop!(PrepareGateState: WlCompositor);
wayland_client::delegate_noop!(PrepareGateState: WlShm);
wayland_client::delegate_noop!(PrepareGateState: ZwlrLayerShellV1);

impl Dispatch<WlOutput, ()> for PrepareGateState {
    fn event(
        state: &mut Self,
        output: &WlOutput,
        event: <WlOutput as Proxy>::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        type Event = <WlOutput as Proxy>::Event;

        match event {
            Event::Mode { width, height, .. } => {
                let id = output.id();
                let mode = Mode {
                    width: width as usize,
                    height: height as usize,
                };

                let None = state.modes.insert(id, mode) else {
                    unreachable!()
                };
            }

            Event::Geometry { .. }
            | Event::Done
            | Event::Scale { .. }
            | Event::Name { .. }
            | Event::Description { .. } => (),

            _ => unreachable!(),
        }
    }
}

// --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

use core::num::NonZeroUsize;
use std::os::fd::OwnedFd;

fn allocate_shm<T: TryInto<usize>>(size: T) -> Result<OwnedFd>
where
    <T as TryInto<usize>>::Error: Error + Send + Sync + 'static,
{
    let size = size.try_into()?;

    let name = {
        let ts = std::time::SystemTime::now().elapsed().unwrap().as_nanos();

        format!("/wl-shm-{ts:32x}")
    };

    let fd = {
        use nix::fcntl::OFlag;
        use nix::sys::stat::Mode;

        let flag = OFlag::O_RDWR | OFlag::O_CREAT | OFlag::O_EXCL;
        let mode = Mode::S_IRUSR | Mode::S_IWUSR | Mode::S_IXUSR;

        nix::sys::mman::shm_open(&*name, flag, mode)?
    };

    nix::sys::mman::shm_unlink(&*name)?;
    nix::unistd::ftruncate(&fd, size as i64)?;

    Ok(fd)
}

fn map_shm<T: TryInto<usize>>(fd: impl AsFd, size: T) -> Result<*mut u8>
where
    <T as TryInto<usize>>::Error: Error + Send + Sync + 'static,
{
    let size: NonZeroUsize = size.try_into()?.try_into()?;

    use nix::sys::mman::MapFlags;
    use nix::sys::mman::ProtFlags;

    let pflag = ProtFlags::PROT_READ | ProtFlags::PROT_WRITE;
    let mflag = MapFlags::MAP_SHARED;

    Ok(unsafe { nix::sys::mman::mmap(None, size, pflag, mflag, Some(fd), 0)? as *mut u8 })
}

// --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

#[derive(Debug)]
struct ReadyGateState {
    windows: Vec<(Window, EventQueue<Window>)>,
}

use slint::platform::software_renderer::RepaintBufferType;

impl ReadyGateState {
    fn forward(self, target: Rc<MinimalSoftwareWindow>) -> Result<State> {
        let instances = self
            .windows
            .into_iter()
            .map(|(window, queue)| -> Result<_> {
                let component = Root::new()?;
                let target = target.clone();

                component.show()?;

                Ok(Instance {
                    window,
                    queue,
                    component,
                    target,
                })
            })
            .try_collect()?;

        Ok(State { instances })
    }
}

// --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::EventQueue;
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::ZwlrLayerSurfaceV1;

#[derive(Debug)]
struct Window {
    output: WlOutput,
    mode: Mode,
    surface: WlSurface,
    layer_surface: ZwlrLayerSurfaceV1,
    rr: RenderingRegion,
}

// use core::fmt::Debug;
//
// struct Raw(&'static mut [u8]);
//
// impl From<&'static mut [u8]> for Raw {
//     fn from(val: &'static mut [u8]) -> Self {
//         Self(val)
//     }
// }
//
// impl Debug for Raw {
//     fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
//         write!(f, "<< raw >>")
//     }
// }
//
// use slint::platform::software_renderer::PremultipliedRgbaColor;
// use slint::platform::software_renderer::TargetPixel;
//
// impl Raw {
//     fn as_buffer(&mut self) -> &mut [impl TargetPixel] {
//         let (pixels, []) = self.0.as_chunks_mut::<4>() else {
//             unreachable!()
//         };
//
//         unsafe { core::mem::transmute::<&mut [[u8; 4]], &mut [PremultipliedRgbaColor]>(pixels) }
//     }
// }

// #[derive(Clone, Copy)]
// #[repr(transparent)]
// struct Argb([u8; 4]);
//
// impl TargetPixel for Argb {
//     fn blend(&mut self, color: PremultipliedRgbaColor) {
//         let alpha = (u8::MAX - color.alpha) as u16;
//
//         self.0[1] = (self.0[1] as u16 * alpha / u8::MAX as u16) as u8 + color.red;
//         self.0[2] = (self.0[2] as u16 * alpha / u8::MAX as u16) as u8 + color.green;
//         self.0[3] = (self.0[3] as u16 * alpha / u8::MAX as u16) as u8 + color.blue;
//
//         self.0[0] = (self.0[0] as u16 + color.alpha as u16
//             - (self.0[0] as u16 * color.alpha as u16) / u8::MAX as u16) as u8;
//     }
//
//     fn from_rgb(r: u8, g: u8, b: u8) -> Self {
//         Self([0xff, r, g, b])
//     }
// }

// --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

impl Dispatch<WlSurface, ()> for Window {
    fn event(
        state: &mut Self,
        surface: &WlSurface,
        event: <WlSurface as Proxy>::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        type Event = <WlSurface as Proxy>::Event;

        match event {
            Event::Enter { output } => {
                if output.id() != state.output.id() {
                    panic!();
                }
            }

            Event::Leave { output } => {
                if output.id() != state.output.id() {
                    panic!();
                }

                unimplemented!()
            }

            Event::PreferredBufferScale { .. } | Event::PreferredBufferTransform { .. }
                if surface.version() >= 6 =>
            {
                unimplemented!()
            }

            _ => unreachable!(),
        }
    }
}

impl Dispatch<WlBuffer, ()> for Window {
    fn event(
        state: &mut Self,
        _: &WlBuffer,
        event: <WlBuffer as Proxy>::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        type Event = <WlBuffer as Proxy>::Event;

        match event {
            Event::Release => {
                println!("wl_buffer: released");
            }

            _ => unreachable!(),
        }
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, ()> for Window {
    fn event(
        _: &mut Self,
        layer_surface: &ZwlrLayerSurfaceV1,
        event: <ZwlrLayerSurfaceV1 as Proxy>::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        type Event = <ZwlrLayerSurfaceV1 as Proxy>::Event;

        match event {
            Event::Configure { serial, .. } => {
                layer_surface.ack_configure(serial);
            }

            Event::Closed => unimplemented!(),

            _ => unreachable!(),
        }
    }
}

// --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

struct State {
    instances: Vec<Instance>,
}

impl State {
    fn tick(&mut self) -> Result {
        self.instances.iter_mut().try_for_each(|i| i.tick())
    }
}

// --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

use slint::platform::software_renderer::MinimalSoftwareWindow;

struct Instance {
    window: Window,
    queue: EventQueue<Window>,
    component: Root,
    target: Rc<MinimalSoftwareWindow>,
}

impl Instance {
    fn tick(&mut self) -> Result {
        slint::platform::update_timers_and_animations();

        self.target.draw_if_needed(|renderer| {
            renderer.render(
                self.window.rr.region.as_pixels_mut(),
                self.target.size().width as usize,
            );
        });

        // ^^^ --- rendering --- ^^^

        self.window.surface.damage(0, 0, i32::MAX, i32::MAX);
        self.window.surface.commit();

        self.queue.roundtrip(&mut self.window)?;

        Ok(())
    }

    fn set_height(&mut self, height: usize) -> Result {
        let width = self.window.mode.width;

        let handle = &self.queue.handle();
        self.window.rr.resize(Mode { width, height }, handle)?;
        self.target.set_size(Mode { width, height });

        self.window
            .surface
            .attach(Some(&self.window.rr.buffer), 0, 0);

        self.window
            .layer_surface
            .set_size(width as u32, height as u32);

        self.window.layer_surface.set_exclusive_zone(height as i32);

        self.queue.roundtrip(&mut self.window)?;

        Ok(())
    }
}
