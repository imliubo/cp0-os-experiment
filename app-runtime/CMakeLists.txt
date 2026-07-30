cmake_minimum_required(VERSION 3.20)
project(cardputerzero-app-runtime C)

if(NOT DEFINED WAMR_ROOT_DIR)
  message(FATAL_ERROR "WAMR_ROOT_DIR must point to the pinned WAMR checkout")
endif()

set(CMAKE_C_STANDARD 11)
set(CMAKE_C_STANDARD_REQUIRED ON)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)

set(WAMR_BUILD_PLATFORM "linux")
set(WAMR_BUILD_TARGET "AARCH64")
set(WAMR_BUILD_INTERP 1)
set(WAMR_BUILD_FAST_INTERP 1)
set(WAMR_BUILD_AOT 1)
set(WAMR_BUILD_JIT 0)
set(WAMR_BUILD_FAST_JIT 0)
set(WAMR_BUILD_LIBC_BUILTIN 0)
set(WAMR_BUILD_LIBC_WASI 0)
set(WAMR_BUILD_LIBC_UVWASI 0)
set(WAMR_BUILD_LIB_PTHREAD 0)
set(WAMR_BUILD_LIB_WASI_THREADS 0)
set(WAMR_BUILD_MULTI_MODULE 0)
set(WAMR_BUILD_SHARED_MEMORY 0)
set(WAMR_BUILD_REF_TYPES 0)
set(WAMR_BUILD_SIMD 0)
set(WAMR_BUILD_TAIL_CALL 0)
set(WAMR_BUILD_DEBUG_INTERP 0)
set(WAMR_BUILD_DUMP_CALL_STACK 0)
set(WAMR_BUILD_MEMORY_PROFILING 0)
set(WAMR_BUILD_PERF_PROFILING 0)

include("${WAMR_ROOT_DIR}/build-scripts/runtime_lib.cmake")

add_library(cp0-wamr STATIC ${WAMR_RUNTIME_LIB_SOURCE})
target_include_directories(cp0-wamr PUBLIC
  "${WAMR_ROOT_DIR}/core/iwasm/include"
)
target_compile_options(cp0-wamr PRIVATE -Os -fstack-protector-strong)

add_executable(cardputerzero-app-runtime
  src/broker_client.c
  src/hostcalls.c
  src/main.c
  src/seccomp.c
)
target_compile_options(cardputerzero-app-runtime PRIVATE
  -Os
  -Wall
  -Wextra
  -Werror
  -Wformat=2
  -Wconversion
  -Wshadow
  -fstack-protector-strong
)
target_link_options(cardputerzero-app-runtime PRIVATE
  -static
  -Wl,--gc-sections
  -Wl,-z,relro
  -Wl,-z,now
  -Wl,-z,noexecstack
)
target_link_libraries(cardputerzero-app-runtime PRIVATE cp0-wamr m dl pthread)

add_executable(cp0-seccomp-probe
  tests/seccomp-probe.c
  src/seccomp.c
)
target_include_directories(cp0-seccomp-probe PRIVATE src)
target_compile_options(cp0-seccomp-probe PRIVATE
  -Os
  -Wall
  -Wextra
  -Werror
  -Wformat=2
)
target_link_options(cp0-seccomp-probe PRIVATE -static -Wl,-z,noexecstack)
