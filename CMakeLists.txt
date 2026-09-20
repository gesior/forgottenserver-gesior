cmake_minimum_required(VERSION 3.19)

set(CMAKE_DISABLE_SOURCE_CHANGES ON)
set(CMAKE_DISABLE_IN_SOURCE_BUILD ON)

# Ensure to pick up the default triplet from the environment if any. This helps
# driving the vcpkg triplet in the same way either when starting vcpkg directly,
# or when letting CMake start vcpkg at configure/generate time.
# Note: this logic must happen before PROJECT command.
if (DEFINED ENV{VCPKG_DEFAULT_TRIPLET} AND NOT DEFINED VCPKG_TARGET_TRIPLET)
    set(VCPKG_TARGET_TRIPLET "$ENV{VCPKG_DEFAULT_TRIPLET}" CACHE STRING "The vcpkg triplet")
endif()

option(BUILD_TESTING "Build unit tests" OFF)
if (BUILD_TESTING)
    list(APPEND VCPKG_MANIFEST_FEATURES "unit-tests")
endif()

option(USE_LUAJIT "Use LuaJIT" ON)
if (USE_LUAJIT)
    list(APPEND VCPKG_MANIFEST_FEATURES "luajit")
else()
    list(APPEND VCPKG_MANIFEST_FEATURES "lua")
endif()

project(tfs CXX)

list(APPEND CMAKE_MODULE_PATH "${CMAKE_CURRENT_SOURCE_DIR}/cmake")

if (NOT WIN32)
    add_compile_options(-Wall -Wextra -Wnon-virtual-dtor -Wold-style-cast -pedantic -Werror -pipe -fvisibility=hidden)
endif ()

if ("${CMAKE_CXX_COMPILER_ID}" STREQUAL "GNU")
    add_compile_options(-fno-strict-aliasing)
endif ()

if ("${CMAKE_CXX_COMPILER_ID}" STREQUAL "Clang")
    add_compile_options(-Wimplicit-fallthrough -Wmove)
endif ()

add_compile_options("$<$<CXX_COMPILER_ID:MSVC>:/utf-8>")

# Find packages.
find_package(OpenSSL 3.0.0 REQUIRED COMPONENTS Crypto)

find_package(fmt CONFIG)
if (NOT fmt_FOUND)
    find_package(fmt 6.1.2 REQUIRED)
endif()

# Look for vcpkg-provided libmariadb first
# If we link to the file directly, we might miss its dependencies from vcpkg
find_package(unofficial-libmariadb CONFIG)
if (unofficial-libmariadb_FOUND)
    set(MYSQL_CLIENT_LIBS "unofficial::libmariadb")
else ()
    find_package(MySQL REQUIRED)
endif ()

find_package(Threads REQUIRED)
find_package(PugiXML CONFIG REQUIRED)

# Selects LuaJIT if user defines or auto-detected
if (DEFINED USE_LUAJIT AND NOT USE_LUAJIT)
    set(FORCE_LUAJIT ${USE_LUAJIT})
else ()
    find_package(LuaJIT)
    set(FORCE_LUAJIT ${LuaJIT_FOUND})
endif ()
option(USE_LUAJIT "Use LuaJIT" ${FORCE_LUAJIT})

if (NOT FORCE_LUAJIT)
    find_package(Lua REQUIRED)
endif ()

if (APPLE)
    find_package(Iconv REQUIRED)
endif()

set(BOOST_REQUIRED_COMPONENTS system iostreams locale)
if (BUILD_TESTING)
    list(APPEND BOOST_REQUIRED_COMPONENTS unit_test_framework)
endif ()
find_package(Boost 1.71.0 REQUIRED COMPONENTS ${BOOST_REQUIRED_COMPONENTS})

include_directories(${Boost_INCLUDE_DIRS} ${OPENSSL_INCLUDE_DIR} ${LUA_INCLUDE_DIR} ${MYSQL_INCLUDE_DIR} ${PUGIXML_INCLUDE_DIR})

# Option to disable OTS Stats
option(ENABLE_OTS_STATISTICS "OTS Statistics" ON)
if (ENABLE_OTS_STATISTICS)
    message(STATUS "OTS Statistics enabled")
    add_definitions(-DSTATS_ENABLED)
endif()

