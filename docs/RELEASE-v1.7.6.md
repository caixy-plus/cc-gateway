# Release v1.7.6

- **Windows CLI 解析** / Resolve agent binaries via PATHEXT and `.cmd` wrappers so npm global installs spawn correctly instead of os error 193
- **Windows 进程树终止** / Kill entire child process trees on stop (`taskkill /T`) so `.cmd` wrappers do not leave orphan agent consoles
- **Windows 语言检测** / Prefer OS locale over Git’s `LANG=en_US.UTF-8` when choosing Chinese vs English UI strings
- **WebUI 目录浏览** / Fix DirModal path joining on Windows (`\` separators, drive roots) and `/ll` absolute-path listing without changing session work_dir
