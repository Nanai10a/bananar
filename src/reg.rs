use core::any::Any;
use core::ops::Deref;
use core::sync::Exclusive;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;
use std::sync::mpsc::TryRecvError;

use wayland_client::Connection;
use wayland_client::Dispatch;
use wayland_client::Proxy;
use wayland_client::QueueHandle;

use wayland_client::protocol::wl_registry::WlRegistry;

#[derive(Debug)]
pub struct Registry {
    inner: Vec<Box<dyn Any + Sync + Send>>,
}

impl Registry {
    pub fn new() -> Self {
        Self { inner: Vec::new() }
    }

    pub fn push<T: Any + Sync + Send + Clone + Proxy>(&mut self, wr: WithRx<T>)
    where
        <T as Proxy>::Event: Sync + Send,
    {
        self.inner.push(Box::new(wr))
    }

    pub fn pull_one<T: Any + Sync + Send + Clone + Proxy>(&mut self) -> Option<WithRx<T>>
    where
        <T as Proxy>::Event: Sync + Send,
    {
        let idx = self
            .inner
            .iter()
            .enumerate()
            .filter_map(|(idx, a)| a.downcast_ref::<WithRx<T>>().map(|_| idx))
            .collect::<OnlyOne<_>>()
            .into_option()?;

        self.inner.remove(idx).downcast().ok().map(Box::into_inner)
    }

    pub fn pull_all<T: Any + Sync + Send + Clone + Proxy>(&mut self) -> Vec<WithRx<T>>
    where
        <T as Proxy>::Event: Sync + Send,
    {
        let idxes = self
            .inner
            .iter()
            .enumerate()
            .filter_map(|(idx, a)| a.downcast_ref::<WithRx<T>>().map(|_| idx))
            .collect::<Vec<_>>();

        idxes
            .into_iter()
            .filter_map(|idx| self.inner.remove(idx).downcast().ok().map(Box::into_inner))
            .collect()
    }
}

impl Dispatch<WlRegistry, QueueHandle<Stub>> for Registry {
    fn event(
        s: &mut Self,
        p: &WlRegistry,
        e: <WlRegistry as Proxy>::Event,
        u: &QueueHandle<Stub>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        type Event = <WlRegistry as Proxy>::Event;

        macro bind($reg:expr, $udata:expr, $state:expr, $name:expr, $version:expr, $interface:expr => $(($p:path))*) {{
            $(
                if <$p as Proxy>::interface().name == $interface {
                    let (tx, rx) = std::sync::mpsc::channel();

                    $state.push::<$p>(WithRx::new($reg.bind($name, $version, $udata, tx), rx));
                }
            )*
        }}

        match e {
            Event::Global {
                name,
                version,
                interface,
            } => bind! {
                p, u, s, name, version, interface =>
                    (wayland_client::protocol::wl_compositor :: WlCompositor)
                    (wayland_client::protocol::wl_output     :: WlOutput    )
                    (wayland_client::protocol::wl_shm        :: WlShm       )

                    (wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1 :: ZwlrLayerShellV1)
            },

            Event::GlobalRemove { name } => {
                unimplemented!("name = {name}");
            }

            _ => unreachable!(),
        }
    }
}

pub struct Stub;

impl<I: Proxy> Dispatch<I, Sender<<I as Proxy>::Event>> for Stub
where
    I::Event: core::fmt::Debug,
{
    fn event(
        _: &mut Self,
        _: &I,
        e: <I as Proxy>::Event,
        u: &Sender<<I as Proxy>::Event>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        dbg!(&e);
        u.send(e).unwrap()
    }
}

#[derive(Debug)]
pub struct WithRx<I: Proxy>
where
    <I as Proxy>::Event: Sync + Send,
{
    proxy: I,
    pub rx: Exclusive<Receiver<I::Event>>,
}

impl<I: Proxy> WithRx<I>
where
    <I as Proxy>::Event: Sync + Send,
{
    pub fn new(proxy: I, rx: Receiver<I::Event>) -> Self {
        let rx = Exclusive::new(rx);

        Self { proxy, rx }
    }

    pub fn try_recv_event(&mut self) -> Option<I::Event> {
        match self.rx.get_mut().try_recv() {
            Ok(e) => Some(e),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => panic!(),
        }
    }
}

impl<I: Proxy> Deref for WithRx<I>
where
    <I as Proxy>::Event: Sync + Send,
{
    type Target = I;

    fn deref(&self) -> &Self::Target {
        &self.proxy
    }
}

pub struct OnlyOne<T>(Option<T>);

impl<T> OnlyOne<T> {
    fn into_option(self) -> Option<T> {
        self.0
    }
}

impl<A> FromIterator<A> for OnlyOne<A> {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = A>,
    {
        let mut iter = iter.into_iter();

        let Some(t) = iter.next() else {
            return OnlyOne(None);
        };

        let None = iter.next() else {
            return OnlyOne(None);
        };

        OnlyOne(Some(t))
    }
}
