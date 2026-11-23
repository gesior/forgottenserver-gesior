// Copyright 2022 The Forgotten Server Authors. All rights reserved.
// Use of this source code is governed by the GPL-2.0 License that can be found in the LICENSE file.

#include "otpch.h"

#include "protocollogin.h"

#include "outputmessage.h"
#include "tasks.h"

#include "configmanager.h"
#include "iologindata.h"
#include "ban.h"
#include "casts.h"
#include "game.h"
#include "vocation.h"

#include <cctype>
#include <fmt/chrono.h>

extern ConfigManager g_config;
extern Game g_game;
extern Vocations g_vocations;

namespace {

std::string vocationAbbreviation(uint16_t vocationId)
{
	Vocation* vocation = g_vocations.getVocation(vocationId);
	if (!vocation) {
		return "N";
	}

	std::string abbreviation;
	bool takeNext = true;
	for (unsigned char ch : vocation->getVocName()) {
		if (ch == ' ') {
			takeNext = true;
			continue;
		}
		if (takeNext) {
			abbreviation.push_back(static_cast<char>(std::toupper(ch)));
			takeNext = false;
		}
	}
	return abbreviation.empty() ? "N" : abbreviation;
}

std::string trimCopy(std::string value)
{
	const auto notSpace = [](unsigned char ch) { return std::isspace(ch) == 0; };
	value.erase(value.begin(), std::find_if(value.begin(), value.end(), notSpace));
	value.erase(std::find_if(value.rbegin(), value.rend(), notSpace).base(), value.end());
	return value;
}

// Format cam start time safely; invalid DB timestamps throw fmt::format_error in fmt::localtime.
std::string formatCamDate(time_t timestamp)
{
	if (timestamp > 0) {
		try {
			return fmt::format("{:%Y-%m-%d %H:%M:%S}", fmt::localtime(timestamp));
		} catch (const fmt::format_error&) {
		}
	}
	return "[NO TIME]";
}

std::string formatAccountCamName(const AccountCam& cam, bool useTitle)
{
	std::string label = cam.playerName.empty() ? "Unknown" : cam.playerName;
	if (useTitle) {
		std::string title = trimCopy(cam.title);
		if (!title.empty()) {
			label = std::move(title);
		}
	}

	if (label.size() > 25) {
		label.resize(22);
		label += "...";
	}

	return fmt::format("{} {}, {:d} {};{:X}", formatCamDate(cam.dateStart), label, cam.level, vocationAbbreviation(cam.vocationId), cam.id);
}

constexpr uint32_t CAM_PUBLIC_PAGE_SIZE = 50;
constexpr uint64_t CAM_PUBLIC_MAX_PAGES = 100;
constexpr uint32_t CAM_LIST_HARD_LIMIT = 255;

enum class CamPasswordKind { PublicPage, AccessCode, InvalidPage };

struct CamPassword {
	CamPasswordKind kind = CamPasswordKind::PublicPage;
	uint64_t page = 1;
};

bool isCamAccount(const std::string& accountName)
{
	return strcasecmp(accountName.c_str(), "cam") == 0 || strcasecmp(accountName.c_str(), "cams") == 0;
}

CamPassword parseCamPassword(const std::string& password)
{
	if (password.empty()) {
		return {CamPasswordKind::PublicPage, 1};
	}

	const bool allDigits = std::all_of(password.begin(), password.end(), [](unsigned char ch) {
		return std::isdigit(ch) != 0;
	});
	if (allDigits && password.size() <= 9) {
		uint64_t page = 0;
		for (char ch : password) {
			page = page * 10 + static_cast<uint64_t>(ch - '0');
		}
		if (page < 1) {
			return {CamPasswordKind::InvalidPage, 0};
		}
		return {CamPasswordKind::PublicPage, page};
	}

	return {CamPasswordKind::AccessCode, 0};
}

std::string loginWorldIp(const Connection& connection)
{
	if (connection.isOtcProxy()) {
		return "127.0.0.1";
	}
	if (connection.isHaProxy()) {
		return g_config.getString(ConfigManager::STATUS_IP);
	}
	return g_config.getString(ConfigManager::IP);
}

std::string camWorldIp(const Connection& connection)
{
	const std::string& configured = g_config.getString(ConfigManager::CAM_PLAYER_IP);
	if (!configured.empty()) {
		return configured;
	}
	return loginWorldIp(connection);
}

} // namespace

void ProtocolLogin::disconnectClient(const std::string& message, uint16_t version)
{
	auto output = OutputMessagePool::getOutputMessage();

	output->addByte(version >= 1076 ? 0x0B : 0x0A);
	output->addString(message);
	send(output);

	disconnect();
}

