// Copyright 2022 The Forgotten Server Authors. All rights reserved.
// Use of this source code is governed by the GPL-2.0 License that can be found in the LICENSE file.

#include "otpch.h"

#include "protocol.h"
#include "outputmessage.h"
#include "rsa.h"
#include "xtea.h"
#include "configmanager.h"

extern ConfigManager g_config;

namespace {

void XTEA_encrypt(OutputMessage& msg, const xtea::round_keys& key)
{
	// The message must be a multiple of 8
	size_t paddingBytes = msg.getLength() % 8u;
	if (paddingBytes != 0) {
		msg.addPaddingBytes(8 - paddingBytes);
	}

	uint8_t* buffer = msg.getOutputBuffer();
	xtea::encrypt(buffer, msg.getLength(), key);
}

bool XTEA_decrypt(NetworkMessage& msg, const xtea::round_keys& key, bool bigPackets)
{
	const uint32_t headerChecksumSize = bigPackets ? 8u : 6u; // size header + checksum
	const uint32_t fullHeaderSize = bigPackets ? 12u : 8u; // + inner message size

	if (((msg.getLength() - headerChecksumSize) & 7) != 0) {
		return false;
	}

	uint8_t* buffer = msg.getBuffer() + msg.getBufferPosition();
	xtea::decrypt(buffer, msg.getLength() - headerChecksumSize, key);

	uint32_t innerLength = bigPackets ? msg.get<uint32_t>() : msg.get<uint16_t>();
	if (innerLength + fullHeaderSize > msg.getLength()) {
		return false;
	}

	msg.setBufferStart(msg.getBufferPosition());
	msg.setLength(innerLength);
	return true;
}

}

Protocol::Protocol(Connection_ptr connection) : connection(std::move(connection))
{
	if (g_config.getBoolean(ConfigManager::PACKET_SIZE_U32)) {
		bigPackets = true;
	}
}

void Protocol::onSendMessage(const OutputMessage_ptr& msg) const
{
	if (!rawMessages) {
		// Inner length (included in XTEA plaintext) or outer length when unencrypted
		msg->writeMessageLength(bigPackets);

		if (encryptionEnabled) {
			XTEA_encrypt(*msg, key);
			msg->addCryptoHeader(checksumEnabled, bigPackets);
		}
	}
}

void Protocol::onRecvMessage(NetworkMessage& msg)
{
	if (encryptionEnabled && !XTEA_decrypt(msg, key, bigPackets)) {
		return;
	}

	parsePacket(msg);
}

OutputMessage_ptr Protocol::getOutputBuffer(int32_t size)
{
	//dispatcher thread
	if (!outputBuffer) {
		outputBuffer = OutputMessagePool::getOutputMessage();
	} else if ((outputBuffer->getLength() + size) > NetworkMessage::MAX_PROTOCOL_BODY_LENGTH) {
		send(outputBuffer);
		outputBuffer = OutputMessagePool::getOutputMessage();
	}
	return outputBuffer;
}

bool Protocol::RSA_decrypt(NetworkMessage& msg)
{
	if (msg.getRemainingBufferLength() < RSA_BUFFER_LENGTH) {
		return false;
	}

	tfs::rsa::decrypt(msg.getRemainingBuffer(), RSA_BUFFER_LENGTH);
	return msg.getByte() == 0;
}

uint32_t Protocol::getIP() const
{
	if (auto connection = getConnection()) {
		return connection->getIP();
	}

	return 0;
}

bool Protocol::isOtcProxy() const
{
	if (auto connection = getConnection()) {
		return connection->isOtcProxy();
	}

	return false;
}

bool Protocol::isHaProxy() const
{
	if (auto connection = getConnection()) {
		return connection->isHaProxy();
	}

	return false;
}
