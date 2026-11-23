// Copyright 2026 The Forgotten Server Authors. All rights reserved.
// Use of this source code is governed by the GPL-2.0 License that can be found in the LICENSE file.

#ifndef FS_CAMS_H
#define FS_CAMS_H

#include "thread_holder_base.h"

class NetworkMessage;

enum class CamPacketType : uint8_t {
	Outgoing = 0x01,
	Incoming = 0x02,
	Synchronize = 0x03,
	CustomData = 0x04,
};

// One block of the packet queue. A record never crosses a block boundary, so a new
// block does not copy the bytes already queued. Layout, packed, host endian:
//   uint64 camId | uint64 timestampMs | uint16 packetSize | uint8 bytes[packetSize]
// packetSize is the type byte plus the payload. Bytes from timestampMs onward are
// exactly one on-disk record.
struct CamSlab {
	static constexpr size_t CAPACITY = 128 * 1024;

	// Default-initialized on purpose: `new uint8_t[n]()` would zero the block.
	CamSlab() : data(new uint8_t[CAPACITY]) {}

	uint32_t used = 0;
	uint32_t packetCount = 0;
	std::unique_ptr<uint8_t[]> data;
};

struct CamInitInfo {
	uint64_t camId = 0;
	int64_t startTimeMs = 0;
	std::string character;
	uint32_t playerId = 0;
	uint32_t playerLevel = 0;
	uint32_t accountId = 0;
	uint32_t ip = 0;
	uint16_t version = 0;
};

struct CamRotateResult {
	uint64_t camId = 0;
	int64_t startTimeMs = 0;
};

enum class CamCommandType : uint8_t {
	Init,
	Cancel,
	Finish,
};

struct CamCommand {
	// Packet count, not a byte offset. The cams thread applies the command
	// before the packet with this index.
	size_t afterPacketIndex = 0;
	CamCommandType type = CamCommandType::Init;
	uint64_t camId = 0;
	CamInitInfo initInfo = {};
};

struct CamData {
	// On-disk packet records only. The TIBIACAM header is written once, on the
	// first flush, and is not kept in this buffer. Cleared after a successful close.
	std::vector<char> fileBytes;
	int64_t startTimeMs = 0;
	int64_t lastPacketSec = 0;
	int64_t lastSaveSec = 0;
	std::string character;
	std::string tempFileName;
	uint64_t camId = 0;
	uint32_t playerId = 0;
	uint32_t playerLevel = 0;
	uint32_t accountId = 0;
	uint32_t ip = 0;
	uint16_t version = 0;
	bool headerWritten = false;
	bool finished = false;
};

#ifdef _MSC_VER
#pragma warning(push)
#pragma warning(disable : 4324) // structure was padded due to alignment specifier
#endif
class Cams : public ThreadHolder<Cams>
{
	public:
		Cams() = default;

		void startup();
		void onConfigReload();
		void shutdown();
		void join();
		void threadMain();

		uint64_t initCam(const std::string& character, uint32_t playerId, uint32_t playerLevel,
		                 uint32_t accountId, uint32_t ip, uint16_t version);
		CamRotateResult rotateCam(uint64_t oldCamId, const std::string& character, uint32_t playerId,
		                          uint32_t playerLevel, uint32_t accountId, uint32_t ip, uint16_t version,
		                          const std::vector<std::vector<uint8_t>>& startPackets);
		void cancelCam(uint64_t id);
		void finishCam(uint64_t id);

		void addOutgoingPacket(uint64_t id, const NetworkMessage& msg);
		void addIncomingPacket(uint64_t id, const NetworkMessage& msg, uint16_t bufferStartPosition, uint16_t length);
		void addSyncPackets(uint64_t id, uint64_t timestampMs, const std::vector<std::vector<uint8_t>>& packets);
		void addCustomDataPacket(uint64_t id, const NetworkMessage& msg);

		// Blocks cam recording until the server restarts (a config reload cannot enable it again).
		// Safe to call from any thread, with or without queueLock held: it only sets atomic flags.
		void emergencyDisable(const char* reason) noexcept;