void ProtocolLogin::getCharacterList(const std::string& accountName, const std::string& password, const std::string& token, uint16_t version)
{
	auto connection = getConnection();
	if (!connection) {
		return;
	}

	Account account;
	if (!IOLoginData::loginserverAuthentication(accountName, password, account)) {
		disconnectClient("Account name or password is not correct.", version);
		return;
	}

	uint32_t ticks = time(nullptr) / AUTHENTICATOR_PERIOD;

	auto output = OutputMessagePool::getOutputMessage();
	if (!account.key.empty()) {
		if (token.empty() || !(token == generateToken(account.key, ticks) || token == generateToken(account.key, ticks - 1) || token == generateToken(account.key, ticks + 1))) {
			output->addByte(0x0D);
			output->addByte(0);
			send(output);
			disconnect();
			return;
		}
		output->addByte(0x0C);
		output->addByte(0);
	}

	const std::string& motd = g_config.getString(ConfigManager::MOTD);
	if (!motd.empty()) {
		//Add MOTD
		output->addByte(0x14);
		output->addString(fmt::format("{:d}\n{:s}", g_game.getMotdNum(), motd));
	}

	//Add session key
	output->addByte(0x28);
	output->addString(accountName + "\n" + password + "\n" + token + "\n" + std::to_string(ticks));

	//Add char list
	const bool onlineOffline = g_config.getBoolean(ConfigManager::ONLINE_OFFLINE_CHARLIST);
	const uint8_t gameWorlds = onlineOffline ? 2 : 1;
	const uint8_t characterCount = static_cast<uint8_t>(std::min<size_t>(CAM_LIST_HARD_LIMIT, account.characters.size()));
	const int32_t camPlayerPort = g_config.getNumber(ConfigManager::CAM_PLAYER_PORT);
	const uint32_t slotsLeft = CAM_LIST_HARD_LIMIT - characterCount;
	const uint32_t configuredCams = static_cast<uint32_t>(std::max<int32_t>(0, g_config.getNumber(ConfigManager::CAM_ACCOUNT_LIST_SIZE)));
	const uint32_t camLimit = std::min(configuredCams, slotsLeft);

	std::vector<AccountCam> cams;
	if (camPlayerPort > 0 && camLimit > 0) {
		cams = IOLoginData::getAccountCams(account.id, camLimit);
	}

	const size_t camSlots = std::min(cams.size(), static_cast<size_t>(slotsLeft));
	const bool showCams = camSlots > 0;
	const uint8_t camWorldId = gameWorlds;
	const std::string worldIp = loginWorldIp(*connection);
	const std::string camIp = camWorldIp(*connection);

	output->addByte(0x64);
	output->addByte(static_cast<uint8_t>(gameWorlds + (showCams ? 1 : 0)));

	if (onlineOffline) {
		for (uint8_t i = 0; i < gameWorlds; i++) {
			output->addByte(i); // world id
			output->addString(i == 0 ? "Offline" : "Online");
			output->addString(worldIp);
			output->add<uint16_t>(g_config.getNumber(ConfigManager::GAME_PORT));
			output->addByte(0);
		}
	} else {
		output->addByte(0); // world id
		output->addString(g_config.getString(ConfigManager::SERVER_NAME));
		output->addString(worldIp);
		output->add<uint16_t>(g_config.getNumber(ConfigManager::GAME_PORT));
		output->addByte(0);
	}

	if (showCams) {
		output->addByte(camWorldId);
		output->addString("Cam");
		output->addString(camIp);
		output->add<uint16_t>(static_cast<uint16_t>(camPlayerPort));
		output->addByte(0);
	}

	output->addByte(static_cast<uint8_t>(characterCount + camSlots));
	for (uint8_t i = 0; i < characterCount; i++) {
		const std::string& character = account.characters[i];
		if (onlineOffline) {
			output->addByte(g_game.getPlayerByName(character) ? 1 : 0);
		} else {
			output->addByte(0);
		}
		output->addString(character);
	}

	for (size_t i = 0; i < camSlots; ++i) {
		output->addByte(camWorldId);
		output->addString(formatAccountCamName(cams[i], false));
	}

	//Add premium days
	output->addByte(0);
	if (g_config.getBoolean(ConfigManager::FREE_PREMIUM)) {
		output->addByte(1);
		output->add<uint32_t>(0);
	} else {
		output->addByte(account.premiumEndsAt > time(nullptr) ? 1 : 0);
		output->add<uint32_t>(account.premiumEndsAt);
	}

	send(output);

	disconnect();
}

