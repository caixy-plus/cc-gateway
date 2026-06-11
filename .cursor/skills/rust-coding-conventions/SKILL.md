---
name: rust-coding-conventions
description: Applies standard Rust coding conventions and best practices. Use when writing, editing, or reviewing Rust code (including Cargo projects), especially when the user mentions Rust style, rustfmt, clippy, or API design.
---

# Rust Coding Conventions

## Canonical references (use these as the source of truth)
- Rust Style Guide (formatting defaults / rustfmt): `https://doc.rust-lang.org/style-guide/`
- Rust API Guidelines (public API design): `https://github.com/rust-lang/api-guidelines`
- Clippy (lint-driven best practices): `https://doc.rust-lang.org/clippy/`

## Default workflow (apply in this order)
1. **Format**: follow rustfmt defaults (Rust Style Guide). Prefer not to introduce custom formatting rules unless necessary.
2. **Lint**: run Clippy and fix issues in changed code; avoid silencing lints unless there is a clear reason.
3. **API design** (when relevant): follow Rust API Guidelines for naming, docs, error types, and ergonomics.

## Formatting rules (Rust Style Guide / rustfmt defaults)
- Use `cargo fmt` (no manual formatting fights).
- Indentation: 4 spaces, no tabs.
- Prefer Rust Style Guide defaults (line width and layout decisions come from rustfmt).
- Keep `use` statements tidy and grouped (std → external crates → crate).

## Module layout (Rust 2018+)

**Use the modern layout only** — do **not** add or revive `mod.rs` (legacy `foo/mod.rs`).

| Layout | Paths | When |
|--------|--------|------|
| **Directory module** | `src/foo.rs` + `src/foo/*.rs` | Several related files; `foo.rs` is the module root |
| **Single file** | `src/foo.rs` only | One implementation file; no `foo/` directory |

- `foo.rs` declares children with `mod child;` (files live in `foo/child.rs`).
- **Forbidden**: `foo.rs` and `foo/mod.rs` at the same time (compile error).
- **Forbidden for new code**: `foo/mod.rs` as the directory entry.
- Put types, `impl`, and logic in `foo/*.rs` (or a dedicated top-level `foo.rs` when there is no directory). Keep `foo.rs` focused on `mod` / `pub use` / API facade when the module is a directory.
- Reference: [Paths for accessing named crates, modules, and items](https://doc.rust-lang.org/book/ch07-05-separating-modules-into-different-files.html) (Rust Book — `mod` in `src/lib.rs` or `src/foo.rs`, not `mod.rs` for new code).

### This repository

`cc-gateway` uses **`foo.rs` + `foo/`** only (no `mod.rs` under `src/`). When adding a module, follow that layout; fix `include_str!` / relative paths if the entry file moves up one directory level.

### cc-gateway layers

| Layer | Entry | Contents |
|-------|--------|----------|
| `core` | `src/core.rs` | `agent`, `command`, `config`, `history`, `prompt`, `runtime`, `session` |
| `api` | `src/api.rs` | `web` (Axum handlers, SSE) |
| `database` | `src/database.rs` | SQLite persistence (`pub use database as db` in `lib.rs`) |
| `platform` | `src/platform.rs` | Feishu / Telegram / QQ |
| `daemon`, `utils` | `src/<name>.rs` + `src/<name>/` | lifecycle, i18n |
| `types` | `src/types.rs` | `pub use` of shared config/session types |

Binary: `src/main.rs` calls `cc_gateway::…` only.

### Submodule example

```
src/core/config.rs     # pub mod loader; pub mod model;
src/core/config/loader.rs
src/core/config/model.rs
```

## Naming & API rules (Rust API Guidelines)
- Types/traits/enums: `UpperCamelCase`; functions/vars/modules: `snake_case`; constants: `SCREAMING_SNAKE_CASE`.
- Boolean names: `is_*`, `has_*`, `should_*`.
- Public items should have `///` docs explaining **what** and **why** (not narrating code).
- Prefer explicit error types in libraries (`thiserror`), and ergonomic errors at app boundaries (`anyhow`).

## Lint & safety rules (Clippy-driven)
- Avoid `unwrap()`/`expect()` in production code (tests are okay).
- Prefer `?` for error propagation and add context at IO/network boundaries.
- Avoid holding locks across `.await` (extract data, drop the guard, then await).
- Avoid unnecessary clones; prefer borrowing unless it complicates lifetimes too much.

## When making changes
- If you modify Rust code, ensure `cargo fmt` and `cargo test` for relevant scope.
- If adding new behavior, add a focused test first (TDD preferred).
- **cc-gateway:** unit tests belong in the same `.rs` (`#[cfg(test)] mod tests` at file bottom). Reserve `src/tests/` for integration/smoke flows; never make production helpers `pub`/`pub(crate)` solely so another file can unit-test them.

## Output expectations (when responding with code)
- Keep diffs minimal and intention-revealing.
- Prefer small functions with clear names over clever one-liners.
- Don’t introduce new dependencies unless needed; if you do, explain why.

