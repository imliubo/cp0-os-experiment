# Third-party software

The CardputerZero App Runtime statically links WebAssembly Micro Runtime
(WAMR) 2.4.5 from the Bytecode Alliance, pinned in `wamr.env`.

WAMR is licensed under Apache License 2.0 with LLVM Exceptions. The authoritative
license text is distributed in the pinned WAMR source checkout as `LICENSE` and
is available at:

<https://github.com/bytecodealliance/wasm-micro-runtime/blob/25bd7eb63e828e4bd242cc9b38d260b4b31c6605/LICENSE>

No local modifications are made to the WAMR source. CardputerZero supplies its
own embedding executable, build configuration and post-initialization seccomp
policy.

The Runtime also statically links Wayland 1.23.1 and libffi 3.5.2. The xdg-shell
client protocol is generated from wayland-protocols 1.44. Exact repositories
and commits are pinned in `wayland.env`; none of the three source trees are
modified by CardputerZero source patches.
