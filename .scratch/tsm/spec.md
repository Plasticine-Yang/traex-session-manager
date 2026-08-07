# tsm — traex 会话管理器 · v1 规格文档

> **状态**:可交接。本文件是 `.scratch/tsm/` wayfinder effort 的终点产物,汇总 R1/R2/R3 与 T-filter/T-layout/T-batch/T-rename/T-proj/T-pkg 的全部决策。**照本文件即可实现 tsm v1,无需回翻 map 或各 ticket。**每节末尾的 `来源` 只为追溯,不是阅读前置。
>
> **领域词汇**以 `CONTEXT.md` 为准(Session / Project / Rollout 文件 / Store / Lifecycle / Scope / Search)。本文沿用,不重定义。

---

## 1. 概述与目标

**tsm**(全名 `traex-session-manager`)是一个 Rust 终端 UI,用来**列出、搜索/过滤、删除、批量删除、归档 / 取消归档 / 看归档并恢复、以及改名** traex CLI 的会话(Session),免去"resume 进选择器 → 切会话 → 敲斜杠命令"的繁琐流程。

**痛点**:traex 本身管理会话要先 `resume` 进一个选择器,再进会话内部敲 `/rename` 之类的斜杠命令;批量删除、按项目筛选、看归档恢复都很别扭。tsm 把这些元数据操作提到一个独立、可直接键盘驱动的列表界面。

**数据地基(charting 锁定,不再是 ticket)**:

- **列表数据 = 只读** traex 的 `state_5.sqlite` 里的 `threads` 表(与 `traex resume` 选择器同源)。接受"耦合 traex 库表结构"的风险,隔离在单一读取模块 `store` 内。
- **delete / archive / unarchive = shell 调用 `traex <cmd> <id> [--force]`**(不直接写库)。
- **改名 = 直接 `UPDATE threads.title`**。事实查证:traex 顶层**没有** `rename` 命令,改名只能进会话敲 `/rename`,故无 CLI 可调 —— 这是 tsm **唯一的写库操作**,安全性由 R3 验证为 GO。
- **语言 = Rust**;TUI = **ratatui + crossterm**(Rust 事实标准)。

**非目标(Out of scope,v1 明确不做)**:

- **从 tsm 启动 resume 进入会话** —— 用户在 traex 内部自己做。tsm 只管元数据。
- **编辑会话内容 / 在 TUI 内浏览完整对话记录** —— 只做元数据管理,不做内容浏览(预览面板只展示 `first_user_message` 首段作为辨识,不是阅读器)。
- **远程 / 跨机会话管理、多 `TRAE_HOME` 切换** —— v1 只管本机默认 traex 目录(可用 `--db` 覆盖单库,但不做多 home 编排)。
- **用量统计 / token 分析视图** —— 列表可展示 token 数,但不做分析。

---

## 2. 数据来源契约(Store)

> 来源:R1(sqlite 读取侧事实,实测 traex `0.200.19` / sqlite `3.51.0`)、R3(改名写库安全性)、T-proj(对 R1 匹配规则的更正)。

### 2.1 数据库路径解析

按以下**优先级**解析到 traex 的 cli 目录,再在其中定位 state 库:

1. `--db <path>` 命令行 flag —— 直接指向一个 `.sqlite` 文件,**跳过**下面的目录解析与 glob。
2. 环境变量 `$TRAECLI_HOME` —— 直接当作 cli 目录。
3. 环境变量 `$TRAE_HOME` —— cli 目录 = `$TRAE_HOME/cli`。
4. 默认 `~/.trae/cli`。

**忽略 `CODEX_HOME`**(本 build 的 traex doctor 完全不认它)。

### 2.2 库文件名 glob(不硬编码 `state_5`)

`_5` 是 traex 的**数据库世代号**(另起干净库时 bump),**不是** schema 版本(该库 `_sqlx_migrations` 已到 34)。未来 build 可能出 `state_6`。故:

- 在解析出的 cli 目录里 glob 正则 `^state_(\d+)\.sqlite$`,**取最大 N**。
- 打开后**校验** `threads` 表含所需列(见 2.3)才信任;列缺失 → 报"无法识别的 traex 数据库结构,tsm 可能已过时",退出(见 §11)。
- `--db` 直指文件时同样做列校验。

### 2.3 `threads` 表关键列语义

| 列 | 类型/语义 | tsm 用法 |
|---|---|---|
| `id` | UUIDv7,主键,唯一 | 传给 delete/archive/unarchive;`selected` 集的键;比较**必须用完整 UUID**(8 字符前缀存在真实碰撞) |
| `cwd` | 项目目录,**恒绝对路径**(NOT NULL) | Scope=Project 的精确匹配键(**不 canonicalize**,见 §9) |
| `title` | 标题,`NOT NULL DEFAULT ''` | 列表主显示;改名写此列;空 ⇔ 该会话尚无用户事件 |
| `first_user_message` | 首条用户消息,`NOT NULL DEFAULT ''` | 搜索的另一匹配字段;title 空时列表兜底显示;预览面板展示首段 |
| `updated_at` | **epoch 秒**,有索引 | 列表 `updated` 列显示(`MM-DD HH:MM`) |
| `updated_at_ms` | **epoch 毫秒**,触发器维护,有索引 | **排序键**(`ORDER BY updated_at_ms DESC`,亚秒 tie-break) |
| `archived` | 严格 `0/1`,有索引 | Lifecycle 过滤;`archived_at`(秒)仅归档时置 |
| `rollout_path` | rollout `.jsonl`,**恒绝对路径**(NOT NULL) | **archive 会改写此列并移动文件**(见 §3);tsm 不直接用,但刷新逻辑要意识到它会变 |
| `git_branch` / `model` / `tokens_used` | 附属元数据 | 预览面板 / 列显示 |

