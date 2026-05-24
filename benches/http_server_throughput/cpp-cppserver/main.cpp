// C++ comparator: bare HTTP/1.1 server using only POSIX sockets so it
// builds without dependencies on Linux/macOS. Identical request shape
// to the Stardust serve_in_memory bench: respond 200 + "ok" to GET.
//
// Build: g++ -O3 -std=c++20 main.cpp -pthread -o http-server-throughput-cpp
// Run:   ./http-server-throughput-cpp --iters 30
//
// Note: For Windows hosts use winsock — not exercised here because the
// recorded numbers come from Linux/macOS. The bare-sockets shape is
// representative of what e.g. cpp-httplib or cppserver give you after
// their request-router overhead is removed.

#ifdef _WIN32
#include <cstdio>
int main() { std::printf("cpp-cppserver: build on a POSIX host\n"); return 0; }
#else

#include <algorithm>
#include <arpa/inet.h>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <netinet/in.h>
#include <netinet/tcp.h>
#include <sys/socket.h>
#include <thread>
#include <unistd.h>
#include <vector>

static int start_server(uint16_t* out_port) {
    int s = socket(AF_INET, SOCK_STREAM, 0);
    int yes = 1;
    setsockopt(s, SOL_SOCKET, SO_REUSEADDR, &yes, sizeof(yes));
    sockaddr_in a{}; a.sin_family = AF_INET; a.sin_addr.s_addr = htonl(INADDR_LOOPBACK); a.sin_port = 0;
    bind(s, (sockaddr*)&a, sizeof(a));
    socklen_t alen = sizeof(a);
    getsockname(s, (sockaddr*)&a, &alen);
    *out_port = ntohs(a.sin_port);
    listen(s, 64);
    return s;
}

static void serve_one(int srv_fd) {
    for (;;) {
        int c = accept(srv_fd, nullptr, nullptr);
        if (c < 0) return;
        char buf[2048];
        ssize_t n = recv(c, buf, sizeof(buf), 0);
        (void)n;
        const char* resp = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";
        send(c, resp, strlen(resp), 0);
        close(c);
    }
}

static long long round_trip(uint16_t port) {
    using namespace std::chrono;
    auto t0 = high_resolution_clock::now();
    int s = socket(AF_INET, SOCK_STREAM, 0);
    sockaddr_in a{}; a.sin_family = AF_INET; a.sin_addr.s_addr = htonl(INADDR_LOOPBACK); a.sin_port = htons(port);
    connect(s, (sockaddr*)&a, sizeof(a));
    const char* req = "GET / HTTP/1.1\r\nHost: bench\r\nConnection: close\r\n\r\n";
    send(s, req, strlen(req), 0);
    char buf[2048];
    while (recv(s, buf, sizeof(buf), 0) > 0) {}
    close(s);
    auto t1 = high_resolution_clock::now();
    return duration_cast<nanoseconds>(t1 - t0).count();
}

int main(int argc, char** argv) {
    int iters = 30;
    for (int i = 1; i < argc; ++i) {
        if (std::string(argv[i]) == "--iters" && i + 1 < argc) {
            iters = std::atoi(argv[i+1]); ++i;
        }
    }
    uint16_t port;
    int srv = start_server(&port);
    std::thread t([&]{ serve_one(srv); });
    t.detach();
    std::vector<long long> samples; samples.reserve((size_t)iters);
    for (int k = 0; k < iters; ++k) samples.push_back(round_trip(port));
    std::sort(samples.begin(), samples.end());
    auto pick = [&](double q) -> long long {
        return samples[(size_t)((samples.size() - 1) * q + 0.5)];
    };
    std::printf(
        "cpp_sockets_http_server: median=%.3f ms  p95=%.3f ms  p99=%.3f ms\n",
        pick(0.50) / 1.0e6, pick(0.95) / 1.0e6, pick(0.99) / 1.0e6);
    return 0;
}

#endif
