// Copyright 2026 The Forgotten Server Authors. All rights reserved.
// Use of this source code is governed by the GPL-2.0 License that can be found in the LICENSE file.

#include "otpch.h"

#include <bit>
#include <cstdlib>

#ifndef _WIN32
#include <unistd.h>
#endif

#include "cams.h"

#include "configmanager.h"
#include "networkmessage.h"
#include "tools.h"

extern ConfigManager g_config;

std::atomic<uint64_t> Cams::minActiveCamId{std::numeric_limits<uint64_t>::max()};
std::atomic<bool> Cams::recordIncomingPackets{false};
std::atomic<int64_t> Cams::syncIntervalMs{0};
std::atomic<int64_t> Cams::maxDurationMs{0};

namespace {

// minActiveCamId value that matches no cam: nothing is queued and the cams thread finishes all open cams
constexpr uint64_t CAMS_NO_ACTIVE_CAM_ID = std::numeric_limits<uint64_t>::max();

// Minimum positive camsMaxDuration in seconds (above the 3s short-cam discard limit and 5s ping interval).
constexpr int32_t CAMS_MIN_MAX_DURATION_SEC = 10;

// The cams thread wakes up at least this often. Only shutdown() wakes it up earlier.
constexpr std::chrono::milliseconds CAMS_TICK_INTERVAL{100};

// At shutdown, a cams thread that made no progress for this long is considered blocked in disk I/O.
constexpr int64_t CAMS_JOIN_STALL_LIMIT_MS = 30 * 1000;

// In-slab record: [camId u64][timestampMs u64][packetSize u16][bytes packetSize].
// On disk the camId is omitted, which is CAM_DISK_RECORD_HEADER + packetSize bytes.
constexpr size_t CAM_SLAB_RECORD_HEADER = sizeof(uint64_t) + sizeof(uint64_t) + sizeof(uint16_t);
constexpr size_t CAM_DISK_RECORD_HEADER = sizeof(uint64_t) + sizeof(uint16_t);
// Returned blocks stay on a free list guarded by queueLock so steady traffic does not malloc.
constexpr size_t CAM_FREE_SLAB_LIMIT = (16 * 1024 * 1024) / CamSlab::CAPACITY;

static_assert(CAM_SLAB_RECORD_HEADER + std::numeric_limits<uint16_t>::max() <= CamSlab::CAPACITY);
static_assert(CAM_SLAB_RECORD_HEADER + 1 + static_cast<size_t>(NETWORKMESSAGE_MAXSIZE) <= CamSlab::CAPACITY);
static_assert(CAM_FREE_SLAB_LIMIT >= 1);
static_assert(std::endian::native == std::endian::little, "TIBIACAM packet records are little-endian");

// A zero size is a batch buffer the queue does not store. countNewSlabs and the
// write loop must both skip those buffers or the slab count will not match.
template <typename SizeFn>
size_t countNewSlabs(const CamSlab* open, size_t recordCount, SizeFn&& sizeAt)
{
	size_t used = open ? static_cast<size_t>(open->used) : CamSlab::CAPACITY;
	size_t newSlabs = 0;
	for (size_t i = 0; i < recordCount; ++i) {
		const size_t recordBytes = sizeAt(i);
		if (recordBytes == 0) {
			continue;
		}
		if (used + recordBytes > CamSlab::CAPACITY) {
			++newSlabs;
			used = 0;
		}
		used += recordBytes;
	}
	return newSlabs;
}

bool isQueuedBatchPayload(size_t size)
{
	return size > 1 && size <= std::numeric_limits<uint16_t>::max() &&
	       CAM_SLAB_RECORD_HEADER + size <= CamSlab::CAPACITY;
}

// Drop retained capacity after a flush when it is well above the configured buffer.
void trimCamFileBuffer(std::vector<char>& buf, size_t limitBytes)
{
	if (!buf.empty() || limitBytes == 0 || limitBytes > (std::numeric_limits<size_t>::max() / 2)) {
		return;
	}
	if (buf.capacity() <= limitBytes * 2) {
		return;
	}

	std::vector<char> trimmed;
	trimmed.reserve(limitBytes);
	buf.swap(trimmed);
}

int64_t camsSteadyTimeMs()
{
	return std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::steady_clock::now().time_since_epoch()).count();
}

template <typename T>
void appendCamPod(std::vector<char>& dst, const T& value)
{
	static_assert(std::is_trivially_copyable_v<T>);
	const char* bytes = reinterpret_cast<const char*>(&value);
	dst.insert(dst.end(), bytes, bytes + sizeof(T));
}

void appendCamBytes(std::vector<char>& dst, const void* src, size_t len)
{
	if (len == 0) {
		return;
	}
	const char* bytes = static_cast<const char*>(src);
	dst.insert(dst.end(), bytes, bytes + len);
}

