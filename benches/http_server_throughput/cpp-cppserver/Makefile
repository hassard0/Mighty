CXX ?= g++
CXXFLAGS ?= -O3 -std=c++20 -Wall -Wextra -pthread

http-server-throughput-cpp: main.cpp
	$(CXX) $(CXXFLAGS) main.cpp -o $@

.PHONY: run clean
run: http-server-throughput-cpp
	./http-server-throughput-cpp --iters 30
clean:
	rm -f http-server-throughput-cpp
