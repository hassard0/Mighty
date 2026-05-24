// Go comparator for parse_throughput.
//
// Hand-written scanner using bufio.Scanner with a custom split
// function — the idiomatic Go shape for a parser front-end without
// pulling in a generator framework. Mirrors the lexer-only path of
// the Rust comparator (no AST construction).
//
// Usage: go run main.go --iters 30
package main

import (
	"flag"
	"fmt"
	"sort"
	"strings"
	"time"
	"unicode"
)

func synth(units int) string {
	var b strings.Builder
	b.WriteString("// go comparator\n")
	for i := 0; i < units; i++ {
		fmt.Fprintf(&b,
			"struct Rec%d {\n  id: I64\n  name: I64\n  flag: I64\n}\nfn bench_f%d(x: I64, y: I64) -> I64 {\n  let z = x + y\n  let w = z * 2 - x\n  w\n}\n",
			i, i)
	}
	return b.String()
}

func lex(src string) int {
	// Single-pass scanner: skip ws/comments, emit ident/number/punct.
	count := 0
	i := 0
	n := len(src)
	for i < n {
		c := rune(src[i])
		if unicode.IsSpace(c) {
			i++
			continue
		}
		if i+1 < n && src[i] == '/' && src[i+1] == '/' {
			for i < n && src[i] != '\n' {
				i++
			}
			continue
		}
		if unicode.IsLetter(c) || c == '_' {
			j := i + 1
			for j < n && (unicode.IsLetter(rune(src[j])) ||
				unicode.IsDigit(rune(src[j])) || src[j] == '_') {
				j++
			}
			i = j
			count++
			continue
		}
		if unicode.IsDigit(c) {
			j := i + 1
			for j < n && unicode.IsDigit(rune(src[j])) {
				j++
			}
			i = j
			count++
			continue
		}
		// punct
		i++
		count++
	}
	return count
}

func percentile(s []time.Duration, q float64) time.Duration {
	idx := int(float64(len(s)-1)*q + 0.5)
	return s[idx]
}

func main() {
	iters := flag.Int("iters", 30, "samples")
	flag.Parse()
	src := synth(1000)
	samples := make([]time.Duration, 0, *iters)
	for k := 0; k < *iters; k++ {
		t0 := time.Now()
		c := lex(src)
		_ = c
		samples = append(samples, time.Since(t0))
	}
	sort.Slice(samples, func(i, j int) bool { return samples[i] < samples[j] })
	p50 := percentile(samples, 0.50)
	p95 := percentile(samples, 0.95)
	p99 := percentile(samples, 0.99)
	fmt.Printf(
		"go_parse_throughput: median=%.3f ms  p95=%.3f ms  p99=%.3f ms  (bytes=%d)\n",
		float64(p50.Nanoseconds())/1.0e6,
		float64(p95.Nanoseconds())/1.0e6,
		float64(p99.Nanoseconds())/1.0e6,
		len(src),
	)
}
