#![feature(error_in_core)]
#![feature(iterator_try_collect)]
#![feature(never_type)]
#![feature(slice_as_chunks)]
#![feature(slice_from_ptr_range)]
#![feature(slice_ptr_get)]
#![feature(stmt_expr_attributes)]

use core::error::Error;
use wayland_client::Connection;

type Result<T = (), E = Box<dyn Error + Send + Sync + 'static>> = core::result::Result<T, E>;

// --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

slint::slint! {
    export component Main inherits Window {
        background: transparent;

        default-font-family: "0xProto";
        default-font-weight: 100;

        in property<string> battery-level;

        GridLayout {
            Row {
                Rectangle {
                    height: 100%;
                    border-radius: 4px;

                    Text { color: #ffffff; font-size: 1.5rem; text: battery-level; }
                }
            }
        }
    }
}

// --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

fn main() -> Result {
    let connection = Connection::connect_to_env()?;
    let display = connection.display();

    let mut queue = connection.new_event_queue();
    let mut state = InitialGateState::default();

    let _ = display.get_registry(&queue.handle(), ());

    queue.roundtrip(&mut state)?;
    queue.roundtrip(&mut state)?;

    let mut queue = connection.new_event_queue();
    let mut state = state.forward(&queue.handle())?;

    queue.roundtrip(&mut state)?;

    let mut state = state.forward(&connection)?;

    let window = create_window();
    let ui = Main::new()?;

    state.windows.iter().for_each(|(w, _)| {
        let width = w.mode.width;
        let height = w.mode.height / 64;

        w.layer_surface.set_size(width as u32, height as u32);
        w.layer_surface.set_exclusive_zone(height as i32);
        w.surface.commit();

        window.set_size(slint::PhysicalSize::new(width as u32, height as u32));
    });

    ui.show()?;

    let mut rbc = Transition::new(read_battery_cap, Duration::from_secs(60));
    ui.set_battery_level(read_battery_cap());

    loop {
        for (w, q) in &mut state.windows {
            slint::platform::update_timers_and_animations();

            use slint::platform::software_renderer::PremultipliedRgbaColor as Pixel;
            let pixels = unsafe { w.raw.as_slice_mut::<Pixel>() }?;

            window.draw_if_needed(|r| {
                r.render(pixels, w.mode.width);

                w.surface.damage(0, 0, i32::MAX, i32::MAX);
                w.surface.commit();
            });

            // ^^^ update ^^^

            q.roundtrip(w)?;

            // ^^^ event loop ^^^

            rbc.update_if_elapsed(|ss| ui.set_battery_level(ss));

            // ^^^ represent ^^^
        }

        std::thread::sleep(Duration::from_secs(1));
    }
}

// --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

fn read_battery_cap() -> slint::SharedString {
    let Ok(raw) = std::fs::read_to_string("/sys/class/power_supply/macsmc-battery/capacity") else {
        return slint::format!("");
    };

    let Ok(num) = raw.trim().parse::<f32>() else {
        return slint::format!("");
    };

    slint::format!("{num}%")
}

// --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

use core::time::Duration;
use std::time::Instant;

struct Transition<F> {
    update: F,
    interval: Duration,
    before: Instant,
}

impl<F: FnMut() -> slint::SharedString> Transition<F> {
    pub fn new(update: F, interval: Duration) -> Self {
        let before = Instant::now();

        Self {
            update,
            interval,
            before,
        }
    }

    pub fn update_if_elapsed(&mut self, set: impl FnOnce(slint::SharedString)) {
        if self.before.elapsed() < self.interval {
            return;
        }

        set((self.update)());
        self.before = Instant::now();
    }
}

// --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

use std::rc::Rc;

use slint::platform::software_renderer::MinimalSoftwareWindow;
use slint::platform::WindowAdapter;

fn create_window() -> Rc<MinimalSoftwareWindow> {
    let window = MinimalSoftwareWindow::new(Default::default());
    let platform = Platform {
        window: window.clone(),
    };

    slint::platform::set_platform(Box::new(platform)).unwrap();

    window
}

struct Platform {
    window: Rc<MinimalSoftwareWindow>,
}

impl slint::platform::Platform for Platform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }

    fn duration_since_start(&self) -> core::time::Duration {
        std::time::SystemTime::now().elapsed().unwrap()
    }
}

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

        use wayland_client::protocol::wl_shm::Format;
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

use wayland_client::protocol::wl_shm_pool::WlShmPool;

