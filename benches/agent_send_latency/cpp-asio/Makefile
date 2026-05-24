CXX ?= g++
CXXFLAGS ?= -O3 -std=c++20 -Wall -Wextra -pthread
# Set ASIO_INCLUDE to the path containing <asio.hpp> to enable the
# coroutine-channel benchmark. Without it, the fallback
# condition-variable shape is built.
ASIO_INCLUDE ?=
ifeq ($(ASIO_INCLUDE),)
    EXTRA :=
else
    EXTRA := -I$(ASIO_INCLUDE) -DHAVE_ASIO=1
endif

agent-send-latency-cpp-asio: main.cpp
	$(CXX) $(CXXFLAGS) $(EXTRA) main.cpp -o $@

.PHONY: run clean
run: agent-send-latency-cpp-asio
	./agent-send-latency-cpp-asio --iters 1000
clean:
	rm -f agent-send-latency-cpp-asio
