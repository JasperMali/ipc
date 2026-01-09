use std::cell::UnsafeCell;
use std::ptr;
use std::slice;
use std::fs::OpenOptions;
use std::os::unix::io::AsRawFd;

use libc::{c_int, pthread_cond_t, pthread_condattr_t, pthread_mutex_t, pthread_mutexattr_t};
use libc::{PTHREAD_PROCESS_SHARED};

pub const RING_CAP: usize = 64 * 1024;

#[repr(C)]
pub struct ShmIpc {
    pub state: UnsafeCell<u32>,    // 0 = idle, 1 = written, 2 = reading
    pub len: UnsafeCell<u32>,      // 消息长度
    pub data: UnsafeCell<[u8; RING_CAP]>, // 数据
    pub mutex: pthread_mutex_t,
    pub cond: pthread_cond_t,
}

unsafe impl Sync for ShmIpc {}

pub struct Ipc {
    shm: &'static ShmIpc,
}

pub struct ZeroCopyRead<'a> {
    pub data: &'a [u8],
    shm: &'a ShmIpc,
}

impl Ipc {
    pub fn new(shm: &'static ShmIpc, first_init: bool) -> Self {
        if first_init {
            unsafe {
                // 初始化 mutex
                let mut mattr: pthread_mutexattr_t = std::mem::zeroed();
                libc::pthread_mutexattr_init(&mut mattr);
                libc::pthread_mutexattr_setpshared(&mut mattr, PTHREAD_PROCESS_SHARED);
                libc::pthread_mutex_init(&mut (*(shm as *const _ as *mut ShmIpc)).mutex, &mattr);

                // 初始化 cond
                let mut cattr: pthread_condattr_t = std::mem::zeroed();
                libc::pthread_condattr_init(&mut cattr);
                libc::pthread_condattr_setpshared(&mut cattr, PTHREAD_PROCESS_SHARED);
                libc::pthread_cond_init(&mut (*(shm as *const _ as *mut ShmIpc)).cond, &cattr);

                // 初始化状态
                *shm.state.get() = 0;
                *shm.len.get() = 0;
            }
        }

        Ipc { shm }
    }

    fn lock(&self) {
        unsafe { libc::pthread_mutex_lock(&self.shm.mutex as *const _ as *mut _) };
    }

    fn unlock(&self) {
        unsafe { libc::pthread_mutex_unlock(&self.shm.mutex as *const _ as *mut _) };
    }

    fn wait(&self) {
        unsafe { libc::pthread_cond_wait(&self.shm.cond as *const _ as *mut _, &self.shm.mutex as *const _ as *mut _) };
    }

    fn broadcast(&self) {
        unsafe { libc::pthread_cond_broadcast(&self.shm.cond as *const _ as *mut _) };
    }

    pub fn write(&self, buf: &[u8]) {
        assert!(buf.len() <= RING_CAP, "message too large");

        self.lock();
        unsafe {
            while *self.shm.state.get() != 0 {
                self.wait();
            }

            ptr::copy_nonoverlapping(buf.as_ptr(), (*self.shm.data.get()).as_mut_ptr(), buf.len());
            *self.shm.len.get() = buf.len() as u32;
        }
        self.unlock();
    }

    pub fn write_done(&self) {
        self.lock();
        unsafe {
            *self.shm.state.get() = 1; // written
            self.broadcast();
        }
        self.unlock();
    }

    pub fn read(&self) -> ZeroCopyRead<'_> {
        self.lock();
        unsafe {
            while *self.shm.state.get() != 1 {
                self.wait();
            }
            *self.shm.state.get() = 2; // reading
            let len = *self.shm.len.get() as usize;
            let slice = slice::from_raw_parts((*self.shm.data.get()).as_ptr(), len);
            self.unlock();
            ZeroCopyRead {
                data: slice,
                shm: self.shm,
            }
        }
    }

    pub fn read_done(&self) {
        self.lock();
        unsafe {
            *self.shm.state.get() = 0; // idle
            self.broadcast();
        }
        self.unlock();
    }
}

/// 打开共享内存（示例用文件映射）
pub fn open_shm(path: &str) -> (&'static mut ShmIpc, bool) {
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;
    use libc::{ftruncate, mmap, MAP_SHARED, PROT_READ, PROT_WRITE, MAP_FAILED};

    let first_init = !std::path::Path::new(path).exists();

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .open(path)
        .unwrap();

    let size = std::mem::size_of::<ShmIpc>();
    unsafe { ftruncate(file.as_raw_fd(), size as i64) };

    let ptr = unsafe {
        mmap(
            std::ptr::null_mut(),
            size,
            PROT_READ | PROT_WRITE,
            MAP_SHARED,
            file.as_raw_fd(),
            0,
        )
    };
    if ptr == MAP_FAILED {
        panic!("mmap failed");
    }

    let shm = unsafe { &mut *(ptr as *mut ShmIpc) };
    (shm, first_init)
}