bool validateCamsDirectory(std::string& dir)
{
	std::string normalized = dir;
	std::replace(normalized.begin(), normalized.end(), '\\', '/');

	std::filesystem::path dirPath = std::filesystem::path(normalized).lexically_normal();
	if (dirPath.empty()) {
		return false;
	}

	std::error_code ec;
	if (!std::filesystem::exists(dirPath, ec) || ec || !std::filesystem::is_directory(dirPath, ec) || ec) {
		return false;
	}

#ifdef _WIN32
	const uint32_t pid = static_cast<uint32_t>(GetCurrentProcessId());
#else
	const uint32_t pid = static_cast<uint32_t>(getpid());
#endif
	const std::string baseName = ".cam_write_test_" + std::to_string(pid) + "_" + std::to_string(OTSYS_TIME());
	const std::filesystem::path tempPath = dirPath / (baseName + ".tmp");
	const std::filesystem::path readyPath = dirPath / (baseName + ".tmp.ready");

	{
		std::ofstream out(tempPath, std::ios::out | std::ios::trunc | std::ios::binary);
		if (!out.is_open()) {
			return false;
		}
		constexpr char testByte = 0x01;
		out.write(&testByte, 1);
		out.flush();
		if (out.fail()) {
			out.close();
			std::filesystem::remove(tempPath, ec);
			return false;
		}
		out.close();
		if (out.fail()) {
			std::filesystem::remove(tempPath, ec);
			return false;
		}
	}

	std::filesystem::rename(tempPath, readyPath, ec);
	if (ec) {
		std::error_code removeEc;
		std::filesystem::remove(tempPath, removeEc);
		return false;
	}

	std::filesystem::remove(readyPath, ec);
	if (ec) {
		return false;
	}

	dir = dirPath.generic_string();
	return true;
}

} // namespace

void Cams::startup()
{
	try {
		camsDir = g_config.getString(ConfigManager::CAMS_DIR);
		applyConfigLimits();

		if (!validateCamsDirectory(camsDir)) {
			disabledUntilRestart.store(true);
			std::cout << "[Warning - Cams::startup] Cams directory '" << camsDir
			          << "' does not exist or is not writable. Cam system is disabled until server restart." << std::endl;
			return;
		}

		applyRecordingState(g_config.getBoolean(ConfigManager::CAMS_ENABLED));

		lastProgressMs.store(camsSteadyTimeMs(), std::memory_order_relaxed);
		threadExited.store(false);
		start();
		cleanupOldCams();
		return;
	} catch (const std::exception& e) {
		std::cout << "[Error - Cams::startup] " << e.what() << std::endl;
	} catch (...) {
		std::cout << "[Error - Cams::startup] Unknown exception." << std::endl;
	}

	// start() may have failed after it marked the thread as running
	setState(THREAD_STATE_TERMINATED);
	threadExited.store(true);
	emergencyDisable("Cam system failed to start.");
}

void Cams::onConfigReload()
{
	applyConfigLimits();

	const bool recording = g_config.getBoolean(ConfigManager::CAMS_ENABLED);
	if (disabledUntilRestart.load()) {
		if (recording) {
			std::cout << "[Warning - Cams::onConfigReload] Cam system stays disabled until server restart "
			             "(camsDir was invalid at startup or cam recording was emergency-disabled)." << std::endl;
		}
		return;
	}

	applyRecordingState(recording);

	// An emergency raised by another thread during this reload always wins
	if (disabledUntilRestart.load()) {
		applyRecordingState(false);
	}
}

void Cams::applyConfigLimits()
{
	bufferSizeLimitBytes.store(
		static_cast<size_t>(std::max<int32_t>(1, g_config.getNumber(ConfigManager::CAMS_BUFFER_SIZE))) * 1024u,
		std::memory_order_relaxed);
	saveIntervalSec.store(std::max<int32_t>(1, g_config.getNumber(ConfigManager::CAMS_SAVE_INTERVAL)),
	                      std::memory_order_relaxed);
	maxTotalRamBytes.store(
		static_cast<size_t>(std::max<int32_t>(10, g_config.getNumber(ConfigManager::CAMS_MAX_TOTAL_RAM))) * 1024u * 1024u,
		std::memory_order_relaxed);
	maxQueueRamBytes.store(
		static_cast<size_t>(std::max<int32_t>(5, g_config.getNumber(ConfigManager::CAMS_MAX_QUEUE_RAM))) * 1024u * 1024u,
		std::memory_order_relaxed);
}

void Cams::applyRecordingState(bool recording)
{
	if (!recording) {
		// CAMS_NO_ACTIVE_CAM_ID also tells the cams thread to finish all open cams
		enabled.store(false);
		minActiveCamId.store(CAMS_NO_ACTIVE_CAM_ID);
		recordIncomingPackets.store(false);
		syncIntervalMs.store(0);
		maxDurationMs.store(0);
		return;
	}

	if (!enabled.load()) {
		// off -> on: only cams started from now on are recorded
		minActiveCamId.store(nextCamId.load());
	}

	const int32_t syncSec = g_config.getNumber(ConfigManager::CAMS_SYNC_INTERVAL);
	const int32_t maxDurationSec = g_config.getNumber(ConfigManager::CAMS_MAX_DURATION);
	recordIncomingPackets.store(g_config.getBoolean(ConfigManager::CAMS_INCOMING_PACKETS));
	syncIntervalMs.store(syncSec > 0 ? static_cast<int64_t>(syncSec) * 1000 : 0);
	maxDurationMs.store(
		maxDurationSec > 0 ? static_cast<int64_t>(std::max<int32_t>(CAMS_MIN_MAX_DURATION_SEC, maxDurationSec)) * 1000 : 0);
	enabled.store(true);
}