impl PrepareGateState {
    fn forward(mut self, connection: &Connection) -> Result<ReadyGateState> {
        let create_buffer = |mode: &Mode, qh: &QueueHandle<Window>| -> Result<_> {
            use wayland_client::protocol::wl_shm::Format;
            let format = Format::Argb8888;
            let pixel_size = 4;

            let size = mode.width * mode.height * pixel_size;
            let raw = Shm::new(size)?;

            let pool = {
                struct Stub;

                // correctness: `wl_shm_pool` has no events
                wayland_client::delegate_noop!(Stub: WlShmPool);
                let qh = connection.new_event_queue::<Stub>().handle();

                let size = size.try_into()?;

                self.shm.create_pool(raw.as_fd(), size, &qh, ())
            };

            let buffer = {
                let w = mode.width.try_into()?;
                let h = mode.height.try_into()?;
                let s = (mode.width * pixel_size).try_into()?;

                pool.create_buffer(0, w, h, s, format, qh, ())
            };

            Ok((buffer, raw))
        };

        use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer;
        let layer = Layer::Background;

        let namespace = "namespace";

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

                layer_surface.set_size(0, 0);
                layer_surface.set_anchor(anchor);
                layer_surface.set_exclusive_zone(0);
                surface.commit();

                let (buffer, raw) = create_buffer(&mode, handle)?;

                let mut window = Window {
                    output,
                    mode,
                    surface,
                    layer_surface,
                    buffer,
                    raw,
                };

                queue.roundtrip(&mut window)?;

                window.surface.attach(Some(&window.buffer), 0, 0);
                window.surface.commit();

                queue.roundtrip(&mut window)?;

                Ok((window, queue))
            })
            .try_collect()?;

        Ok(ReadyGateState { windows })
    }
}

// --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

#[derive(Debug)]
struct Mode {
    width: usize,
    height: usize,
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

use core::ptr::NonNull;
use std::os::fd::AsFd;
use std::os::fd::OwnedFd;

#[derive(Debug)]
struct Shm {
    ptr: NonNull<[u8]>,
    fd: OwnedFd,
}

impl Shm {
    pub fn new(size: usize) -> Result<Self> {
        let fd = Self::open()?;

        let ptr = Self::mmap(&fd, size)?;
        let ptr = core::ptr::slice_from_raw_parts_mut(ptr, size);
        let ptr = NonNull::new(ptr).ok_or(Unhandled)?;

        Ok(Self { ptr, fd })
    }

    fn open() -> Result<OwnedFd> {
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

        Ok(fd)
    }

    fn mmap<F: AsFd>(fd: F, size: usize) -> Result<*mut u8> {
        nix::unistd::ftruncate(&fd, size.try_into()?)?;

        let ptr = {
            use nix::sys::mman::MapFlags;
            use nix::sys::mman::ProtFlags;

            let pflag = ProtFlags::PROT_READ | ProtFlags::PROT_WRITE;
            let mflag = MapFlags::MAP_SHARED;

            unsafe { nix::sys::mman::mmap(None, size.try_into()?, pflag, mflag, Some(&fd), 0) }?
        };

        Ok(ptr.cast::<u8>())
    }

    pub fn resize(&mut self, size: usize) -> Result<()> {
        nix::unistd::ftruncate(&self.fd, size.try_into()?)?;

        let ptr = self.ptr.as_mut_ptr();
        let ptr = core::ptr::slice_from_raw_parts_mut(ptr, size);
        let ptr = NonNull::new(ptr).ok_or(Unhandled)?;

        self.ptr = ptr;

        Ok(())
    }

    pub unsafe fn as_slice_mut<T>(&mut self) -> Result<&mut [T]> {
        let len = self.ptr.len();
        let ptr = self.ptr.as_mut_ptr().cast::<T>();
        let ptr = core::ptr::slice_from_raw_parts_mut(ptr, len / core::mem::size_of::<T>());

        Ok(ptr.as_mut().ok_or(Unhandled)?)
    }

    pub fn as_fd(&self) -> std::os::fd::BorrowedFd<'_> {
        self.fd.as_fd()
    }
}

// --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

#[derive(Debug)]
struct Unhandled;

impl Display for Unhandled {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl Error for Unhandled {}

// --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

#[derive(Debug)]
struct ReadyGateState {
    windows: Vec<(Window, EventQueue<Window>)>,
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
    buffer: WlBuffer,
    raw: Shm,
}

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
        _: &mut Self,
        _: &WlBuffer,
        event: <WlBuffer as Proxy>::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        type Event = <WlBuffer as Proxy>::Event;

        match event {
            Event::Release => {
                unimplemented!()
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
