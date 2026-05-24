// C++ + asio coroutines comparator.
//
// Approximates the Stardust mailbox shape: a single asio::channel,
// post a message, dequeue on the same coroutine.
//
// Build:
//   g++ -O3 -std=c++20 main.cpp -I/path/to/asio/include -pthread \
//       -o agent-send-latency-cpp-asio
// Run: ./agent-send-latency-cpp-asio --iters 1000

#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <vector>

#ifdef HAVE_ASIO
#include <asio.hpp>
#include <asio/experimental/channel.hpp>

using asio::experimental::channel;
using namespace std::chrono;

asio::awaitable<long long> one_round(channel<void(asio::error_code, int)>& ch) {
    auto t0 = high_resolution_clock::now();
    co_await ch.async_send(asio::error_code{}, 1, asio::use_awaitable);
    int v = co_await ch.async_receive(asio::use_awaitable);
    (void)v;
    auto t1 = high_resolution_clock::now();
    co_return duration_cast<nanoseconds>(t1 - t0).count();
}

int main(int argc, char** argv) {
    int iters = 1000;
    for (int i = 1; i < argc; ++i) {
        if (std::string(argv[i]) == "--iters" && i + 1 < argc) {
            iters = std::atoi(argv[i+1]); ++i;
        }
    }
    asio::io_context ctx{1};
    std::vector<long long> samples; samples.reserve((size_t)iters);
    for (int k = 0; k < iters; ++k) {
        channel<void(asio::error_code, int)> ch{ctx, 8};
        long long elapsed = 0;
        asio::co_spawn(ctx, [&]() -> asio::awaitable<void> {
            elapsed = co_await one_round(ch);
        }, asio::detached);
        ctx.run();
        ctx.restart();
        samples.push_back(elapsed);
    }
    std::sort(samples.begin(), samples.end());
    auto pick = [&](double q) -> long long {
        return samples[(size_t)((samples.size() - 1) * q + 0.5)];
    };
    std::printf(
        "cpp_asio_agent_send_latency: median=%.3f ms  p95=%.3f ms  p99=%.3f ms\n",
        pick(0.50) / 1.0e6, pick(0.95) / 1.0e6, pick(0.99) / 1.0e6);
    return 0;
}

#else  // !HAVE_ASIO

// Fallback: blocking std::condition_variable to give *some* signal
// when asio isn't installed. Documented in docs/benchmarks/methodology.md.

#include <condition_variable>
#include <mutex>
#include <queue>
#include <thread>

int main(int argc, char** argv) {
    int iters = 1000;
    for (int i = 1; i < argc; ++i) {
        if (std::string(argv[i]) == "--iters" && i + 1 < argc) {
            iters = std::atoi(argv[i+1]); ++i;
        }
    }
    std::vector<long long> samples; samples.reserve((size_t)iters);
    for (int k = 0; k < iters; ++k) {
        std::mutex m; std::condition_variable cv; std::queue<int> q;
        auto t0 = std::chrono::high_resolution_clock::now();
        { std::lock_guard<std::mutex> lk(m); q.push(1); }
        cv.notify_one();
        {
            std::unique_lock<std::mutex> lk(m);
            cv.wait(lk, [&]{ return !q.empty(); });
            q.pop();
        }
        auto t1 = std::chrono::high_resolution_clock::now();
        samples.push_back(std::chrono::duration_cast<std::chrono::nanoseconds>(t1 - t0).count());
    }
    std::sort(samples.begin(), samples.end());
    auto pick = [&](double q) -> long long {
        return samples[(size_t)((samples.size() - 1) * q + 0.5)];
    };
    std::printf(
        "cpp_cv_agent_send_latency: median=%.3f ms  p95=%.3f ms  p99=%.3f ms (asio not linked)\n",
        pick(0.50) / 1.0e6, pick(0.95) / 1.0e6, pick(0.99) / 1.0e6);
    return 0;
}
#endif
