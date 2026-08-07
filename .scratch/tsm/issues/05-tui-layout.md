# T-layout — TUI 布局、列与交互范式

Type: prototype
Status: resolved
Blocked by: 01

## Question

定 tsm 主界面的骨架,并用原型让用户看到样子:

1. **交互范式**:单列表 + 底部/侧边详情面板(主从预览),还是纯单列表?你要看的信息(标题、时间、cwd、是否归档、token)能否一行放下,还是需要预览区展开 `first_user_message`?
2. **列**:默认(当前项目)显示哪些列 —— 选择框、标题、更新时间、[归档标记]、token?切到"全部项目"时追加 CWD 列。列宽/截断策略。
3. **多选呈现**:多选打勾(空格键)如何视觉呈现;顶部是否显示"已选 N 项"。
4. **状态栏/页脚**:显示当前 scope/lifecycle/搜索状态 + 快捷键提示。
5. **ratatui 具体用法**:用 `ratatui::widgets::Table` + `TableState`?列表滚动、选中高亮、多选态如何建模(`Vec<bool>` 还是 `HashSet<id>`)。事件循环用 crossterm。
6. **主题**:是否沿用 plasticine 的中性暗色 + cyan/purple 高亮(见迷雾)。

## 交付

`## Answer` 记录:选定的范式、列定义(当前项目 vs 全部)、多选与状态栏设计、ratatui 组件与状态结构草案。用 `/prototype` 出一个界面草样(ASCII mock 或极简 ratatui stub 皆可),链接到 ticket 供用户拍板。这里顺带把 **T-filter/T-batch/T-rename 的键位**汇总成一张初版键位表,清掉迷雾里的"键位表"项。

## Answer

**原型(primary source)**:`.scratch/tsm/prototypes/tui-layout.html`(单文件双击即开)。用本项目 `state_5.sqlite` 真实数据渲染(本项目 7 个会话、全机 36 个),含 4 个结构上不同的布局(A 密集/B 底部预览/C 右侧预览/D 按项目分组)与 5 个叠加态(search/batch-confirm/rename/archived/help)。用户拍板前实测:标题两极分化——要么极短(`wayfinder: T-proj`)要么是超长 prompt dump;8 字符 id 前缀存在真实碰撞(`019fdcb5`×2、`019fdba1`×3),故任何 id 展示都要保留区分性、比较必须用完整 UUID。

### 1. 交互范式 —— 选定 **B:单列表 + 底部横向预览面板**(用户 2026-08-07 拍板)

- 主从预览。上方是会话 `Table`,下方 `preview` 面板显示当前光标行的**完整** `title` / `id`(截断中段但两端可辨)/ `git_branch` / `model` / `tokens` / `cwd` / `first_user_message` 首段。
- **窄屏降级**:终端列宽 `< 100` 时自动隐藏预览面板 = 退化为 A(纯密集列表),`Enter` 手动切换预览显隐。理由:标题信息量两极分化,长标题/首消息一行放不下,需要预览区展开;横向底部切分比 C 的竖向切分更抗窄屏、且不受 CJK 双宽字符把竖分隔线搞得参差不齐之苦。
- **D(按项目分组)不作默认**,只作为"全部项目"视图的可选渲染(默认单项目视图里分组只有一个组、纯噪声)。是否真的启用分组留到 spec 合成时定,不阻塞。

### 2. 列定义与截断

- **默认(当前项目)**:`✓(选择框) · updated · session(title) · model · tokens`。
  - `updated` = `MM-DD HH:MM`(取 `updated_at` 秒;排序键用 `updated_at_ms DESC`,见 R1)。
  - `session` = `COALESCE(NULLIF(title,''), NULLIF(first_user_message,''), '(untitled)')`(R1),弹性列,尾部省略号截断。
  - `tokens` = 人类可读 `442k` / `4.3M`。
- **全部项目(scope=all)**:在 `updated` 与 `session` 之间**追加 `cwd` 列**(相对 `~` 显示,中段截断);此视图信息密度高,可省略 `tokens` 列。
- **归档标记**:不占独立列。lifecycle=all 时在行尾用一个 `⌂`/dim 标记归档行;lifecycle=archived 视图里 `tokens` 位置换成 `archived_at`。
- 截断策略:固定列(✓/updated/model/tokens)定宽;`session`(及 all 视图的 `cwd`)吃剩余宽度,`unicode-width` 感知截断 + `…`(避免 CJK 半个字符)。

