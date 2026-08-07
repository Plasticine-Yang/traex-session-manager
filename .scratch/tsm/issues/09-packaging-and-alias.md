# T-pkg — 二进制、`tsm` 别名与安装方式

Type: grilling
Status: resolved
Blocked by: —

## Question

交付一个能用 `traex-session-manager` 和缩写 `tsm` 两种方式触发的工具:

1. **crate 结构**:单 binary crate?crate 名 / 包名叫什么(`traex-session-manager`?)。
2. **两个入口**:`tsm` 是独立第二 binary、还是安装时建的 symlink/别名?Cargo 支持 `[[bin]]` 定义多个 binary(同一份 main)。推荐哪种。
3. **安装方式**:`cargo install --path .`?还是提供 `just install` / 脚本把二进制拷到 `~/.local/bin` 并建 `tsm` 链接?你机器上 `~/.local/bin` 已在 PATH(traex 就在那)。
4. **配置/env**:是否读 `TRAE_HOME`(见 R1);是否需要任何配置文件(见迷雾);零配置能否直接跑。
5. **最小依赖集**:ratatui、crossterm、rusqlite(bundled sqlite?)、以及子进程/并发所需(std::process + threads 还是 tokio)。列一份 Cargo 依赖草案。

## 交付

`## Answer` 记录:crate/bin 结构、`tsm` 别名落地方式、安装命令、依赖草案、零配置运行前提。用 `/grilling` 推进。

## Answer

用 `/grilling` + `/domain-modeling` 推进,9 问 2 轮清空前沿。**Q2 把 destination 长大了**:从"源码安装"改为"预编译二进制 + curl 一键装 + 自更新 + CI 发 release"——分发/CI/自更新自此进入 v1 范围(见 map Destination 更新)。GitHub 仓库 = **`Plasticine-Yang/traex-session-manager`**(安装脚本 + spec 用它做单一常量,不散落)。

**Crate 与模块(Q1)**:单 binary crate,包名 `traex-session-manager`,`version="0.1.0"`,`edition="2024"`。不拆 lib / 不上 workspace(过度工程)。模块按已定关注点切:`store`(只读 sqlite,R1)/ `mutate`(shell 调 traex,R2)/ `rename`(写 title,R3)/ `ui`(ratatui,T-layout)/ `app`(状态机,T-layout 状态草案)。

**依赖草案 + 运行时(Q4)**:**std-only,不引 tokio**——`std::thread` + `std::process::Command`,与 R2"小线程池、上限 4、只为限 spawn 不为锁"一致。Cargo 依赖:
- `ratatui` + `crossterm` — TUI。
- `rusqlite { features = ["bundled"] }` — **自带 SQLite 静态编译**,保证 R1/R3 依赖的 WAL / `busy_timeout` / `query_only` pragma 行为可复现、无系统 libsqlite 依赖(也是 musl 静态发布的前提)。
- `unicode-width` — T-layout 的宽度感知截断。
- `anyhow`(可选)— 错误处理人体工学。
- **不要** serde(只读 `threads` 列,不碰 jsonl)/ tokio / 任何 HTTP crate。
- 具体版本号留到写 `Cargo.toml` 时钉。

**配置模型(Q3,顺带清迷雾)**:**v1 零配置文件,全靠 flag + env**。只认 `--db <path>` 覆盖 + R1 的 env 链(`--db` → `$TRAECLI_HOME` → `$TRAE_HOME/cli` → `~/.trae/cli`);不带任何参数即可对默认 traex home 直接跑(零配置前提)。**据此毕业并清除 map "Not yet specified" 的"默认配置"整条**。

**目标平台矩阵(Q5)**:四个 target,各有原生 CI runner,无需交叉编译——
- `aarch64-apple-darwin`、`x86_64-apple-darwin`
- `x86_64-unknown-linux-musl`、`aarch64-unknown-linux-musl`(**Linux 一律 musl 静态**,避 glibc 版本地狱,curl 下来到处能跑)。
- `rusqlite{bundled}` 编 C 源码,musl 构建需 `musl-tools`(CI 里装)。
- macOS **v1 不签名 / 不公证**:curl 下载的 CLI 无 quarantine 属性,终端跑未签名 CLI 不被 Gatekeeper 硬拦。

