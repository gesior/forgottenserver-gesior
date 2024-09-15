// Copyright 2022 The Forgotten Server Authors. All rights reserved.
// Use of this source code is governed by the GPL-2.0 License that can be found in the LICENSE file.

#include "otpch.h"

#include "position.h"

std::size_t hash_value(const Position& p)
{
	std::size_t seed = 0;
	boost::hash_combine(seed, p.x);
	boost::hash_combine(seed, p.y);
	boost::hash_combine(seed, p.z);
	return seed;
}

std::ostream& operator<<(std::ostream& os, const Position& pos)
{
	os << "( " << std::setw(5) << std::setfill('0') << pos.x;
	os << " / " << std::setw(5) << std::setfill('0') << pos.y;
	os << " / " << std::setw(3) << std::setfill('0') << pos.getZ();
	os << " )";
	return os;
}

std::ostream& operator<<(std::ostream& os, const Direction& dir)
{
	switch (dir) {
		case DIRECTION_NORTH:
			os << "North";
			break;

		case DIRECTION_EAST:
			os << "East";
			break;

		case DIRECTION_WEST:
			os << "West";
			break;

		case DIRECTION_SOUTH:
			os << "South";
			break;

		case DIRECTION_SOUTHWEST:
			os << "South-West";
			break;

		case DIRECTION_SOUTHEAST:
			os << "South-East";
			break;

		case DIRECTION_NORTHWEST:
			os << "North-West";
			break;

		case DIRECTION_NORTHEAST:
			os << "North-East";
			break;

		default:
			break;
	}

	return os;
}
