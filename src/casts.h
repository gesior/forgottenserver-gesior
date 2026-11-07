// Copyright 2026 The Forgotten Server Authors. All rights reserved.
// Use of this source code is governed by the GPL-2.0 License that can be found in the LICENSE file.

#ifndef TFS_CASTS
#define TFS_CASTS

#include "position.h"

class Player;
class ProtocolGame;
using ProtocolGame_ptr = std::shared_ptr<ProtocolGame>;

struct CastEntryInfo {
	Player* player = nullptr;
	std::string name;
	uint32_t level = 0;
	uint32_t publicViewers = 0;
	bool passwordProtected = false;
};

enum class CastSwitchDirection : uint8_t {
	Next,
	Previous,
};

enum class CastLoginStatus : uint8_t {
	Allowed,
	Disabled,
	InvalidAdminPassword,
	TargetOffline,
	TargetNotBroadcasting,
	InvalidCastPassword,
	ServerViewerLimitReached,
	CastViewerLimitReached,
};

class Casts
{
	public:
		Casts() = delete;

		static constexpr const char* CAST_ACCOUNT_NAME_1 = "cast";
		static constexpr const char* CAST_ACCOUNT_NAME_2 = "casts";
		static constexpr const char* ADMIN_CAST_ACCOUNT_NAME_1 = "admincast";
		static constexpr const char* ADMIN_CAST_ACCOUNT_NAME_2 = "admincasts";
		static constexpr size_t MAX_PUBLIC_LIST_ENTRIES = 255;
		static constexpr size_t MAX_ADMIN_LIST_ENTRIES = 50;
		static constexpr size_t MAX_CAST_PASSWORD_LENGTH = 32;
		static constexpr int64_t SWITCH_COOLDOWN_MS = 500;
		static constexpr int64_t ADMIN_SWITCH_COOLDOWN_MS = 300;

		static bool isCastAccount(const std::string& accountName);
		static bool isAdminCastAccount(const std::string& accountName);
		static bool validateAdminPassword(const std::string& password);
		static bool isValidCastPassword(const std::string& password);

		static void onPlayerAdded(Player* player);
		static void onPlayerRemoved(Player* player);

		static bool setBroadcasting(Player* player, bool enabled);
		static bool setPrivateBroadcast(Player* player, const std::string& password);
		static bool isBroadcasting(const Player* player);
		static bool isPublicCast(const Player* player);

		static uint32_t getPublicViewerCount(const Player* player);
		static uint32_t getTotalPublicViewers();
		static int32_t getDatabaseViewerValue(const Player* player);

		static std::vector<CastEntryInfo> getPublicCastsForLoginList(const std::string& viewerPassword);
		static std::vector<CastEntryInfo> getAdminTargetsForLoginList();

		static CastLoginStatus canJoinCast(const Player* target, bool isAdmin, const std::string& password, std::string& errorMessage);
		static bool addViewer(Player* target, const ProtocolGame_ptr& viewer, bool isAdmin);
		static void removeViewer(const ProtocolGame_ptr& viewer);

		static bool switchCastByDirection(const ProtocolGame_ptr& viewer, Direction dir);
		static bool switchCastByName(const ProtocolGame_ptr& viewer, const std::string& targetName);

		static const std::vector<ProtocolGame_ptr>& getViewerProtocols(const Player* player);

		static void disconnectAllViewers(bool includeAdmins, const std::string& reason);
		static void disablePublicCasts();
};

#endif
