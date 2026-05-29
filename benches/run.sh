#!/usr/bin/env bash
# Run whichever comparator implementations the local toolchain
# supports. Output to stdout in a stable shape so doc scripts can
# parse it. Always runs the Mighty impls first so a developer with
# no extra toolchains still sees the numbers that go into
# docs/benchmarks/*.md.
#
# Usage:
#   ./benches/run.sh             # auto-detect available toolchains
#   ./benches/run.sh --rust      # rust comparators only
#   ./benches/run.sh --go        # go comparators only
#   ./benches/run.sh --cpp       # c++ comparators only
#   ./benches/run.sh --all       # require all toolchains
#   ./benches/run.sh --mighty    # mighty impl only (cargo build req'd)
#   ./benches/run.sh --stardust  # legacy alias for --mighty (deprecated)

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

want_rust=auto; want_go=auto; want_cpp=auto; want_sd=auto
for arg in "$@"; do
  case "$arg" in
    --rust)     want_rust=yes; want_go=no;  want_cpp=no;  want_sd=no ;;
    --go)       want_rust=no;  want_go=yes; want_cpp=no;  want_sd=no ;;
    --cpp)      want_rust=no;  want_go=no;  want_cpp=yes; want_sd=no ;;
    --mighty)   want_rust=no;  want_go=no;  want_cpp=no;  want_sd=yes ;;
    --stardust) want_rust=no;  want_go=no;  want_cpp=no;  want_sd=yes;
                echo "(deprecated) --stardust is now spelled --mighty" >&2 ;;
    --all)      want_rust=yes; want_go=yes; want_cpp=yes; want_sd=yes ;;
    *) echo "unknown arg: $arg" >&2; exit 2 ;;
  esac
done

have_rust() { command -v cargo >/dev/null 2>&1; }
have_go()   { command -v go    >/dev/null 2>&1; }
have_cpp()  { command -v g++   >/dev/null 2>&1 || command -v clang++ >/dev/null 2>&1; }
# v0.36 T4 — the bench runner binary is `mty-bench-runner`; for back-compat
# with pre-rename builds we also accept the legacy `sdust-bench-runner` name.
have_sd()   { [ -x target/release/mty-bench-runner ] || [ -x target/release/mty-bench-runner.exe ] \
              || [ -x target/release/sdust-bench-runner ] || [ -x target/release/sdust-bench-runner.exe ]; }

run_rust_one() { # $1 = path to crate
  echo "==> rust: $1"
  ( cd "$1" && cargo run --release --quiet -- 30 )
}
run_go_one() {
  echo "==> go: $1"
  ( cd "$1" && go run main.go --iters 30 )
}
run_cpp_one() { # $1 = dir with Makefile
  echo "==> cpp: $1"
  ( cd "$1" && make --silent run )
}

# --- Mighty impls ----------------------------------------------------------
if [ "$want_sd" != "no" ]; then
  if have_sd; then
    echo "==> mighty"
    # Prefer the post-rename binary name; fall back to the pre-v0.7
    # `sdust-bench-runner` for legacy build trees.
    runner="target/release/mty-bench-runner"
    if [ ! -x "$runner" ] && [ ! -x "${runner}.exe" ]; then
      runner="target/release/sdust-bench-runner"
    fi
    if [ -x "${runner}.exe" ]; then runner="${runner}.exe"; fi
    "$runner" --all --iters 30
  else
    echo "(skip) mighty: build target/release/mty-bench-runner first (cargo build --release -p mty-bench)"
  fi
fi

# --- Rust comparators ------------------------------------------------------
if [ "$want_rust" != "no" ] && have_rust; then
  run_rust_one benches/parse_throughput/rust
  run_rust_one benches/agent_send_latency/rust-tokio
  run_rust_one benches/mailbox_throughput/rust-tokio
  run_rust_one benches/http_server_throughput/rust-hyper
elif [ "$want_rust" = "yes" ]; then
  echo "(skip) rust: cargo not on PATH"
fi

# --- Go comparators --------------------------------------------------------
if [ "$want_go" != "no" ] && have_go; then
  run_go_one benches/parse_throughput/go
  run_go_one benches/agent_send_latency/go-channels
  run_go_one benches/mailbox_throughput/go-channels
  run_go_one benches/http_server_throughput/go-stdhttp
elif [ "$want_go" = "yes" ]; then
  echo "(skip) go: go not on PATH"
fi

# --- C++ comparators -------------------------------------------------------
if [ "$want_cpp" != "no" ] && have_cpp; then
  run_cpp_one benches/parse_throughput/cpp
  run_cpp_one benches/agent_send_latency/cpp-asio
  run_cpp_one benches/mailbox_throughput/cpp-asio
  run_cpp_one benches/http_server_throughput/cpp-cppserver
elif [ "$want_cpp" = "yes" ]; then
  echo "(skip) c++: g++/clang++ not on PATH"
fi

echo "done"