已存在复合索引 `(archived, cwd, updated_at_ms DESC)` —— tsm 的查询被覆盖,无需自建索引。

### 2.4 只读打开参数(列表 / 搜索 / 刷新)

WAL 模式下"读不阻塞写、写不阻塞读、读见一致快照"已实测(读取期间行数 35→41→34、一条 `archived` 翻位,全程零锁错误)。打开:

```rust
let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
          | OpenFlags::SQLITE_OPEN_URI
          | OpenFlags::SQLITE_OPEN_NO_MUTEX;
let conn = Connection::open_with_flags(&db_path, flags)?;
conn.busy_timeout(Duration::from_millis(3000))?; // 仍需处理瞬时 SQLITE_BUSY
conn.pragma_update(None, "query_only", true)?;    // 额外写保护
```

**硬约束**:

- 用 `mode=ro`;**绝不用 `immutable=1`**(对 live 库会读到错误结果 / `SQLITE_CORRUPT`);不用 `nolock`。
- **读事务保持短**:每次刷新一条独立短查询,不长持只读快照(长读会饿死 traex 的 WAL checkpointer → WAL 膨胀)。
- 改名的写连接与读连接**分离**(见 2.6)。

### 2.5 列表 / 过滤 / 搜索 / 排序 SQL(均一律用绑定参数)

`scope + lifecycle` 决定查库语句(切换时重查 `all_rows`);`search + 排序`在内存快照上生效(见 §4 的 seam)。基础查询:

```sql
-- 当前项目 · 活跃(默认落点)
SELECT id,title,first_user_message,cwd,updated_at,updated_at_ms,
       archived,archived_at,git_branch,model,tokens_used
FROM threads WHERE cwd = ?1 AND archived = 0 ORDER BY updated_at_ms DESC;

-- 当前项目 · 归档:同上,archived = 1
-- 全部项目 · 活跃:去掉 cwd 谓词,archived = 0
-- 全部项目 · 归档:去掉 cwd 谓词,archived = 1
```

搜索**不下推 SQL**(在内存快照上 `to_lowercase().contains()`,见 §4);若日后要下推,等价 SQL 为 `(title LIKE ?1 COLLATE NOCASE OR first_user_message LIKE ?1 COLLATE NOCASE)`,`?1='%term%'`。

排序统一 `updated_at_ms DESC`。

### 2.6 改名写库参数(唯一的写操作)

**结论 = GO**(R3 实测:直接写的 title 熬过完整 resume+turn 不被覆盖;即便 `session_index.jsonl` sidecar 冲突,`threads.title` 也胜出)。

```rust
let conn = Connection::open_with_flags(
    &state_db_path,
    OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_URI, // 不带 _CREATE:库缺失绝不新建
)?;
conn.busy_timeout(Duration::from_millis(5000))?; // 显式设 5s
let n = conn.execute(
    "UPDATE threads SET title = ?1 WHERE id = ?2",
    params![new_title, id],
)?; // n==1 成功;n==0 = id 已被别处删
```

**硬约束**:

- **只写 `title` 列**(与 traex `/rename` 的 SQL 逐字节一致)。`title` 是 NOT NULL;空标题在 tsm 侧已被校验拦下(见 §7),不会写到这里。
- **不 bump 时间戳**(与 traex `/rename` 一致,排序稳定;改名是元数据非活动)。**绝不单独写 `updated_at_ms`**(会造成秒/毫秒 skew)。将来若要"改名置顶",只 bump 秒级 `updated_at` 让触发器同步毫秒。
- 打开**不带 `_CREATE`**;不改 `journal_mode`(已是持久 WAL)。
- `rowcount`:`1`=成功;`0`=id 已被别处删 → 优雅提示;超时后仍 `SQLITE_BUSY`(罕见)→ 提示重试不崩。
- **v1 不同步 `session_index.jsonl`**:选择器不读它,同步对正确性非必需,保持"唯一写只写 title"的干净地基。

---

## 3. 变更操作契约(Mutate:delete / archive / unarchive)

> 来源:R2(在隔离 cwd 的一次性 `exec` 会话上一手实测,只删自造会话)。

tsm 通过 **shell 调用 `traex`** 执行变更,不直接写库(改名除外)。

### 3.1 各命令改动的 store

| 命令 | 改动 |
|---|---|
| `traex delete <uuid> --force` | 删 `threads` 行 + 删 rollout `.jsonl` + 删同级 `.artifacts/` 目录 + 删 `session_index.jsonl` 对应行。日期目录保留。**不可逆**。 |
| `traex archive <uuid>` | 置 `threads.archived=1` + 写 `archived_at` + **改写 `threads.rollout_path`** + 把 `.jsonl`/`.artifacts/` **物理移动**到 `~/.trae/cli/archived_sessions/`。 |
| `traex unarchive <uuid>` | 上述全部反向(文件移回)。 |

**关键推论**:archive 不是"翻标志位"而是**移动文件**,故变更后必须**全量重查库**才能反映权威真相(见 §6.7)。

### 3.2 硬约束

1. **delete 一律带 `--force`**:tsm spawn 的 traex **无 TTY**,不带 `--force` 会直接中止(`Error: cannot confirm session deletion without an interactive terminal; rerun with --force`)。
2. **archive/unarchive 前必须按当前 `archived` 状态门控**:两者都**不幂等**,重复归档已归档 / 取消归档未归档的都是硬错误 exit 1(`no rollout found` / `no archived rollout found`)。tsm 只对活跃行 archive、只对归档行 unarchive —— 由 Lifecycle 视图天然门控(active 视图只出 `a`=archive,archived 视图只出 `a`=unarchive)。
3. **id 必须完整规范 UUID**:前缀/乱码在查库前就被拒(exit 1)。直接传 `threads.id` 原值。
4. 并发上限 **4**(见 §6.4)。

