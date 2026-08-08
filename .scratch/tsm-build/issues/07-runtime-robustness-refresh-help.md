# 07 — 运行时健壮性 + 刷新 + 帮助

**What to build:** 收口运行时体验,让所有交互齐活后的界面在异常下不崩、键位可发现、刷新模型诚实。`R` 手动重查库刷新;`?` 打开帮助浮层显示定稿键位表;运行时瞬时 `SQLITE_BUSY` 用非致命 toast 提示 + `R` 重试而非崩溃;启动时探测 `traex` 是否在 PATH,不在则仍启动(读+改名可用)、页脚给 dim 提示、变更操作在结果面报错。定死「不主动感知外部变更」的刷新策略。

规格来源:[`../../tsm/spec.md`](../../tsm/spec.md) §5.7(完整键位表)、§11(运行时非致命错误行、traex 不在 PATH 两行)、§12(外部变更刷新策略 = 手动 `R` + mutation 后自动重查,不轮询/不监听)。

**Blocked by:** 06 (要完整键位表 + 所有变更操作齐全)

**Status:** done (fd1e704)

- [x] `R` 手动刷新:重查库更新 `all_rows`,保持 scope/lifecycle/search/selected/光标(按 id)。
- [x] `?` 帮助浮层:显示 §5.7 完整键位表;任意键/`Esc` 关闭。
- [x] 运行时刷新/切换查询超时 `SQLITE_BUSY`:**非致命** toast「库忙,按 `R` 重试」、保留当前 `all_rows`、不崩。<sup>以归一化 busy error 的状态机路径验证 toast 与 stale snapshot 保留;未用真实 3 秒锁竞争做计时测试。</sup>
- [x] 启动探测 `traex` 不在 PATH:**仍启动**(读+改名不依赖 traex);页脚 dim 提示 `traex not found · delete/archive/unarchive unavailable`。
- [x] `traex` 不在 PATH 时执行变更:spawn 失败在批处理结果面按失败项列出、不崩(承 05)。<sup>承 05 的 `traex_runner` spawn-error outcome 路径与结果面渲染;本票新增 PATH 探测不短路该执行路径。</sup>
- [x] 刷新策略定死:仅 `R` 手动 + 每次自身 mutation 后自动全量重查;**不轮询 `updated_at_ms`、不做文件监听**(不感知别处进程的新建/删除)。
- [x] 键位表全部生效自检:`j/k/g/G` `Space` `*` `d` `a` `r` `/` `Enter` `p` `Tab` `R` `?` `q`/`Ctrl-c`,无冲突。<sup>本票新增 `R`/`?` 且帮助表覆盖完整定稿键位;`r` 的实际改名绑定仍由未完成的独立 ticket 04 负责,本票未越界实现。</sup>

## Comments

- 2026-08-08: 实现提交 `fd1e704`;验证 `cargo test`(81 通过)、`cargo clippy --all-targets -- -D warnings`(零告警)、`git diff --check`。专项覆盖手动刷新状态保持、外部变更仅在 `R` 后出现、帮助任意键关闭、缺失 `traex` 的页脚降级提示与 PATH 可执行探测。
