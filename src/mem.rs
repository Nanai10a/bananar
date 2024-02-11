use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_shm::Format;
use wayland_client::protocol::wl_shm::WlShm;
use wayland_client::protocol::wl_shm_pool::WlShmPool;

use alloc::sync::Arc;
use alloc::sync::Weak;
use core::ops::Deref;
use core::ops::DerefMut;
use std::os::fd::OwnedFd;
use std::sync::RwLock;

use crate::reg::Stub;
use crate::reg::WithRx;

#[derive(Debug)]
pub struct Mem {
    shm: WithRx<WlShm>,
    shm_pool: WithRx<WlShmPool>,
    buffer: Option<WithRx<WlBuffer>>,
    shared: Arc<RwLock<Shared>>,
}

impl Mem {
    pub fn new(shm: WithRx<WlShm>, size: Size, qh: &wayland_client::QueueHandle<Stub>) -> Self {
        use std::os::fd::AsFd as _;

        // HACK: allocate 1 [MB]
        let len = 1024 * 1024 * 1024;

        let shared = Shared::open(len as usize, size);
        let shm_pool = {
            let (tx, rx) = std::sync::mpsc::channel();
            let p = shm.create_pool(shared.fd.as_fd(), len as i32, qh, tx);

            WithRx::new(p, rx)
        };

        let buffer = None;
        let shared = Arc::new(RwLock::new(shared));

        Self {
            shm,
            shm_pool,
            buffer,
            shared,
        }
    }

    pub fn share_allocation(&self) -> Weak<RwLock<Shared>> {
        Arc::downgrade(&self.shared)
    }

    pub fn make_buffer(
        &mut self,
        width: i32,
        height: i32,
        stride: i32,
        format: Format,
        qh: &wayland_client::QueueHandle<Stub>,
    ) -> &WlBuffer {
        self.buffer.take().map(|buffer| buffer.destroy());

        let buffer = {
            let (tx, rx) = std::sync::mpsc::channel();
            let p = self
                .shm_pool
                .create_buffer(0, width, height, stride, format, qh, tx);

            WithRx::new(p, rx)
        };

        self.buffer.insert(buffer)
    }
}

#[derive(Debug)]
pub struct Shared {
    fd: OwnedFd,
    ptr: *mut (),
    len: usize,
    size: Size,
}

impl Shared {
    fn open(len: usize, size: Size) -> Self {
        let name = {
            let ts = std::time::SystemTime::now().elapsed().unwrap().as_nanos();

            format!("/wl-shm-{ts:32x}")
        };

        let fd = {
            use nix::fcntl::OFlag;
            use nix::sys::stat::Mode;

            let flag = OFlag::O_RDWR | OFlag::O_CREAT | OFlag::O_EXCL;
            let mode = Mode::S_IRUSR | Mode::S_IWUSR | Mode::S_IXUSR;

            nix::sys::mman::shm_open(&*name, flag, mode).unwrap()
        };

        nix::sys::mman::shm_unlink(&*name).unwrap();
        nix::unistd::ftruncate(&fd, len as i64).unwrap();

        Self::map(fd, len, size)
    }

    fn map(fd: OwnedFd, len: usize, size: Size) -> Self {
        let ptr = unsafe {
            use nix::sys::mman::MapFlags;
            use nix::sys::mman::ProtFlags;

            let len = len.try_into().unwrap();

            let pflag = ProtFlags::PROT_READ | ProtFlags::PROT_WRITE;
            let mflag = MapFlags::MAP_SHARED;

            nix::sys::mman::mmap(None, len, pflag, mflag, Some(&fd), 0)
                .unwrap()
                .cast()
        };

        Self { fd, ptr, len, size }
    }

    fn as_bytes(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.ptr.cast(), self.len) }
    }

    fn as_bytes_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.ptr.cast(), self.len) }
    }

    pub fn size(&self) -> &Size {
        &self.size
    }

    unsafe fn unmap(&self) {
        nix::sys::mman::munmap(self.ptr.cast(), self.len).unwrap();
    }
}

impl Deref for Shared {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_bytes()
    }
}

impl DerefMut for Shared {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_bytes_mut()
    }
}

impl Drop for Shared {
    fn drop(&mut self) {
        unsafe { self.unmap() }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Size {
    pub width: usize,
    pub height: usize,
    pub pixel_size: usize,
}

impl Size {
    pub fn new(width: usize, height: usize, pixel_size: usize) -> Self {
        Self {
            width,
            height,
            pixel_size,
        }
    }
}

mod _dp {
    use super::{Mem, WlBuffer, WlShm, WlShmPool};

    wayland_client::delegate_noop! { Mem: ignore WlShm     }
    wayland_client::delegate_noop! { Mem: ignore WlShmPool }
    wayland_client::delegate_noop! { Mem: ignore WlBuffer  }
}
