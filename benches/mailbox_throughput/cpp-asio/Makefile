CXX ?= g++
CXXFLAGS ?= -O3 -std=c++20 -Wall -Wextra -pthread

mailbox-throughput-cpp: main.cpp
	$(CXX) $(CXXFLAGS) main.cpp -o $@

.PHONY: run clean
run: mailbox-throughput-cpp
	./mailbox-throughput-cpp --iters 30
clean:
	rm -f mailbox-throughput-cpp