void Cams::cleanupOldCams() const noexcept
{
	try {
		const bool removeOnCrash = g_config.getBoolean(ConfigManager::CAM_REMOVE_ON_CRASH);

		std::error_code ec;
		std::filesystem::directory_iterator it(camsDir, ec);
		if (ec) {
			std::cout << "[Warning - Cams::cleanupOldCams] Failed to open cams directory '" << camsDir
			          << "': " << ec.message() << std::endl;
			return;
		}

		const std::filesystem::directory_iterator end;
		while (it != end) {
			const std::filesystem::path& entryPath = it->path();
			if (entryPath.extension() == ".cam") {
				std::error_code statusEc;
				const bool isRegular = it->is_regular_file(statusEc);
				if (statusEc) {
					std::cout << "[Warning - Cams::cleanupOldCams] Failed to inspect old cam '" << entryPath.string()
					          << "': " << statusEc.message() << std::endl;
				} else if (isRegular) {
					if (removeOnCrash) {
						std::error_code removeEc;
						std::filesystem::remove(entryPath, removeEc);
						if (removeEc) {
							std::cout << "[Warning - Cams::cleanupOldCams] Failed to remove old cam '" << entryPath.string()
							          << "': " << removeEc.message() << std::endl;
						}
					} else {
						std::filesystem::path crashPath = entryPath;
						crashPath += ".crash";
						std::error_code renameEc;
						std::filesystem::rename(entryPath, crashPath, renameEc);
						if (renameEc) {
							std::cout << "[Warning - Cams::cleanupOldCams] Failed to rename old cam '" << entryPath.string()
							          << "' to '" << crashPath.string() << "': " << renameEc.message() << std::endl;
						}
					}
				}
			}

			it.increment(ec);
			if (ec) {
				std::cout << "[Warning - Cams::cleanupOldCams] Failed to iterate cams directory '" << camsDir
				          << "': " << ec.message() << std::endl;
				break;
			}
		}
	} catch (const std::exception& e) {
		std::cout << "[Warning - Cams::cleanupOldCams] " << e.what() << std::endl;
	} catch (...) {
		std::cout << "[Warning - Cams::cleanupOldCams] Unknown exception." << std::endl;
	}
}

void Cams::shutdown()
{
	{
		std::lock_guard<std::mutex> lockClass(queueLock);
		setState(THREAD_STATE_TERMINATED);
	}
	queueSignal.notify_one();
}

void Cams::join()
{
	std::lock_guard<std::mutex> lockClass(joinLock);

	lastProgressMs.store(camsSteadyTimeMs(), std::memory_order_relaxed);

	// A disk write that never returns (frozen NFS/SMB mount, dying disk) would keep the process alive forever.
	// Wait as long as the cams thread makes progress, give up when it is silent for too long.
	while (!threadExited.load()) {
		if (camsSteadyTimeMs() - lastProgressMs.load(std::memory_order_relaxed) > CAMS_JOIN_STALL_LIMIT_MS) {
			std::cout << "[CRITICAL WARNING - Cams System] Cams thread is blocked in disk I/O. "
			             "Exiting without it, cams that were not saved yet are lost." << std::endl;
			// g_cams.join() is the last step of the shutdown, the game state is already saved.
			// Returning would destroy g_cams while its thread is still running.
			std::_Exit(EXIT_SUCCESS);
		}
		std::this_thread::sleep_for(std::chrono::milliseconds(50));
	}

	ThreadHolder::join();
}

void Cams::emergencyDisable(const char* reason) noexcept
{
	if (disabledUntilRestart.exchange(true)) {
		return;
	}

	minActiveCamId.store(CAMS_NO_ACTIVE_CAM_ID);
	recordIncomingPackets.store(false);
	syncIntervalMs.store(0);
	maxDurationMs.store(0);

	std::cout << "[CRITICAL WARNING - Cams System] " << reason << std::endl;
	std::cout << "[CRITICAL WARNING - Cams System] Cam recording is disabled until server restart. "
	             "Cams that were not saved yet are dropped to free RAM." << std::endl;
}

uint64_t Cams::initCam(const std::string& character, uint32_t playerId, uint32_t playerLevel,
                       uint32_t accountId, uint32_t ip, uint16_t version)
{
	if (!isEnabled()) {
		return 0;
	}

	const uint64_t id = nextCamId.fetch_add(1, std::memory_order_relaxed);
	const int64_t nowMs = OTSYS_TIME();

	CamInitInfo info{id, nowMs, character, playerId, playerLevel, accountId, ip, version};

	std::lock_guard<std::mutex> lockClass(queueLock);
	if (getState() == THREAD_STATE_TERMINATED || !isEnabled()) {
		return 0;
	}
	pendingCommands.push_back(CamCommand{pendingPacketCount, CamCommandType::Init, id, std::move(info)});
	return id;
}

