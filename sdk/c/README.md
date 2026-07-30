# CardputerZero C/C++ SDK 0.1

Include `include/cardputerzero.h` from a freestanding Clang C11 or C++17
project targeting `wasm32-unknown-unknown`. The header declares only the public
CardputerZero Runtime imports; it does not expose WASI, Linux syscalls or native
linking.

Strings are UTF-8 byte buffers with explicit lengths. Applications should keep
notification titles at 32 Unicode characters and bodies at 160; the Runtime and
broker enforce byte, encoding and character limits again across the trust
boundary.
