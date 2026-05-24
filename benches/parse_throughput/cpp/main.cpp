// C++ comparator for parse_throughput.
//
// Hand-written single-pass scanner — the idiomatic shape that a C/C++
// compiler's tokenizer takes before LR/LALR machinery kicks in.
// Mirrors the Rust + Go comparators: lexer only, no AST.
//
// Build: g++ -O3 -std=c++20 main.cpp -o parse-throughput-cpp
// Run:   ./parse-throughput-cpp --iters 30

#include <algorithm>
#include <cctype>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <string>
#include <vector>

static std::string synth(int units) {
    std::string out;
    out.reserve((size_t)units * 256);
    out += "// cpp comparator\n";
    char buf[512];
    for (int i = 0; i < units; ++i) {
        int n = std::snprintf(buf, sizeof(buf),
            "struct Rec%d {\n  id: I64\n  name: I64\n  flag: I64\n}\n"
            "fn bench_f%d(x: I64, y: I64) -> I64 {\n"
            "  let z = x + y\n  let w = z * 2 - x\n  w\n}\n",
            i, i);
        out.append(buf, (size_t)n);
    }
    return out;
}

static size_t lex(const std::string& src) {
    size_t count = 0;
    size_t i = 0, n = src.size();
    while (i < n) {
        unsigned char c = (unsigned char)src[i];
        if (std::isspace(c)) { ++i; continue; }
        if (i + 1 < n && src[i] == '/' && src[i+1] == '/') {
            while (i < n && src[i] != '\n') ++i;
            continue;
        }
        if (std::isalpha(c) || c == '_') {
            size_t j = i + 1;
            while (j < n && (std::isalnum((unsigned char)src[j]) || src[j] == '_')) ++j;
            i = j; ++count; continue;
        }
        if (std::isdigit(c)) {
            size_t j = i + 1;
            while (j < n && std::isdigit((unsigned char)src[j])) ++j;
            i = j; ++count; continue;
        }
        ++i; ++count;
    }
    return count;
}

int main(int argc, char** argv) {
    int iters = 30;
    for (int i = 1; i < argc; ++i) {
        if (std::string(argv[i]) == "--iters" && i + 1 < argc) {
            iters = std::atoi(argv[i+1]); ++i;
        }
    }
    std::string src = synth(1000);
    std::vector<long long> samples;
    samples.reserve((size_t)iters);
    for (int k = 0; k < iters; ++k) {
        auto t0 = std::chrono::high_resolution_clock::now();
        size_t c = lex(src);
        auto t1 = std::chrono::high_resolution_clock::now();
        volatile size_t sink = c; (void)sink;
        samples.push_back(
            std::chrono::duration_cast<std::chrono::nanoseconds>(t1 - t0).count());
    }
    std::sort(samples.begin(), samples.end());
    auto pick = [&](double q) -> long long {
        return samples[(size_t)((samples.size() - 1) * q + 0.5)];
    };
    std::printf(
        "cpp_parse_throughput: median=%.3f ms  p95=%.3f ms  p99=%.3f ms  (bytes=%zu)\n",
        pick(0.50) / 1.0e6, pick(0.95) / 1.0e6, pick(0.99) / 1.0e6, src.size());
    return 0;
}