void ProtocolLogin::getCastsList(const std::string& accountName, const std::string& password, uint16_t version)
{
	auto connection = getConnection();
	if (!connection) {
		return;
	}

	const bool adminMode = Casts::isAdminCastAccount(accountName);

	std::string errorMessage;
	if (Casts::canJoinCast(nullptr, adminMode, password, errorMessage) != CastLoginStatus::Allowed) {
		disconnectClient(errorMessage, version);
		return;
	}

	std::vector<CastEntryInfo> entries = adminMode ? Casts::getAdminTargetsForLoginList() : Casts::getPublicCastsForLoginList(password);
	if (entries.empty()) {
		disconnectClient(adminMode ? "There are currently no players online." : "There are currently no active casts.", version);
		return;
	}

	const uint32_t ticks = static_cast<uint32_t>(time(nullptr) / AUTHENTICATOR_PERIOD);
	auto output = OutputMessagePool::getOutputMessage();

	// Add session key
	output->addByte(0x28);
	output->addString(accountName + "\n" + password + "\n\n" + std::to_string(ticks));

	// Add cast list as character list
	output->addByte(0x64);

	const uint8_t size = static_cast<uint8_t>(std::min<size_t>(std::numeric_limits<uint8_t>::max(), entries.size()));

	std::string worldIp;
	if (connection->isOtcProxy()) {
		worldIp = "127.0.0.1";
	} else if (connection->isHaProxy()) {
		const std::string& statusIp = g_config.getString(ConfigManager::STATUS_IP);
		worldIp = !statusIp.empty() ? statusIp : g_config.getString(ConfigManager::IP);
	} else {
		worldIp = g_config.getString(ConfigManager::IP);
	}
	const uint16_t gamePort = static_cast<uint16_t>(g_config.getNumber(ConfigManager::GAME_PORT));

	output->addByte(size);
	for (uint8_t i = 0; i < size; ++i) {
		const CastEntryInfo& entry = entries[i];
		output->addByte(i);
		if (adminMode) {
			output->addString(fmt::format("Lvl {:d}", entry.level));
		} else {
			std::string worldName = fmt::format("{:d} {:s} | Lvl {:d}", entry.publicViewers, (entry.publicViewers == 1 ? "viewer" : "viewers"), entry.level);
			if (entry.passwordProtected) {
				worldName += " PASS";
			}
			output->addString(worldName);
		}
		output->addString(worldIp);
		output->add<uint16_t>(gamePort);
		output->addByte(0);
	}

	output->addByte(size);
	for (uint8_t i = 0; i < size; ++i) {
		output->addByte(i);
		output->addString(entries[i].name);
	}

	// Account status (OK, Unlimited)
	output->addByte(0);
	output->addByte(1);
	output->add<uint32_t>(0);

	send(output);

	disconnect();
}

void ProtocolLogin::getCamsList(const std::string& accountName, const std::string& password, uint16_t version)
{
	auto connection = getConnection();
	if (!connection) {
		return;
	}

	const int32_t camPlayerPort = g_config.getNumber(ConfigManager::CAM_PLAYER_PORT);
	if (camPlayerPort <= 0 || camPlayerPort > 65535) {
		disconnectClient("Cam player is disabled.", version);
		return;
	}

	const CamPassword parsed = parseCamPassword(password);
	if (parsed.kind == CamPasswordKind::InvalidPage) {
		disconnectClient("Invalid page.", version);
		return;
	}

	std::vector<AccountCam> cams;
	if (parsed.kind == CamPasswordKind::PublicPage) {
		if (parsed.page > CAM_PUBLIC_MAX_PAGES) {
			disconnectClient(fmt::format("Only the first {:d} pages of public recordings are available.", CAM_PUBLIC_MAX_PAGES), version);
			return;
		}

		cams = IOLoginData::getPublicCams(parsed.page, CAM_PUBLIC_PAGE_SIZE);
		if (cams.empty()) {
			disconnectClient(parsed.page == 1 ? "There are currently no public recordings." : "There are no public recordings on this page.", version);
			return;
		}
	} else {
		cams = IOLoginData::getCamsByAccessCode(password);
		if (cams.empty()) {
			disconnectClient("Recording not found or the access code is invalid.", version);
			return;
		}
	}

	if (cams.size() > CAM_LIST_HARD_LIMIT) {
		cams.resize(CAM_LIST_HARD_LIMIT);
	}

	const uint32_t ticks = static_cast<uint32_t>(time(nullptr) / AUTHENTICATOR_PERIOD);
	auto output = OutputMessagePool::getOutputMessage();

	output->addByte(0x28);
	output->addString(accountName + "\n" + password + "\n\n" + std::to_string(ticks));

	output->addByte(0x64);
	output->addByte(1);
	output->addByte(0);
	output->addString("Cam");
	output->addString(camWorldIp(*connection));
	output->add<uint16_t>(static_cast<uint16_t>(camPlayerPort));
	output->addByte(0);

	const bool useTitle = parsed.kind == CamPasswordKind::PublicPage;
	const uint8_t size = static_cast<uint8_t>(cams.size());
	output->addByte(size);
	for (uint8_t i = 0; i < size; ++i) {
		output->addByte(0);
		output->addString(formatAccountCamName(cams[i], useTitle));
	}

	output->addByte(0);
	output->addByte(1);
	output->add<uint32_t>(0);

	send(output);
	disconnect();
}

