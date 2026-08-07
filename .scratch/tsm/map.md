<!-- wayfinder:map -->

# tsm — traex 会话管理器 TUI

## Destination

一份可交接、可迭代的 **`tsm` 规格文档**(spec),描述一个 Rust 终端 UI:列出 traex 会话(默认按当前项目 `cwd` 过滤,可切换显示全部并带 CWD 列)、搜索/过滤、单个删除、并发批量删除、归档 / 取消归档 / 查看归档会话并恢复、以及会话改名。列表数据只读取自 traex 的 `state_5.sqlite`;删除/归档/取消归档通过 shell 调用 `traex`;改名因无对应 CLI 而直接写库。**并覆盖分发:预编译二进制经 curl 一键安装脚本落地、`tsm self-update`、以及 CI 按 tag 发 GitHub Release(macOS + Linux/musl 四 target)**(T-pkg 时由用户扩入范围)。

规格文档写清足以让人(或另一个 agent 会话)据此实现 `tsm` v1 的所有决策后,本 map 即告完成。

## Notes

**领域词汇(ubiquitous language)** — 见 `CONTEXT.md`。核心:
- **Session(会话)**:traex 中一次对话,由 UUID 标识,对应一个 rollout `.jsonl` 文件与 `threads` 表中一行。
- **Project(项目)**:会话创建时的工作目录 `cwd`。"当前项目会话"= `cwd` 匹配 tsm 启动目录的会话。
- **Rollout 文件**:`~/.trae/cli/sessions/年/月/日/rollout-<ts>-<uuid>.jsonl`,第一行 `session_meta` 记录 `cwd`/`id`/`timestamp`/`git`。
- **threads 表**:`~/.trae/cli/state_5.sqlite` 中的会话索引表,是列表的权威数据源,含 `id/cwd/title/first_user_message/updated_at_ms/archived/rollout_path/git_branch/model/tokens_used`。

**已锁定的决策(charting 时定,不再作为 ticket):**
- **语言 = Rust**;TUI = **ratatui + crossterm**(Rust 事实标准)。
- **列表数据 = rusqlite 只读 `state_5.sqlite` 的 `threads` 表**(与 `traex resume` 选择器同源)。接受"耦合 traex 库表结构"的风险,隔离在单一读取模块内。
- **delete / archive / unarchive = shell 调用 `traex <cmd> <id> [--force]`**(不直接写库)。
- **改名 = 直接 `UPDATE threads.title`**。事实查证:traex 顶层**没有** `rename` 命令,改名只能进会话 `/rename`,故无 CLI 可调。这是已接受的库耦合风险的延伸,把地基从"纯只读"改为"读为主 + 改名是唯一的写"。安全性细节交由 R3。
- **范围**:全纳入(列表/搜索/单删/批删/归档三件套/看归档/改名)。

**每个 session 都应参考的 skills**:`/grilling`、`/domain-modeling`(默认);TUI 交互草样用 `/prototype`;外部事实用 `/research`。

**环境事实(charting 时已测)**:本机共 31 个会话,本项目 `traex-session-manager` 下 2 个;所有 rollout 的 `originator` 均为 `codex-tui`;traex 版本 `0.200.19`;`traex delete <UUID> [--force]`、`archive`、`unarchive` 均存在且接受 UUID。

## Decisions so far

<!-- 索引:每个已关闭 ticket 一行,gist + 链接;详情在 ticket 里,map 不复述 -->

- [R1 — sqlite 读取侧事实](issues/01-sqlite-read-facts.md) — DB 路径解析 `--db`→`TRAECLI_HOME`→`$TRAE_HOME/cli`→`~/.trae/cli`(**不认 `CODEX_HOME`**);`state_5` 的 `_5` 是**数据库世代号非 schema 版本**,tsm 应 glob `state_(\d+).sqlite` 取 max + 列校验;`updated_at` 是秒、`updated_at_ms` 是毫秒(排序用后者);`cwd`/`rollout_path` 恒绝对(**匹配不 canonicalize**,见 T-proj 更正);WAL 只读**安全**,用 `mode=ro`+`busy_timeout`+`query_only`,**禁用 `immutable`**,读事务保持短。
- [R3 — 改名写库安全性](issues/03-rename-writeback-safety.md) — **GO**:改名直接 `UPDATE threads SET title=?1 WHERE id=?2`(与 traex `/rename` 逐字节一致)。实测直接写的 title 熬过 resume+turn 不被覆盖,即便 `session_index.jsonl` sidecar 冲突也是 `threads.title` 胜出。RW 打开(**不带 `_CREATE`**)+ 写前 `busy_timeout(5000)`;**只写 title 不 bump 时间戳**(与 traex 一致、排序稳定,且绝不能单写 `updated_at_ms` 否则秒/毫秒 skew);rowcount 0=id 已被删、超时 `SQLITE_BUSY`→提示重试不崩;sidecar 同步非必需(选择器不读它),换行需去除。