		bool isEnabled() const {
			return enabled.load(std::memory_order_relaxed) && !disabledUntilRestart.load(std::memory_order_relaxed);
		}
		static uint64_t getMinActiveCamId() {
			return minActiveCamId.load(std::memory_order_relaxed);
		}
		static bool shouldRecordIncoming() {
			return recordIncomingPackets.load(std::memory_order_relaxed);
		}
		static int64_t getSyncIntervalMs() {
			return syncIntervalMs.load(std::memory_order_relaxed);
		}
		static int64_t getMaxDurationMs() {
			return maxDurationMs.load(std::memory_order_relaxed);
		}

	private:
		void applyConfigLimits();
		void applyRecordingState(bool recording);
		void cleanupOldCams() const noexcept;

		void addMessagePacket(uint64_t id, CamPacketType type, const NetworkMessage& msg);
		void addPacket(uint64_t id, CamPacketType type, const uint8_t* payload, size_t length);

		// Require queueLock.
		std::unique_ptr<CamSlab> takeFreeSlab();
		bool prepareSlabRecords(size_t newSlabs, std::vector<std::unique_ptr<CamSlab>>& fresh);
		bool writeSlabRecord(std::vector<std::unique_ptr<CamSlab>>& fresh, uint64_t camId, uint64_t timestampMs,
		                     const uint8_t* prefix, size_t prefixLen, const uint8_t* payload, size_t payloadLen);
		void releaseUnusedSlabs(std::vector<std::unique_ptr<CamSlab>>& fresh);
		// `bytes` is memory the queue keeps. A slab is charged for its whole block.
		bool reserveQueueBytes(size_t bytes);
		void freePendingQueues();

		// Frees the block after the session buffer has copied it, or keeps it on the free list.
		void recycleProcessedSlab(std::unique_ptr<CamSlab> slab);
		void recordLoop();
		bool flushCamToDisk(CamData& cam, bool finalizeRename);

		static std::atomic<uint64_t> minActiveCamId;
		static std::atomic<bool> recordIncomingPackets;
		static std::atomic<int64_t> syncIntervalMs;
		static std::atomic<int64_t> maxDurationMs;

		// Read-mostly flags and limits (shares the first cache line with ThreadHolder<Cams>).
		// Config switch, written only by the dispatcher thread (startup and config reload)
		std::atomic<bool> enabled{false};
		// One-way latch set by an emergency or an invalid camsDir, never cleared
		std::atomic<bool> disabledUntilRestart{false};
		// Let join() detect a cams thread that is blocked in disk I/O forever
		std::atomic<bool> threadExited{true};
		std::atomic<int32_t> saveIntervalSec{30};
		std::atomic<size_t> bufferSizeLimitBytes{100 * 1024};
		std::atomic<size_t> maxTotalRamBytes{512 * 1024 * 1024};
		std::atomic<size_t> maxQueueRamBytes{128 * 1024 * 1024};

		// Shared producer/consumer queue state, isolated to avoid false sharing with read-mostly flags.
		alignas(64) std::mutex queueLock;
		std::condition_variable queueSignal;
		// Sum of the capacities of slabs still in the queue, including the open slab.
		size_t pendingPacketsBytes = 0;
		// Number of packet records in pendingSlabs and the open slab.
		size_t pendingPacketCount = 0;
		std::unique_ptr<CamSlab> openSlab;
		std::vector<std::unique_ptr<CamSlab>> pendingSlabs;
		std::vector<std::unique_ptr<CamSlab>> freeSlabs;
		std::vector<CamCommand> pendingCommands;
		std::atomic<uint64_t> nextCamId{1};

		// Accessed exclusively from the cams thread (join() reads lastProgressMs only on shutdown).
		alignas(64) std::atomic<int64_t> lastProgressMs{0};
		std::unordered_map<uint64_t, CamData> camsMap;
		std::string camsDir;
		std::mutex joinLock;
};
#ifdef _MSC_VER
#pragma warning(pop)
#endif

extern Cams g_cams;

#endif
