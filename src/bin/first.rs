#![feature(decl_macro)]
#![feature(error_in_core)]
#![feature(never_type)]

extern crate alloc;

slint::slint! {
    export component Main {
        Text {
            text: "hi, slint?";
            color: green;
        }
    }
}

use slint::platform::set_platform;

fn main() -> Result<(), Box<dyn Error>> {
    set_platform(Box::new(MyPlatform::new()?)).expect("already initialized platform!");

    Ok(Main::new()?.run()?)
}

use std::sync::RwLock;

struct MyPlatform {
    client: MyWaylandClient,
    data: Arc<RwLock<MyWindowData>>,
}

impl MyPlatform {
    fn new() -> Result<Self, PlatformError> {
        let client = MyWaylandClient::connect()?;

        let data = Arc::new(RwLock::new(MyWindowData {
            width: 0,
            height: 0,
        }));

        Ok(Self { client, data })
    }
}

struct MyWindowData {
    width: usize,
    height: usize,
}

use alloc::rc::Rc;
use slint::platform::Platform;
use slint::platform::WindowAdapter;
use slint::PlatformError;

impl Platform for MyPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(Rc::<MyWindowAdapter>::new_cyclic(|weak| {
            MyWindowAdapter::new(Window::new(weak.clone()), self.data.clone())
        }))
    }

    fn run_event_loop(&self) -> Result<(), PlatformError> {
        loop {}
    }
}

use alloc::sync::Arc;
use slint::platform::software_renderer::SoftwareRenderer;

struct MyWindowAdapter {
    window: Window,
    data: Arc<RwLock<MyWindowData>>,
    renderer: SoftwareRenderer,
}

impl MyWindowAdapter {
    fn new(window: Window, data: Arc<RwLock<MyWindowData>>) -> Self {
        Self {
            window,
            data,
            renderer: SoftwareRenderer::new(),
        }
    }
}

use slint::platform::Renderer;
use slint::PhysicalSize;
use slint::Window;

impl WindowAdapter for MyWindowAdapter {
    fn window(&self) -> &Window {
        &self.window
    }

    fn size(&self) -> PhysicalSize {
        let data = self.data.read().unwrap();

        PhysicalSize {
            width: data.width as u32,
            height: data.height as u32,
        }
    }

    fn renderer(&self) -> &dyn Renderer {
        &self.renderer
    }
}

use core::error::Error;
use core::ffi::c_void;

use wayland_client::Connection;

struct MyWaylandClient {
    conn: Connection,
    reg: MyWaylandState,
}

use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer;
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::Anchor;

impl MyWaylandClient {
    fn connect() -> Result<Self, Box<dyn Error + Send + Sync + 'static>> {
        let conn = Connection::connect_to_env()?;
        let disp = conn.display();

        let (cmp, out, shm, shl) = {
            let mut queue = conn.new_event_queue::<RegistryPoller>();
            let qh = queue.handle();

            let mut state = RegistryPoller::default();
            disp.get_registry(&qh, ());
            queue.roundtrip(&mut state)?;

            state.destruct().unwrap()
        };

        // FIXME: `wl_output` may exist more than one
        // FIXME: subscribe `mode.{width,height}` from `wl_output`
        macro width() {
            3456
        }
        macro height() {
            128
        }

        let wm = {
            let mut queue = conn.new_event_queue::<WindowManager>();
            let qh = queue.handle();

            let wl_surface = cmp.create_surface(&qh, ());
            let zwlr_layer_surface_v1 = shl.get_layer_surface(
                &wl_surface,
                Some(&out),
                Layer::Overlay,
                "namespace".to_owned(),
                &qh,
                (),
            );

            zwlr_layer_surface_v1.set_size(width!(), height!());
            zwlr_layer_surface_v1.set_anchor(Anchor::Top);

            wl_surface.commit();

            let mut wm = WindowManager {
                wl_surface,
                zwlr_layer_surface_v1,
            };

            queue.roundtrip(&mut wm)?;

            wm
        };

        let (wl_buffer, canvas) = {
            let len = width!() * height!() * 4;

            let (fd, raw) = unsafe {
                let (fd, ptr) = allocate_shm(len);

                let ptr = ptr as *mut PremultipliedRgbaColor;
                let len = len / 4;
                assert_eq!(len % 4, 0);

                let raw = core::slice::from_raw_parts_mut(ptr, len);

                (fd, raw)
            };

            let mut queue = conn.new_event_queue::<Canvas>();
            let qh = queue.handle();

            use std::os::fd::AsFd as _;
            let wl_shm_pool = shm.create_pool(fd.as_fd(), len as i32, &qh, ());

            let wl_buffer = wl_shm_pool.create_buffer(
                0,
                width!(),
                height!(),
                width!() * 4,
                Format::Rgba8888,
                &qh,
                (),
            );

            let mut canvas = Canvas { raw };

            queue.roundtrip(&mut canvas)?;

            (wl_buffer, canvas)
        };

        wm.wl_surface.attach(Some(&wl_buffer), 0, 0);
        wm.wl_surface.damage(0, 0, i32::MAX, i32::MAX);
        wm.wl_surface.commit();

        Ok(Self { conn, reg })
    }
}

use slint::platform::software_renderer::PremultipliedRgbaColor;
use wayland_client::protocol::wl_shm::Format;

struct Canvas {
    raw: &'static mut [PremultipliedRgbaColor],
}

impl_dispatch_stub! { impl stub [ WlShmPool, WlBuffer ] for Canvas }

