# T-filter — 过滤与搜索模型

Type: prototype
Status: resolved
Blocked by: 01

## Question

列表要同时支持三个正交维度的过滤,需要定清模型、默认值、切换方式,并用一个粗糙原型让用户对交互有感觉:

1. **项目范围(scope)**:`当前项目`(默认,按启动 `cwd` 精确匹配)↔ `全部项目`(带 CWD 列)。用一个键循环还是两态开关?
2. **生命周期(lifecycle)**:`活跃`(archived=0,默认)/ `归档`(archived=1)/ `全部`。这直接支撑你要的"查看归档会话再取消归档"。三态如何切换?
3. **文本搜索**:对 `title` + `first_user_message` 做子串匹配。是"/" 进入搜索模式实时过滤(fuzzy 还是 substring?),还是持久的过滤输入框?大小写敏感?
4. **组合语义**:三者是 AND 叠加(scope ∧ lifecycle ∧ search)。确认默认落点 = 当前项目 ∧ 活跃 ∧ 无搜索。
5. **排序**:默认按 `updated_at` 倒序;是否提供其他排序?

## 交付

`## Answer` 记录:三维过滤的状态模型、默认值、各自的切换键(草案,最终键位表在 T-layout 汇总)、搜索匹配语义。用 `/prototype` 做一个纯文本或极简的交互草样(状态转换示意即可),链接到 ticket,让用户对"切范围/切归档/搜索"的手感拍板。

## Answer

**原型(primary source)**:`.scratch/tsm/prototypes/filter-model.html`(单文件双击即开)。用真实 `state_5.sqlite` 采样数据 + 2 条合成 `[DEMO-ARCHIVED]` 行(真实库当前 0 归档)驱动。过滤逻辑写成纯模块 `FilterModel`(`buildView` / `toggleScope` / `toggleLifecycle`),可直接搬进 Rust `App`。用户 2026-08-08 逐项拍板(下列 G/Q 编号即拍板项)。

### 状态模型 = 三维正交过滤,AND 叠加

`App` 的过滤相关字段:
```rust
scope:     Scope,      // Project | All
lifecycle: Lifecycle,  // Active | Archived   (严格两态,见下)
search:    String,     // "" => 关闭;否则大小写不敏感子串
mode:      Mode,        // Normal | Search (搜索是一个模式,见下)
selected:  HashSet<SessionId>,
```

**分界线(seam,已定)**:`scope` + `lifecycle` 决定**查库语句**(切换时按 R1 索引 SQL 重查 `all_rows`);`search` + 排序在**内存**里对该快照生效——不为每次按键重查(遵守 R1"读事务保持短")。这正是 T-layout `all_rows`=查库结果、`view`=过滤+排序后索引的由来。

**组合语义**:`view = sort(filter_search(filter_lifecycle(filter_scope(all_rows))))`,三者 AND 叠加。

### 默认值(默认落点)

`scope=Project ∧ lifecycle=Active ∧ search="" ∧ mode=Normal`,排序 `updated_at_ms DESC`。即"启动就只看本项目活跃会话、最新在前",不是全机一大坨。

### 各维语义与切换键

1. **Scope(两态开关,`p`)**:`Project`(默认,`threads.cwd` 与启动目录**精确相等**,归一化规则见 R1/T-proj)↔ `All`(追加 cwd 列)。两态故用开关不用循环。**(G1)**
2. **Lifecycle(严格两态开关,`Tab`)**:`Active`(`archived=0`,默认)↔ `Archived`(`archived=1`)。**明确非目标**:不做"全部/混合"三态视图,日后也不做(用户 2026-08-08 拍板,不进迷雾)。**(Q1)** `Tab` 键确认可用(用户确认)。
3. **Search(模式,`/`)**:大小写不敏感**子串**匹配,覆盖 `title` + `first_user_message`(R1 的 `LIKE COLLATE NOCASE`,内存里等价用 `to_lowercase().contains()`)。**非 fuzzy**——数据双峰(标题极短 / 首消息几千字 dump),fuzzy 在长文本上噪声大。**(Q2, Q3, Q4)**
4. **排序**:v1 固定 `updated_at_ms DESC`。多排序(按 token / 标题)留迷雾。**(Q6)**

### 搜索的模式模型(用户核心追问:搜索时怎么切 scope / 选中?)

搜索是一个**模式**,因为 `p`/`Tab`/`Space`/`*` 都是可打印字符,输入时只会打进搜索框。故:
- `/` → 进入 **Search 模式**,实时增量过滤(边打字列表边收窄)。输入期间仅 `↑`/`↓`(非可打印,不污染词)可移光标;`p`/`Tab`/`Space` 此刻是文本,**不可用**。
- `Enter` → **提交并保留**过滤,`mode→Normal`。列表仍是过滤后结果(标题栏 `/term  N/M`)。此时 `p`/`Tab`/`Space`/`*`/`j`/`k` 全部恢复,作用在**过滤后的集合**上。这就是"搜一批 → `*` 全选 → `d` 删"的自然流。
- Normal 模式下:`/` 带当前词重新进入编辑;`Esc` 清除已提交的过滤。
- Search 模式下 `Esc` → 清除并退出到 Normal。

选 modal(而非 fzf 式"打字时用 Ctrl 组合键操作"):保留 `p`/`Tab`/`Space` 这些光秃字母键,键位表轻;fzf 模式会逼所有操作键戴 `Ctrl` 前缀。

### 多选 × 过滤:静默保留(Q5)

多选是 `HashSet<SessionId>`,过滤(scope/lifecycle/search 任一)**静默保留**被隐藏的已选行——**不弹警告**(用户 2026-08-08 明确"没必要警告,保留就好")。被隐藏的已选行仍在集合里,批量操作仍会包含它们;由 T-batch 的确认弹窗逐条列出兜底(不在本票范围)。这推翻了原型初版的琥珀警告条。

### 空状态(清掉迷雾中"空状态 UX"的搜索/过滤部分)

按成因分文案,各自引导到修复动作:
- 搜索无果 → "no sessions match "term" in this scope · clear search, or switch to all projects"。
- 归档为空 → "no archived sessions in this scope · switch lifecycle back to active"。
- 本项目为空 → "no sessions in this project · switch to all projects"。
(找不到库 / 库被锁 / `traex` 不在 PATH 等**非过滤**空状态仍留在 map 迷雾,交 spec 合成。)

### 键位(并入 T-layout 初版键位表,本票定稿以下语义)

| 键 | 语义(本票定稿) |
|---|---|
| `p` | 切 scope:Project ↔ All(两态开关) |
| `Tab` | 切 lifecycle:Active ↔ Archived(两态开关) |
| `/` | 进入 Search 模式(实时过滤) |
| `Enter`(Search 内) | 提交并保留过滤 → Normal |
| `Esc`(Search 内) | 清除并退出 → Normal |
| `Esc`(Normal,有过滤) | 清除已提交的过滤 |
| `*` | 反选当前可见(过滤后)集 |

`Space`(勾选)/ `j`/`k`(移动)/ `d`/`a`/`r` 仍由 T-layout / T-batch / T-rename 定;本票只定过滤三维 + 搜索模式 + 多选保留语义。
