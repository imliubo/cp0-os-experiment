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
