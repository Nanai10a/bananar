#![feature(error_in_core)]
#![feature(iterator_try_collect)]
#![feature(never_type)]
#![feature(slice_from_ptr_range)]
#![feature(stmt_expr_attributes)]

use core::error::Error;
use wayland_client::Connection;

type Result<T = (), E = Box<dyn Error + Send + Sync + 'static>> = core::result::Result<T, E>;

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

    loop {
        for (w, q) in &mut state.windows {
            w.raw.0.fill(0xff);

            // ^^^ --- rendering --- ^^^

            w.surface.damage(0, 0, i32::MAX, i32::MAX);
            w.surface.commit();

            q.roundtrip(w)?;
        }
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
        // HACK: allocate 8 [MB]
        const POOL_SIZE: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(8 * 1024 * 1024) };

        let (fd, ptr) = allocate_shm(POOL_SIZE)?;

        let pool = {
            let fd = std::os::fd::AsFd::as_fd(&fd);

            struct Stub;

            // correctness: `wl_shm_pool` has no events
            wayland_client::delegate_noop!(Stub: WlShmPool);

            let queue = connection.new_event_queue::<Stub>();

            self.shm
                .create_pool(fd, POOL_SIZE.get() as i32, &queue.handle(), ())
        };

        let mut create_buffer = {
            // RGBA8888 has 4 [B]
            const PIXEL_SIZE: i32 = 4;
            let mut offset = 0;

            use wayland_client::protocol::wl_shm::Format;
            let format = Format::Argb8888;

            move |width: i32, height: i32, handle: &QueueHandle<Window>| {
                let stride = width * PIXEL_SIZE;
                let buffer = pool.create_buffer(offset, width, height, stride, format, handle, ());
                let raw = unsafe {
                    let start = ptr.offset(offset as isize);
                    let end = ptr.offset((offset + width * height * PIXEL_SIZE) as isize);

                    core::slice::from_mut_ptr_range(start..end)
                };

                offset += width * height * PIXEL_SIZE;
                (buffer, raw)
            }
        };

        use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer;
        let layer = Layer::Overlay;

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

                let width = mode.width;
                let height = mode.height / 32;

                layer_surface.set_size(width as u32, height as u32);
                layer_surface.set_anchor(anchor);
                layer_surface.set_exclusive_zone(height as i32);
                surface.commit();

                let (buffer, raw) = create_buffer(width as i32, height as i32, handle);

                let mut window = Window {
                    output,
                    mode,
                    surface,
                    layer_surface,
                    buffer,
                    raw: raw.into(),
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

use core::num::NonZeroUsize;
use std::os::fd::OwnedFd;

fn allocate_shm(size: NonZeroUsize) -> Result<(OwnedFd, *mut u8)> {
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
    nix::unistd::ftruncate(&fd, size.get() as i64)?;

    let ptr = {
        use nix::sys::mman::MapFlags;
        use nix::sys::mman::ProtFlags;

        let pflag = ProtFlags::PROT_READ | ProtFlags::PROT_WRITE;
        let mflag = MapFlags::MAP_SHARED;

        unsafe { nix::sys::mman::mmap(None, size, pflag, mflag, Some(&fd), 0) }?
    };

    Ok((fd, ptr as *mut u8))
}

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
    raw: Hide<&'static mut [u8]>,
}

use core::fmt::Debug;

struct Hide<T>(T);

impl<T> From<T> for Hide<T> {
    fn from(val: T) -> Self {
        Self(val)
    }
}

impl<T> Debug for Hide<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "<< hidden >>")
    }
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
        state: &mut Self,
        layer_surface: &ZwlrLayerSurfaceV1,
        event: <ZwlrLayerSurfaceV1 as Proxy>::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        type Event = <ZwlrLayerSurfaceV1 as Proxy>::Event;

        match event {
            Event::Configure {
                serial,
                width,
                height,
            } => {
                if width as usize != state.mode.width || height as usize != state.mode.height / 32 {
                    panic!()
                }

                layer_surface.ack_configure(serial);
            }

            Event::Closed => unimplemented!(),

            _ => unreachable!(),
        }
    }
}