void ProtocolLogin::onRecvFirstMessage(NetworkMessage& msg)
{
	if (g_game.getGameState() == GAME_STATE_SHUTDOWN) {
		disconnect();
		return;
	}

	msg.skipBytes(2); // client OS

	uint16_t version = msg.get<uint16_t>();
	if (version >= 971) {
		msg.skipBytes(17);
	} else {
		msg.skipBytes(12);
	}
	/*
	 * Skipped bytes:
	 * 4 bytes: protocolVersion
	 * 12 bytes: dat, spr, pic signatures (4 bytes each)
	 * 1 byte: 0
	 */

	if (version <= 760) {
		disconnectClient(fmt::format("Only clients with protocol {:s} allowed!", CLIENT_VERSION_STR), version);
		return;
	}

	if (!Protocol::RSA_decrypt(msg)) {
		disconnect();
		return;
	}

	xtea::key key;
	key[0] = msg.get<uint32_t>();
	key[1] = msg.get<uint32_t>();
	key[2] = msg.get<uint32_t>();
	key[3] = msg.get<uint32_t>();
	enableXTEAEncryption();
	setXTEAKey(std::move(key));

	if (version < CLIENT_VERSION_MIN || version > CLIENT_VERSION_MAX) {
		disconnectClient(fmt::format("Only clients with protocol {:s} allowed!", CLIENT_VERSION_STR), version);
		return;
	}

	if (g_game.getGameState() == GAME_STATE_STARTUP) {
		disconnectClient("Gameworld is starting up. Please wait.", version);
		return;
	}

	if (g_game.getGameState() == GAME_STATE_MAINTAIN) {
		disconnectClient("Gameworld is under maintenance.\nPlease re-connect in a while.", version);
		return;
	}

	BanInfo banInfo;
	auto connection = getConnection();
	if (!connection) {
		return;
	}

	if (IOBan::isIpBanned(connection->getIP(), banInfo)) {
		if (banInfo.reason.empty()) {
			banInfo.reason = "(none)";
		}

		disconnectClient(fmt::format("Your IP has been banned until {:s} by {:s}.\n\nReason specified:\n{:s}", formatDateShort(banInfo.expiresAt), banInfo.bannedBy, banInfo.reason), version);
		return;
	}

	std::string accountName = msg.getString();
	if (accountName.empty()) {
		disconnectClient("Invalid account name.", version);
		return;
	}

	std::string password = msg.getString();
	if (Casts::isCastAccount(accountName) || Casts::isAdminCastAccount(accountName)) {
		auto thisPtr = std::static_pointer_cast<ProtocolLogin>(shared_from_this());
		g_dispatcher.addTask(createTask(std::bind(&ProtocolLogin::getCastsList, thisPtr, accountName, password, version)));
		return;
	}
	if (isCamAccount(accountName)) {
		auto thisPtr = std::static_pointer_cast<ProtocolLogin>(shared_from_this());
		g_dispatcher.addTask(createTask(std::bind(&ProtocolLogin::getCamsList, thisPtr, accountName, password, version)));
		return;
	}
	if (password.empty()) {
		disconnectClient("Invalid password.", version);
		return;
	}

	// read authenticator token and stay logged in flag from last 128 bytes
	msg.skipBytes(msg.getRemainingBufferLength() - Protocol::RSA_BUFFER_LENGTH);
	if (!Protocol::RSA_decrypt(msg)) {
		disconnectClient("Invalid authentication token.", version);
		return;
	}

	std::string authToken = msg.getString();

	auto thisPtr = std::static_pointer_cast<ProtocolLogin>(shared_from_this());
	g_dispatcher.addTask(createTask(std::bind(&ProtocolLogin::getCharacterList, thisPtr, accountName, password, authToken, version)));
}