- [R2 — 变更命令确切行为](issues/02-mutation-command-facts.md) — delete/archive/unarchive 均改文件不止改库:delete 删 threads 行+rollout+.artifacts+index 行,**tsm 必须带 `--force`**(无 TTY 否则中止);**archive/unarchive 是物理移动文件到/出 `archived_sessions/` 且不幂等**,tsm 必须按 `archived` 状态门控;并发**安全**,建议上限 4;成败看退出码,失败取 stderr 行;id 必须完整 UUID。

- [T-layout — TUI 布局与交互范式](issues/05-tui-layout.md) — 范式选定 **B:单列表 + 底部横向预览面板**(用户拍板),窄屏(<100 列)降级为纯密集列表、`Enter` 切预览。列:默认 `✓·updated·title·model·tokens`,全部项目视图追加 `cwd` 列、中段/尾部 unicode-width 感知截断。多选 = **`HashSet<SessionId>`**(过滤会重排,禁用索引位)。ratatui `Table`+`TableState`,给出 `App{all_rows/view/selected/scope/lifecycle/search/mode}` 状态草案。主题沿用 plasticine 暗色 + cyan/purple。产出**初版键位表**(汇总 T-filter/T-batch/T-rename;`d`/`a`/`r`/`/` 语义最终仍由各自 ticket 定稿)。原型 primary source:`prototypes/tui-layout.html`。

- [T-proj — "当前项目"判定](issues/08-current-project-semantics.md) — **匹配 = `threads.cwd` 与 tsm 启动目录逐字节精确相等**(无前缀/repo 根/worktree 归并);锚点 = 进程 `current_dir()`(与 traex 同算法);项目外 = 空列表 + 提示"按 X 看全部",**不**自动回退;**归一化 = 不做**——两端都 getcwd,符号链接已解析、大小写规范、目录删了照样匹配,`--cd <symlink>` 启动的会话按设计落到"全部项目"。**据此更正 R1 硬约束 #4:不再 canonicalize**。

- [T-rename — 改名交互与写库落地](issues/07-rename-interaction.md) — `r` 光标行**行内**输入框(非弹窗)、预填原始 `title` 原文、`Enter` 存 / `Esc` 取消。写库承 R3 GO:只 `UPDATE threads.title`、**不 bump 时间戳**、**v1 不同步 `session_index.jsonl`**。校验:trim + 换行/Tab 折叠为空格 + **拒绝空标题**(停编辑态)+ **不设长度上限**。并发:last-writer-wins race **v1 接受不加锁**。失败均非致命:成功重查库+光标按 id 归位、`rowcount==0`→提示已删并刷新、超时 `SQLITE_BUSY`→提示重试并**保留 buffer**。**批量改名排除**,`r` 恒作用光标行。

- [T-batch — 批量删除/归档执行](issues/06-batch-execution.md) — v1 执行形态 = **阻塞进度模态**(边删边浏览推 v2);一个统一 `BatchJob{op,ids}` 引擎驱动 delete/archive/unarchive,三 op 差异仅确认门 / lifecycle 门控(R2)/ `--force`。删除(**含单删,revise 原「单删=`y`」**)走确认模态、大写 **`D`** 确认 + 列将删标题 + 「不可逆、连带删磁盘文件」警告;归档/取消归档免确认但按 lifecycle 门控。并发 = **`std::thread` 定长池 4 + `mpsc`**、`std::process::Command`(delete 带 `--force`)、退出码判成败取 stderr 行,**不用 tokio**(池大小可配吊 T-pkg)。进度 = `N/总数` + `✓/✗/⟳` 聚合(不逐行);失败转结果面 + **保持选中**、成功项去选,`d` 重试失败集。刷新 = 收尾**全量重查库**(R1 短只读事务)非增量。`Esc` 取消 = 停派发、不杀在飞,cancelled 与 failed 一并留选可重试。

- [T-filter — 过滤与搜索模型](issues/04-filter-search-model.md) — 三维正交过滤 **AND 叠加**,默认落点 = `当前项目 ∧ 活跃 ∧ 无搜索`,排序固定 `updated_at_ms DESC`(多排序留迷雾)。**seam**:scope+lifecycle 决定查库(R1 SQL 重查 `all_rows`),search+排序走内存快照(遵守 R1 短读)。**Scope** 两态开关 `p`;**Lifecycle** 严格两态开关 `Tab`(**明确不做**三态/混合视图,用户 2026-08-08 拍板、不进迷雾);**Search** 是**模式**(`/` 实时过滤 → `Enter` 提交并保留 → `Esc` 清除;Normal 下 `Esc` 清已提交过滤、`/` 编辑),大小写不敏感**子串**(非 fuzzy)覆盖 title+first_user_message。搜索时须先 `Enter` 提交才能切 scope/选中(答用户核心追问;选 modal 而非 fzf,保住光秃字母键)。多选 `HashSet` 过滤时**静默保留**隐藏的已选行、**不警告**(用户拍板,推翻原型初版警告条)。空状态按成因分文案(搜索无果/归档空/本项目空)。原型 primary source:`prototypes/filter-model.html`。