CamRotateResult Cams::rotateCam(uint64_t oldCamId, const std::string& character, uint32_t playerId,
                                uint32_t playerLevel, uint32_t accountId, uint32_t ip, uint16_t version,
                                const std::vector<std::vector<uint8_t>>& startPackets)
{
	if (!isEnabled() || oldCamId == 0 || oldCamId < minActiveCamId.load(std::memory_order_relaxed) || startPackets.empty()) {
		return {};
	}

	bool any = false;
	for (const auto& buffer : startPackets) {
		if (isQueuedBatchPayload(buffer.size())) {
			any = true;
			break;
		}
	}
	if (!any) {
		return {};
	}

	// Copy character name before taking queueLock so string allocation stays outside the critical section.
	CamInitInfo info{0, 0, character, playerId, playerLevel, accountId, ip, version};

	std::lock_guard<std::mutex> lockClass(queueLock);
	if (getState() == THREAD_STATE_TERMINATED || !isEnabled() ||
	    oldCamId < minActiveCamId.load(std::memory_order_relaxed)) {
		return {};
	}

	const size_t newSlabs = countNewSlabs(openSlab.get(), startPackets.size(), [&](size_t index) {
		const size_t size = startPackets[index].size();
		if (!isQueuedBatchPayload(size)) {
			return size_t{0};
		}
		return CAM_SLAB_RECORD_HEADER + size;
	});

	// Reserve before taking slabs or pushing commands so an allocation failure leaves the queue untouched.
	pendingCommands.reserve(pendingCommands.size() + 2);

	std::vector<std::unique_ptr<CamSlab>> fresh;
	if (!prepareSlabRecords(newSlabs, fresh)) {
		return {};
	}

	// Under the lock, so startTimeMs and start-packet timestamps stay monotonic with the queue.
	const uint64_t newCamId = nextCamId.fetch_add(1, std::memory_order_relaxed);
	const int64_t nowMs = OTSYS_TIME();
	info.camId = newCamId;
	info.startTimeMs = nowMs;

	pendingCommands.push_back(CamCommand{pendingPacketCount, CamCommandType::Finish, oldCamId, {}});
	pendingCommands.push_back(CamCommand{pendingPacketCount, CamCommandType::Init, newCamId, std::move(info)});

	const uint64_t startTimestampMs = static_cast<uint64_t>(nowMs);
	for (const auto& buffer : startPackets) {
		if (!isQueuedBatchPayload(buffer.size())) {
			continue;
		}
		if (!writeSlabRecord(fresh, newCamId, startTimestampMs, buffer.data(), buffer.size(), nullptr, 0)) {
			fresh.clear();
			return {};
		}
	}
	releaseUnusedSlabs(fresh);
	return {newCamId, nowMs};
}

void Cams::cancelCam(uint64_t id)
{
	if (id == 0 || disabledUntilRestart.load(std::memory_order_relaxed)) {
		return;
	}

	std::lock_guard<std::mutex> lockClass(queueLock);
	if (getState() == THREAD_STATE_TERMINATED || disabledUntilRestart.load(std::memory_order_relaxed)) {
		return;
	}
	pendingCommands.push_back(CamCommand{pendingPacketCount, CamCommandType::Cancel, id, {}});
}

void Cams::finishCam(uint64_t id)
{
	if (id == 0 || disabledUntilRestart.load(std::memory_order_relaxed) ||
	    id < minActiveCamId.load(std::memory_order_relaxed)) {
		return;
	}

	std::lock_guard<std::mutex> lockClass(queueLock);
	if (getState() == THREAD_STATE_TERMINATED || disabledUntilRestart.load(std::memory_order_relaxed) ||
	    id < minActiveCamId.load(std::memory_order_relaxed)) {
		return;
	}
	pendingCommands.push_back(CamCommand{pendingPacketCount, CamCommandType::Finish, id, {}});
}

void Cams::addOutgoingPacket(uint64_t id, const NetworkMessage& msg)
{
	addMessagePacket(id, CamPacketType::Outgoing, msg);
}

void Cams::addCustomDataPacket(uint64_t id, const NetworkMessage& msg)
{
	addMessagePacket(id, CamPacketType::CustomData, msg);
}

void Cams::addMessagePacket(uint64_t id, CamPacketType type, const NetworkMessage& msg)
{
	const size_t length = msg.getLength();
	if (static_cast<size_t>(NetworkMessage::INITIAL_BUFFER_POSITION) + length > static_cast<size_t>(NETWORKMESSAGE_MAXSIZE)) {
		return;
	}

	addPacket(id, type, msg.getBuffer() + NetworkMessage::INITIAL_BUFFER_POSITION, length);
}

void Cams::addIncomingPacket(uint64_t id, const NetworkMessage& msg, uint16_t bufferStartPosition, uint16_t length)
{
	if (!shouldRecordIncoming() ||
	    static_cast<size_t>(bufferStartPosition) + length > static_cast<size_t>(NETWORKMESSAGE_MAXSIZE)) {
		return;
	}

	addPacket(id, CamPacketType::Incoming, msg.getBuffer() + bufferStartPosition, length);
}

void Cams::addPacket(uint64_t id, CamPacketType type, const uint8_t* payload, size_t length)
{
	if (!isEnabled() || id < minActiveCamId.load(std::memory_order_relaxed)) {
		return;
	}

	if (length == 0 || length > std::numeric_limits<uint16_t>::max() - 1) {
		return;
	}

	const size_t recordBytes = CAM_SLAB_RECORD_HEADER + 1 + length;
	if (recordBytes > CamSlab::CAPACITY) {
		return;
	}

	std::lock_guard<std::mutex> lockClass(queueLock);
	if (id < minActiveCamId.load(std::memory_order_relaxed)) {
		return;
	}

	const size_t newSlabs = countNewSlabs(openSlab.get(), 1, [&](size_t) { return recordBytes; });
	std::vector<std::unique_ptr<CamSlab>> fresh;
	if (!prepareSlabRecords(newSlabs, fresh)) {
		return;
	}

	// Under the lock, so timestamps stay in queue order.
	const uint64_t nowMs = static_cast<uint64_t>(OTSYS_TIME());
	const uint8_t typeByte = static_cast<uint8_t>(type);
	if (!writeSlabRecord(fresh, id, nowMs, &typeByte, 1, payload, length)) {
		fresh.clear();
		return;
	}
	releaseUnusedSlabs(fresh);
}