### 3.3 退出码与输出形态

| 结果 | exit | stdout | stderr |
|---|---|---|---|
| 成功 | 0 | 一行 `Deleted/Archived/Unarchived session <uuid>.` | 空 |
| 失败 | 1 | 空 | 一行 `Error: <msg>` |

**tsm 按退出码分支**:成功忽略 stdout;失败**提取 stderr 那一行**呈现给用户(见 §6.6)。单条墙上耗时 ~0.08–2.0s(进程启动主导,非 DB),进度按每条更新。

---

## 4. 过滤 / 搜索模型

> 来源:T-filter(原型 `prototypes/filter-model.html`,用户 2026-08-08 逐项拍板)。

### 4.1 三维正交过滤,AND 叠加

```rust
scope:     Scope,      // Project | All
lifecycle: Lifecycle,  // Active | Archived   (严格两态)
search:    String,     // "" => 关闭;否则大小写不敏感子串
mode:      Mode,        // Normal | Search | ...(见 §5.5)
selected:  HashSet<SessionId>,
```

**组合语义**:`view = sort(filter_search(filter_lifecycle(filter_scope(all_rows))))`。

**默认落点**:`scope=Project ∧ lifecycle=Active ∧ search="" ∧ mode=Normal`,排序 `updated_at_ms DESC`。即"启动就只看本项目活跃会话、最新在前"。

### 4.2 seam(实现分界线,已定)

- `scope + lifecycle` → 决定**查库语句**,切换时按 §2.5 重查 `all_rows`。
- `search + 排序` → 在**内存**里对 `all_rows` 快照生效,**不为每次按键重查库**(遵守 R1 短读)。

这就是 `all_rows`(查库结果快照)与 `view`(过滤+排序后指向 `all_rows` 的索引 `Vec<usize>`)分离的由来。

### 4.3 各维语义与切换

1. **Scope(两态开关 `p`)**:`Project`(默认,`threads.cwd` 与启动目录**逐字节精确相等**,归一化规则见 §9)↔ `All`(追加 `cwd` 列)。
2. **Lifecycle(严格两态开关 `Tab`)**:`Active`(`archived=0`,默认)↔ `Archived`(`archived=1`)。**明确非目标**:不做"全部/混合"三态视图(用户 2026-08-08 拍板,不进 v2 迷雾)。
3. **Search(模式 `/`)**:大小写不敏感**子串**匹配(内存 `to_lowercase().contains()`),覆盖 `title` + `first_user_message`。**非 fuzzy**(数据双峰:标题极短 / 首消息几千字 dump,fuzzy 在长文本上噪声大)。
4. **排序**:v1 固定 `updated_at_ms DESC`(多排序留 v2 迷雾)。

### 4.4 搜索的模式模型

因 `p`/`Tab`/`Space`/`*` 都是可打印字符,搜索必须是一个**模式**(否则输入会打进过滤维度):

- `/` → 进入 **Search 模式**,实时增量过滤(边打字列表边收窄)。输入期间仅 `↑`/`↓` 可移光标;`p`/`Tab`/`Space` 此刻是文本,不可用。
- `Enter` → **提交并保留**过滤,`mode→Normal`(标题栏显示 `/term  N/M`)。此时 `p`/`Tab`/`Space`/`*`/`j`/`k` 全部恢复,作用在**过滤后的集合**上。这就是"搜一批 → `*` 全选 → `d` 删"的自然流。
- Normal 模式下:`/` 带当前词重新进入编辑;`Esc` 清除已提交的过滤。
- Search 模式下 `Esc` → 清除并退出到 Normal。

**选 modal 而非 fzf 式组合键**:保留 `p`/`Tab`/`Space` 这些光秃字母键,键位表轻。

### 4.5 多选 × 过滤:静默保留

多选是 `HashSet<SessionId>`;过滤(scope/lifecycle/search 任一)**静默保留**被隐藏的已选行,**不弹警告**(用户 2026-08-08 拍板)。隐藏的已选行仍在集合里,批量操作仍会包含它们;由批量确认弹窗逐条列出兜底(见 §6.3)。

### 4.6 过滤/搜索相关空状态(按成因分文案)

- 搜索无果 → `no sessions match "term" in this scope · clear search, or switch to all projects`
- 归档为空 → `no archived sessions in this scope · switch lifecycle back to active`
- 本项目为空 → `no sessions in this project · switch to all projects`(此空状态**行为**由 §9 定死:空列表 + 提示,**不**自动回退)

> 库级/环境级空状态(找不到库 / 库被锁 / traex 不在 PATH)见 §11。

---

## 5. UI 规格

> 来源:T-layout(原型 `prototypes/tui-layout.html`,范式 B 由用户 2026-08-07 拍板)+ 各交互 ticket 的键位。

### 5.1 交互范式 —— B:单列表 + 底部横向预览面板

- 上方是会话 `Table`,下方 `preview` 面板显示**当前光标行**的完整 `title` / `id`(中段截断、两端可辨)/ `git_branch` / `model` / `tokens` / `cwd` / `first_user_message` 首段。
- **窄屏降级**:终端列宽 `< 100` 时自动隐藏预览面板(退化为纯密集列表);`Enter` 手动切换预览显隐。理由:标题信息量两极分化,长标题/首消息一行放不下;横向底部切分比竖向更抗窄屏、不受 CJK 双宽字符把竖分隔线搞参差之苦。
- **按项目分组(原型 D)**:**v1 不启用**。全部项目视图是带 `cwd` 列的扁平列表(分组渲染的 `TableState` 复杂度 + 单项目视图里只有一个组纯噪声,不划算),推 v2 迷雾。

### 5.2 列定义与截断

