# Release checklist

How to publish **cc-gateway** binaries and GitHub Releases. Read this before pushing a `v*` tag.

## Two repositories

| Repo | Role |
|------|------|
| **cc-gateway** (this repo) | Rust backend; CI builds 5 platform packages |
| **cc-gateway-webui** (sibling) | React WebUI; **CI always builds from `main` on GitHub** |

Embedded WebUI in release binaries comes **only** from the webui repo at tag time.  
**Uncommitted or unpushed frontend work will not appear in the release**, even if you built `webui/dist/` locally.

Do **not** commit `webui/dist/` in the backend repo (gitignored; CI rebuilds it).

### What is `webui/dist/`?

| | Local (`install_local.sh` / `build-with-frontend.sh`) | GitHub Release CI |
|--|--|--|
| **Source used** | Your sibling `../cc-gateway-webui` checkout | Latest commit on **`cc-gateway-webui` `main` on GitHub** |
| **Output** | Copied into this repo’s `webui/dist/`, then `cargo build` embeds it | CI runs `npm run build`, copies into `webui/dist/`, then compiles — **does not use your laptop’s `dist/`** |
| **Git commit?** | **No** — `webui/.gitignore` ignores `dist/` | N/A (fresh build every run) |

**`webui/dist/` is only a local build artifact.** For releases, what matters is whether **frontend source is pushed to GitHub**, not whether your local `dist/` folder looks up to date.

## Pre-release checklist (in order)

### 1. Frontend (`cc-gateway-webui`)

- [ ] All WebUI changes **committed** on `main`
- [ ] **`git push origin main`** completed (verify on GitHub)
- [ ] Optional: `npm run build` locally to catch TS errors early

### 2. Backend (`cc-gateway`)

- [ ] Backend changes committed on `main`
- [ ] `cargo test` (at least modules you touched; full suite before a large release)
- [ ] Bump `Cargo.toml` `[package].version` per project rules (PATCH 0–9, etc.)
- [ ] User/docs updated if behavior or config fields changed (`docs/config`, README, i18n, bot guides)
- [ ] Run **`./scripts/check-release-ready.sh`** (fails if webui is dirty or not pushed)

### 3. Tag and CI

- [ ] `git tag vX.Y.Z` matches `Cargo.toml` version exactly
- [ ] `git push origin main` then `git push origin vX.Y.Z`
- [ ] Wait for [Release workflow](https://github.com/caixy-plus/cc-gateway/actions/workflows/release.yml) — all matrix jobs green (~5–10 min)
- [ ] Release page shows **Assets**: `cc-gateway-<target>.tar.gz` / `.zip` (not only “Source code”)
- [ ] Edit GitHub Release notes: **bilingual bullets** (ZH / EN). WebUI update UI reads these notes.

### 4. Tell users / operators

- **Update badge**: appears when installed version &lt; GitHub `releases/latest` (no new binary required for the *prompt*).
- **Install packages**: wait until Assets exist; Windows = `.zip`, macOS/Linux = `.tar.gz`.
- **Wrong download**: “Source code (zip)” is the git tree, **not** the app — use Assets or `install.sh` / `install.ps1`.

## What CI does

On tag `v*`, each build job:

1. Checks out **backend** at the tag
2. Checks out **cc-gateway-webui** `main` from GitHub (not your laptop)
3. `npm ci && npm run build` → copies into `webui/dist/`
4. `cargo build --release` for the matrix target
5. Upload artifacts; final job attaches them to the GitHub Release

Check the workflow log line **“WebUI commit embedded in this release”** to see which frontend SHA was baked in.

## Local build (not the same as CI release)

- **`./install_local.sh`** or **`./scripts/build-with-frontend.sh`**: uses **local** `../cc-gateway-webui` — good for dev, **not** a substitute for pushing webui before a tag.
- **`cargo build --release`** without frontend: placeholder WebUI only.

## Quick commands

```sh
# Pre-flight (from backend repo root)
./scripts/check-release-ready.sh

# After version bump in Cargo.toml
git push origin main
git tag vX.Y.Z
git push origin vX.Y.Z
```

## Common mistakes

| Mistake | Symptom |
|---------|---------|
| Tag backend before `git push` webui | New API/config in binary, **old Settings UI** in same release |
| Download “Source code (zip)” on Releases | Gets source tree, not `cc-gateway.exe` |
| Tell users to install before CI finishes | Only Source code links visible; no `cc-gateway-*.zip` yet |
| Version tag ≠ `Cargo.toml` | CI fails at “Verify version matches tag” |

## See also

- [CLAUDE.md](../CLAUDE.md) — version bump rules, release notes format
- [docs/RELEASE-v1.7.3.md](RELEASE-v1.7.3.md) — example release note bullets
