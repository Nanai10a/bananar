use core::sync::atomic::AtomicBool;

use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::Proxy;
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::ZwlrLayerSurfaceV1;

use crate::reg::WithRx;

#[derive(Debug)]
pub struct Out {
    layer_surface: WithRx<ZwlrLayerSurfaceV1>,
    surface: WithRx<WlSurface>,
    output: WithRx<WlOutput>,
    known: Known,
    ack: AtomicBool,
}

impl Out {
    pub fn new(
        layer_surface: WithRx<ZwlrLayerSurfaceV1>,
        surface: WithRx<WlSurface>,
        output: WithRx<WlOutput>,
    ) -> Self {
        let known = Known::default();
        let ack = AtomicBool::new(false);

        surface.commit();

        Self {
            layer_surface,
            surface,
            output,
            known,
            ack,
        }
    }

    // pub fn wait_ack(&self) {
    //     use core::sync::atomic::Ordering;

    //     while !self.ack.load(Ordering::Relaxed) {
    //         type Event = <ZwlrLayerSurfaceV1 as Proxy>::Event;

    //         match self.layer_surface.rx.recv().unwrap() {
    //             Event::Configure {
    //                 serial,
    //                 width,
    //                 height,
    //             } => {
    //                 // assert_eq!(width, self.known.size().width());
    //                 // assert_eq!(height, self.known.size().height());

    //                 self.layer_surface.ack_configure(serial);
    //                 self.ack.store(true, Ordering::Relaxed);
    //             }

    //             _ => (),
    //         }
    //     }
    // }

    pub fn wait_ack(&mut self) {
        let serial = loop {
            type Event = <ZwlrLayerSurfaceV1 as Proxy>::Event;

            match self.layer_surface.rx.get_mut().recv().unwrap() {
                Event::Configure { serial, .. } => break serial,

                _ => (),
            }
        };

        self.layer_surface.ack_configure(serial);
    }

    pub fn configure(&self) {
        use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::Anchor;

        self.layer_surface.set_anchor(Anchor::Top);
    }

    pub fn attach(&self, buffer: &WlBuffer) {
        self.surface.attach(Some(buffer), 0, 0);
    }

    pub fn redraw(&self) {
        self.surface.damage(0, 0, i32::MAX, i32::MAX);
        self.surface.commit();
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        self.layer_surface.set_size(width as u32, height as u32);
        self.layer_surface.set_exclusive_zone(height as i32);

        self.known.size.replace(Size { width, height });
    }

    pub fn commit(&self) {
        self.surface.commit();
    }
}

// impl Dispatch<...> for Out { ... } is located on bottom of source code

#[derive(Debug)]
struct Known {
    size: Option<Size>,
}

impl Known {
    fn size(&self) -> Size {
        self.size.unwrap_or_else(Size::zeroed)
    }
}

impl Default for Known {
    fn default() -> Self {
        let size = None;

        Self { size }
    }
}

#[derive(Debug, Clone, Copy)]
struct Size {
    width: usize,
    height: usize,
}

impl Size {
    fn zeroed() -> Self {
        Self {
            width: 0,
            height: 0,
        }
    }

    fn width<T: TryFrom<usize>>(&self) -> T {
        self.width.try_into().ok().unwrap()
    }

    fn height<T: TryFrom<usize>>(&self) -> T {
        self.height.try_into().ok().unwrap()
    }
}

mod _dp {
    use super::{Out, WlOutput, WlSurface, ZwlrLayerSurfaceV1};

    use wayland_client::Connection;
    use wayland_client::Dispatch;
    use wayland_client::Proxy;
    use wayland_client::QueueHandle;

    macro dispatch($si:ident: $s:ident ($ei:ident: $e:ident) <= $pi:ident: $p:ident $block:block) {
        impl Dispatch<$p, ()> for $s {
            fn event(
                $si: &mut $s,
                $pi: &$p,
                $ei: <$p as Proxy>::Event,
                (): &(),
                _: &Connection,
                _: &QueueHandle<$s>,
            ) {
                type $e = <$p as Proxy>::Event;
                dbg!(&$ei);
                $block
            }
        }
    }

    dispatch! { s: Out (e: Event) <= p: WlOutput {
        assert_eq!(p.id(), s.output.id());

        match e {
            Event::Mode { width, height, .. } => {
                s.known.size = Some(super::Size {
                    width: width as usize,
                    height: height as usize,
                });
            }

            _ => (),
        }
    }}

    dispatch! { s: Out (e: Event) <= p: WlSurface {
        assert_eq!(p.id(), s.surface.id());

        match e {
            Event::Enter { output } | Event::Leave { output } => {
                assert_eq!(output.id(), s.output.id());
            }

            _ => (),
        }
    }}

    dispatch! { s: Out (e: Event) <= p: ZwlrLayerSurfaceV1 {
        assert_eq!(p.id(), s.layer_surface.id());

        match e {
            Event::Configure { serial, width, height } => {
                assert_eq!(width, s.known.size().width());
                assert_eq!(height, s.known.size().height());

                p.ack_configure(serial);
            }

            Event::Closed => {
                unimplemented!()
            },

            _ => (),
        }
    }}
}
