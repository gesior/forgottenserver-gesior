// Copyright 2022 The Forgotten Server Authors. All rights reserved.
// Use of this source code is governed by the GPL-2.0 License that can be found in the LICENSE file.

#ifndef FS_NETWORKMESSAGE_H_B853CFED58D1413A87ACED07B2926E03
#define FS_NETWORKMESSAGE_H_B853CFED58D1413A87ACED07B2926E03

#include "const.h"

class Item;
class Creature;
class Player;
struct Position;
class RSA;

class NetworkMessage
{
	public:
		using MsgSize_t = uint32_t;
		// Headers (GamePacketSizeU32 / big packets):
		// 4 bytes for unencrypted message size
		// 4 bytes for checksum
		// 4 bytes for encrypted message size
		// Standard headers use 2+4+2; we always reserve the larger space.
		static constexpr MsgSize_t INITIAL_BUFFER_POSITION = 12;
		enum {
			HEADER_LENGTH = 2,
			HEADER_LENGTH_U32 = 4,
			CHECKSUM_LENGTH = 4,
			XTEA_MULTIPLE = 8,
			MAX_BODY_LENGTH = NETWORKMESSAGE_MAXSIZE - INITIAL_BUFFER_POSITION - XTEA_MULTIPLE,
			MAX_PROTOCOL_BODY_LENGTH = MAX_BODY_LENGTH - 10
		};

		NetworkMessage() : buffer(std::make_unique<uint8_t[]>(NETWORKMESSAGE_MAXSIZE)) {}
		NetworkMessage(const NetworkMessage&) = delete;
		NetworkMessage& operator=(const NetworkMessage&) = delete;
		NetworkMessage(NetworkMessage&&) noexcept = default;
		NetworkMessage& operator=(NetworkMessage&&) noexcept = default;

		void reset() {
			info = {};
		}

		// simply read functions for incoming message
		uint8_t getByte() {
			if (!canRead(1)) {
				return 0;
			}

			return buffer[info.position++];
		}

		uint8_t getPreviousByte() {
			return buffer[--info.position];
		}

		template<typename T>
		T get() {
			if (!canRead(sizeof(T))) {
				return 0;
			}

			T v;
			memcpy(&v, buffer.get() + info.position, sizeof(T));
			info.position += sizeof(T);
			return v;
		}

		std::string getString(uint16_t stringLen = 0);
		Position getPosition();

		// skips count unknown/unused bytes in an incoming message
		void skipBytes(int32_t count) {
			info.position += count;
		}

		// simply write functions for outgoing message
		void addByte(uint8_t value) {
			if (!canAdd(1)) {
				return;
			}

			buffer[info.position++] = value;
			info.length++;
		}

		template<typename T>
		void add(T value) {
			if (!canAdd(sizeof(T))) {
				return;
			}

			memcpy(buffer.get() + info.position, &value, sizeof(T));
			info.position += sizeof(T);
			info.length += sizeof(T);
		}

		void addBytes(const char* bytes, size_t size);
		void addPaddingBytes(size_t n);

		void addString(const std::string& value);

		void addDouble(double value, uint8_t precision = 2);

		// write functions for complex types
		void addPosition(const Position& pos);
		void addItem(uint16_t id, uint8_t count);
		void addItem(const Item* item);
		void addItemId(uint16_t itemId);

		MsgSize_t getLength() const {
			return info.length;
		}

		void setLength(MsgSize_t newLength) {
			info.length = newLength;
		}

		MsgSize_t getBufferPosition() const {
			return info.position;
		}

		MsgSize_t getRemainingBufferLength() const { return info.length - info.position; }

		bool setBufferPosition(MsgSize_t pos) {
			if (pos < NETWORKMESSAGE_MAXSIZE - INITIAL_BUFFER_POSITION) {
				info.position = pos + INITIAL_BUFFER_POSITION;
				return true;
			}
			return false;
		}

		uint16_t getLengthHeader() const {
			return static_cast<uint16_t>(buffer[0] | buffer[1] << 8);
		}

		uint32_t getLengthHeaderU32() const {
			return static_cast<uint32_t>(buffer[0] | buffer[1] << 8 | buffer[2] << 16 | buffer[3] << 24);
		}

		bool isOverrun() const {
			return info.overrun;
		}

		uint8_t* getBuffer() {
			return buffer.get();
		}

		const uint8_t* getBuffer() const {
			return buffer.get();
		}

		uint8_t* getRemainingBuffer() { return buffer.get() + info.position; }

		uint8_t* getBodyBuffer(uint8_t headerLength = HEADER_LENGTH) {
			info.position = headerLength;
			info.bufferStart = headerLength;
			return buffer.get() + headerLength;
		}

		void setBufferStart(MsgSize_t start) {
			info.bufferStart = start;
		}

		MsgSize_t getBufferStart() const {
			return info.bufferStart;
		}

	protected:
		struct NetworkMessageInfo {
			MsgSize_t length = 0;
			MsgSize_t position = INITIAL_BUFFER_POSITION;
			MsgSize_t bufferStart = INITIAL_BUFFER_POSITION;
			bool overrun = false;
		};

		NetworkMessageInfo info;
		std::unique_ptr<uint8_t[]> buffer;

	private:
		bool canAdd(size_t size) const {
			return (size + info.position) < MAX_BODY_LENGTH;
		}

		bool canRead(MsgSize_t size) {
			if ((info.position + size) > (info.length + info.bufferStart) ||
			    (info.position + size) >= static_cast<MsgSize_t>(NETWORKMESSAGE_MAXSIZE)) {
				info.overrun = true;
				return false;
			}
			return true;
		}
};

#endif // #ifndef __NETWORK_MESSAGE_H__