struct WindowManager {
    wl_surface: WlSurface,
    zwlr_layer_surface_v1: ZwlrLayerSurfaceV1,
}

impl_dispatch_stub! { impl stub [ WlSurface, ZwlrLayerSurfaceV1 ] for WindowManager }

#[derive(Default)]
struct MyWaylandState {
    cmp: Option<WlCompositor>,
    out: Option<WlOutput>,
    shm: Option<WlShm>,
    shl: Option<ZwlrLayerShellV1>,
}

use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_shm::WlShm;
use wayland_client::protocol::wl_shm_pool::WlShmPool;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::Dispatch;
use wayland_client::Proxy;
use wayland_client::QueueHandle;
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::ZwlrLayerShellV1;
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::ZwlrLayerSurfaceV1;

impl Dispatch<WlRegistry, ()> for MyWaylandState {
    fn event(
        state: &mut Self,
        proxy: &WlRegistry,
        event: <WlRegistry as Proxy>::Event,
        (): &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        type Event = <WlRegistry as Proxy>::Event;

        match event {
            Event::Global {
                name,
                interface,
                version,
            } => {
                if <WlCompositor as Proxy>::interface().name == interface {
                    let _ = state.cmp.insert(proxy.bind(name, version, qh, ()));
                }

                if <WlOutput as Proxy>::interface().name == interface {
                    let _ = state.out.insert(proxy.bind(name, version, qh, ()));
                }

                if <WlShm as Proxy>::interface().name == interface {
                    let _ = state.shm.insert(proxy.bind(name, version, qh, ()));
                }

                if <ZwlrLayerShellV1 as Proxy>::interface().name == interface {
                    let _ = state.shl.insert(proxy.bind(name, version, qh, ()));
                }
            }

            Event::GlobalRemove { name } => {
                unimplemented!("wl_registry dispatches global_remove name = {name}");
            }

            _ => unreachable!(),
        }
    }
}

macro impl_dispatch_stub(impl stub [ $($ty:ident $(,)?)* ] for $self:ident) {
    $(
        impl Dispatch<$ty, ()> for $self {
            fn event(
                _: &mut Self,
                _: &$ty,
                event: <$ty as Proxy>::Event,
                (): &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
                panic!("stub receives event: {event:?}");
            }
        }
    )*
}

#[derive(Default)]
struct RegistryPoller {
    cmp: Option<WlCompositor>,
    out: Option<WlOutput>,
    shm: Option<WlShm>,
    shl: Option<ZwlrLayerShellV1>,
}

impl RegistryPoller {
    fn destruct(self) -> Option<(WlCompositor, WlOutput, WlShm, ZwlrLayerShellV1)> {
        Some((self.cmp?, self.out?, self.shm?, self.shl?))
    }
}

impl Dispatch<WlRegistry, ()> for RegistryPoller {
    fn event(
        state: &mut Self,
        proxy: &WlRegistry,
        event: <WlRegistry as Proxy>::Event,
        (): &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        type Event = <WlRegistry as Proxy>::Event;

        match event {
            Event::Global {
                name,
                interface,
                version,
            } => {
                if <WlCompositor as Proxy>::interface().name == interface {
                    let _ = state.cmp.insert(proxy.bind(name, version, qh, ()));
                }

                if <WlOutput as Proxy>::interface().name == interface {
                    let _ = state.out.insert(proxy.bind(name, version, qh, ()));
                }

                if <WlShm as Proxy>::interface().name == interface {
                    let _ = state.shm.insert(proxy.bind(name, version, qh, ()));
                }

                if <ZwlrLayerShellV1 as Proxy>::interface().name == interface {
                    let _ = state.shl.insert(proxy.bind(name, version, qh, ()));
                }
            }

            Event::GlobalRemove { name } => {
                unimplemented!("wl_registry dispatches global_remove name = {name}");
            }

            _ => unreachable!(),
        }
    }
}

impl_dispatch_stub! {
    impl stub [ WlCompositor, WlOutput, WlShm, ZwlrLayerShellV1 ] for RegistryPoller
}

use std::os::fd::OwnedFd;

unsafe fn allocate_shm(len: usize) -> (OwnedFd, *mut c_void) {
    use nix::fcntl::OFlag;
    use nix::sys::mman::mmap;
    use nix::sys::mman::shm_open;
    use nix::sys::mman::shm_unlink;
    use nix::sys::mman::MapFlags;
    use nix::sys::mman::ProtFlags;
    use nix::sys::stat::Mode;
    use nix::unistd::ftruncate;

    assert_ne!(len, 0);

    let name = {
        let ts = std::time::SystemTime::now().elapsed().unwrap().as_nanos();

        format!("/wl-shm-{ts:32x}")
    };

    let fd = {
        let flag = OFlag::O_RDWR | OFlag::O_CREAT | OFlag::O_EXCL;
        let mode = Mode::S_IRUSR | Mode::S_IWUSR | Mode::S_IXUSR;

        shm_open(&*name, flag, mode).unwrap()
    };

    shm_unlink(&*name).unwrap();
    ftruncate(&fd, len as i64).unwrap();

    let ptr = {
        let len = len.try_into().unwrap();

        let pflag = ProtFlags::PROT_READ | ProtFlags::PROT_WRITE;
        let mflag = MapFlags::MAP_SHARED;

        mmap(None, len, pflag, mflag, Some(&fd), 0).unwrap()
    };

    (fd, ptr)
}
