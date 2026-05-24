// C++ comparator: lock-free SPSC ring buffer pumping 10 000 msgs per
// iter on a single thread (synchronous, no asio dependency). Provides
// a lower-bound number that's strictly faster than any blocking channel.
//
// Build: g++ -O3 -std=c++20 main.cpp -o mailbox-throughput-cpp
// Run:   ./mailbox-throughput-cpp --iters 30

#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <thread>
#include <vector>

constexpr size_t N = 10000;

template <typename T, size_t Cap>
struct SpscRing {
    std::array<T, Cap> buf{};
    std::atomic<size_t> head{0}, tail{0};
    bool push(const T& v) {
        size_t t = tail.load(std::memory_order_relaxed);
        size_t nxt = (t + 1) % Cap;
        if (nxt == head.load(std::memory_order_acquire)) return false;
        buf[t] = v;
        tail.store(nxt, std::memory_order_release);
        return true;
    }
    bool pop(T& out) {
        size_t h = head.load(std::memory_order_relaxed);
        if (h == tail.load(std::memory_order_acquire)) return false;
        out = buf[h];
        head.store((h + 1) % Cap, std::memory_order_release);
        return true;
    }
};

int main(int argc, char** argv) {
    int iters = 30;
    for (int i = 1; i < argc; ++i) {
        if (std::string(argv[i]) == "--iters" && i + 1 < argc) {
            iters = std::atoi(argv[i+1]); ++i;
        }
    }
    std::vector<long long> samples; samples.reserve((size_t)iters);
    for (int k = 0; k < iters; ++k) {
        auto ring = std::make_unique<SpscRing<int, N + 1>>();
        auto t0 = std::chrono::high_resolution_clock::now();
        std::thread prod([&ring]() {
            for (size_t i = 0; i < N; ++i) {
                while (!ring->push((int)i)) std::this_thread::yield();
            }
        });
        int v;
        for (size_t i = 0; i < N; ++i) {
            while (!ring->pop(v)) std::this_thread::yield();
        }
        prod.join();
        auto t1 = std::chrono::high_resolution_clock::now();
        samples.push_back(
            std::chrono::duration_cast<std::chrono::nanoseconds>(t1 - t0).count());
    }
    std::sort(samples.begin(), samples.end());
    auto pick = [&](double q) -> long long {
        return samples[(size_t)((samples.size() - 1) * q + 0.5)];
    };
    std::printf(
        "cpp_spsc_mailbox_throughput: median=%.3f ms  p95=%.3f ms  p99=%.3f ms  (%zu msgs/iter)\n",
        pick(0.50) / 1.0e6, pick(0.95) / 1.0e6, pick(0.99) / 1.0e6, N);
    return 0;
}
