package shmipc

/*
#include <pthread.h>
#include <stdlib.h>
#include <stdint.h>

#define RING_CAP (64*1024)
#define HEADER   4

typedef struct {
    uint32_t Head;
    uint32_t Tail;
    char Data[RING_CAP];
    pthread_mutex_t Mutex;
    pthread_cond_t Cond;
} ShmIpc;

static void init_mutex_cond(ShmIpc* shm) {
    pthread_mutexattr_t mattr;
    pthread_condattr_t cattr;

    pthread_mutexattr_init(&mattr);
    pthread_mutexattr_setpshared(&mattr, PTHREAD_PROCESS_SHARED);
    pthread_mutex_init(&shm->Mutex, &mattr);

    pthread_condattr_init(&cattr);
    pthread_condattr_setpshared(&cattr, PTHREAD_PROCESS_SHARED);
    pthread_cond_init(&shm->Cond, &cattr);
}

static void lock(ShmIpc* shm)      { pthread_mutex_lock(&shm->Mutex); }
static void unlock(ShmIpc* shm)    { pthread_mutex_unlock(&shm->Mutex); }
static void wait_cond(ShmIpc* shm) { pthread_cond_wait(&shm->Cond, &shm->Mutex); }
static void broadcast(ShmIpc* shm) { pthread_cond_broadcast(&shm->Cond); }
*/
import "C"

import (
	"encoding/binary"
	"errors"
	"os"
	"sync/atomic"
	"syscall"
	"time"
	"unsafe"
)

const (
	RING_CAP = 64 * 1024
	HEADER   = 4
)

type Ipc struct {
	shm    *C.ShmIpc
	data   []byte
	fd     int
	closed bool
}

func NewIpc(path string) (*Ipc, error) {
	fd, err := os.OpenFile(path, os.O_CREATE|os.O_RDWR|os.O_TRUNC, 0o600)
	if err != nil {
		return nil, err
	}
	defer fd.Close()

	info, _ := fd.Stat()
	firstInit := info.Size() == 0

	size := int(unsafe.Sizeof(C.ShmIpc{}))
	if err := fd.Truncate(int64(size)); err != nil {
		return nil, err
	}

	shmFd, err := syscall.Open(path, syscall.O_RDWR, 0o600)
	if err != nil {
		return nil, err
	}

	data, err := syscall.Mmap(shmFd, 0, size,
		syscall.PROT_READ|syscall.PROT_WRITE, syscall.MAP_SHARED)
	if err != nil {
		syscall.Close(shmFd)
		return nil, err
	}

	shm := (*C.ShmIpc)(unsafe.Pointer(&data[0]))

	if firstInit {
		C.init_mutex_cond(shm)
		shm.Head = 0
		shm.Tail = 0
	}

	return &Ipc{
		shm:  shm,
		data: data,
		fd:   shmFd,
	}, nil
}

// FromExisting 打开已有共享内存文件，而不重新初始化
func FromExisting(path string) (*Ipc, error) {
	size := int(unsafe.Sizeof(C.ShmIpc{}))

	fd, err := os.OpenFile(path, os.O_RDWR, 0o600) // 只打开，不创建
	if err != nil {
		return nil, err
	}
	defer fd.Close()

	data, err := syscall.Mmap(int(fd.Fd()), 0, size,
		syscall.PROT_READ|syscall.PROT_WRITE, syscall.MAP_SHARED)
	if err != nil {
		return nil, err
	}

	shm := (*C.ShmIpc)(unsafe.Pointer(&data[0]))

	return &Ipc{
		shm:  shm,
		data: data,
		fd:   -1, // 已关闭 fd
	}, nil
}

func (ipc *Ipc) Close() error {
	ipc.lock()
	ipc.closed = true
	ipc.wakeup() // 唤醒所有阻塞的消费者
	ipc.unlock()

	time.Sleep(10 * time.Millisecond)

	if ipc.data != nil {
		syscall.Munmap(ipc.data)
	}
	if ipc.fd > 0 {
		syscall.Close(ipc.fd)
	}

	return nil
}

func (ipc *Ipc) lock()   { C.lock(ipc.shm) }
func (ipc *Ipc) unlock() { C.unlock(ipc.shm) }
func (ipc *Ipc) wait()   { C.wait_cond(ipc.shm) }
func (ipc *Ipc) wakeup() { C.broadcast(ipc.shm) }

// 使用原子操作安全地读取指针位置
func (ipc *Ipc) atomicLoadHead() uint32 {
	return uint32(atomic.LoadUint32((*uint32)(unsafe.Pointer(&ipc.shm.Head))))
}

func (ipc *Ipc) atomicLoadTail() uint32 {
	return uint32(atomic.LoadUint32((*uint32)(unsafe.Pointer(&ipc.shm.Tail))))
}

