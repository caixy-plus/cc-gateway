## Rust Style Guide (rustfmt)

- Source of truth: `https://doc.rust-lang.org/style-guide/`
- Practice: use `cargo fmt` and prefer defaults; only add `rustfmt.toml` when you have a concrete, shared need.

## Rust API Guidelines

- Source of truth: `https://github.com/rust-lang/api-guidelines`
- Use for:
  - Naming and module structure
  - Public docs (`///`) quality
  - Error types and ergonomics
  - Trait bounds, builder patterns, and API clarity

## Clippy

- Source of truth: `https://doc.rust-lang.org/clippy/`
- Practice:
  - `cargo clippy --all-targets -- -D warnings` (team preference)
  - Don’t silence lints by default; prefer refactoring

