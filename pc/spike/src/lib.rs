//! spike 的库入口。
//!
//! 拆出 lib 的唯一理由是**可隔离测试**：`examples/` 不能引用 bin crate 的模块，
//! 而 spike 崩溃时必须能逐段复现（见 `examples/minimal.rs`）。
//!
//! **本 crate 不产出产品功能** —— 见 `docs/plans/2026-10-06-pc-spike.md`。

pub mod hw;
pub mod render;
