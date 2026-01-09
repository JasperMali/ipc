use std::cell::UnsafeCell;
use std::slice;
use std::sync::atomic::{AtomicUsize, Ordering};

use nix::sys::event::{
    kevent, kqueue, EventFilter, EventFlag, FilterFlag, KEvent, Kqueue,
};

pub const MAX_DATA: usize = 4096;
const KEVENT_IDENT: usize = 1;

#[repr(C)]
pub struct ShmIpc {
    pub write_lock: AtomicUsize,
    pub len: AtomicUsize,
    pub ready: AtomicUsize,
    pub data: UnsafeCell<[u8; MAX_DATA]>,
}

unsafe impl Sync for ShmIpc {}

pub struct Ipc {
    pub shm: &'static ShmIpc,
    kq: Kqueue,
}

pub struct ZeroCopyRead<'a> {
    pub data: &'a [u8],
    shm: &'a ShmIpc,
}

impl<'a> Drop for ZeroCopyRead<'a> {
    fn drop(&mut self) {
        // 读结束，释放写权限
        self.shm.ready.store(0, Ordering::Release);
    }
}


impl Ipc {
    pub fn new(shm: &'static ShmIpc) -> Self {
        let kq = kqueue().expect("kqueue failed");

        let kev = KEvent::new(
            KEVENT_IDENT,
            EventFilter::EVFILT_USER,
            EventFlag::EV_ADD | EventFlag::EV_CLEAR,
            FilterFlag::NOTE_FFNOP,
            0,
            0,
        );

        kevent(&kq, &[kev], &mut [], 0).expect("kevent register");

        Self { shm, kq }
    }

    pub fn write(&self, buf: &[u8]) {
        // 等 reader 消费完成
        while self.shm.ready.load(Ordering::Acquire) != 0 {
            std::thread::yield_now();
        }

        // 抢跨进程写锁
        while self
            .shm
            .write_lock
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            std::thread::yield_now();
        }

        let len = buf.len().min(MAX_DATA);

        unsafe {
            let ptr = (*self.shm.data.get()).as_mut_ptr();
            slice::from_raw_parts_mut(ptr, len).copy_from_slice(&buf[..len]);
        }

        self.shm.len.store(len, Ordering::Relaxed);

        self.shm.ready.store(1, Ordering::Release);

        self.shm.write_lock.store(0, Ordering::Release);

        let kev = KEvent::new(
            KEVENT_IDENT,
            EventFilter::EVFILT_USER,
            EventFlag::empty(),
            FilterFlag::NOTE_TRIGGER,
            0,
            0,
        );
        kevent(&self.kq, &[kev], &mut [], 0).unwrap();
    }


    pub fn read_blocking(&self) -> ZeroCopyRead<'_> {
        loop {
            let mut ev = [KEvent::new(
                0,
                EventFilter::EVFILT_USER,
                EventFlag::empty(),
                FilterFlag::empty(),
                0,
                0,
            )];

            kevent(&self.kq, &[], &mut ev, 0).unwrap();

            if self.shm.ready.load(Ordering::Acquire) == 0 {
                continue;
            }

            let len = self.shm.len.load(Ordering::Acquire);

            let slice = unsafe {
                let ptr = (*self.shm.data.get()).as_ptr();
                slice::from_raw_parts(ptr, len)
            };

            return ZeroCopyRead {
                data: slice,
                shm: self.shm,
            };
        }
    }

}
