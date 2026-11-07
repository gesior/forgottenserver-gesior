// Copyright 2026 The Forgotten Server Authors. All rights reserved.
// Use of this source code is governed by the GPL-2.0 License that can be found in the LICENSE file.

#include "otpch.h"

#include "casts.h"

#include "configmanager.h"
#include "game.h"
#include "player.h"
#include "protocolgame.h"
#include "tools.h"

extern ConfigManager g_config;
extern Game g_game;

namespace {

struct PlayerCastState {
	Player* player = nullptr;
	bool broadcasting = false;
	std::vector<ProtocolGame_ptr> publicViewers;
	std::vector<ProtocolGame_ptr> adminViewers;
	std::vector<ProtocolGame_ptr> allViewers;

	void rebuildAllViewers() {
		allViewers.clear();
		allViewers.reserve(publicViewers.size() + adminViewers.size());
		allViewers.insert(allViewers.end(), publicViewers.begin(), publicViewers.end());
		allViewers.insert(allViewers.end(), adminViewers.begin(), adminViewers.end());
		if (player) {
			player->setHasCastViewers(!allViewers.empty());
		}
	}
};

std::unordered_map<const Player*, PlayerCastState> g_castStates;
uint32_t g_totalPublicViewers = 0;
const std::vector<ProtocolGame_ptr> g_emptyViewers;

bool removeViewerFromVector(std::vector<ProtocolGame_ptr>& viewers, const ProtocolGame_ptr& viewer)
{
	auto it = std::find(viewers.begin(), viewers.end(), viewer);
	if (it == viewers.end()) {
		return false;
	}
	viewers.erase(it);
	return true;
}

bool comparePublicEntries(const CastEntryInfo& a, const CastEntryInfo& b)
{
	if (a.publicViewers != b.publicViewers) {
		return a.publicViewers > b.publicViewers;
	}
	if (a.level != b.level) {
		return a.level > b.level;
	}
	int cmp = strcasecmp(a.name.c_str(), b.name.c_str());
	if (cmp != 0) {
		return cmp < 0;
	}
	return a.name < b.name;
}

bool compareAlphabeticalPlayers(const Player* a, const Player* b)
{
	int cmp = strcasecmp(a->getName().c_str(), b->getName().c_str());
	if (cmp != 0) {
		return cmp < 0;
	}
	return a->getName() < b->getName();
}

bool comparePlayersByLevel(const Player* a, const Player* b)
{
	if (a->getLevel() != b->getLevel()) {
		return a->getLevel() > b->getLevel();
	}
	if (a->getExperience() != b->getExperience()) {
		return a->getExperience() > b->getExperience();
	}
	return compareAlphabeticalPlayers(a, b);
}

bool isPublicCastOpen()
{
	const GameState_t gameState = g_game.getGameState();
	return gameState == GAME_STATE_NORMAL || gameState == GAME_STATE_CLOSED;
}

void relocateOrDisconnectPublicViewers(const std::vector<ProtocolGame_ptr>& viewersToMove, const std::string& disconnectReason)
{
	if (viewersToMove.empty()) {
		return;
	}

	const bool canRelocate = g_config.getBoolean(ConfigManager::CAST_ENABLED) && g_game.getGameState() == GAME_STATE_NORMAL;
	const int32_t maxTotal = g_config.getNumber(ConfigManager::CAST_MAX_VIEWERS);
	const int32_t maxPerCast = g_config.getNumber(ConfigManager::CAST_MAX_VIEWERS_PER_CAST);

	std::vector<Player*> candidates;
	if (canRelocate && maxTotal > 0 && maxPerCast > 0) {
		candidates.reserve(g_castStates.size());
		for (const auto& entry : g_castStates) {
			const PlayerCastState& state = entry.second;
			if (state.broadcasting && state.player && !state.player->isRemoved() &&
			    static_cast<int32_t>(state.publicViewers.size()) < maxPerCast) {
				candidates.push_back(state.player);
			}
		}

		std::sort(candidates.begin(), candidates.end(), [](const Player* a, const Player* b) {
			const uint32_t viewersA = Casts::getPublicViewerCount(a);
			const uint32_t viewersB = Casts::getPublicViewerCount(b);
			if (viewersA != viewersB) {
				return viewersA > viewersB;
			}
			return compareAlphabeticalPlayers(a, b);
		});
	}

	size_t candidateIndex = 0;
	for (const auto& viewer : viewersToMove) {
		if (!viewer || viewer->isProtocolReleased() || viewer->isConnectionExpired()) {
			if (viewer) {
				viewer->setCastTarget(nullptr);
			}
			continue;
		}

		bool switched = false;
		while (candidateIndex < candidates.size() && static_cast<int32_t>(g_totalPublicViewers) < maxTotal) {
			auto candIt = g_castStates.find(candidates[candidateIndex]);
			if (candIt == g_castStates.end() || !candIt->second.broadcasting ||
			    !candIt->second.player || candIt->second.player->isRemoved() ||
			    static_cast<int32_t>(candIt->second.publicViewers.size()) >= maxPerCast) {
				++candidateIndex;
				continue;
			}

			candIt->second.publicViewers.push_back(viewer);
			++g_totalPublicViewers;
			candIt->second.rebuildAllViewers();
			viewer->setLastCastSwitchTime(OTSYS_TIME());
			viewer->switchCastTarget(candIt->second.player);
			switched = true;
			break;
		}

		if (!switched) {
			viewer->disconnectCastViewer(disconnectReason);
		}
	}
}

} // namespace

