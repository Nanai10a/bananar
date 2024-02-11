#![feature(decl_macro)]
#![feature(error_in_core)]
#![feature(never_type)]
#![feature(stmt_expr_attributes)]

extern crate alloc;

slint::slint! {
    export component Ui {
        Text {
            text: "hi, slint + layer-shell";
        }
    }
}

fn main() {
    MyPlatform::new();
}

struct MyPlatform {}

impl MyPlatform {
    fn new() -> Self {
        Self {}
    }
}

use alloc::rc::Rc;
use slint::platform::Platform;
use slint::platform::WindowAdapter;
use slint::PlatformError;

impl Platform for MyPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(Rc::new(MyWindowAdapter::new()?))
    }
}

use slint::platform::femtovg_renderer::FemtoVGRenderer;
use slint::platform::femtovg_renderer::OpenGLInterface;

struct MyWindowAdapter {
    renderer: FemtoVGRenderer,
    client: MyWaylandClient,
}

impl MyWindowAdapter {
    fn new() -> Result<Self, PlatformError> {
        let client = MyWaylandClient::connect()?;

        let ctx = MyOpenGLInterface::new(client.get_raw_ptr())?;
        let renderer = FemtoVGRenderer::new(ctx)?;

        Ok(Self { renderer, client })
    }
}

use slint::platform::Renderer;
use slint::PhysicalSize;
use slint::Window;

impl WindowAdapter for MyWindowAdapter {
    fn window(&self) -> &Window {
        todo!()
    }

    fn size(&self) -> PhysicalSize {
        todo!()
    }

    fn renderer(&self) -> &dyn Renderer {
        &self.renderer
    }
}

use glutin::display::Display;
use glutin::display::DisplayApiPreference;
use raw_window_handle::WaylandDisplayHandle;

struct MyOpenGLInterface {
    display: Display,
}

impl MyOpenGLInterface {
    fn new(wl_display: *mut c_void) -> Result<Self, Box<dyn Error + Send + Sync + 'static>> {
        let display = unsafe {
            let mut wdh = WaylandDisplayHandle::empty();
            wdh.display = wl_display;

            let dap = DisplayApiPreference::Egl;

            Display::new(wdh.into(), dap)?
        };

        Ok(Self { display })
    }
}

use core::error::Error;
use core::ffi::c_void;
use core::ffi::CStr;
use core::num::NonZeroU32;
use glutin::display::GlDisplay;

unsafe impl OpenGLInterface for MyOpenGLInterface {
    fn ensure_current(&self) -> Result<(), Box<dyn Error + Sync + Send>> {
        todo!()
    }

    fn swap_buffers(&self) -> Result<(), Box<dyn Error + Sync + Send>> {
        todo!()
    }

    fn resize(
        &self,
        width: NonZeroU32,
        height: NonZeroU32,
    ) -> Result<(), Box<dyn Error + Sync + Send>> {
        todo!()
    }

    fn get_proc_address(&self, name: &CStr) -> *const c_void {
        GlDisplay::get_proc_address(&self.display, name)
    }
}

use wayland_client::Connection;

struct MyWaylandClient {
    conn: Connection,
}

impl MyWaylandClient {
    fn connect() -> Result<Self, Box<dyn Error + Send + Sync + 'static>> {
        let conn = Connection::connect_to_env()?;
        let wl_display = conn.display();

        let mut queue = conn.new_event_queue::<MyWaylandState>();
        let qh = queue.handle();

        let collector = Arc::new(RwLock::new(MyWlCollector::default()));
        wl_display.get_registry(&qh, collector.clone());

        queue.roundtrip(todo!())?;

        todo!()
    }
}

use wayland_client::Proxy;

impl MyWaylandClient {
    fn get_raw_ptr(&self) -> *mut c_void {
        // SAFETY: unknown
        Proxy::id(&self.conn.display()).as_ptr() as *mut c_void
    }
}

struct MyWaylandState {}

use alloc::sync::Arc;
use std::sync::RwLock;
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_shm::WlShm;
use wayland_client::Dispatch;
use wayland_client::QueueHandle;
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::ZwlrLayerShellV1;

impl<'a> Dispatch<WlRegistry, Arc<RwLock<MyWlCollector>>> for MyWaylandState {
    fn event(
        _: &mut Self,
        wl_registry: &WlRegistry,
        event: <WlRegistry as Proxy>::Event,
        collector: &Arc<RwLock<MyWlCollector>>,
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
                    collector
                        .write()
                        .unwrap()
                        .wl_compositor
                        .insert(wl_registry.bind(name, version, qh, ()));
                }

                #[rustfmt::skip]
                if <WlOutput as Proxy>::interface().name == interface {
                    collector
                        .write()
                        .unwrap()
                        .wl_outputs
                        .push(wl_registry.bind(name,version,qh,()));
                }

                #[rustfmt::skip]
                if <WlShm as Proxy>::interface().name == interface {
                    collector
                        .write()
                        .unwrap()
                        .wl_shm.insert(wl_registry.bind(name, version, qh, ()));
                }

                if <ZwlrLayerShellV1 as Proxy>::interface().name == interface {
                    collector
                        .write()
                        .unwrap()
                        .zwlr_layer_shell_v1
                        .insert(wl_registry.bind(name, version, qh, ()));
                }
            }

            _ => unreachable!(),
        }
    }
}

wayland_client::delegate_noop!(MyWaylandState: WlCompositor);
wayland_client::delegate_noop!(MyWaylandState: ZwlrLayerShellV1);

wayland_client::delegate_noop!(MyWaylandState: WlShm);

wayland_client::delegate_noop!(MyWaylandState: WlOutput);

#[derive(Default)]
struct MyWlCollector {
    wl_compositor: Option<WlCompositor>,
    wl_outputs: Vec<WlOutput>,
    wl_shm: Option<WlShm>,
    zwlr_layer_shell_v1: Option<ZwlrLayerShellV1>,
}