### 3. 多选呈现

- 勾选态:行首 `▣`(已选,cyan/高亮)/ `▢`(未选,dim);被选行叠加一条高亮底色带。
- **状态模型 = `HashSet<SessionId>`**(**不是** `Vec<bool>`)。关键理由:列表会因 search/scope/lifecycle 过滤而重排,索引位选择会串位;按会话 id 存选择,跨过滤/刷新稳定。
- 顶部/页脚显示 `N selected`;`Space` 切换当前行,`*` 反选当前可见集。

### 4. 状态栏 / 页脚

- 顶部标题栏右侧显示当前 `scope · lifecycle`(如 `project · active` / `all · archived`)。
- 底部页脚(B 布局里与预览面板边框合并)显示 `N selected` + 上下文相关快捷键提示。
- 搜索态:顶部输入行 `/term▏` + `命中/总数`;批删态:模态框;改名态:行内输入框 + 底部 `[Enter]save [Esc]cancel`。

### 5. ratatui 用法与状态结构草案

- 组件:`ratatui::widgets::Table` + `TableState`(光标/滚动);预览面板 `Paragraph`+`Wrap`;模态用居中 `Block` 浮层。事件循环 crossterm(raw mode + alternate screen)。
- 草案:

```rust
struct App {
    all_rows: Vec<Session>,      // 上次查库结果(权威快照)
    view: Vec<usize>,           // 过滤+排序后指向 all_rows 的索引(渲染用)
    table: TableState,          // 光标/滚动
    selected: HashSet<SessionId>,// 跨过滤稳定的多选
    scope: Scope,               // Project(cwd) | All
    lifecycle: Lifecycle,       // Active | Archived | All
    search: Option<String>,     // Some => 过滤中
    show_preview: bool,         // 宽度<100 或用户 Enter 切换 => false
    mode: Mode,                 // Normal | Search | Rename{buf} | ConfirmDelete{ids} | Help
}
```

- 每次 mutation(删/归档/改名)后重查库刷新 `all_rows`(R1:短只读事务、每次刷新独立查询,不长持快照)。`selected` 按 id 过滤掉已消失的行。

### 6. 主题

沿用 plasticine 中性暗色 + cyan/purple 高亮(清掉迷雾里的"主题/配色"项):bg `#16161e`、fg `#c0caf5`、dim `#565f89`、cyan `#7dcfff`(高亮/光标/选中)、purple `#bb9af7`(标题栏/键位)、green/red/yellow 用于成功/失败/警告。

### 初版键位表(汇总 T-filter / T-batch / T-rename,清掉迷雾里的"键位表")

> 这是**草案**,`d`/`a`/`r`/`/` 的最终确认分别交由 T-batch(确认强度/并发)、T-rename(编辑交互)、T-filter(搜索语义)敲定;本表把它们放进同一张不冲突的映射里。

| 键 | 动作 |
|---|---|
| `j`/`↓` `k`/`↑` | 上下移动光标 |
| `g` / `G` | 跳到顶部 / 底部 |
| `Space` | 切换当前行选中 |
| `*` | 反选当前可见集 |
| `d` | 删除:有选中则删全部选中,否则删光标行 → 确认模态(T-batch) |
| `a` | 归档(active 视图)/ 取消归档(archived 视图),按 lifecycle 门控(R2);可逆,免模态 |
| `r` | 行内改名 → 写 `threads.title`(R3;`Enter` 保存 / `Esc` 取消)(T-rename) |
| `/` | 搜索:对 `title`+`first_user_message` 子串匹配、大小写不敏感;`Esc` 清除(T-filter) |
| `p` | 切换 scope:当前项目 ↔ 全部项目(+cwd 列) |
| `Tab` | 切换 lifecycle:active → archived → all |
| `Enter` | 切换预览面板显隐(B/C 布局) |
| `R` | 手动刷新(重查库;mutation 后也自动刷新) |
| `?` | 帮助 / 键位表 |
| `q` / `Ctrl-c` | 退出 |

**批删确认键**:模态里用大写 **`D`**(非 `Enter`,防误触),模态列出将删标题 + 数量 + "不可逆、同时删除 rollout 文件"警告。归档/取消归档可逆,免确认。(最终由 T-batch 定稿。)
