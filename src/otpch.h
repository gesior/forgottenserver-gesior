// Copyright 2022 The Forgotten Server Authors. All rights reserved.
// Use of this source code is governed by the GPL-2.0 License that can be found in the LICENSE file.

#ifndef FS_OTPCH_H
#define FS_OTPCH_H

// Definitions should be global.
#include "definitions.h"

#include <algorithm>
#include <array>
#include <atomic>
#include <bitset>
#include <boost/asio.hpp>
#include <boost/iostreams/device/mapped_file.hpp>
#include <boost/lockfree/stack.hpp>
#include <boost/unordered/unordered_flat_map.hpp>
#include <boost/unordered/unordered_flat_set.hpp>
#include <boost/variant.hpp>
#include <chrono>
#include <cmath>
#include <condition_variable>
#include <cstdint>
#include <deque>
#include <filesystem>
#include <fmt/color.h>
#include <forward_list>
#include <fstream>
#include <functional>
#include <iomanip>
#include <iostream>
#include <limits>
#include <list>
#include <map>
#include <memory>
#include <mutex>
#include <mysql/mysql.h>
#include <pugixml.hpp>
#include <random>
#include <set>
#include <sstream>
#include <string>
#include <string_view>
#include <thread>
#include <typeinfo>
#include <queue>
#include <unordered_map>
#include <unordered_set>
#include <utility>
#include <valarray>
#include <vector>

#if __has_include("luajit/lua.hpp")
#include <luajit/lua.hpp>
#else
#include <lua.hpp>
#endif

#endif // FS_OTPCH_H
