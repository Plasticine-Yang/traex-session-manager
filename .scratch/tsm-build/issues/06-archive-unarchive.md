# 06 — 归档 / 取消归档(复用批处理引擎)

**What to build:** 一键归档和取消归档,复用 05 的批处理引擎。`a` 在 Active 视图 = 归档、在 Archived 视图 = 取消归档,按 lifecycle 视图天然门控;因可逆而**免确认、即时执行**(有选中批量、否则光标行)。收尾全量重查后,归档的行从 Active 视图消失、取消归档的行从 Archived 视图消失。这样「看归档 → 恢复」的闭环打通。

规格来源:[`../../tsm/spec.md`](../../tsm/spec.md) §3.1–3.2(archive/unarchive 改动 store + 门控 + 不幂等)、§6.2(三 op 差异仅确认门/门控/`--force`)、§6.5–6.7(进度/失败/刷新复用)、§5.7(`a` 键)。

**Blocked by:** 05 (引擎), 02 (lifecycle 视图)

**Status:** ready-for-agent

- [ ] `a` 复用 `BatchJob` 引擎,`Op::Archive`(Active 视图)/ `Op::Unarchive`(Archived 视图);与 delete 的差异仅:无 `--force`、按 lifecycle 门控、动词换 `Archiving…`/`Unarchiving…`。
- [ ] **免确认、即时执行**(可逆);有选中则批量、否则作用光标行。
- [ ] lifecycle 门控:只对 Active 行 archive、只对 Archived 行 unarchive(由当前视图保证,不会对错误状态调用 → 避开 traex 的不幂等硬错误)。
- [ ] 进度/部分失败/重试/取消语义与 05 一致(同一流水线)。
- [ ] 收尾全量重查:归档行从 Active 视图消失、取消归档行从 Archived 视图消失(archive 改 `rollout_path` 且移文件,全量重查反映权威真相)。
- [ ] `selected` 收尾按 id 过滤掉已变动行。
