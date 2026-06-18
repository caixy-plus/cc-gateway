# User-facing Documentation Sync Checklist

> Back: [CLAUDE.md](../CLAUDE.md). Companion: [Adding a New Agent Provider](adding-agent-provider.md), [Adding a New Chat Platform](adding-chat-platform.md), [Release checklist](release.md).

Treat documentation as part of the feature. When reviewers (or release prep) grep for a new `id`, it should appear in setup guides—not only in Rust/WebUI code. Adding or materially changing an **agent provider** or **chat platform** is not complete until the matching table below is satisfied (English + Chinese where paired files exist). Do not ship integration-only PRs without the matching `docs/` and README updates.

**Removing or tightening** (retired platform/provider, dropped config field, API behavior change) is less common than adding, but still needs a fixed sweep — see [Retire, remove, or tighten](#retire-remove-or-tighten) below.

## New agent provider

| File | What to update |
|------|----------------|
| `docs/config.md` / `docs/config.zh-CN.md` | `agent` fields, example JSON, defaults; link to provider CLI install if non-obvious |
| `README.md` / `README.zh-CN.md` | Provider name in features / gateway-command provider list / quick start when behavior differs |
| `CLAUDE.md` | § Adding a New Agent Provider — refresh “Current providers” line; optional note under Agent Runtime if protocol is new |
| `src/utils/i18n/dict.rs` | Provider-specific user strings (see i18n rules in CLAUDE.md) |

No `docs/bots/` change unless the provider is only relevant on one platform (unusual).

## New chat platform (bot channel)

| File | What to update |
|------|----------------|
| `docs/bots/<platform>.md` / `docs/bots/<platform>.zh-CN.md` | **Create** setup guide: developer console steps, `config.json` fields, pairing, transport, UX (`/ll`, @ rules), **whether MCP `send_file` is supported**, troubleshooting, official API links |
| `docs/bots/README.md` / `docs/bots/README.zh-CN.md` | Add row to the platform table; mention pairing if applicable |
| `docs/config.md` / `docs/config.zh-CN.md` | New `GatewayConfig` section, field table, example JSON, restart vs live fields; link to `docs/bots/<platform>` |
| `docs/usage.md` / `docs/usage.zh-CN.md` | Usage section for that platform (how to talk to the bot, command quirks) |
| `README.md` / `README.zh-CN.md` | Features, architecture line, quick-start platform table, documentation index table |
| `scripts/install-docs.sh` / `scripts/install-docs.ps1` | Add EN + zh-CN URL lines (install scripts source these; do not duplicate URLs in `install.sh` / `install.ps1`) |
| `CLAUDE.md` | Project Overview; Platform layer; § Adding a New Chat Platform; **§ Platform Reference Docs** — add official vendor URLs (required) |
| `../cc-gateway-webui` | Settings / pairing / session source labels (see platform frontend checklist) |

## Retire, remove, or tighten

Use when **removing** a platform or provider, **dropping** a config/API field, or **narrowing** documented behavior (not only full feature deletion). Code may land first; finish the doc/UI sweep before merge when the change is user-visible.

1. **Grep both repos** for the retired `id` and display name (e.g. `qq`, provider slug). Backend: `src/`, `docs/`, `README*`, `CLAUDE.md`, `Cargo.toml`, `home-page/`. Frontend: `../cc-gateway-webui/src/`, `e2e/`. Release note files (`docs/RELEASE-*.md`) are historical — update only if you are correcting actively misleading text, not every past mention.

2. **Registry is source of truth** — `platform_registry.rs` (`PLATFORM_DEFS`) and `agent_registry.rs` (`AGENT_PROVIDER_DEFS`). Refresh every “current platforms/providers” line: `CLAUDE.md`, `docs/architecture.md`, `docs/platform-reference.md`, `docs/platform-integration-checklist*.md`, `docs/adding-chat-platform.md`, `docs/adding-agent-provider.md`, paired `docs/bots/*` index tables, `README.md` / `README.zh-CN.md`.

3. **Delete or trim user guides** — remove `docs/bots/<id>.md` + `.zh-CN.md` when a platform goes away; remove `platforms.<id>` sections and example JSON keys from `docs/config.md` / `.zh-CN.md`; drop usage sections from `docs/usage.md` / `.zh-CN.md`.

4. **Install output** — `scripts/install-docs.sh` / `scripts/install-docs.ps1` (local path lines and GitHub URLs).

5. **Sibling WebUI** — drop hard-coded platform types, i18n keys, filters, and normalize-config defaults that no longer exist in `GET /api/platforms` / registry. Behavior-only API changes still need matching WebUI client updates when the UI sent the old shape.

Optional: remove orphaned `i18n` keys in `src/utils/i18n/dict.rs` when user-facing strings for the retired feature are gone.

## Conventions

- **Bilingual pairs**: every new `docs/foo.md` user guide should have `docs/foo.zh-CN.md` (or live under `docs/bots/*.zh-CN.md`). README uses `README.md` + `README.zh-CN.md` with language links at the top.
- **Single source for setup steps**: long console walkthroughs live in `docs/bots/<platform>.md`; `docs/config.md` and README only summarize fields and link there.
- **Install output**: `install.sh` / `install.ps1` / `install_local.*` call `scripts/install-docs.*` — extend those scripts when adding a platform so fresh installs list the new guide.
