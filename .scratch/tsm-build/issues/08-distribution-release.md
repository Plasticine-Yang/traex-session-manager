# 08 — 分发与发布

**What to build:** 让 tsm 能被别人一键装上、双名启动、自更新,并由 CI 按 tag 发四 target 产物。写 `install.sh`(curl 一键装,SHA256 校验后落 `~/.local/bin/tsm` + 建 `traex-session-manager` 软链)、`tsm self-update [--check]` 子命令(复用安装脚本、幂等)、`.github/workflows/release.yml`(推 `vX.Y.Z` tag → 4-target 矩阵 → tarball + `SHA256SUMS` → GitHub Release)、README 写 `cargo install --path .` 源码备选。

> spec §10 在决策阶段是 **plan-only**;本 ticket 是进入构建阶段后的**真正落地**。

规格来源:[`../../tsm/spec.md`](../../tsm/spec.md) §9.4(安装位置+双名软链)、§10.1–10.5(target 矩阵/release 产物/`install.sh`/`self-update`/`release.yml` 大纲)。单点常量 `OWNER/REPO = Plasticine-Yang/traex-session-manager`。

**Blocked by:** 01 (需可构建 crate;独立于 02–07,可并行)

**Status:** in-progress

- [ ] `install.sh`(仓库根):探测 OS/arch 选 target → 从 `releases/latest/download/<asset>` 下载 → 按 `SHA256SUMS` **校验 SHA256** → 解包落 `~/.local/bin/tsm` → 建软链 `~/.local/bin/traex-session-manager -> tsm` → 检测 `~/.local/bin` 不在 PATH 时提示(不擅改 profile)。
- [ ] **双名触发**:装后 `tsm` 与 `traex-session-manager` 都能启动(后者是软链)。
- [ ] `tsm self-update [--check]`:内部执行 `curl -fsSL <install-url> | sh` 复用脚本(**零 HTTP 依赖**);`--check` 只比对本地 `--version` 与最新 release tag;**幂等**——已最新报告 `already up to date`、不重装/不降级、重复跑无副作用。
- [ ] `release.yml`:`on: push: tags: ['v*.*.*']`;job `verify` 校验 tag == `Cargo.toml` version;job `build`(matrix 4 target,Linux 装 `musl-tools`)产 `tsm-<version>-<target>.tar.gz`(含 `tsm` + LICENSE/README);job `release` 汇集产物 + 生成 `SHA256SUMS` + 建 GitHub Release。
- [ ] 四 target 齐备:`{aarch64,x86_64}-apple-darwin` + `{x86_64,aarch64}-unknown-linux-musl`;Linux musl 静态(`ldd` 无动态依赖);macOS 不签名但 curl 下来终端可跑。
- [ ] `OWNER/REPO=Plasticine-Yang/traex-session-manager` 在脚本/workflow/self-update 中单点引用。
- [ ] README 写 `cargo install --path .` 源码安装备选。
