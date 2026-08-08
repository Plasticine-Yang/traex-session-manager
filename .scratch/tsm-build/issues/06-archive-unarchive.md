# 06 — 归档 / 取消归档(复用批处理引擎)

**What to build:** 一键归档和取消归档,复用 05 的批处理引擎。`a` 在 Active 视图 = 归档、在 Archived 视图 = 取消归档,按 lifecycle 视图天然门控;因可逆而**免确认、即时执行**(有选中批量、否则光标行)。收尾全量重查后,归档的行从 Active 视图消失、取消归档的行从 Archived 视图消失。这样「看归档 → 恢复」的闭环打通。

规格来源:[`../../tsm/spec.md`](../../tsm/spec.md) §3.1–3.2(archive/unarchive 改动 store + 门控 + 不幂等)、§6.2(三 op 差异仅确认门/门控/`--force`)、§6.5–6.7(进度/失败/刷新复用)、§5.7(`a` 键)。

**Blocked by:** 05 (引擎), 02 (lifecycle 视图)

**Status:** done (64f719a)

- [x] `a` 复用 `BatchJob` 引擎,`Op::Archive`(Active 视图)/ `Op::Unarchive`(Archived 视图);与 delete 的差异仅:无 `--force`、按 lifecycle 门控、动词换 `Archiving…`/`Unarchiving…`。
- [x] **免确认、即时执行**(可逆);有选中则批量、否则作用光标行。
- [x] lifecycle 门控:只对 Active 行 archive、只对 Archived 行 unarchive(由当前视图保证,不会对错误状态调用 → 避开 traex 的不幂等硬错误)。
- [x] 进度/部分失败/重试/取消语义与 05 一致(同一流水线)。<sup>`request_archive` 直接调 05 的 `start_batch`,`poll_batch`/`finish_batch`/`retry_failed`/`cancel_batch` 全未改动;archive 专项测 `archive_partial_failure_shows_result_and_keeps_failure_selected` 验证部分失败转结果面。</sup>
- [x] 收尾全量重查:归档行从 Active 视图消失、取消归档行从 Archived 视图消失(archive 改 `rollout_path` 且移文件,全量重查反映权威真相)。<sup>由 `refresh_after_mutation` 提供;测试用注入 runner 翻 `archived` 位模拟(`archive_*_leave_active_view` / `unarchive_from_archived_view_leaves_it`),真实 `traex archive` 的移文件行为属 §13 人工验收。</sup>
- [x] `selected` 收尾按 id 过滤掉已变动行。<sup>复用 05 的 `prune_selection`;`archive_batch_selected_all_leave_active_view` 验证成功项去选后 `selected_count()==0`。</sup>

## Comments

### 2026-08-08

引擎在 05 已建成可复用(`Op::Archive/Unarchive` + `start_batch`/`poll_batch`/`finish_batch` 全 op 通吃),本票基本是布线:新增 `archive_op()`(按 lifecycle 视图选 op,天然门控 §3.2)与 `request_archive()`(免确认、直接 `start_batch`,承 §6.3 可逆即时),把 `delete_targets` 提炼为 `d`/`a` 共用的 `batch_targets`,main.rs 绑 `a` 键,footer 的 `a` 动词由 `archive_op()` 派生(不重复 `match lifecycle`)。TDD 先写 7 条 app 层测试(lifecycle 映射、免确认不进 ConfirmDelete、单删/批量离开 Active 视图、Archived 视图 unarchive 离开、部分失败转结果面、空视图 no-op)。验证:`cargo test`(72 通过,+7)+ `cargo clippy --all-targets`(零告警)。`/code-review` 双轴:standards 仅一处判断级 Repeated Switches(`match lifecycle` 在 app/ui 两处)——已修,footer 改从 `archive_op()` 取动词单点化;spec 轴零缺口。真实 `traex archive` 的移文件行为属 §13 人工验收范围,自动化测试用注入 runner 翻 `archived` 位模拟。
