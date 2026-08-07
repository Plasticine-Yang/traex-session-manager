# 03 — 搜索模式 + 多选

**What to build:** 加实时搜索和多选,把「搜一批 → 全选 → (下一步删)」这条流打通。`/` 进入搜索模式,边打字边收窄列表(大小写不敏感子串,匹配 `title` + `first_user_message`);`Enter` 提交并保留过滤回到 Normal,`Esc` 清除。Normal 下 `Space` 勾选当前行、`*` 反选当前可见集,页脚显示 `N selected`。多选按 session id 存,过滤/切换导致行隐藏时**静默保留**已选、不弹警告。

规格来源:[`../../tsm/spec.md`](../../tsm/spec.md) §4.1(`search`/`mode` 字段)、§4.2(search 走内存快照的 seam)、§4.3(子串非 fuzzy)、§4.4(搜索模式模型)、§4.5(多选静默保留)、§4.6(搜索无果文案)、§5.3(多选呈现 `HashSet`)。

**Blocked by:** 02

**Status:** ready-for-agent

- [ ] `/` 进入 Search 模式,实时增量过滤;搜索在**内存快照**上做(`to_lowercase().contains()`),**不重查库**(遵守短读 seam)。
- [ ] 匹配覆盖 `title` + `first_user_message`,大小写不敏感子串(**非 fuzzy**)。
- [ ] Search 模式内:仅 `↑`/`↓` 移光标可用,`p`/`Tab`/`Space` 作为文本输入;`Enter` 提交保留过滤(标题栏 `/term  N/M`)→ Normal;`Esc` 清除并退出 → Normal。
- [ ] Normal 模式:`/` 带当前词重新进入编辑;`Esc`(有已提交过滤时)清除过滤。
- [ ] `selected: HashSet<SessionId>`;`Space` 切换当前行,`*` 反选当前可见(过滤后)集,页脚 `N selected`。
- [ ] 过滤(scope/lifecycle/search 任一)使已选行隐藏时**静默保留**在集合里、**不弹警告**;取消过滤后仍选中。
- [ ] 搜索无果空状态:`no sessions match "term" in this scope · clear search, or switch to all projects`。