**安装位置 + 别名机制(Q6)**:装到 **`~/.local/bin`**(已在 PATH、traex 就住这儿做 companion、XDG 惯例、不改 shell profile)。真实二进制名 **`tsm`**(`[[bin]] name = "tsm"`,包名仍 `traex-session-manager`);安装脚本建软链 **`traex-session-manager -> tsm`**——与 traex 自己的 `traecli`/`trae-cli -> traex` 软链模式一致,release 里只需塞一个二进制。脚本检测 `~/.local/bin` 是否在 PATH,不在则提示。

**自更新(Q7)**:**`tsm self-update` 复用安装脚本**——子命令内部执行 `curl -fsSL <install-url> | sh`,零 Rust HTTP 依赖、安装逻辑单一真源(守住 Q4);`--check` 先比 `--version` 与最新 release tag 再动手。放弃 `self_update` crate(会拉 reqwest/ureq + tar,违背 Q4)。macOS/Linux 恒有 curl/sh,不构成新前提。

**Release 触发 + 产物约定(Q8)**:
- 推 SemVer tag **`vX.Y.Z`** 触发;CI 校验 tag 与 `Cargo.toml` version 一致。
- 矩阵构建 4 target → 每 target 产 `tsm-<version>-<target>.tar.gz`(含 `tsm` 二进制 + LICENSE/README)+ 汇总 `SHA256SUMS`。
- 建 GitHub Release 上传全部产物。
- 安装脚本走 `releases/latest/download/<asset>`(GitHub latest 重定向,免 API token、无限流),**下载后校验 SHA256** 再解包落地 + 建软链。
- `install.sh` 放**仓库根**,raw URL 即 curl 一键装目标。

**Plan-vs-do(Q9)**:**spec-only(不动手写 CI/installer)**。本 map destination 是 spec,保持 wayfinder"只决策不动手"默认。以上分发/CI/自更新决策作为 spec 的**可实现条款**由 T-spec 收录(新增"分发与发布"一节:target 矩阵 / `install.sh` 行为 / 自更新流程 / `release.yml` workflow 大纲);真正写 `.github/workflows/release.yml` + `install.sh` 留到照 spec 实现的阶段。**未开 Notes 执行 override,未新建 task ticket。**

### 对 spec / T-spec 的硬约束(供 T-spec 落地)
1. **crate**:单 bin crate,包名 `traex-session-manager`,bin 名 `tsm`,`edition 2024`;模块 `store/mutate/rename/ui/app`。
2. **依赖**:`ratatui`+`crossterm`+`rusqlite{bundled}`+`unicode-width`+可选 `anyhow`;**禁** tokio/serde/HTTP crate。
3. **配置**:零配置文件;仅 `--db` + R1 env 链;裸跑即可。
4. **分发 target**:`{aarch64,x86_64}-apple-darwin` + `{x86_64,aarch64}-unknown-linux-musl`;Linux musl 静态;macOS 不签名。
5. **安装**:curl 一键装 `install.sh`(仓库根)→ 落 `~/.local/bin/tsm` + 软链 `traex-session-manager`;`cargo install --path .` 作为源码安装备选写进 README。
6. **自更新**:`tsm self-update[ --check]` 复用安装脚本,不引 HTTP 依赖。
7. **release**:tag `vX.Y.Z` 触发 → 4-target 矩阵 → `tsm-<ver>-<target>.tar.gz` + `SHA256SUMS` → GitHub Release;安装脚本用 `releases/latest/download/` + SHA256 校验。
8. **常量**:`OWNER/REPO = Plasticine-Yang/traex-session-manager`(spec 与脚本单点引用)。
9. **T-spec 新增 §"分发与发布"**,并把验收清单扩到覆盖:一键装成功 / `tsm` 与 `traex-session-manager` 两名可触发 / `self-update` 幂等到最新 / 四 target CI 产物齐备且 SHA256 可校验。
