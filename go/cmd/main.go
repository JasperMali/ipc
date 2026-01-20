package main

import (
	"fmt"
	"os"
	"time"

	"github.com/spf13/pflag" // 更灵活的 flag 包
	"ipc/ipc"
)

func main() {
	// --------------------
	// 定义参数
	// --------------------
	mode := pflag.StringP("mode", "m", "produce", "Mode: produce | consume")
	id := pflag.IntP("id", "i", 0, "Producer/Consumer ID")
	msgCount := pflag.IntP("count", "c", 10, "Number of messages to produce")
	ipcPath := pflag.StringP("ipc", "f", "/tmp/shm_ipc.dat", "Shared memory file path")
	pflag.Parse()

	// 根据模式选择执行函数
	switch *mode {
	case "produce":
		produce(*id, *msgCount, *ipcPath)
	case "consume":
		consume(*id, *ipcPath)
	default:
		fmt.Printf("Unknown mode: %s\n", *mode)
		os.Exit(1)
	}
}

func produce(pid int, count int, path string) {
	// 尝试打开共享内存，如果不存在就创建
	ipc, err := shmipc.FromExisting(path)
	if err != nil {
		ipc, err = shmipc.NewIpc(path)
		if err != nil {
			panic(err)
		}
	}
	defer ipc.Close()

	for i := 0; i < count; i++ {
		msg := fmt.Sprintf("P%d-%d", pid, i)
		if err := ipc.Write([]byte(msg)); err != nil {
			fmt.Println("Write error:", err)
		}
		fmt.Printf("[Producer %d] Sent: %s\n", pid, msg)
		time.Sleep(30 * time.Millisecond)
	}

	// 每个生产者发送结束标志给所有消费者
	if err := ipc.Write([]byte("__END__")); err != nil {
		fmt.Println("Write end error:", err)
	}
	fmt.Printf("[Producer %d] Done\n", pid)
}

func consume(cid int, path string) {
	ipc, err := shmipc.FromExisting(path)
	if err != nil {
		fmt.Println("Cannot open shared memory:", err)
		return
	}
	defer ipc.Close()

	for {
		msg, ok := ipc.Read()
		if !ok {
			break
		}
		s := string(msg)
		fmt.Printf("[Consumer %d] Got: %s\n", cid, s)
		//if s == "__END__" {
		//	fmt.Printf("[Consumer %d] Exiting\n", cid)
		//	break
		//}
		time.Sleep(20 * time.Millisecond)
	}
}
