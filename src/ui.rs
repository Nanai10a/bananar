use slint::platform::software_renderer::SoftwareRenderer;
use slint::platform::Platform;
use slint::platform::PlatformError;
use slint::platform::Renderer;
use slint::platform::WindowAdapter;
use slint::PhysicalSize;
use slint::Window;

use alloc::rc::Rc;
use alloc::sync::Arc;
use alloc::sync::Weak as Aweak;
use core::fmt::Debug;
use std::sync::RwLock;

type Result<T, E = PlatformError> = core::result::Result<T, E>;

#[derive(Debug, Clone)]
pub struct Ui {
    pub inner: Arc<RwLock<UiInner>>,
}

impl Ui {
    pub fn new() -> Self {
        let inner = Arc::new(RwLock::new(UiInner::new()));

        Self { inner }
    }
}

impl Platform for Ui {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>> {
        Ok(self.inner.write().unwrap().create_adapter())
    }

    fn run_event_loop(&self) -> Result<()> {
        loop {
            self.inner.write().unwrap().tick()?
        }
    }
}

#[derive(Debug)]
pub struct UiInner {
    feeded: Option<Aweak<RwLock<crate::mem::Shared>>>,
    adapters: Vec<Rc<Adapter>>,
}

impl UiInner {
    fn new() -> Self {
        let feeded = None;
        let adapters = Vec::new();

        Self { feeded, adapters }
    }

    pub fn feed_shared(&mut self, shared: Aweak<RwLock<crate::mem::Shared>>) {
        let None = self.feeded.replace(shared) else {
            panic!("already been feed")
        };
    }

    pub fn create_adapter(&mut self) -> Rc<Adapter> {
        let Some(shared) = self.feeded.take() else {
            panic!("hadn't been feed")
        };

        let adapter = Adapter::new(shared);
        self.adapters.push(adapter.clone());

        adapter
    }

    pub fn tick(&mut self) -> Result<()> {
        self.adapters
            .iter()
            .map(AsRef::as_ref)
            .try_for_each(Adapter::tick)
    }
}

struct Adapter {
    window: Window,
    size: PhysicalSize,
    renderer: SoftwareRenderer,
    shared: Aweak<RwLock<crate::mem::Shared>>,
}

impl Adapter {
    fn new(shared: Aweak<RwLock<crate::mem::Shared>>) -> Rc<Self> {
        Rc::<Self>::new_cyclic(|weak| {
            let window = Window::new(weak.clone());
            let size = PhysicalSize::default();
            let renderer = SoftwareRenderer::new();

            Self {
                window,
                size,
                renderer,
                shared,
            }
        })
    }

    fn tick(&self) -> Result<()> {
        let Some(shared) = self.shared.upgrade() else {
            panic!()
        };

        let mut shared = shared.write().unwrap();
        let stride = shared.size().width;

        let shared = unsafe {
            type Target = slint::platform::software_renderer::PremultipliedRgbaColor;

            assert_eq!(shared.size().pixel_size, core::mem::size_of::<Target>());

            let len = shared.len() / core::mem::size_of::<Target>();
            let ptr = shared.as_mut().as_mut_ptr().cast::<Target>();

            core::slice::from_raw_parts_mut(ptr, len)
        };

        self.renderer.render(shared, stride);

        Ok(())
    }
}

impl Debug for Adapter {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Adapter")
            .field("window", &"{opaque}")
            .field("size", &self.size)
            .field("renderer", &"{opaque}")
            .field("shared", &self.shared)
            .finish()
    }
}

impl WindowAdapter for Adapter {
    fn window(&self) -> &Window {
        &self.window
    }

    fn size(&self) -> PhysicalSize {
        self.size
    }

    fn renderer(&self) -> &dyn Renderer {
        &self.renderer
    }
}
