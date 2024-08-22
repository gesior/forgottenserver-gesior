#ifndef __PATHFINDING_H__
#define __PATHFINDING_H__

#include <vector>
#include <thread>
#include <atomic>
#include <condition_variable>
#include "enums.h"

class PathFinding
{
	public:
		explicit PathFinding(int _threads) : threads(_threads) {}

		void start() {
			setState(THREAD_STATE_RUNNING);
			for (auto& thread : threads) {
				thread = std::thread(&PathFinding::threadMain, this);
			}
		}

		void stop() {
			setState(THREAD_STATE_TERMINATED);
		}


		void join() {
			for (auto& thread : threads) {
				if (thread.joinable()) {
					thread.join();
				}
			}
		}

		void threadMain();
		void runTask(std::function<void(void)> func);
		void shutdown();

	protected:
		void setState(ThreadState newState) {
			threadState.store(newState, std::memory_order_relaxed);
		}

		ThreadState getState() const {
			return threadState.load(std::memory_order_relaxed);
		}

	private:
		std::atomic<ThreadState> threadState{ THREAD_STATE_TERMINATED };
		std::vector<std::thread> threads;
		std::function<void(void)> task;
		std::mutex taskLock;
		std::condition_variable taskSignal;
		size_t ready = 0;
};

extern PathFinding g_pathfinding;

#endif
