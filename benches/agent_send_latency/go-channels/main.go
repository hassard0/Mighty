// Go comparator: unbuffered channel send/recv between two goroutines.
//
// Usage: go run main.go --iters 1000
package main

import (
	"flag"
	"fmt"
	"sort"
	"time"
)

func main() {
	iters := flag.Int("iters", 1000, "samples")
	flag.Parse()
	samples := make([]time.Duration, 0, *iters)
	for k := 0; k < *iters; k++ {
		ch := make(chan int, 8)
		t0 := time.Now()
		ch <- 1
		<-ch
		samples = append(samples, time.Since(t0))
	}
	sort.Slice(samples, func(i, j int) bool { return samples[i] < samples[j] })
	p := func(q float64) time.Duration {
		return samples[int(float64(len(samples)-1)*q+0.5)]
	}
	fmt.Printf(
		"go_channels_agent_send_latency: median=%.3f ms  p95=%.3f ms  p99=%.3f ms\n",
		float64(p(0.50).Nanoseconds())/1.0e6,
		float64(p(0.95).Nanoseconds())/1.0e6,
		float64(p(0.99).Nanoseconds())/1.0e6,
	)
}