void Cams::addSyncPackets(uint64_t id, uint64_t timestampMs, const std::vector<std::vector<uint8_t>>& packets)
{
	if (!isEnabled() || id == 0 || id < minActiveCamId.load(std::memory_order_relaxed) || packets.empty()) {
		return;
	}

	bool any = false;
	for (const auto& buffer : packets) {
		if (isQueuedBatchPayload(buffer.size())) {
			any = true;
			break;
		}
	}
	if (!any) {
		return;
	}

	std::lock_guard<std::mutex> lockClass(queueLock);
	if (id < minActiveCamId.load(std::memory_order_relaxed)) {
		return;
	}

	// One timestamp for the whole batch. The player groups 0x03 records by equal time.
	const size_t newSlabs = countNewSlabs(openSlab.get(), packets.size(), [&](size_t index) {
		const size_t size = packets[index].size();
		if (!isQueuedBatchPayload(size)) {
			return size_t{0};
		}
		return CAM_SLAB_RECORD_HEADER + size;
	});

	std::vector<std::unique_ptr<CamSlab>> fresh;
	if (!prepareSlabRecords(newSlabs, fresh)) {
		return;
	}

	for (const auto& buffer : packets) {
		if (!isQueuedBatchPayload(buffer.size())) {
			continue;
		}
		if (!writeSlabRecord(fresh, id, timestampMs, buffer.data(), buffer.size(), nullptr, 0)) {
			fresh.clear();
			return;
		}
	}
	releaseUnusedSlabs(fresh);
}

std::unique_ptr<CamSlab> Cams::takeFreeSlab()
{
	if (!freeSlabs.empty()) {
		std::unique_ptr<CamSlab> slab = std::move(freeSlabs.back());
		freeSlabs.pop_back();
		slab->used = 0;
		slab->packetCount = 0;
		return slab;
	}

	if (freeSlabs.capacity() < CAM_FREE_SLAB_LIMIT) {
		freeSlabs.reserve(CAM_FREE_SLAB_LIMIT);
	}
	return std::make_unique<CamSlab>();
}

bool Cams::prepareSlabRecords(size_t newSlabs, std::vector<std::unique_ptr<CamSlab>>& fresh)
{
	if (getState() == THREAD_STATE_TERMINATED || !isEnabled()) {
		return false;
	}
	if (newSlabs == 0) {
		return true;
	}

	// Reserve before taking blocks. A throw here does not change the queue.
	if (freeSlabs.capacity() < CAM_FREE_SLAB_LIMIT) {
		freeSlabs.reserve(CAM_FREE_SLAB_LIMIT);
	}
	pendingSlabs.reserve(pendingSlabs.size() + newSlabs);
	fresh.reserve(newSlabs);
	for (size_t i = 0; i < newSlabs; ++i) {
		fresh.push_back(takeFreeSlab());
	}

	if (!reserveQueueBytes(newSlabs * CamSlab::CAPACITY)) {
		// Emergency already dropped the queue. These blocks were not linked into it.
		fresh.clear();
		return false;
	}
	return true;
}

bool Cams::writeSlabRecord(std::vector<std::unique_ptr<CamSlab>>& fresh, uint64_t camId, uint64_t timestampMs,
                           const uint8_t* prefix, size_t prefixLen, const uint8_t* payload, size_t payloadLen)
{
	if (prefixLen > std::numeric_limits<uint16_t>::max() ||
	    payloadLen > std::numeric_limits<uint16_t>::max() - prefixLen) {
		emergencyDisable("Cam packet does not fit in a queue slab.");
		freePendingQueues();
		return false;
	}

	const size_t dataSize = prefixLen + payloadLen;
	const size_t recordBytes = CAM_SLAB_RECORD_HEADER + dataSize;
	if (dataSize == 0 || recordBytes > CamSlab::CAPACITY) {
		emergencyDisable("Cam packet does not fit in a queue slab.");
		freePendingQueues();
		return false;
	}

	// The block is chosen before any byte is published. used/packetCount move only
	// after both the header and the payload are in place.
	if (!openSlab || static_cast<size_t>(openSlab->used) + recordBytes > CamSlab::CAPACITY) {
		if (fresh.empty()) {
			emergencyDisable("Cam queue ran out of slab space while copying a packet.");
			freePendingQueues();
			return false;
		}
		if (openSlab) {
			pendingSlabs.push_back(std::move(openSlab));
		}
		openSlab = std::move(fresh.back());
		fresh.pop_back();
	}

	uint8_t* const dst = openSlab->data.get() + openSlab->used;
	std::memcpy(dst, &camId, sizeof(camId));
	std::memcpy(dst + sizeof(uint64_t), &timestampMs, sizeof(timestampMs));
	const uint16_t packetSize = static_cast<uint16_t>(dataSize);
	std::memcpy(dst + sizeof(uint64_t) * 2, &packetSize, sizeof(packetSize));
	if (prefixLen != 0) {
		std::memcpy(dst + CAM_SLAB_RECORD_HEADER, prefix, prefixLen);
	}
	if (payloadLen != 0) {
		std::memcpy(dst + CAM_SLAB_RECORD_HEADER + prefixLen, payload, payloadLen);
	}

	openSlab->used = static_cast<uint32_t>(static_cast<size_t>(openSlab->used) + recordBytes);
	openSlab->packetCount += 1;
	pendingPacketCount += 1;
	return true;
}

