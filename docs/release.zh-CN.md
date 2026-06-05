# 发版 checklist

发布 **cc-gateway** 安装包与 GitHub Release 前必读。打 `v*` tag 之前按顺序做。

## 两个仓库

| 仓库 | 作用 |
|------|------|
| **cc-gateway**（本仓库） | Rust 后端；CI 打 5 个平台的包 |
| **cc-gateway-webui**（兄弟目录） | React WebUI；**CI 只用 GitHub 上 `main` 的代码** |

Release 里嵌进去的 WebUI **只来自** 打 tag 那一刻 GitHub 上的 webui 仓库。  
本地改过但没 push 的前端，**不会**进安装包——哪怕你在本机编过 `webui/dist/`。

后端仓库 **不要** 提交 `webui/dist/`（已 gitignore；CI 会重新构建）。

### `webui/dist/` 是什么？

| | 本地（`install_local.sh` / `scripts/build-with-frontend.sh`） | GitHub Release CI |
|--|--|--|
| **用的代码** | 你磁盘上的 `../cc-gateway-webui` | GitHub 上 **cc-gateway-webui 仓库 `main` 最新 commit** |
| **产物放哪** | 编完后复制到本仓库 `webui/dist/`，再 `cargo build` 嵌进二进制 | CI 在临时目录 `npm run build`，复制到 `webui/dist/` 再编译，**不读你本机的 dist** |
| **要不要 git commit** | **不要** — `webui/.gitignore` 已忽略 `dist/` | 不适用（CI 每次现编） |

所以：**`webui/dist/` 只是本地打包/安装的中间产物**；正式发版看的是 **前端项目有没有 push 到 GitHub**，不是看你本机 `dist/` 里有没有新文件。

## 发版前 checklist（按顺序）

### 1. 前端（`cc-gateway-webui`）

- [ ] WebUI 改动已在 `main` 上 **commit**
- [ ] 已执行 **`git push origin main`**（在 GitHub 上核对最新 commit）
- [ ] 可选：本地 `npm run build` 提前发现 TS 错误

### 2. 后端（`cc-gateway`）

- [ ] 后端改动已 commit 到 `main`
- [ ] `cargo test`（至少跑改动相关模块；大版本前建议全量）
- [ ] 按项目规则 bump `Cargo.toml` 的 `version`（PATCH 0–9 等）
- [ ] 若行为或配置项有变：更新用户文档（`docs/config`、`README`、i18n、机器人指南等）
- [ ] 运行 **`./scripts/check-release-ready.sh`**（webui 未提交或未 push 会失败）

### 3. Tag 与 CI

- [ ] `git tag vX.Y.Z` 与 `Cargo.toml` 版本 **完全一致**
- [ ] `git push origin main`，再 `git push origin vX.Y.Z`
- [ ] 等待 [Release 工作流](https://github.com/caixy-plus/cc-gateway/actions/workflows/release.yml) 全部绿灯（约 5～10 分钟）
- [ ] Release 页 **Assets** 里出现 `cc-gateway-<target>.tar.gz` / `.zip`（不能只有 “Source code”）
- [ ] 编辑 GitHub Release 说明：**中英双语条目**（`中文 / English`）。WebUI 更新说明会读这里。

### 4. 跟用户 / 运维说清楚

- **更新提示**：已安装版本低于 GitHub `releases/latest` 时会提示（不必等安装包也能看到“有新版本”）。
- **安装包**：等 Assets 出来再下；Windows 是 `.zip`，macOS/Linux 是 `.tar.gz`。
- **下错了**：「Source code (zip)」是源码树，**不是**可执行安装包——用 Assets 或 `install.sh` / `install.ps1`。

## CI 实际在做什么

打 `v*` tag 后，每个构建 job：

1. 检出 **后端**（该 tag）
2. 从 GitHub 检出 **cc-gateway-webui** 的 `main`（不是你笔记本上的目录）
3. `npm ci && npm run build` → 复制到 `webui/dist/`
4. 对应平台 `cargo build --release`
5. 上传产物；最后 job 挂到 GitHub Release

看 workflow 日志里的 **「WebUI commit embedded in this release」**，可知本次包嵌了哪版前端。

## 本地构建（≠ 正式发版）

- **`./install_local.sh`** 或 **`./scripts/build-with-frontend.sh`**：用**本地** `../cc-gateway-webui`，适合开发，**不能代替**发版前 push webui。
- 不编前端只 `cargo build --release`：WebUI 是占位页。

## 常用命令

```sh
# 发版前检查（在后端仓库根目录）
./scripts/check-release-ready.sh

# Cargo.toml 已改版本后
git push origin main
git tag vX.Y.Z
git push origin vX.Y.Z
```

## 常见坑

| 错误 | 现象 |
|------|------|
| 先打后端 tag，webui 还没 push | 二进制有新 API/配置，**设置界面还是旧的** |
| 下了 Releases 里的 “Source code (zip)” | 拿到的是源码，不是 `cc-gateway.exe` |
| CI 没跑完就让用户装 | 页面上只有 Source code，没有 `cc-gateway-*.zip` |
| tag 版本 ≠ `Cargo.toml` | CI 在 “Verify version matches tag” 失败 |

## 参见

- [CLAUDE.md](../CLAUDE.md) — 版本号规则、Release 说明格式
- [docs/RELEASE-v1.7.3.md](RELEASE-v1.7.3.md) — Release 说明示例
