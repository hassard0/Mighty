// Go stdlib net/http server comparator.
package main

import (
	"flag"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"sort"
	"time"
)

func main() {
	iters := flag.Int("iters", 30, "samples")
	flag.Parse()
	srv := httptest.NewUnstartedServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(200)
		_, _ = io.WriteString(w, "ok")
	}))
	srv.Start()
	defer srv.Close()
	addr := srv.URL[len("http://"):]
	samples := make([]time.Duration, 0, *iters)
	for k := 0; k < *iters; k++ {
		t0 := time.Now()
		c, err := net.Dial("tcp", addr)
		if err != nil {
			panic(err)
		}
		_, _ = c.Write([]byte("GET / HTTP/1.1\r\nHost: bench\r\nConnection: close\r\n\r\n"))
		_, _ = io.ReadAll(c)
		c.Close()
		samples = append(samples, time.Since(t0))
	}
	sort.Slice(samples, func(i, j int) bool { return samples[i] < samples[j] })
	p := func(q float64) time.Duration {
		return samples[int(float64(len(samples)-1)*q+0.5)]
	}
	fmt.Printf(
		"go_stdhttp_http_server: median=%.3f ms  p95=%.3f ms  p99=%.3f ms\n",
		float64(p(0.50).Nanoseconds())/1.0e6,
		float64(p(0.95).Nanoseconds())/1.0e6,
		float64(p(0.99).Nanoseconds())/1.0e6,
	)
}