void Cams::releaseUnusedSlabs(std::vector<std::unique_ptr<CamSlab>>& fresh)
{
	while (!fresh.empty()) {
		if (pendingPacketsBytes >= CamSlab::CAPACITY) {
			pendingPacketsBytes -= CamSlab::CAPACITY;
		}
		std::unique_ptr<CamSlab> slab = std::move(fresh.back());
		fresh.pop_back();
		if (!slab) {
			continue;
		}
		slab->used = 0;
		slab->packetCount = 0;
		if (freeSlabs.size() < CAM_FREE_SLAB_LIMIT && freeSlabs.size() < freeSlabs.capacity()) {
			freeSlabs.push_back(std::move(slab));
		}
	}
}

bool Cams::reserveQueueBytes(size_t bytes)
{
	if (getState() == THREAD_STATE_TERMINATED || !isEnabled()) {
		return false;
	}

	const size_t limit = maxQueueRamBytes.load(std::memory_order_relaxed);
	if (bytes > limit || pendingPacketsBytes > limit - bytes) {
		emergencyDisable("Cam queue memory limit exceeded (disk thread hung or storage unresponsive).");
		// The cams thread may be blocked in disk I/O, so the queue is released here
		freePendingQueues();
		return false;
	}

	pendingPacketsBytes += bytes;
	return true;
}

void Cams::freePendingQueues()
{
	std::vector<std::unique_ptr<CamSlab>>().swap(pendingSlabs);
	openSlab.reset();
	std::vector<std::unique_ptr<CamSlab>>().swap(freeSlabs);
	std::vector<CamCommand>().swap(pendingCommands);
	pendingPacketCount = 0;
	pendingPacketsBytes = 0;
}

void Cams::recycleProcessedSlab(std::unique_ptr<CamSlab> slab)
{
	if (!slab) {
		return;
	}

	slab->used = 0;
	slab->packetCount = 0;
	{
		std::lock_guard<std::mutex> lockClass(queueLock);
		if (freeSlabs.capacity() < CAM_FREE_SLAB_LIMIT) {
			freeSlabs.reserve(CAM_FREE_SLAB_LIMIT);
		}
		if (freeSlabs.size() < CAM_FREE_SLAB_LIMIT && freeSlabs.size() < freeSlabs.capacity()) {
			freeSlabs.push_back(std::move(slab));
		}
	}
	slab.reset();
}

void Cams::threadMain()
{
	try {
		recordLoop();
	} catch (const std::exception& e) {
		std::cout << "[Error - Cams::threadMain] " << e.what() << std::endl;
		emergencyDisable("Unexpected exception in cams thread.");
	} catch (...) {
		emergencyDisable("Unknown exception in cams thread.");
	}

	// The thread ends here (server shutdown, emergency or exception).
	// Release all RAM and make sure nothing is queued for a thread that no longer exists.
	std::unordered_map<uint64_t, CamData>().swap(camsMap);
	{
		std::lock_guard<std::mutex> lockClass(queueLock);
		setState(THREAD_STATE_TERMINATED);
		freePendingQueues();
	}
	threadExited.store(true);
}

