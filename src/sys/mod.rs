/// Fixed-capacity bump allocator for per-frame UI scratch allocations.
pub mod arena;
/// Bounded undo/redo value history stack.
pub mod history;
/// Deterministic hierarchical FNV-1a widget ID stack.
pub mod id;
/// JSON-RPC 2.0 MCP UI inspection and automation server.
pub mod mcp;
/// Global panic-flush callback registry for `panic = "abort"` persistence.
pub mod panic_flush;
/// Traversal-resistant storage sandbox (`SafeDir`) with `localStorage` support on `wasm32`.
pub mod sandbox;