bool Casts::isCastAccount(const std::string& accountName)
{
	return strcasecmp(accountName.c_str(), CAST_ACCOUNT_NAME) == 0;
}

bool Casts::isAdminCastAccount(const std::string& accountName)
{
	return strcasecmp(accountName.c_str(), ADMIN_CAST_ACCOUNT_NAME) == 0;
}

bool Casts::validateAdminPassword(const std::string& password)
{
	const std::string& adminPass = g_config.getString(ConfigManager::CAST_ADMIN_PASSWORD);
	return !adminPass.empty() && password == adminPass;
}

void Casts::onPlayerAdded(Player* player)
{
	if (!player) {
		return;
	}

	PlayerCastState& state = g_castStates[player];
	state.player = player;
	state.rebuildAllViewers();
}

void Casts::onPlayerRemoved(Player* player)
{
	if (!player) {
		return;
	}

	player->setHasCastViewers(false);

	auto it = g_castStates.find(player);
	if (it == g_castStates.end()) {
		return;
	}

	std::vector<ProtocolGame_ptr> publicViewers = std::move(it->second.publicViewers);
	std::vector<ProtocolGame_ptr> adminViewers = std::move(it->second.adminViewers);

	if (g_totalPublicViewers >= publicViewers.size()) {
		g_totalPublicViewers -= static_cast<uint32_t>(publicViewers.size());
	} else {
		g_totalPublicViewers = 0;
	}

	g_castStates.erase(it);

	for (const auto& adminViewer : adminViewers) {
		if (!adminViewer || adminViewer->isProtocolReleased() || adminViewer->isConnectionExpired()) {
			if (adminViewer) {
				adminViewer->setCastTarget(nullptr);
			}
			continue;
		}
		adminViewer->disconnectCastViewer("The watched player has logged out.");
	}

	relocateOrDisconnectPublicViewers(publicViewers, "The cast has ended.");
}

bool Casts::setBroadcasting(Player* player, bool enabled)
{
	if (!player || player->isRemoved()) {
		return false;
	}

	PlayerCastState& state = g_castStates[player];
	state.player = player;

	if (enabled) {
		if (!g_config.getBoolean(ConfigManager::CAST_ENABLED)) {
			return false;
		}
		state.broadcasting = true;
		return true;
	}

	if (!state.broadcasting) {
		return true;
	}

	state.broadcasting = false;
	std::vector<ProtocolGame_ptr> publicViewers = std::move(state.publicViewers);
	state.publicViewers.clear();

	if (g_totalPublicViewers >= publicViewers.size()) {
		g_totalPublicViewers -= static_cast<uint32_t>(publicViewers.size());
	} else {
		g_totalPublicViewers = 0;
	}

	state.rebuildAllViewers();

	relocateOrDisconnectPublicViewers(publicViewers, "The cast has ended.");
	return true;
}

bool Casts::isBroadcasting(const Player* player)
{
	if (!player) {
		return false;
	}

	auto it = g_castStates.find(player);
	return it != g_castStates.end() && it->second.broadcasting;
}

uint32_t Casts::getPublicViewerCount(const Player* player)
{
	if (!player) {
		return 0;
	}

	auto it = g_castStates.find(player);
	if (it == g_castStates.end()) {
		return 0;
	}
	return static_cast<uint32_t>(it->second.publicViewers.size());
}