void Cams::recordLoop()
{
	std::vector<std::unique_ptr<CamSlab>> localSlabs;
	std::unique_ptr<CamSlab> localOpen;
	std::vector<CamCommand> localCommands;
	size_t totalCamsBufferedBytes = 0;

	while (true) {
		bool lastIteration = false;
		uint64_t minActiveId = 0;
		size_t numPackets = 0;
		{
			std::unique_lock<std::mutex> lock(queueLock);
			// Only shutdown() wakes this thread up. Everything else is picked up on the next tick.
			queueSignal.wait_for(lock, CAMS_TICK_INTERVAL, [this]() {
				return getState() == THREAD_STATE_TERMINATED;
			});

			lastIteration = (getState() == THREAD_STATE_TERMINATED);
			// Read together with the handoff: a cam below this id got its last packets in this batch
			minActiveId = minActiveCamId.load(std::memory_order_relaxed);
			localSlabs.swap(pendingSlabs);
			if (openSlab && openSlab->packetCount > 0) {
				localOpen = std::move(openSlab);
			}
			localCommands.swap(pendingCommands);
			numPackets = pendingPacketCount;
			pendingPacketCount = 0;
			// Slabs just handed off are no longer queued. An empty open slab stays charged.
			pendingPacketsBytes = openSlab ? CamSlab::CAPACITY : 0;
		}
		lastProgressMs.store(camsSteadyTimeMs(), std::memory_order_relaxed);

		if (disabledUntilRestart.load()) {
			// Emergency: drop everything, no more disk I/O (not even on shutdown)
			return;
		}

		const size_t numCommands = localCommands.size();
		size_t cmdIdx = 0;
		size_t pktIdx = 0;

		auto applyCommand = [&](CamCommand& cmd) {
			switch (cmd.type) {
				case CamCommandType::Init: {
					CamData data;
					data.startTimeMs = cmd.initInfo.startTimeMs;
					data.lastPacketSec = cmd.initInfo.startTimeMs / 1000;
					data.lastSaveSec = cmd.initInfo.startTimeMs / 1000;
					data.character = std::move(cmd.initInfo.character);
					data.camId = cmd.camId;
					data.tempFileName = std::to_string(cmd.initInfo.accountId) + "_" +
					                    std::to_string(cmd.initInfo.playerId) + "_" +
					                    std::to_string(cmd.initInfo.playerLevel) + "_" +
					                    std::to_string(cmd.initInfo.startTimeMs / 1000) + "_" +
					                    std::to_string(cmd.initInfo.ip) + "_" +
					                    std::to_string(cmd.camId) + "_.cam";
					data.playerId = cmd.initInfo.playerId;
					data.playerLevel = cmd.initInfo.playerLevel;
					data.accountId = cmd.initInfo.accountId;
					data.ip = cmd.initInfo.ip;
					data.version = cmd.initInfo.version;
					data.fileBytes.reserve(bufferSizeLimitBytes.load(std::memory_order_relaxed));
					camsMap.emplace(cmd.camId, std::move(data));
					break;
				}
				case CamCommandType::Cancel: {
					auto it = camsMap.find(cmd.camId);
					if (it != camsMap.end()) {
						if (it->second.headerWritten) {
							std::error_code ec;
							std::filesystem::remove(std::filesystem::path(camsDir) / it->second.tempFileName, ec);
						}
						totalCamsBufferedBytes -= it->second.fileBytes.size();
						camsMap.erase(it);
					}
					break;
				}
				case CamCommandType::Finish: {
					auto it = camsMap.find(cmd.camId);
					if (it != camsMap.end()) {
						it->second.finished = true;
					}
					break;
				}
			}
		};

		auto consumeSlab = [&](CamSlab& slab) -> bool {
			if (!slab.data || slab.used > CamSlab::CAPACITY) {
				emergencyDisable("Cam queue slab is unusable.");
				return false;
			}

			const uint8_t* data = slab.data.get();
			size_t offset = 0;
			for (uint32_t n = 0; n < slab.packetCount; ++n) {
				while (cmdIdx < numCommands && localCommands[cmdIdx].afterPacketIndex <= pktIdx) {
					applyCommand(localCommands[cmdIdx++]);
				}

				if (offset > slab.used || slab.used - offset < CAM_SLAB_RECORD_HEADER) {
					emergencyDisable("Cam queue slab contains a truncated packet.");
					return false;
				}

				uint64_t camId = 0;
				uint64_t timestampMs = 0;
				uint16_t packetSize = 0;
				std::memcpy(&camId, data + offset, sizeof(camId));
				std::memcpy(&timestampMs, data + offset + sizeof(uint64_t), sizeof(timestampMs));
				std::memcpy(&packetSize, data + offset + sizeof(uint64_t) * 2, sizeof(packetSize));

				const size_t recordBytes = CAM_SLAB_RECORD_HEADER + static_cast<size_t>(packetSize);
				if (packetSize == 0 || slab.used - offset < recordBytes) {
					emergencyDisable("Cam queue slab contains a truncated packet.");
					return false;
				}

				// timestampMs through the payload is the on-disk record.
				const uint8_t* disk = data + offset + sizeof(uint64_t);
				const size_t diskLen = CAM_DISK_RECORD_HEADER + static_cast<size_t>(packetSize);
				auto it = camsMap.find(camId);
				if (it != camsMap.end() && !it->second.finished) {
					CamData& cam = it->second;
					cam.fileBytes.insert(cam.fileBytes.end(),
					                     reinterpret_cast<const char*>(disk),
					                     reinterpret_cast<const char*>(disk) + diskLen);
					totalCamsBufferedBytes += diskLen;
					cam.lastPacketSec = std::max<int64_t>(cam.lastPacketSec, static_cast<int64_t>(timestampMs / 1000));
				}

				offset += recordBytes;
				++pktIdx;
			}
			return true;
		};

		for (auto& slab : localSlabs) {
			if (!slab) {
				emergencyDisable("Cam queue slab is missing.");
				return;
			}
			if (!consumeSlab(*slab)) {
				return;
			}
			// Drop the block once its bytes live in the session buffers.
			recycleProcessedSlab(std::move(slab));
		}
		if (localOpen) {
			if (!consumeSlab(*localOpen)) {
				return;
			}
			recycleProcessedSlab(std::move(localOpen));
		}
		localSlabs.clear();
		if (pktIdx != numPackets) {
			emergencyDisable("Cam queue packet count does not match the slabs just consumed.");
			return;
		}
		while (cmdIdx < numCommands && localCommands[cmdIdx].afterPacketIndex <= pktIdx) {
			applyCommand(localCommands[cmdIdx++]);
		}

		localCommands.clear();

		const size_t maxTotalRam = maxTotalRamBytes.load(std::memory_order_relaxed);
		if (totalCamsBufferedBytes > maxTotalRam) {
			emergencyDisable("Total cam RAM limit exceeded. Disk is too slow to handle packet throughput.");
			return;
		}

		if (!camsMap.empty()) {
			const size_t limitBytes = bufferSizeLimitBytes.load(std::memory_order_relaxed);
			const int64_t intervalSec = static_cast<int64_t>(saveIntervalSec.load(std::memory_order_relaxed));
			const int64_t nowSec = OTSYS_TIME() / 1000;

			if (lastIteration) {
				size_t camsToSave = 0;
				for (const auto& entry : camsMap) {
					if (entry.second.headerWritten || !entry.second.fileBytes.empty()) {
						++camsToSave;
					}
				}
				if (camsToSave > 0) {
					std::cout << ">> Saving " << camsToSave << " cams, it may take a moment..." << std::endl;
				}
			}

			for (auto it = camsMap.begin(); it != camsMap.end();) {
				if (disabledUntilRestart.load()) {
					// Emergency raised by another thread while this one was writing
					return;
				}

				CamData& cam = it->second;
				// Cams below minActiveId were started before recording was switched off by a config reload
				const bool finalize = cam.finished || lastIteration || it->first < minActiveId;
				const bool saveDue = cam.fileBytes.size() >= limitBytes || (nowSec - cam.lastSaveSec >= intervalSec);
				if (!finalize && !saveDue) {
					++it;
					continue;
				}

				if (!cam.fileBytes.empty() || (finalize && cam.headerWritten)) {
					if (!flushCamToDisk(cam, finalize)) {
						emergencyDisable("Disk write failure while saving cam (disk full, I/O error or permission denied).");
						return;
					}
				}

				// Close has succeeded, or this short cam is being discarded. Either way the bytes leave RAM.
				totalCamsBufferedBytes -= cam.fileBytes.size();
				if (finalize) {
					it = camsMap.erase(it);
				} else {
					cam.fileBytes.clear();
					trimCamFileBuffer(cam.fileBytes, limitBytes);
					cam.lastSaveSec = nowSec;
					++it;
				}
			}
		}

		if (lastIteration) {
			return;
		}
	}
}

