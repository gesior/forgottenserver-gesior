#include "otpch.h"
#include "pathfinding.h"

#include <utility>

void PathFinding::threadMain() {
	std::unique_lock<std::mutex> taskLockUnique(taskLock, std::defer_lock);

	while (getState() != THREAD_STATE_TERMINATED) {
		taskLockUnique.lock();
		taskSignal.wait(taskLockUnique);
		if (!task) {
			taskLockUnique.unlock();
			continue;
		}
		taskLockUnique.unlock();
		task();
		taskLockUnique.lock();
		ready += 1;
		bool notify = ready == threads.size();
		if (ready == threads.size())
			task = nullptr;
		taskLockUnique.unlock();
		if (notify)
			taskSignal.notify_all();
	}
}

void PathFinding::runTask(std::function<void(void)> func) {
	std::unique_lock<std::mutex> taskLockUnique(taskLock, std::defer_lock);
	taskLockUnique.lock();
	task = std::move(func);
	ready = 0;
	taskSignal.notify_all();
	taskSignal.wait(taskLockUnique);
	taskLockUnique.unlock();
}

void PathFinding::shutdown()
{
	std::lock_guard<std::mutex> lockClass(taskLock);
	setState(THREAD_STATE_TERMINATED);
	taskSignal.notify_all();
}