uint32_t Casts::getTotalPublicViewers()
{
	return g_totalPublicViewers;
}

int32_t Casts::getDatabaseViewerValue(const Player* player)
{
	if (!player) {
		return -1;
	}

	auto it = g_castStates.find(player);
	if (it == g_castStates.end() || !it->second.broadcasting) {
		return -1;
	}
	return static_cast<int32_t>(it->second.publicViewers.size());
}

std::vector<CastEntryInfo> Casts::getPublicCastsForLoginList()
{
	std::vector<CastEntryInfo> entries;
	entries.reserve(g_castStates.size());

	for (const auto& pair : g_castStates) {
		const PlayerCastState& state = pair.second;
		if (!state.broadcasting || !state.player || state.player->isRemoved()) {
			continue;
		}

		CastEntryInfo info;
		info.player = state.player;
		info.name = state.player->getName();
		info.level = state.player->getLevel();
		info.publicViewers = static_cast<uint32_t>(state.publicViewers.size());
		entries.push_back(std::move(info));
	}

	std::sort(entries.begin(), entries.end(), comparePublicEntries);

	if (entries.size() > MAX_PUBLIC_LIST_ENTRIES) {
		entries.resize(MAX_PUBLIC_LIST_ENTRIES);
	}
	return entries;
}

std::vector<CastEntryInfo> Casts::getAdminTargetsForLoginList()
{
	const auto& players = g_game.getPlayers();
	std::vector<Player*> onlinePlayers;
	onlinePlayers.reserve(players.size());

	for (const auto& pair : players) {
		Player* player = pair.second;
		if (player && !player->isRemoved()) {
			onlinePlayers.push_back(player);
		}
	}

	std::sort(onlinePlayers.begin(), onlinePlayers.end(), comparePlayersByLevel);

	if (onlinePlayers.size() > MAX_ADMIN_LIST_ENTRIES) {
		onlinePlayers.resize(MAX_ADMIN_LIST_ENTRIES);
	}

	std::vector<CastEntryInfo> entries;
	entries.reserve(onlinePlayers.size());
	for (Player* player : onlinePlayers) {
		CastEntryInfo info;
		info.player = player;
		info.name = player->getName();
		info.level = player->getLevel();
		info.publicViewers = getPublicViewerCount(player);
		entries.push_back(std::move(info));
	}
	return entries;
}

CastLoginStatus Casts::canJoinCast(const Player* target, bool isAdmin, const std::string& password, std::string& errorMessage)
{
	if (isAdmin) {
		if (!validateAdminPassword(password)) {
			errorMessage = "Invalid admin cast password.";
			return CastLoginStatus::InvalidAdminPassword;
		}

		const GameState_t gameState = g_game.getGameState();
		if (gameState == GAME_STATE_SHUTDOWN || gameState == GAME_STATE_STARTUP || gameState == GAME_STATE_MAINTAIN) {
			errorMessage = "Server is currently closed. Please try again later.";
			return CastLoginStatus::Disabled;
		}

		if (target && target->isRemoved()) {
			errorMessage = "Target player is offline.";
			return CastLoginStatus::TargetOffline;
		}
		return CastLoginStatus::Allowed;
	}

	if (!isPublicCastOpen()) {
		errorMessage = "Server is currently closed. Please try again later.";
		return CastLoginStatus::Disabled;
	}

	if (!g_config.getBoolean(ConfigManager::CAST_ENABLED)) {
		errorMessage = "Cast system is currently disabled.";
		return CastLoginStatus::Disabled;
	}

	const int32_t maxTotal = g_config.getNumber(ConfigManager::CAST_MAX_VIEWERS);
	if (maxTotal <= 0 || static_cast<int32_t>(g_totalPublicViewers) >= maxTotal) {
		errorMessage = "Cast viewer limit has been reached. Please try again later.";
		return CastLoginStatus::ServerViewerLimitReached;
	}

	if (target) {
		if (target->isRemoved() || (!isBroadcasting(target) && target->isInGhostMode())) {
			errorMessage = "Target player is offline.";
			return CastLoginStatus::TargetOffline;
		}

		if (!isBroadcasting(target)) {
			errorMessage = "This player is not broadcasting right now.";
			return CastLoginStatus::TargetNotBroadcasting;
		}

		const int32_t maxPerCast = g_config.getNumber(ConfigManager::CAST_MAX_VIEWERS_PER_CAST);
		if (maxPerCast <= 0 || static_cast<int32_t>(getPublicViewerCount(target)) >= maxPerCast) {
			errorMessage = "This cast is full.";
			return CastLoginStatus::CastViewerLimitReached;
		}
	}

	return CastLoginStatus::Allowed;
}