bool Cams::flushCamToDisk(CamData& cam, bool finalizeRename)
{
	// join() waits for the cams thread only as long as it keeps getting here
	lastProgressMs.store(camsSteadyTimeMs(), std::memory_order_relaxed);

	const std::filesystem::path dirPath(camsDir);
	const std::filesystem::path tempPath = dirPath / cam.tempFileName;

	const int64_t startTimeSec = cam.startTimeMs / 1000;
	const int64_t endTimeSec = std::max<int64_t>(startTimeSec, cam.lastPacketSec);
	if (finalizeRename && endTimeSec - startTimeSec < 3) {
		if (cam.headerWritten) {
			std::error_code ec;
			std::filesystem::remove(tempPath, ec);
			if (ec) {
				std::cout << "[Warning - Cams::flushCamToDisk] Failed to remove short cam '" << tempPath.string()
				          << "': " << ec.message() << std::endl;
			}
		}
		return true;
	}

	if (!cam.headerWritten || !cam.fileBytes.empty()) {
		const uint16_t nameLength = static_cast<uint16_t>(
			std::min<size_t>(cam.character.size(), std::numeric_limits<uint16_t>::max()));

		// Header stays out of fileBytes so a later clear() cannot rewrite it.
		std::vector<char> header;
		if (!cam.headerWritten) {
			header.reserve(static_cast<size_t>(8 + 1 + 4 + 2) + nameLength + sizeof(uint16_t) + sizeof(int64_t));
			appendCamBytes(header, "TIBIACAM", 8);
			appendCamPod<uint8_t>(header, 0x01);
			appendCamPod<uint32_t>(header, cam.playerId);
			appendCamPod<uint16_t>(header, nameLength);
			appendCamBytes(header, cam.character.data(), nameLength);
			appendCamPod<uint16_t>(header, cam.version);
			appendCamPod<int64_t>(header, cam.startTimeMs);
		}

		if (cam.fileBytes.size() > static_cast<size_t>(std::numeric_limits<std::streamsize>::max())) {
			std::cout << "[Error - Cams::flushCamToDisk] Cam '" << tempPath.string() << "' is too large to write."
			          << std::endl;
			return false;
		}

		const std::ios::openmode mode = std::ios::out | std::ios::binary |
		                                (cam.headerWritten ? std::ios::app : std::ios::trunc);
		std::ofstream out(tempPath, mode);
		if (!out.is_open()) {
			std::cout << "[Error - Cams::flushCamToDisk] Cannot open '" << tempPath.string()
			          << "' for writing (check permissions, disk space and open files limit)." << std::endl;
			return false;
		}

		if (!header.empty()) {
			out.write(header.data(), static_cast<std::streamsize>(header.size()));
			if (out.fail()) {
				std::cout << "[Error - Cams::flushCamToDisk] Failed while writing to '" << tempPath.string() << "'."
				          << std::endl;
				out.close();
				return false;
			}
		}

		if (!cam.fileBytes.empty()) {
			out.write(cam.fileBytes.data(), static_cast<std::streamsize>(cam.fileBytes.size()));
			if (out.fail()) {
				std::cout << "[Error - Cams::flushCamToDisk] Failed while writing to '" << tempPath.string() << "'."
				          << std::endl;
				out.close();
				return false;
			}
		}

		out.close();
		if (out.fail()) {
			std::cout << "[Error - Cams::flushCamToDisk] Failed while closing '" << tempPath.string() << "'." << std::endl;
			return false;
		}

		cam.headerWritten = true;
	}

	if (finalizeRename && cam.headerWritten) {
		const std::string readyFileName = std::to_string(cam.accountId) + "_" +
		                                  std::to_string(cam.playerId) + "_" +
		                                  std::to_string(cam.playerLevel) + "_" +
		                                  std::to_string(startTimeSec) + "_" +
		                                  std::to_string(endTimeSec) + "_" +
		                                  std::to_string(cam.ip) + "_" +
		                                  std::to_string(cam.camId) + "_.cam.ready";
		const std::filesystem::path readyPath = dirPath / readyFileName;

		std::error_code ec;
		std::filesystem::rename(tempPath, readyPath, ec);
		if (ec) {
			// The cam is complete on disk, only its final name is missing: not a reason to stop recording
			std::cout << "[Warning - Cams::flushCamToDisk] Failed to rename '" << tempPath.string()
			          << "' to '" << readyPath.string() << "': " << ec.message() << std::endl;
		}
	}

	return true;
}