- [T-pkg — 二进制、`tsm` 别名与安装方式](issues/09-packaging-and-alias.md) — **用户在此把 destination 扩到含分发**。crate = 单 bin,包名 `traex-session-manager`、bin 名 `tsm`、`edition 2024`,模块 `store/mutate/rename/ui/app`。依赖 **std-only 无 tokio**:`ratatui`+`crossterm`+`rusqlite{bundled}`+`unicode-width`+可选 `anyhow`(**禁** serde/HTTP/tokio,合 R2/T-batch 线程池)。**零配置**:仅 `--db`+R1 env 链,裸跑即可(**据此毕业清除迷雾「默认配置」**)。分发 = 四 target `{aarch64,x86_64}-apple-darwin`+`{x86_64,aarch64}-unknown-linux-musl`(Linux musl 静态、macOS 不签名);curl 一键 `install.sh`(仓库根)落 `~/.local/bin/tsm` + 软链 `traex-session-manager -> tsm`(仿 traex 的 `traecli` 软链);`cargo install --path .` 作源码备选写 README。`tsm self-update[ --check]` **复用安装脚本**(不引 HTTP 依赖)。CI 推 tag `vX.Y.Z` 触发 → 4-target 矩阵 → `tsm-<ver>-<target>.tar.gz`+`SHA256SUMS` → GitHub Release,安装脚本用 `releases/latest/download/`+SHA256 校验。**常量 `OWNER/REPO=Plasticine-Yang/traex-session-manager`**。**Q9=plan-only**:CI/installer 作为 spec 可实现条款,不在本 effort 动手写。

- [T-spec — 汇总为可交接规格文档(终点产物)](issues/10-spec-synthesis.md) — **map 终点达成**。产出 [`spec.md`](spec.md):13 节 + 3 附录,把 R1/R2/R3 + 六个 T-ticket 全部决策汇总成单一自洽可交接文档,照它即可实现 v1。按 T-pkg #9 **新增 §10「分发与发布」**(四 target / `install.sh` SHA256 校验 + 双名软链 / `self-update` 幂等 / tag→tarball+`SHA256SUMS`→Release / `release.yml` 大纲,Q9=plan-only)并把**验收清单扩到覆盖一键装 / 双名触发 / self-update 幂等到最新 / 四 target 产物+SHA256**。合成时清掉下方三条迷雾:库/环境级错误文案(§11)、spec 验证策略(§13 可勾选清单、不附测试脚本/截图)、外部变更刷新(§12 v1 手动 `R`+mutation 后重查、主动感知推 v2)。**Notes 未开执行 override,到此交接实现。**

## Not yet specified

<!-- 朝向目的地、在范围内、但还不够清晰无法切成 ticket 的迷雾 -->

<!-- 以下三条已由 T-spec 合成时定稿并收进 spec.md,不再是迷雾;保留记录以示去向。 -->

- ~~**错误文案与空状态 UX**:找不到库、库被锁、`traex` 不在 PATH 时的呈现/文案。~~ **[T-spec 定]** 库级/环境级错误与空状态收进 `spec.md` §11(启动期致命 vs 运行时非致命 toast 的分界)。过滤相关空状态已由 T-filter 定、当前项目空行为已由 T-proj 定。
- ~~**spec 的验证策略**:是否带验收清单 / 手动测试脚本 / 截图占位。~~ **[T-spec 定]** 采用**可勾选验收清单**(`spec.md` §13,分核心/契约/交互/分发/健壮性),**不**附测试脚本或截图(一手实测在 `research/`、交互 primary source 是两个原型)。
- ~~**外部变更自动刷新策略**:tsm 运行时别处新建/删除会话是否主动感知。~~ **[T-spec 定]** v1 = 手动 `R` + mutation 后自动重查,**不**轮询/文件监听(合 std-only、避后台竞态);主动感知推 v2(`spec.md` §12)。

## Out of scope

<!-- 由目的地圈定、被有意排除的工作;关闭,不再毕业 -->

- **从 tsm 启动 resume 进入会话** —— 用户在 traex 内部自己做。
- **编辑会话内容 / 在 TUI 内查看完整对话记录** —— 只做元数据管理,不做内容浏览。
- **远程 / 跨机会话管理**、多 `TRAE_HOME` 切换 —— v1 只管本机默认 traex 目录。
- **用量统计 / token 分析视图** —— 列表里可展示 token 数,但不做分析。