// 计算可用空间（修正版本）
func (ipc *Ipc) freeSpace() int {
	head := ipc.atomicLoadHead()
	tail := ipc.atomicLoadTail()

	// 环形缓冲区空间计算
	if head >= tail {
		return RING_CAP - int(head-tail) - 1
	}
	return int(tail-head) - 1
}

// 计算可读数据（修正版本）
func (ipc *Ipc) dataAvailable() int {
	head := ipc.atomicLoadHead()
	tail := ipc.atomicLoadTail()

	if head >= tail {
		return int(head - tail)
	}
	return RING_CAP - int(tail-head)
}

func (ipc *Ipc) Write(msg []byte) error {
	if len(msg) > RING_CAP-HEADER-1 {
		panic("message too large")
	}

	ipc.lock()
	defer ipc.unlock()

	needed := len(msg) + HEADER

	// 等待有足够空间
	for ipc.freeSpace() < needed {
		if ipc.closed {
			return errors.New("closed ipc")
		}
		ipc.wait()
	}

	head := uint32(ipc.shm.Head)
	buf := (*[RING_CAP]byte)(unsafe.Pointer(&ipc.shm.Data[0]))

	// 写入消息长度
	lengthBytes := make([]byte, 4)
	binary.LittleEndian.PutUint32(lengthBytes, uint32(len(msg)))

	// 写入长度头（考虑环绕）
	for i := 0; i < HEADER; i++ {
		buf[(head+uint32(i))%RING_CAP] = lengthBytes[i]
	}
	head = (head + HEADER) % RING_CAP

	// 写入消息数据（考虑环绕）
	for i := 0; i < len(msg); i++ {
		buf[(head+uint32(i))%RING_CAP] = msg[i]
	}
	head = (head + uint32(len(msg))) % RING_CAP

	// 原子更新Head指针
	atomic.StoreUint32((*uint32)(unsafe.Pointer(&ipc.shm.Head)), uint32(head))
	ipc.wakeup()
	return nil
}

func (ipc *Ipc) Read() ([]byte, bool) {
	ipc.lock()
	defer ipc.unlock()

	// 等待有足够数据读取（至少HEADER字节）
	for ipc.dataAvailable() < HEADER {
		if ipc.closed {
			return nil, false
		}
		ipc.wait()
	}

	tail := uint32(ipc.shm.Tail)
	buf := (*[RING_CAP]byte)(unsafe.Pointer(&ipc.shm.Data[0]))

	// 读取消息长度
	lengthBytes := make([]byte, HEADER)
	for i := 0; i < HEADER; i++ {
		lengthBytes[i] = buf[(tail+uint32(i))%RING_CAP]
	}
	length := binary.LittleEndian.Uint32(lengthBytes)

	// 验证消息长度有效性
	if length > RING_CAP-HEADER {
		// 数据损坏，重置缓冲区
		ipc.shm.Head = 0
		ipc.shm.Tail = 0
		return []byte{}, false
	}

	tail = (tail + HEADER) % RING_CAP

	// 等待完整消息可用
	for ipc.dataAvailable() < int(length) {
		if ipc.closed {
			return nil, false
		}
		ipc.wait()
	}

	// 读取消息数据
	data := make([]byte, length)
	for i := 0; i < int(length); i++ {
		data[i] = buf[(tail+uint32(i))%RING_CAP]
	}
	tail = (tail + uint32(length)) % RING_CAP

	// 原子更新Tail指针
	atomic.StoreUint32((*uint32)(unsafe.Pointer(&ipc.shm.Tail)), uint32(tail))
	ipc.wakeup()
	return data, true
}

// 验证并修复环形缓冲区状态
func validateAndFixRingBuffer(shm *C.ShmIpc) {
	head := uint32(shm.Head)
	tail := uint32(shm.Tail)

	// 基本验证：Head 和 Tail 必须在合理范围内
	if head >= RING_CAP || tail >= RING_CAP {
		// 指针越界，重置缓冲区
		shm.Head = 0
		shm.Tail = 0
		return
	}

	// 检查是否有未完成的写操作（消息长度头可能损坏）
	// 我们无法完美恢复，所以保守策略是重置
	if head != tail {
		// 尝试读取第一条消息的长度
		buf := (*[RING_CAP]byte)(unsafe.Pointer(&shm.Data[0]))
		if tail+HEADER <= RING_CAP {
			// 读取长度头
			var length uint32
			for i := 0; i < HEADER; i++ {
				length |= uint32(buf[(tail+uint32(i))%RING_CAP]) << (8 * i)
			}

			// 验证长度是否合理
			if length > RING_CAP-HEADER {
				// 长度不合理，重置缓冲区
				shm.Head = 0
				shm.Tail = 0
			} else {
				// 计算消息总长度
				totalLength := HEADER + length
				newTail := (tail + totalLength) % RING_CAP

				// 如果新Tail等于Head，说明这是一条完整的消息
				// 否则，状态不一致，重置
				if newTail != head {
					// 状态不一致，重置缓冲区
					shm.Head = 0
					shm.Tail = 0
				}
			}
		}
	}
}