bool Casts::addViewer(Player* target, const ProtocolGame_ptr& viewer, bool isAdmin)
{
	if (!target || target->isRemoved() || !viewer || viewer->isProtocolReleased() || viewer->isConnectionExpired()) {
		return false;
	}

	PlayerCastState& state = g_castStates[target];
	state.player = target;

	if (isAdmin) {
		if (std::find(state.adminViewers.begin(), state.adminViewers.end(), viewer) == state.adminViewers.end()) {
			state.adminViewers.push_back(viewer);
			state.rebuildAllViewers();
		}
		return true;
	}

	if (std::find(state.publicViewers.begin(), state.publicViewers.end(), viewer) == state.publicViewers.end()) {
		state.publicViewers.push_back(viewer);
		++g_totalPublicViewers;
		state.rebuildAllViewers();
	}
	return true;
}

void Casts::removeViewer(const ProtocolGame_ptr& viewer)
{
	if (!viewer) {
		return;
	}

	if (Player* currentTarget = viewer->getCurrentCastTarget()) {
		auto it = g_castStates.find(currentTarget);
		if (it != g_castStates.end()) {
			bool removed = false;
			if (removeViewerFromVector(it->second.publicViewers, viewer)) {
				if (g_totalPublicViewers > 0) {
					--g_totalPublicViewers;
				}
				removed = true;
			}
			if (removeViewerFromVector(it->second.adminViewers, viewer)) {
				removed = true;
			}
			if (removed) {
				it->second.rebuildAllViewers();
				return;
			}
		}
	}

	for (auto& pair : g_castStates) {
		PlayerCastState& state = pair.second;
		bool removed = false;
		if (removeViewerFromVector(state.publicViewers, viewer)) {
			if (g_totalPublicViewers > 0) {
				--g_totalPublicViewers;
			}
			removed = true;
		}
		if (removeViewerFromVector(state.adminViewers, viewer)) {
			removed = true;
		}
		if (removed) {
			state.rebuildAllViewers();
		}
	}
}