- **默认(当前项目)**:`✓(选择框) · updated · session · model · tokens`
  - `updated` = `MM-DD HH:MM`(取 `updated_at` 秒;排序键仍是 `updated_at_ms`)。
  - `session` = `COALESCE(NULLIF(title,''), NULLIF(first_user_message,''), '(untitled)')`,弹性列,尾部省略号截断。
  - `tokens` = 人类可读 `442k` / `4.3M`(取 `tokens_used`)。
- **全部项目(scope=All)**:在 `updated` 与 `session` 之间**追加 `cwd` 列**(相对 `~` 显示,中段截断);此视图信息密度高,可省略 `tokens` 列。
- **归档标记**:不占独立列。`Archived` 视图里把 `tokens` 位置换成 `archived_at`。
- **截断策略**:固定列(✓/updated/model/tokens)定宽;`session`(及 All 视图的 `cwd`)吃剩余宽度,**`unicode-width` 感知截断 + `…`**(避免切出半个 CJK 字符)。

### 5.3 多选呈现

- 勾选态:行首 `▣`(已选,cyan/高亮)/ `▢`(未选,dim);被选行叠加高亮底色带。
- **状态模型 = `HashSet<SessionId>`**(**不是** `Vec<bool>`):列表会因 search/scope/lifecycle 过滤重排,索引位选择会串位;按 id 存,跨过滤/刷新稳定。
- 页脚显示 `N selected`;`Space` 切换当前行,`*` 反选当前可见集。

### 5.4 状态栏 / 页脚

- 顶部标题栏右侧显示 `scope · lifecycle`(如 `project · active` / `all · archived`)。
- 底部页脚(与预览面板边框合并)显示 `N selected` + 上下文相关快捷键提示;若 traex 不在 PATH,追加一条 dim 提示(见 §11)。
- 搜索态:顶部输入行 `/term▏` + `命中/总数`;批处理态:模态框(见 §6);改名态:行内输入框 + 底部 `[Enter] save [Esc] cancel`。

### 5.5 ratatui 用法与状态结构

- 组件:`ratatui::widgets::Table` + `TableState`(光标/滚动);预览 `Paragraph` + `Wrap`;模态用居中 `Block` 浮层。事件循环 crossterm(raw mode + alternate screen)。
- 状态草案:

```rust
struct App {
    all_rows: Vec<Session>,       // 上次查库结果(权威快照)
    view: Vec<usize>,             // 过滤+排序后指向 all_rows 的索引(渲染用)
    table: TableState,            // 光标/滚动
    selected: HashSet<SessionId>, // 跨过滤稳定的多选
    scope: Scope,                 // Project | All
    lifecycle: Lifecycle,         // Active | Archived
    search: String,               // "" => 关闭
    show_preview: bool,           // 宽度 < 100 或用户 Enter 切换 => false
    mode: Mode,
}

enum Mode {
    Normal,
    Search,                                   // 实时过滤编辑中
    Rename { buf: String },                   // 行内改名(§7)
    ConfirmDelete { ids: Vec<SessionId> },    // 删除确认模态(§6.3)
    Running { job: BatchJob, done: usize, failed: Vec<(SessionId, String)> }, // 进度模态(§6.5)
    Result { failed: Vec<(SessionId, String)> }, // 部分失败结果面(§6.6)
    Help,
}
```

- 每次 mutation(删/归档/改名)后**重查库刷新 `all_rows`**(R1 短只读事务),`selected` 按 id 过滤掉已消失的行,光标按 id 归位。

### 5.6 主题(plasticine 暗色)

`bg #16161e` · `fg #c0caf5` · `dim #565f89` · `cyan #7dcfff`(高亮/光标/选中)· `purple #bb9af7`(标题栏/键位)· green/red/yellow 用于成功/失败/警告。

### 5.7 完整键位表(v1 定稿)

| 键 | 动作 |
|---|---|
| `j`/`↓` `k`/`↑` | 上下移动光标 |
| `g` / `G` | 跳到顶部 / 底部 |
| `Space` | 切换当前行选中 |
| `*` | 反选当前可见(过滤后)集 |
| `d` | 删除:有选中则删全部选中,否则删光标行 → 确认模态(§6) |
| `a` | active 视图 = 归档;archived 视图 = 取消归档。按 Lifecycle 门控(§3.2),可逆,**免确认** |
| `r` | 光标行行内改名 → 写 `threads.title`(§7);**忽略多选**,恒作用光标行 |
| `/` | 进入 Search 模式(实时过滤,§4.4) |
| `Enter`(Search 内) | 提交并保留过滤 → Normal |
| `Esc`(Search 内) | 清除并退出 → Normal |
| `Esc`(Normal 有过滤) | 清除已提交的过滤 |
| `Enter`(Normal) | 切换预览面板显隐 |
| `p` | 切 scope:Project ↔ All(+cwd 列) |
| `Tab` | 切 lifecycle:Active ↔ Archived(严格两态) |
| `R` | 手动刷新(重查库;mutation 后也自动刷新) |
| `?` | 帮助 / 键位表 |
| `q` / `Ctrl-c` | 退出 |
| `D`(删除确认模态内) | 确认删除(**大写**,防误触) |
| `Esc` / `n`(删除确认模态内) | 取消 |

---

## 6. 批量执行(delete / archive / unarchive)

> 来源:T-batch(`/grilling` 三轮,用户 2026-08-08 逐条采纳)。承 R2 + T-layout。

### 6.1 执行形态 —— 阻塞进度模态(非后台)

v1 批量删/归档跑起来时**阻塞在一个进度模态**,删完才放行继续浏览。理由:核心痛点是"多选→一次性删掉"这个聚焦动作,阻塞式心智最简;单条 ~0.08–2.0s、并发 4,一批通常 ~2s、最坏几十条约十几秒,模态内进度 + `Esc` 取消足够;后台跑会引入真实竞态(光标停在正被删的行、一批未完又起第二批、列表眼皮下重排)。**边删边浏览的后台模式推 v2。**

