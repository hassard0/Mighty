CXX ?= g++
CXXFLAGS ?= -O3 -std=c++20 -Wall -Wextra

parse-throughput-cpp: main.cpp
	$(CXX) $(CXXFLAGS) main.cpp -o $@

.PHONY: run clean
run: parse-throughput-cpp
	./parse-throughput-cpp --iters 30
clean:
	rm -f parse-throughput-cpp
