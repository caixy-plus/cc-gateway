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

## Output expectations (when responding with code)
- Keep diffs minimal and intention-revealing.
- Prefer small functions with clear names over clever one-liners.
- Don’t introduce new dependencies unless needed; if you do, explain why.