### 6.2 统一批处理引擎

```rust
struct BatchJob { op: Op, ids: Vec<SessionId> }
enum Op { Delete, Archive, Unarchive }
```

一个 `BatchJob` 驱动同一条 spawn/进度/失败/刷新流水线。三个 op 的差异**只有三处**:确认门(§6.3)、lifecycle 门控(§3.2)、`--force` 仅加在 delete 上。一套代码路径。

### 6.3 确认强度

- **删除(不可逆 + 连带删磁盘文件)**:弹确认模态,列出**将删的标题**(>10 条则"前 10 条 + 还有 M 条")+ 数量 + "不可逆、同时删除 rollout 文件"警告;用大写 **`D`** 确认(非 `Enter`,防误触),`Esc`/`n` 取消。
- **单删(无选中、只删光标行)统一走同一模态**(只列 1 条)。**这里 revise 了原方向"单删=`y`"**:删除既不可逆又连带删磁盘文件,单键 `y` 恰是模态要防的误触。
- **归档 / 取消归档(可逆)= 免确认,即时执行**,按 §3.2 lifecycle 门控。

### 6.4 并发 fan-out 机制

**`std::thread` 定长池(4)+ `mpsc` channel**;每个 worker 用 `std::process::Command` 跑 `traex <op> <uuid>`(delete 带 `--force`),收集**退出码 + stderr 行**。**不用 tokio**(这是 fan-out 外部进程而非进程内异步 I/O,async 运行时零收益、徒增复杂度,也与 §10 的 std-only 依赖集一致)。池大小 = 4(R2 建议上限,硬编码)。

### 6.5 进度 UI

模态**顶部一行** `Deleting… 12/30` + 进度条;下面一行**聚合计数** `✓ 20  ✗ 1  ⟳ 4`(成功/失败/在飞)。**不逐行渲染全部条目**(并发 4 乱序完成 + CJK 宽度抖动会成噪声)。失败项累积到模态底部小列表(带 `Error: <msg>` 行)。归档/取消归档同套,动词换 `Archiving…` / `Unarchiving…`。

### 6.6 部分失败上报 + 重试

- 跑完有失败 → 模态**转结果面**(`Mode::Result`)列出失败项 + 每条 stderr(R2 已是 `Error:` 前缀的人类可读行)。**spawn 失败(如 traex 不在 PATH)是一类失败**,同样在此列出(见 §11)。
- 全成功 → **自动关闭**,可选一行短 toast(`Deleted 30.` / `Archived 5.`)。
- **失败项保持选中,成功项移除**(承 T-layout"`selected` 按 id 过滤掉消失/变动行")。
- 结果面提示"按 `d` 重试失败项 / `Esc` 关闭";重试 = 拿仍选中的失败集再走一遍同一流水线。

### 6.7 刷新时机

收尾**全量重查库一次**(单个短只读事务,§2.4),**不做逐条增量移除**。理由:阻塞期间列表不可见,增量移除零收益;archive 改 `rollout_path` 且移文件,全量重查才反映权威真相、也自然处理"归档后从 active 视图消失";避开中途重排与秒/毫秒 skew。重查后按 §6.6 过滤 `selected`。

### 6.8 取消(`Esc`)语义

`Esc` = **停止派发新任务,不 kill 已在飞的 ≤4 个**(traex 删到一半被 SIGKILL 有留残状态风险,而单条最坏才 ~2s),让在飞的自然收尾。随后进结果面:**已成功**去选,**已失败 + 未发起(cancelled)**一并保持选中可重试。即"取消"只保证不再多删,已发出的听天由命 —— 对不可逆操作这是最安全的语义。

---

## 7. 改名(Rename)

> 来源:T-rename(`/grilling` 六项全采纳)+ R3 GO。承 T-layout `Rename{buf}` 范式。

### 7.1 触发与编辑交互

- `r` 在光标行**原地弹出单行行内输入框**(非弹窗);底部提示 `[Enter] save [Esc] cancel`。
- 预填 = 该行**原始 `threads.title` 原文**,光标置末尾;标准单行编辑(←/→、Home/End、Backspace/Delete)。
- 若 `title` 为空(列表靠 `first_user_message` 兜底显示),输入框预填**空** —— 所见即 store 真实 title,不预填兜底文案。
- `Enter` 提交、`Esc` 取消(丢弃 buffer,还原原 title 显示)。

### 7.2 写入落地

见 §2.6:`UPDATE threads SET title = ?1 WHERE id = ?2`,**不 bump 时间戳**,**不同步 sidecar**,写连接 `SQLITE_OPEN_READ_WRITE`(不带 `_CREATE`)+ `busy_timeout(5000)`。

### 7.3 校验规则(提交时按序清洗/拦截)

1. 首尾空白 → **trim**。
2. 内嵌换行/Tab → **折叠成单空格**(title 会显示在 traex 选择器,不能带换行)。
3. trim 后为空 → **拒绝提交**,保留旧 title,提示"标题不能为空",**停在编辑态**(不写 DB)。
4. 长度 → **不设硬上限**(DB 不限、traex 自动标题本就长;靠列表 `unicode-width` 截断显示)。

### 7.4 并发 / 占用

同会话正被 traex 打开时的 last-writer-wins race:**v1 接受,不做检测/加锁**(无法可靠检测"正在打开",且需 traex 在写后自身 `/rename`/auto-name 才会覆盖,概率低)。正常 resume/turn 不覆盖 tsm 写入(R3 实证)。

### 7.5 失败与刷新(均非致命不崩)

- **成功**:重查库刷新 `all_rows`;光标按 session id 停回原行;行尾短暂"已重命名"提示。
- **rowcount==0**(id 已被别处删):提示"会话已不存在,可能已在别处删除",刷新列表、丢弃该行。
- **超时后仍 `SQLITE_BUSY`**(罕见):提示"库忙,请重试",**保留已输入 buffer 停在编辑态**,再按 `Enter` 重试,无需重打。

