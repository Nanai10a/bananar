use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_shm::WlShm;
use wayland_client::EventQueue;
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::ZwlrLayerShellV1;

use crate::reg::Registry;
use crate::reg::Stub;

pub fn init(mut req: EventQueue<Registry>, mut seq: EventQueue<Stub>) {
    let mut reg = crate::reg::Registry::new();
    req.roundtrip(&mut reg).unwrap();

    let compositor = reg.pull_one::<WlCompositor>().unwrap();
    let shm = reg.pull_one::<WlShm>().unwrap();
    let layer_shell = reg.pull_one::<ZwlrLayerShellV1>().unwrap();

    let outputs = reg.pull_all::<WlOutput>();
}
