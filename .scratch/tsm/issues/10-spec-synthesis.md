# T-spec — 汇总为可交接的规格文档(终点产物)

Type: task
Status: resolved
Blocked by: 01, 02, 03, 04, 05, 06, 07, 08, 09

## Question

这是 map 的终点。把上游所有已解决 ticket 的决策汇总成一份**单一、自洽、可交接**的 `tsm` 规格文档,让人或另一个 agent 会话据此实现 v1,无需再回头翻 map。

规格文档应覆盖:
1. **概述与目标**:tsm 是什么、解决什么痛点、非目标(引用 OOS)。
2. **数据来源契约**:sqlite 路径解析、`threads` 关键列语义、只读参数、改名写库参数(来自 R1/R3)。
3. **变更操作契约**:delete/archive/unarchive 的 shell 调用形态、退出码处理、并发上限(来自 R2)。
4. **过滤/搜索模型**:三维过滤 + 默认值(来自 T-filter)。
5. **UI 规格**:布局、列、状态栏、完整键位表(来自 T-layout + 各交互 ticket)。
6. **批量执行**:确认、并发、进度、部分失败(来自 T-batch)。
7. **改名**:交互 + 落地(来自 T-rename)。
8. **项目判定**:当前项目语义(来自 T-proj)。
9. **打包**:crate/bin/别名/安装/依赖(来自 T-pkg)。
10. **验收清单**:v1 完成的可勾选标准。

## 交付

产出 `.scratch/tsm/spec.md`(遵循本仓 issue-tracker 约定:一个 feature 目录一份 spec)。这是本 map 的可交接终点;若 Notes 未开启"把执行带进 map",则到此为止、交接实现。写完在本文件 `## Answer` 记录 spec 路径与一句话摘要,并给 map 追加 Decisions-so-far 指针。

## Answer

**产物**:[`.scratch/tsm/spec.md`](../spec.md) —— 一份单一、自洽、可交接的 tsm v1 规格文档,把 R1/R2/R3 与 T-filter/T-layout/T-batch/T-rename/T-proj/T-pkg 的全部决策汇总为 13 节 + 3 附录,照它即可实现 v1,无需回翻 map 或各 ticket。

**覆盖的 10 个必需面**均已落地:概述与目标(§1)/ 数据来源契约(§2,含 R1 只读 + R3 写库参数,并已并入 T-proj 对 R1 canonicalize 的更正)/ 变更操作契约(§3)/ 过滤搜索模型(§4)/ UI 规格含完整键位表(§5)/ 批量执行(§6)/ 改名(§7)/ 项目判定(§8)/ 打包(§9)/ 验收清单(§13)。

**按 T-pkg 硬约束 #9 落实的两处扩写**:

1. **新增 §10「分发与发布」**:四 target 矩阵 / `install.sh` 一键装行为(SHA256 校验 + `~/.local/bin` + 双名软链)/ `self-update` 复用脚本且幂等 / tag `vX.Y.Z` → `tsm-<ver>-<target>.tar.gz` + `SHA256SUMS` → GitHub Release 的产物约定 / `release.yml` workflow 大纲;`OWNER/REPO=Plasticine-Yang/traex-session-manager` 作单点常量。明确标注 **Q9 = plan-only**(不动手写 CI/installer)。
2. **验收清单(§13)扩到覆盖四条 T-pkg 项**:一键装成功(§13.5)/ 双名触发(`tsm` 与 `traex-session-manager` 软链)/ `self-update` 幂等到最新(已最新不重装不降级)/ 四 target CI 产物齐备且 SHA256 可校验。

**顺带清掉 map 三条迷雾**(合成时定):库级/环境级错误文案与空状态(§11)/ spec 验证策略 = 可勾选验收清单、不附测试脚本或截图,理由见末附(§13 + 附录)/ 外部变更自动刷新 = v1 手动 `R` + mutation 后自动重查,主动感知(轮询/文件监听)推 v2(§12)。

本 map Notes 未开启"把执行带进 map",到此为止、交接实现。