# Linux process-allocator A/B. Default (both OFF) is glibc malloc.
# Mutually exclusive; C++ new/delete override is only needed for mimalloc.
#   cmake -DUSE_MIMALLOC=ON ...
#   cmake -DUSE_JEMALLOC=ON ...
# Same binary, no rebuild:
#   LD_PRELOAD=libmimalloc.so.2 ./tfs
#   LD_PRELOAD=libjemalloc.so.2 ./tfs
#   LD_PRELOAD=libtcmalloc.so ./tfs
# Measure (same map/load):
#   perf record -g -p $(pidof tfs) -- sleep 60
#   perf report / perf annotate on sendMagicEffect, malloc, operator new
option(USE_MIMALLOC "Link mimalloc and override operator new/delete (Linux)" OFF)
option(USE_JEMALLOC "Link jemalloc as the process allocator (Linux)" OFF)

if (USE_MIMALLOC AND USE_JEMALLOC)
    message(FATAL_ERROR "USE_MIMALLOC and USE_JEMALLOC are mutually exclusive")
endif()

if ((USE_MIMALLOC OR USE_JEMALLOC) AND NOT CMAKE_SYSTEM_NAME STREQUAL "Linux")
    message(FATAL_ERROR "USE_MIMALLOC / USE_JEMALLOC are supported on Linux only")
endif()

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

add_subdirectory(src)
add_executable(tfs ${tfs_MAIN})
target_link_libraries(tfs tfslib)

if (USE_MIMALLOC)
    find_package(mimalloc REQUIRED)
    target_sources(tfs PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}/src/memory_allocator.cpp")
    set_source_files_properties("${CMAKE_CURRENT_SOURCE_DIR}/src/memory_allocator.cpp"
            PROPERTIES SKIP_PRECOMPILE_HEADERS ON SKIP_UNITY_BUILD_INCLUSION ON)
    if (TARGET mimalloc)
        target_link_libraries(tfs PRIVATE mimalloc)
    elseif (TARGET mimalloc-static)
        target_link_libraries(tfs PRIVATE mimalloc-static)
    else ()
        message(FATAL_ERROR "mimalloc was found but provides neither mimalloc nor mimalloc-static")
    endif ()
    message(STATUS "Process allocator: mimalloc")
elseif (USE_JEMALLOC)
    find_package(PkgConfig REQUIRED)
    pkg_check_modules(JEMALLOC REQUIRED IMPORTED_TARGET jemalloc)
    # --as-needed would drop jemalloc if we never call je_* directly.
    target_link_libraries(tfs PRIVATE "-Wl,--no-as-needed" PkgConfig::JEMALLOC "-Wl,--as-needed")
    message(STATUS "Process allocator: jemalloc")
else ()
    message(STATUS "Process allocator: libc (default)")
endif ()

target_link_options(tfs PUBLIC -flto=auto)

if (BUILD_TESTING)
    message(STATUS "Building unit tests")
    enable_testing()
    add_subdirectory(src/tests)
endif()

### INTERPROCEDURAL_OPTIMIZATION ###
cmake_policy(SET CMP0069 NEW)
include(CheckIPOSupported)
check_ipo_supported(RESULT result OUTPUT error)
if (result)
    message(STATUS "IPO / LTO enabled")
    set_target_properties(tfs PROPERTIES INTERPROCEDURAL_OPTIMIZATION True)
else ()
    message(STATUS "IPO / LTO not supported: <${error}>")
endif ()
### END INTERPROCEDURAL_OPTIMIZATION ###

### Git Version ###
# Define the two required variables before including
# the source code for watching a git repository.
option(SKIP_GIT "Skip checking for git updates" OFF)
if(NOT SKIP_GIT)
    set(PRE_CONFIGURE_FILE "cmake/gitmetadata.h.in")
    set(POST_CONFIGURE_FILE "${CMAKE_CURRENT_BINARY_DIR}/gitmetadata.h")
    include(git_watcher)
    if(Git_FOUND)
        add_dependencies(tfs check_git)
        target_include_directories(tfs PRIVATE ${CMAKE_CURRENT_BINARY_DIR})
    endif()
endif()
### END  Git Version ###

# Option to disable unity builds
option(ENABLE_UNITY_BUILD "Enable unity build" ON)
if(ENABLE_UNITY_BUILD)
    set_target_properties(tfslib PROPERTIES UNITY_BUILD ON)
endif()

target_precompile_headers(tfs PUBLIC src/otpch.h)