### 7.6 批量改名:排除

改名语义天然单条(一 title 对一 id)。即便有多选(多选服务于删除/归档),`r` **恒定只作用于光标行**、忽略多选集。v1 不提供批量改名。

---

## 8. 项目判定("当前项目"语义)

> 来源:T-proj(`/grilling`,已沉淀进 `CONTEXT.md` 的 "Project" 定义)。**据此更正 R1 硬约束 #4:不再 canonicalize。**

1. **匹配规则 = 逐字节精确相等**。会话属于当前项目 ⟺ `threads.cwd` 与 tsm 启动目录**逐字节相等**。**不做**前缀/子树匹配、仓库根归一化、git worktree 归并 —— 子目录、repo 根、不同 worktree 一律算不同项目。理由:实现最简、零 git 依赖、对已删除的会话目录最稳(纯字符串比)。看得窄由 `p`(全部项目)兜底。
2. **启动目录锚点 = 进程 CWD `std::env::current_dir()`**(= getcwd,与 traex 建会话时**相同的算法**)。不"向上找 `.git`"(那等于偷偷退化成仓库根方案)。
3. **项目外启动 = 空列表 + 提示,不自动回退**。当前项目筛下来为空时,显示空列表并提示"本项目无会话,按 `p` 看全部"(文案见 §4.6)。理由:诚实 —— 用户始终清楚自己看的是"本项目"还是"全部"。
4. **cwd 归一化 = 不做任何归一,直接精确比**。关键事实:traex 存的 cwd 就是 `current_dir()`/getcwd 输出的**物理路径**(符号链接已由内核解析),traex 不读逻辑 `$PWD`、不额外 canonicalize。tsm 也用 `current_dir()`,故符号链接天然一致、大小写用磁盘规范、尾斜杠/`//` 从不产生、会话目录删了照样匹配。
   - **已知可接受漏网**:用 `traex --cd <符号链接路径>` 启动的会话可能存了未解析路径 → 按设计落到"全部项目",v1 不特殊处理。

---

## 9. 打包(crate / bin / 别名 / 依赖 / 配置)

> 来源:T-pkg(`/grilling` + `/domain-modeling`)。

### 9.1 crate 与模块

- 单 binary crate。包名 `traex-session-manager`,**bin 名 `tsm`**(`[[bin]] name = "tsm"`),`version = "0.1.0"`,`edition = "2024"`。
- **不拆 lib、不上 workspace**(过度工程)。
- 模块按关注点:`store`(只读 sqlite,§2)/ `mutate`(shell 调 traex,§3)/ `rename`(写 title,§2.6/§7)/ `ui`(ratatui,§5)/ `app`(状态机,§5.5)。

### 9.2 依赖草案(std-only,不引 tokio)

- `ratatui` + `crossterm` —— TUI。
- `rusqlite { features = ["bundled"] }` —— **自带 SQLite 静态编译**,保证 WAL / `busy_timeout` / `query_only` pragma 行为可复现、无系统 libsqlite 依赖(也是 musl 静态发布的前提)。
- `unicode-width` —— 宽度感知截断。
- `anyhow`(可选)—— 错误处理人体工学。
- **禁止**:`serde`(只读 `threads` 列,不碰 jsonl)/ `tokio` / 任何 HTTP crate。
- 具体版本号留到写 `Cargo.toml` 时钉。

### 9.3 配置模型(零配置)

**v1 无配置文件,全靠 flag + env**:只认 `--db <path>` 覆盖 + §2.1 的 env 链;不带任何参数即可对默认 traex home 直接跑。并发池大小(4)v1 硬编码,不做可配。

### 9.4 安装位置与别名机制

- 装到 **`~/.local/bin`**(已在 PATH、traex 就住这儿做 companion、XDG 惯例、不改 shell profile)。
- 真实二进制 = `tsm`;安装脚本建软链 **`traex-session-manager -> tsm`**(与 traex 自己的 `traecli`/`trae-cli -> traex` 软链模式一致,release 里只需塞一个二进制)。这就是"双名触发":`tsm` 与 `traex-session-manager` 指向同一 bin。
- `cargo install --path .` 作为**源码安装备选**写进 README。

---

## 10. 分发与发布(§ 新增,T-pkg Q8/Q9)

> **Q9 = plan-only**:本 effort 只把分发/CI/自更新写成 spec 可实现条款,**不动手写** `.github/workflows/release.yml` 与 `install.sh` —— 真正落地留到照 spec 实现的阶段。
>
> **单点常量**:`OWNER/REPO = Plasticine-Yang/traex-session-manager`。spec、`install.sh`、`self-update` 全部单点引用,不散落。

### 10.1 目标平台矩阵(四 target)

| target | 备注 |
|---|---|
| `aarch64-apple-darwin` | macOS Apple Silicon |
| `x86_64-apple-darwin` | macOS Intel |
| `x86_64-unknown-linux-musl` | Linux x86_64,**musl 静态** |
| `aarch64-unknown-linux-musl` | Linux arm64,**musl 静态** |

- 各有原生 CI runner,无需交叉编译。
- **Linux 一律 musl 静态**(避 glibc 版本地狱,curl 下来到处能跑)。`rusqlite{bundled}` 编 C 源码,musl 构建需 CI 里装 `musl-tools`。
- macOS **v1 不签名 / 不公证**:curl 下载的 CLI 无 quarantine 属性,终端跑未签名 CLI 不被 Gatekeeper 硬拦。

### 10.2 Release 触发与产物约定