bool Casts::switchCastByDirection(const ProtocolGame_ptr& viewer, Direction dir)
{
	if (!viewer || viewer->isProtocolReleased() || viewer->isConnectionExpired()) {
		return false;
	}

	const bool isAdmin = viewer->isCastAdminViewer();
	const int64_t now = OTSYS_TIME();
	const int64_t cooldown = isAdmin ? ADMIN_SWITCH_COOLDOWN_MS : SWITCH_COOLDOWN_MS;
	if (now - viewer->getLastCastSwitchTime() < cooldown) {
		viewer->sendCancelWalk();
		return false;
	}

	if (!isAdmin && (!g_config.getBoolean(ConfigManager::CAST_ENABLED) || !isPublicCastOpen())) {
		viewer->sendCancelWalk();
		viewer->sendCastSystemMessage(MESSAGE_STATUS_SMALL, "Cast system is currently disabled.");
		return false;
	}

	Player* currentTarget = viewer->getCurrentCastTarget();

	std::vector<Player*> candidates;
	if (isAdmin) {
		const auto& players = g_game.getPlayers();
		candidates.reserve(players.size());
		for (const auto& pair : players) {
			Player* player = pair.second;
			if (player && !player->isRemoved()) {
				candidates.push_back(player);
			}
		}
	} else {
		const int32_t maxPerCast = g_config.getNumber(ConfigManager::CAST_MAX_VIEWERS_PER_CAST);
		candidates.reserve(g_castStates.size());
		for (const auto& pair : g_castStates) {
			const PlayerCastState& state = pair.second;
			if (!state.player || state.player->isRemoved()) {
				continue;
			}

			if (state.player == currentTarget) {
				candidates.push_back(state.player);
			} else if (state.broadcasting && maxPerCast > 0 && static_cast<int32_t>(state.publicViewers.size()) < maxPerCast) {
				candidates.push_back(state.player);
			}
		}
	}

	const char* noOtherMessage = isAdmin ? "No other players online." : "No other available casts.";
	if (candidates.empty()) {
		viewer->sendCancelWalk();
		viewer->sendCastSystemMessage(MESSAGE_STATUS_SMALL, noOtherMessage);
		return false;
	}

	if (isAdmin) {
		std::sort(candidates.begin(), candidates.end(), comparePlayersByLevel);
	} else {
		std::sort(candidates.begin(), candidates.end(), compareAlphabeticalPlayers);
	}

	const CastSwitchDirection switchDir = (dir == DIRECTION_NORTH || dir == DIRECTION_WEST)
		? CastSwitchDirection::Previous
		: CastSwitchDirection::Next;

	Player* nextTarget = nullptr;
	auto currentIt = std::find(candidates.begin(), candidates.end(), currentTarget);
	if (currentIt != candidates.end()) {
		if (candidates.size() <= 1) {
			viewer->sendCancelWalk();
			viewer->sendCastSystemMessage(MESSAGE_STATUS_SMALL, noOtherMessage);
			return false;
		}

		const size_t currentIndex = static_cast<size_t>(std::distance(candidates.begin(), currentIt));
		const size_t nextIndex = (switchDir == CastSwitchDirection::Next)
			? ((currentIndex + 1) % candidates.size())
			: ((currentIndex + candidates.size() - 1) % candidates.size());
		nextTarget = candidates[nextIndex];
	} else {
		nextTarget = (switchDir == CastSwitchDirection::Previous) ? candidates.back() : candidates.front();
	}

	if (!nextTarget || nextTarget == currentTarget) {
		viewer->sendCancelWalk();
		viewer->sendCastSystemMessage(MESSAGE_STATUS_SMALL, noOtherMessage);
		return false;
	}

	if (isAdmin) {
		if (currentTarget) {
			auto oldIt = g_castStates.find(currentTarget);
			if (oldIt != g_castStates.end() && removeViewerFromVector(oldIt->second.adminViewers, viewer)) {
				oldIt->second.rebuildAllViewers();
			}
		}

		PlayerCastState& newState = g_castStates[nextTarget];
		newState.player = nextTarget;
		if (std::find(newState.adminViewers.begin(), newState.adminViewers.end(), viewer) == newState.adminViewers.end()) {
			newState.adminViewers.push_back(viewer);
		}
		newState.rebuildAllViewers();
	} else {
		bool wasTrackedPublic = false;
		if (currentTarget) {
			auto oldIt = g_castStates.find(currentTarget);
			if (oldIt != g_castStates.end() && removeViewerFromVector(oldIt->second.publicViewers, viewer)) {
				wasTrackedPublic = true;
				oldIt->second.rebuildAllViewers();
			}
		}

		PlayerCastState& newState = g_castStates[nextTarget];
		newState.player = nextTarget;
		if (std::find(newState.publicViewers.begin(), newState.publicViewers.end(), viewer) == newState.publicViewers.end()) {
			newState.publicViewers.push_back(viewer);
			if (!wasTrackedPublic) {
				++g_totalPublicViewers;
			}
		}
		newState.rebuildAllViewers();
	}

	viewer->setLastCastSwitchTime(now);
	viewer->switchCastTarget(nextTarget);
	return true;
}

