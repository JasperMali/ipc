package main

/*
#include <pthread.h>
#include <stdlib.h>
#include <stdint.h>

#define RING_CAP (64*1024)

typedef struct {
    uint32_t WriteLock;
    uint32_t State;      // 0:empty, 1:data ready, 2:reading
    uint32_t Len;
    uint32_t Head;       // 写位置
    uint32_t Tail;       // 读位置
    char Data[RING_CAP];
    pthread_mutex_t Mutex;
    pthread_cond_t Cond;
} ShmIpc;

// 初始化共享 mutex 和 cond
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

// lock/unlock/wait/broadcast
static void lock(ShmIpc* shm) { pthread_mutex_lock(&shm->Mutex); }
static void unlock(ShmIpc* shm) { pthread_mutex_unlock(&shm->Mutex); }
static void wait_cond(ShmIpc* shm) { pthread_cond_wait(&shm->Cond, &shm->Mutex); }
static void broadcast(ShmIpc* shm) { pthread_cond_broadcast(&shm->Cond); }
*/
import "C"

import (
	"fmt"
	"os"
	"syscall"
	"unsafe"
)

const RING_CAP = 64 * 1024

type Ipc struct {
	shm *C.ShmIpc
}

// 打开共享内存
func OpenShm(path string) (*C.ShmIpc, []byte, bool, error) {
	firstInit := false
	fd, err := os.OpenFile(path, os.O_CREATE|os.O_RDWR, 0o600)
	if err != nil {
		return nil, nil, false, err
	}

	info, _ := fd.Stat()
	if info.Size() == 0 {
		firstInit = true
	}

	size := int(unsafe.Sizeof(C.ShmIpc{}))
	if err := fd.Truncate(int64(size)); err != nil {
		fd.Close()
		return nil, nil, false, err
	}
	fd.Close()

	// 重新以共享方式打开
	shmFd, err := syscall.Open(path, syscall.O_RDWR, 0o600)
	if err != nil {
		return nil, nil, false, err
	}

	data, err := syscall.Mmap(shmFd, 0, size,
		syscall.PROT_READ|syscall.PROT_WRITE, syscall.MAP_SHARED)
	if err != nil {
		syscall.Close(shmFd)
		return nil, nil, false, err
	}

	shm := (*C.ShmIpc)(unsafe.Pointer(&data[0]))
	return shm, data, firstInit, nil
}

// 构造 Ipc
func NewIpc(shm *C.ShmIpc, firstInit bool) *Ipc {
	if firstInit {
		// 初始化 mutex/cond 并清零状态
		C.init_mutex_cond(shm)
		shm.State = 0 // 0表示空，可以写
		shm.WriteLock = 0
		shm.Len = 0
		shm.Head = 0
		shm.Tail = 0
	}
	return &Ipc{shm: shm}
}

// 内部封装
func (ipc *Ipc) lock()      { C.lock(ipc.shm) }
func (ipc *Ipc) unlock()    { C.unlock(ipc.shm) }
func (ipc *Ipc) wait()      { C.wait_cond(ipc.shm) }
func (ipc *Ipc) broadcast() { C.broadcast(ipc.shm) }

// 写消息
func (ipc *Ipc) Write(msg []byte) {
	ipc.lock()
	defer ipc.unlock()

	// 等待空闲状态
	for ipc.shm.State != 0 {
		ipc.wait()
	}

	if len(msg) > RING_CAP {
		panic("message too large")
	}

	dst := (*[RING_CAP]byte)(unsafe.Pointer(&ipc.shm.Data[0]))[:len(msg):len(msg)]
	copy(dst, msg)

	ipc.shm.Len = C.uint32_t(len(msg))
	ipc.shm.State = 1 // 标记有数据
	ipc.broadcast()   // 唤醒消费者
}

// 读消息（简化的版本，实际应该使用环形缓冲区）
func (ipc *Ipc) Read() []byte {
	ipc.lock()
	defer ipc.unlock()

	// 等待有数据
	for ipc.shm.State != 1 {
		ipc.wait()
	}

	length := uint32(ipc.shm.Len)
	data := make([]byte, length)
	dst := (*[RING_CAP]byte)(unsafe.Pointer(&ipc.shm.Data[0]))[:length:length]
	copy(data, dst)

	ipc.shm.State = 0 // 标记空闲
	ipc.broadcast()   // 唤醒生产者
	return data
}

func main() {
	if len(os.Args) < 2 {
		fmt.Println("Usage: ./ipc [producer|consumer]")
		return
	}

	shm, _, firstInit, err := OpenShm("/tmp/ipc_cond_demo.dat")
	if err != nil {
		panic(err)
	}

	ipc := NewIpc(shm, firstInit)

	switch os.Args[1] {
	case "producer":
		for i := 0; i < 10; i++ {
			msg := fmt.Sprintf("hello-%d", i)
			fmt.Printf("[P] Writing: %s\n", msg)
			ipc.Write([]byte(msg))
		}
		fmt.Println("[P] Producer finished")
	case "consumer":
		for i := 0; i < 10; i++ {
			data := ipc.Read()
			fmt.Printf("[C] Received: %s\n", string(data))
		}
		fmt.Println("[C] Consumer finished")
	default:
		fmt.Println("Invalid argument. Use 'producer' or 'consumer'")
	}
}