- 推 SemVer tag **`vX.Y.Z`** 触发;CI **校验 tag 与 `Cargo.toml` version 一致**。
- 矩阵构建 4 target → 每 target 产 **`tsm-<version>-<target>.tar.gz`**(含 `tsm` 二进制 + LICENSE/README)+ 一份汇总 **`SHA256SUMS`**。
- 建 GitHub Release,上传全部产物(4 个 tarball + `SHA256SUMS`)。

### 10.3 `install.sh`(curl 一键装)

- 放**仓库根**,raw URL 即 curl 一键装目标:`curl -fsSL https://raw.githubusercontent.com/Plasticine-Yang/traex-session-manager/main/install.sh | sh`。
- 行为:
  1. 探测 OS/arch → 选对应 target 资产名。
  2. 从 `https://github.com/Plasticine-Yang/traex-session-manager/releases/latest/download/<asset>` 下载(GitHub latest 重定向,免 API token、无限流)。
  3. **下载后校验 SHA256**(比对 `SHA256SUMS`)再解包。
  4. 落 `~/.local/bin/tsm`,建软链 `~/.local/bin/traex-session-manager -> tsm`。
  5. 检测 `~/.local/bin` 是否在 `PATH`,不在则提示用户如何加入(不擅自改 shell profile)。

### 10.4 自更新 `tsm self-update [--check]`

- **复用安装脚本**:子命令内部执行 `curl -fsSL <install-url> | sh`,零 Rust HTTP 依赖、安装逻辑单一真源(守住 §9.2 无 HTTP crate)。
- `--check` 先比对本地 `--version` 与最新 release tag,再决定是否动手。
- **幂等**:已是最新版时 `self-update` 应报告"already up to date"并**不重装/不降级**;重复跑落到相同版本、相同软链,不产生副作用。
- 放弃 `self_update` crate(会拉 reqwest/ureq + tar,违背 §9.2)。macOS/Linux 恒有 curl/sh,不构成新前提。

### 10.5 `release.yml` workflow 大纲(实现阶段照此写)

1. `on: push: tags: ['v*.*.*']`。
2. job `verify`:checkout → 校验 tag == `Cargo.toml` version。
3. job `build`(matrix 4 target):装 toolchain(Linux 装 `musl-tools`)→ `cargo build --release --target <t>` → 打包 `tsm-<ver>-<target>.tar.gz`。
4. job `release`:汇集 4 产物 → 生成 `SHA256SUMS` → `gh release create vX.Y.Z <assets>`。

---

## 11. 错误文案与空状态 UX(库级 / 环境级)

> 解决 map 迷雾"错误文案与空状态 UX"的**库级/环境级**残留部分(过滤相关空状态见 §4.6,当前项目空行为见 §8)。

| 情形 | 时机 | 处理 |
|---|---|---|
| **解析目录下无 `state_(\d+).sqlite`**(或 `--db` 指向的文件不存在) | 启动 | **致命**:stderr 打印解析出的路径 + env 链提示(`no traex state database found at <dir> · set --db or $TRAE_HOME`),非零退出,不进 TUI |
| **库存在但列校验失败 / 未知世代** | 启动 | **致命**:`unrecognized traex database schema (state_N); tsm may be outdated`,非零退出 |
| **首次只读查询超时 `SQLITE_BUSY`**(3s 后) | 启动 | **致命**:`traex database is busy · try again`,非零退出(极罕见) |
| **运行时刷新/切换查询超时 `SQLITE_BUSY`** | 运行时 | **非致命**:页脚 toast"库忙,按 `R` 重试",保留当前 `all_rows`,不崩 |
| **改名写库超时 `SQLITE_BUSY`** | 运行时 | 见 §7.5(保留 buffer 停编辑态,提示重试) |
| **`traex` 不在 PATH** | 启动探测 | **非致命**:仍启动(读+改名不依赖 traex);页脚 dim 提示"traex not found · delete/archive/unarchive unavailable" |
| **`traex` 不在 PATH** | 变更时 spawn 失败 | 在批处理结果面(§6.6)按失败项列出该错误,不崩 |

**原则**:凡是让列表根本无法呈现的(库找不到/结构不认/首查即锁)= 启动期致命、清晰退出;运行时的瞬时错误一律非致命 toast + `R` 重试或保留编辑态。

---

## 12. 外部变更自动刷新策略

> 解决 map 迷雾"外部变更自动刷新策略"。

**v1 = 手动刷新 + mutation 后自动重查,不做主动感知**:

- **`R`** 手动重查库(§5.7)。
- 每次 tsm 自身发起的 mutation(delete/archive/unarchive/rename)后**自动全量重查**(§6.7 / §7.5)。
- **不轮询 `updated_at_ms`、不做文件监听**去感知 tsm 运行时**别处**(另一个 traex 进程)新建/删除的会话。理由:与 §9.2 std-only/无 async 一致,避免后台线程 + 竞态;R1 要求"读事务短、不长持快照",轮询会与之别扭;用户手上永远有 `R`。
- 主动感知(轮询 / `notify` 文件监听)推 **v2 迷雾**。

---

## 13. 验收清单(v1 完成标准)

> 每条应可勾选。分组:核心功能 / 数据契约 / 交互 / **分发与发布(T-pkg 扩入)** / 健壮性。

### 13.1 列表 / 过滤 / 搜索

- [ ] 启动即显示**当前项目 · 活跃**会话,按 `updated_at_ms DESC` 排序(默认落点)。
- [ ] `p` 在 Project ↔ All 间切换;All 视图追加 `cwd` 列。
- [ ] `Tab` 在 Active ↔ Archived 间切换(严格两态,无第三态)。
- [ ] `/` 进入实时子串过滤(大小写不敏感,覆盖 title + first_user_message);`Enter` 提交保留,`Esc` 清除。
- [ ] 三维 AND 叠加正确;过滤后 `*` 全选、`Space` 勾选作用在可见集上。
- [ ] 多选跨过滤/刷新按 id 稳定;隐藏的已选行静默保留(无警告)。
- [ ] 空状态按成因显示正确文案(搜索无果 / 归档空 / 本项目空)。

