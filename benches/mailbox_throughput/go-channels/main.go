// Go comparator: buffered channel 1p/1c throughput, 10 000 msgs/iter.
package main

import (
	"flag"
	"fmt"
	"sort"
	"time"
)

func main() {
	iters := flag.Int("iters", 30, "samples")
	flag.Parse()
	const N = 10000
	samples := make([]time.Duration, 0, *iters)
	for k := 0; k < *iters; k++ {
		ch := make(chan int, N)
		done := make(chan struct{})
		go func() {
			for i := 0; i < N; i++ {
				ch <- i
			}
			close(done)
		}()
		t0 := time.Now()
		for i := 0; i < N; i++ {
			<-ch
		}
		samples = append(samples, time.Since(t0))
		<-done
	}
	sort.Slice(samples, func(i, j int) bool { return samples[i] < samples[j] })
	p := func(q float64) time.Duration {
		return samples[int(float64(len(samples)-1)*q+0.5)]
	}
	fmt.Printf(
		"go_channels_mailbox_throughput: median=%.3f ms  p95=%.3f ms  p99=%.3f ms  (%d msgs/iter)\n",
		float64(p(0.50).Nanoseconds())/1.0e6,
		float64(p(0.95).Nanoseconds())/1.0e6,
		float64(p(0.99).Nanoseconds())/1.0e6,
		N,
	)
}
