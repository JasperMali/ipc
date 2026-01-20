use std::fs::OpenOptions;
use std::mem::size_of;
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicU32, Ordering};

use libc::{mmap, munmap, off_t, pthread_cond_broadcast, pthread_cond_t, pthread_cond_init,
           pthread_cond_wait, pthread_condattr_init, pthread_condattr_setpshared,
           pthread_condattr_t, pthread_mutex_init, pthread_mutex_lock, pthread_mutex_t,
           pthread_mutex_unlock, pthread_mutexattr_init, pthread_mutexattr_setpshared, pthread_mutexattr_t,
           MAP_FAILED, MAP_SHARED, PROT_READ, PROT_WRITE, PTHREAD_PROCESS_SHARED};

const RING_CAP: usize = 64 * 1024;
const HEADER: usize = 4;

#[repr(C)]
pub struct ShmIpc {
    pub head: AtomicU32,
    pub tail: AtomicU32,
    pub data: [u8; RING_CAP],
    pub mutex: pthread_mutex_t,
    pub cond: pthread_cond_t,
    pub closed: AtomicU32,
}

impl ShmIpc {
    pub fn init(&mut self) {
        unsafe {
            let mut mattr: pthread_mutexattr_t = std::mem::zeroed();
            pthread_mutexattr_init(&mut mattr);
            pthread_mutexattr_setpshared(&mut mattr, PTHREAD_PROCESS_SHARED);
            pthread_mutex_init(&mut self.mutex, &mattr);

            let mut cattr: pthread_condattr_t = std::mem::zeroed();
            pthread_condattr_init(&mut cattr);
            pthread_condattr_setpshared(&mut cattr, PTHREAD_PROCESS_SHARED);
            pthread_cond_init(&mut self.cond, &cattr);
        }
        self.head.store(0, Ordering::SeqCst);
        self.tail.store(0, Ordering::SeqCst);
        self.closed.store(0, Ordering::SeqCst);
    }

    unsafe fn lock(&mut self) { pthread_mutex_lock(&mut self.mutex); }
    unsafe fn unlock(&mut self) { pthread_mutex_unlock(&mut self.mutex); }
    unsafe fn wait(&mut self) { pthread_cond_wait(&mut self.cond, &mut self.mutex); }
    unsafe fn wakeup(&mut self) { pthread_cond_broadcast(&mut self.cond); }

    fn free_space(&self) -> usize {
        let head = self.head.load(Ordering::SeqCst);
        let tail = self.tail.load(Ordering::SeqCst);
        if head >= tail { RING_CAP - (head - tail) as usize - 1 } else { (tail - head) as usize - 1 }
    }

    fn data_available(&self) -> usize {
        let head = self.head.load(Ordering::SeqCst);
        let tail = self.tail.load(Ordering::SeqCst);
        if head >= tail { (head - tail) as usize } else { RING_CAP - (tail - head) as usize }
    }

    pub fn write(&mut self, msg: &[u8]) {
        if msg.len() > RING_CAP - HEADER - 1 { panic!("message too large"); }
        unsafe { self.lock(); }
        while self.free_space() < msg.len() + HEADER {
            if self.closed.load(Ordering::SeqCst) != 0 { break; }
            unsafe { self.wait(); }
        }
        let mut head = self.head.load(Ordering::SeqCst) as usize;

        for i in 0..HEADER {
            self.data[(head + i) % RING_CAP] = (msg.len() as u32 >> (i * 8)) as u8;
        }
        head = (head + HEADER) % RING_CAP;

        for i in 0..msg.len() {
            self.data[(head + i) % RING_CAP] = msg[i];
        }
        head = (head + msg.len()) % RING_CAP;

        self.head.store(head as u32, Ordering::SeqCst);
        unsafe { self.wakeup(); self.unlock(); }
    }

    pub fn read(&mut self) -> Vec<u8> {
        unsafe { self.lock(); }
        while self.data_available() < HEADER {
            if self.closed.load(Ordering::SeqCst) != 0 { break; }
            unsafe { self.wait(); }
        }
        let mut tail = self.tail.load(Ordering::SeqCst) as usize;
        let mut length: usize = 0;
        for i in 0..HEADER {
            length |= (self.data[(tail + i) % RING_CAP] as usize) << (i * 8);
        }
        tail = (tail + HEADER) % RING_CAP;

        while self.data_available() < length {
            if self.closed.load(Ordering::SeqCst) != 0 { break; }
            unsafe { self.wait(); }
        }

        let mut msg = vec![0u8; length];
        for i in 0..length {
            msg[i] = self.data[(tail + i) % RING_CAP];
        }
        tail = (tail + length) % RING_CAP;
        self.tail.store(tail as u32, Ordering::SeqCst);

        unsafe { self.wakeup(); self.unlock(); }
        msg
    }

    pub fn close(&mut self) {
        unsafe { self.closed.store(1, Ordering::SeqCst); self.wakeup(); }
    }
}

pub struct Ipc {
    pub shm: *mut ShmIpc,
    pub mmap_len: usize,
}

unsafe impl Send for Ipc {}
unsafe impl Sync for Ipc {}

impl Ipc {
    pub fn new(path: &str) -> Self {
        let size = size_of::<ShmIpc>();
        let file = OpenOptions::new().read(true).write(true).create(true).open(path).unwrap();
        let first_init = file.metadata().unwrap().len() == 0;
        if first_init { file.set_len(size as u64).unwrap(); }

        let shm_ptr = unsafe {
            let ptr = mmap(
                std::ptr::null_mut(),
                size,
                PROT_READ | PROT_WRITE,
                MAP_SHARED,
                file.as_raw_fd(),
                0 as off_t,
            );
            if ptr == libc::MAP_FAILED { panic!("mmap failed"); }
            ptr as *mut ShmIpc
        };

        if first_init {
            unsafe { (*shm_ptr).init(); }
        }

        Ipc { shm: shm_ptr, mmap_len: size }
    }

    pub fn from_existing(path: &str) -> Self {
        let size = std::mem::size_of::<ShmIpc>();
        // 如果文件不存在就创建
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)  // 关键
            .open(path)
            .unwrap();

        let first_init = file.metadata().unwrap().len() == 0;
        if first_init {
            file.set_len(size as u64).unwrap();
        }

        let shm_ptr = unsafe {
            let ptr = libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            );
            if ptr == libc::MAP_FAILED {
                panic!("mmap failed");
            }
            ptr as *mut ShmIpc
        };

        if first_init {
            unsafe { (*shm_ptr).init(); }
        }

        Ipc { shm: shm_ptr, mmap_len: size }
    }


    pub fn write(&mut self, msg: &[u8]) { unsafe { (*self.shm).write(msg); } }
    pub fn read(&mut self) -> Vec<u8> { unsafe { (*self.shm).read() } }
    pub fn close(&mut self) { unsafe { (*self.shm).close(); } }
}

impl Drop for Ipc {
    fn drop(&mut self) {
        unsafe { munmap(self.shm as *mut _, self.mmap_len); }
    }
}
