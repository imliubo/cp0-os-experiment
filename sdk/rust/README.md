# CardputerZero Rust SDK 0.1

This `no_std` crate is the supported Rust API for CardputerZero applications.
Applications compile for `wasm32-unknown-unknown` and must not declare private
Runtime imports directly.

The initial API exposes a monotonic clock, bounded event waiting and the typed
notification capability. `Error::Unavailable` means a capability may be
waiting for a System Shell permission decision and can be retried later.