### 13.2 数据契约

- [ ] DB 路径按 `--db → $TRAECLI_HOME → $TRAE_HOME/cli → ~/.trae/cli` 解析,忽略 `CODEX_HOME`。
- [ ] 库文件按 `state_(\d+).sqlite` 取 max + `threads` 列校验,不硬编码 `state_5`。
- [ ] 只读连接用 `mode=ro` + `busy_timeout(3s)` + `query_only`,不用 `immutable`;读事务短。
- [ ] traex 正在写库时 tsm 只读列表无锁错误、无脏数据(WAL 并发)。

### 13.3 变更 / 批量 / 改名

- [ ] `d` 删除走确认模态,大写 `D` 确认;单删也走同一模态(非 `y`)。
- [ ] delete 一律带 `--force`;并发上限 4(`std::thread` 池 + `mpsc`,非 tokio)。
- [ ] 进度模态显示 `N/总数` + `✓/✗/⟳` 聚合;部分失败转结果面列出 stderr 行,失败项保持选中可 `d` 重试。
- [ ] `Esc` 取消 = 停派发不 kill 在飞;cancelled + failed 保持选中。
- [ ] `a` 按 Lifecycle 门控做 archive/unarchive,免确认;归档后从 active 视图消失(收尾全量重查)。
- [ ] `r` 行内改名,预填原始 title,`Enter` 存 / `Esc` 取消;只 `UPDATE threads.title`、不 bump 时间戳、不同步 sidecar。
- [ ] 改名校验:trim + 换行/Tab 折叠空格 + 拒绝空标题(停编辑态)+ 不设长度上限。
- [ ] 改名 `rowcount==0` / `SQLITE_BUSY` 均非致命,按 §7.5 提示。

### 13.4 UI

- [ ] 范式 B(单列表 + 底部预览);列宽 `<100` 自动隐藏预览,`Enter` 手动切换。
- [ ] 列按 §5.2 定义;`session` / `cwd` 列 `unicode-width` 感知截断 + `…`,不切半个 CJK。
- [ ] 完整键位表(§5.7)全部生效;`?` 显示帮助;`q`/`Ctrl-c` 退出。
- [ ] 主题为 plasticine 暗色(§5.6)。

### 13.5 分发与发布(T-pkg 扩入范围)

- [ ] **一键装成功**:`curl -fsSL <install.sh raw URL> | sh` 能在干净的 macOS 与 Linux 上把 `tsm` 落到 `~/.local/bin` 并可运行;`~/.local/bin` 不在 PATH 时有提示。
- [ ] **双名触发**:安装后 `tsm` 与 `traex-session-manager` 两个名字都能启动(后者是指向前者的软链)。
- [ ] **`self-update` 幂等到最新**:`tsm self-update` 能升级到最新 release;已是最新时报告"already up to date"、**不重装/不降级**;重复跑落到相同版本与软链、无副作用;`--check` 只比对不动手。
- [ ] **四 target 产物齐备且 SHA256 可校验**:tag `vX.Y.Z` 触发 CI,产出 `{aarch64,x86_64}-apple-darwin` + `{x86_64,aarch64}-unknown-linux-musl` 四个 `tsm-<ver>-<target>.tar.gz` + `SHA256SUMS`,挂上 GitHub Release;`install.sh` 下载后按 `SHA256SUMS` 校验通过才落地。
- [ ] CI 校验 tag 与 `Cargo.toml` version 一致。
- [ ] Linux 产物为 musl 静态(`ldd` 无动态依赖);macOS 产物未签名但 curl 下来终端可跑。

> 注:§10 是 **plan-only** 条款 —— 上述分发项验收的是"照本节实现后应满足的标准",`install.sh` / `release.yml` 的实际编写在照 spec 实现的阶段完成,不在本 effort 内。

### 13.6 健壮性(库级/环境级)

- [ ] 找不到库 / 结构不认 / 首查即锁 → 启动期清晰退出(§11)。
- [ ] 运行时瞬时 `SQLITE_BUSY` → 非致命 toast + `R` 重试;不崩。
- [ ] `traex` 不在 PATH → 仍启动(读+改名可用),页脚提示,变更时在结果面报错。
- [ ] 项目外启动 → 空列表 + "按 `p` 看全部",不自动回退。

---

## 附:验证策略说明(解决 map 迷雾"spec 的验证策略")

本 spec **采用可勾选验收清单(§13)作为 v1 完成标准**,不附带自动化测试脚本或截图占位:

- **可勾选清单**:覆盖核心功能 / 数据契约 / 交互 / 分发 / 健壮性,足以让实现者自检"做完没有"。
- **不附测试脚本**:R1/R2/R3 的一手实测已固化在 `research/` 与各 ticket,是实现期的对照事实源;单元/集成测试的具体写法留给实现阶段(测试策略不属于本决策 effort)。
- **不附截图**:交互样子的 primary source 是两个原型(`prototypes/tui-layout.html` / `prototypes/filter-model.html`),比静态截图更能传达手感。

---

## 附:v2 迷雾(有意排除出 v1,记录以免遗忘)

- 边删边浏览的**后台批处理模式**(v1 = 阻塞进度模态)。
- **多排序**(按 token / 标题;v1 固定 `updated_at_ms DESC`)。
- **按项目分组**渲染(v1 = All 视图扁平 + `cwd` 列)。
- **外部变更主动感知**(轮询 / 文件监听;v1 = 手动 `R` + mutation 后自动重查)。
- **改名同步 `session_index.jsonl`**(若将来有 traex 特性开始信任 sidecar)。
- macOS **签名 / 公证**;并发池大小可配。
