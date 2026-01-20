package main

import (
	"fmt"
	shmipc "ipc/ipc"
	"math/rand"
	"sync"
	"time"
)

func main() {
	rand.Seed(time.Now().UnixNano())

	ipc, err := shmipc.NewIpc("/tmp/ipc_mpmc_perf.dat")
	if err != nil {
		panic(err)
	}
	defer ipc.Close()

	const (
		Producers       = 5
		Consumers       = 6
		MessagesPerProd = 1000
		MaxMsgLen       = 32 * 1024
		EndMsg          = "__END__"
	)

	var wgProd sync.WaitGroup
	var wgCons sync.WaitGroup
	var producedCount int
	var consumedCount int
	var mu sync.Mutex

	start := time.Now()

	// --------------------
	// 启动消费者
	// --------------------
	for c := 0; c < Consumers; c++ {
		wgCons.Add(1)
		go func(cid int) {
			defer wgCons.Done()
			for {
				data, ok := ipc.Read()
				if !ok {
					// IPC 已关闭
					fmt.Printf("[C%d] exiting (IPC closed)\n", cid)
					return
				}
				if string(data) == EndMsg {
					fmt.Printf("[C%d] exiting\n", cid)
					return
				}
				mu.Lock()
				consumedCount++
				mu.Unlock()
			}
		}(c)
	}

	// --------------------
	// 启动生产者
	// --------------------
	for p := 0; p < Producers; p++ {
		wgProd.Add(1)
		go func(pid int) {
			defer wgProd.Done()
			for i := 0; i < MessagesPerProd; i++ {
				msgLen := rand.Intn(MaxMsgLen) + 1
				msg := make([]byte, msgLen)
				for j := range msg {
					msg[j] = byte('A' + rand.Intn(26))
				}
				ipc.Write(msg)

				mu.Lock()
				producedCount++
				mu.Unlock()
			}
		}(p)
	}

	// 等待所有生产者完成，再统一发送结束信号
	go func() {
		wgProd.Wait()
		for c := 0; c < Consumers; c++ {
			ipc.Write([]byte(EndMsg))
		}
	}()

	// 等待所有消费者完成
	wgCons.Wait()

	elapsed := time.Since(start)
	fmt.Printf("All messages produced: %d, consumed: %d, elapsed: %s\n",
		producedCount, consumedCount, elapsed)
	fmt.Printf("Throughput: %.2f msg/sec\n", float64(consumedCount)/elapsed.Seconds())
}