bool Casts::switchCastByName(const ProtocolGame_ptr& viewer, const std::string& targetName)
{
	if (!viewer || viewer->isProtocolReleased() || viewer->isConnectionExpired()) {
		return false;
	}

	const bool isAdmin = viewer->isCastAdminViewer();
	const int64_t now = OTSYS_TIME();
	if (!isAdmin && now - viewer->getLastCastSwitchTime() < SWITCH_COOLDOWN_MS) {
		viewer->sendCastSystemMessage(MESSAGE_STATUS_SMALL, "Please wait a moment before switching casts again.");
		return false;
	}

	Player* target = g_game.getPlayerByName(targetName);
	if (!target || target->isRemoved()) {
		viewer->sendCastSystemMessage(MESSAGE_STATUS_SMALL, isAdmin ? "Player is offline." : "Player is offline or not broadcasting.");
		return false;
	}

	Player* currentTarget = viewer->getCurrentCastTarget();
	if (target == currentTarget) {
		viewer->sendCastSystemMessage(MESSAGE_STATUS_SMALL, "You are already watching this player.");
		return false;
	}

	if (!isAdmin) {
		if (!g_config.getBoolean(ConfigManager::CAST_ENABLED) || !isPublicCastOpen()) {
			viewer->sendCastSystemMessage(MESSAGE_STATUS_SMALL, "Cast system is currently disabled.");
			return false;
		}

		if (!isBroadcasting(target)) {
			viewer->sendCastSystemMessage(MESSAGE_STATUS_SMALL, "Player is offline or not broadcasting.");
			return false;
		}

		const int32_t maxPerCast = g_config.getNumber(ConfigManager::CAST_MAX_VIEWERS_PER_CAST);
		if (maxPerCast <= 0 || static_cast<int32_t>(getPublicViewerCount(target)) >= maxPerCast) {
			viewer->sendCastSystemMessage(MESSAGE_STATUS_SMALL, "This cast is full.");
			return false;
		}
	}

	if (isAdmin) {
		if (currentTarget) {
			auto oldIt = g_castStates.find(currentTarget);
			if (oldIt != g_castStates.end() && removeViewerFromVector(oldIt->second.adminViewers, viewer)) {
				oldIt->second.rebuildAllViewers();
			}
		}

		PlayerCastState& newState = g_castStates[target];
		newState.player = target;
		if (std::find(newState.adminViewers.begin(), newState.adminViewers.end(), viewer) == newState.adminViewers.end()) {
			newState.adminViewers.push_back(viewer);
		}
		newState.rebuildAllViewers();
	} else {
		bool wasTrackedPublic = false;
		if (currentTarget) {
			auto oldIt = g_castStates.find(currentTarget);
			if (oldIt != g_castStates.end() && removeViewerFromVector(oldIt->second.publicViewers, viewer)) {
				wasTrackedPublic = true;
				oldIt->second.rebuildAllViewers();
			}
		}

		PlayerCastState& newState = g_castStates[target];
		newState.player = target;
		if (std::find(newState.publicViewers.begin(), newState.publicViewers.end(), viewer) == newState.publicViewers.end()) {
			newState.publicViewers.push_back(viewer);
			if (!wasTrackedPublic) {
				++g_totalPublicViewers;
			}
		}
		newState.rebuildAllViewers();
	}

	viewer->setLastCastSwitchTime(now);
	viewer->switchCastTarget(target);
	return true;
}

const std::vector<ProtocolGame_ptr>& Casts::getViewerProtocols(const Player* player)
{
	if (!player) {
		return g_emptyViewers;
	}

	auto it = g_castStates.find(player);
	if (it == g_castStates.end()) {
		return g_emptyViewers;
	}
	return it->second.allViewers;
}

void Casts::disablePublicCasts()
{
	std::vector<Player*> broadcasters;
	broadcasters.reserve(g_castStates.size());
	for (const auto& pair : g_castStates) {
		const PlayerCastState& state = pair.second;
		if (state.broadcasting && state.player && !state.player->isRemoved()) {
			broadcasters.push_back(state.player);
		}
	}

	disconnectAllViewers(false, "Cast system is currently disabled.");

	for (auto& pair : g_castStates) {
		pair.second.broadcasting = false;
	}

	for (Player* player : broadcasters) {
		if (player->isRemoved()) {
			continue;
		}
		player->sendTextMessage(MESSAGE_STATUS_CONSOLE_BLUE, "Cast system has been disabled. Your broadcast was stopped.");
	}
}

void Casts::disconnectAllViewers(bool includeAdmins, const std::string& reason)
{
	std::vector<ProtocolGame_ptr> toDisconnect;

	for (auto& pair : g_castStates) {
		PlayerCastState& state = pair.second;
		if (!state.publicViewers.empty()) {
			toDisconnect.insert(toDisconnect.end(), state.publicViewers.begin(), state.publicViewers.end());
			state.publicViewers.clear();
		}
		if (includeAdmins && !state.adminViewers.empty()) {
			toDisconnect.insert(toDisconnect.end(), state.adminViewers.begin(), state.adminViewers.end());
			state.adminViewers.clear();
		}
		state.rebuildAllViewers();
	}

	g_totalPublicViewers = 0;

	for (const auto& viewer : toDisconnect) {
		if (!viewer || viewer->isProtocolReleased() || viewer->isConnectionExpired()) {
			if (viewer) {
				viewer->setCastTarget(nullptr);
			}
			continue;
		}
		viewer->disconnectCastViewer(reason);
	}
}
