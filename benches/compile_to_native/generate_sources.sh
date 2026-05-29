#!/usr/bin/env bash
# Generate the four parallel ~1 KLOC source files used by the
# compile_to_native comparators. Outputs go into ./generated/.
set -euo pipefail

cd "$(dirname "$0")"
mkdir -p generated

UNITS=100

# Mighty
{
  echo "// auto-generated"
  for i in $(seq 0 $((UNITS - 1))); do
    cat <<EOF
struct Rec${i} {
  id: I64
  name: I64
  flag: I64
}
fn bench_f${i}(x: I64, y: I64) -> I64 {
  let z = x + y
  let w = z * 2 - x
  w
}
EOF
  done
} > generated/synth.mty

# Rust
{
  echo "// auto-generated"
  for i in $(seq 0 $((UNITS - 1))); do
    cat <<EOF
struct Rec${i} { id: i64, name: i64, flag: i64 }
fn bench_f${i}(x: i64, y: i64) -> i64 { let z = x + y; let w = z * 2 - x; w }
EOF
  done
  echo "fn main() { println!(\"{}\", bench_f0(1, 2)); }"
} > generated/synth.rs

# Go
{
  echo "package main"
  echo "import \"fmt\""
  for i in $(seq 0 $((UNITS - 1))); do
    cat <<EOF
type Rec${i} struct { id, name, flag int64 }
func benchF${i}(x, y int64) int64 { z := x + y; w := z*2 - x; return w }
EOF
  done
  echo "func main() { fmt.Println(benchF0(1, 2)) }"
} > generated/synth.go

# C++
{
  echo "#include <cstdint>"
  echo "#include <cstdio>"
  for i in $(seq 0 $((UNITS - 1))); do
    cat <<EOF
struct Rec${i} { int64_t id, name, flag; };
int64_t bench_f${i}(int64_t x, int64_t y) { int64_t z = x + y; int64_t w = z * 2 - x; return w; }
EOF
  done
  echo "int main() { std::printf(\"%lld\\n\", (long long)bench_f0(1, 2)); return 0; }"
} > generated/synth.cpp

echo "wrote generated/synth.{mty,rs,go,cpp}"
