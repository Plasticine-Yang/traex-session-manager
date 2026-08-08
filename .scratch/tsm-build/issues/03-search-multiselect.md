# 03 — 搜索模式 + 多选

**What to build:** 加实时搜索和多选,把「搜一批 → 全选 → (下一步删)」这条流打通。`/` 进入搜索模式,边打字边收窄列表(大小写不敏感子串,匹配 `title` + `first_user_message`);`Enter` 提交并保留过滤回到 Normal,`Esc` 清除。Normal 下 `Space` 勾选当前行、`*` 反选当前可见集,页脚显示 `N selected`。多选按 session id 存,过滤/切换导致行隐藏时**静默保留**已选、不弹警告。

规格来源:[`../../tsm/spec.md`](../../tsm/spec.md) §4.1(`search`/`mode` 字段)、§4.2(search 走内存快照的 seam)、§4.3(子串非 fuzzy)、§4.4(搜索模式模型)、§4.5(多选静默保留)、§4.6(搜索无果文案)、§5.3(多选呈现 `HashSet`)。

**Blocked by:** 02

**Status:** done (12050e7)

- [x] `/` 进入 Search 模式,实时增量过滤;搜索在**内存快照**上做(`to_lowercase().contains()`),**不重查库**(遵守短读 seam)。
- [x] 匹配覆盖 `title` + `first_user_message`,大小写不敏感子串(**非 fuzzy**)。
- [x] Search 模式内:仅 `↑`/`↓` 移光标可用,`p`/`Tab`/`Space` 作为文本输入;`Enter` 提交保留过滤(标题栏 `/term  N/M`)→ Normal;`Esc` 清除并退出 → Normal。 <sup>光标限制/提交/清除有单测覆盖;键位「p/Tab/Space 作为文本输入」由 `handle_key_search` 的 `KeyCode::Char(c) => search_push` 机制保证,未加按键级测试。</sup>
- [x] Normal 模式:`/` 带当前词重新进入编辑;`Esc`(有已提交过滤时)清除过滤。 <sup>`enter_search` 不清 `search`,故 `/` 带词;Normal `Esc` 无过滤时也调用 `search_clear`(幂等空操作),行为等价。</sup>
- [x] `selected: HashSet<SessionId>`;`Space` 切换当前行,`*` 反选当前可见(过滤后)集,页脚 `N selected`。 <sup>集合类型为 `HashSet<String>`(`SessionId` = `Session.id: String`,未引入 newtype)。</sup>
- [x] 过滤(scope/lifecycle/search 任一)使已选行隐藏时**静默保留**在集合里、**不弹警告**;取消过滤后仍选中。
- [x] 搜索无果空状态:`no sessions match "term" in this scope · clear search, or switch to all projects`。 <sup>页脚优先级已调整,使该文案胜过 `N selected`,避免「搜一批→全选到空视图」时被遮挡。</sup>

## Comments

- 2026-08-08 — 实现于 `12050e7`。`app.rs` 加 `Mode`/`search`/`selected` + 搜索/多选方法与 9 条新单测;`main.rs` 按 mode 分派键位;`ui.rs` 加 ▣/▢ 勾选列 + 选中底色带、搜索标题行 `/term▏ N/M`、`N selected` 页脚。验证:`cargo test`(47 passed)、`cargo clippy --all-targets`(clean)。`/code-review` 双轴复审后修正:footer 优先级(no-match 文案胜 `N selected`)、去除近似死分支 `empty_text`、抽出 `toggle_id` 消重、cursor glyph 改为规格的细条 `▏`。
