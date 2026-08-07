# tsm — traex 会话管理器

`tsm`(全名 `traex-session-manager`)是一个 Rust 终端 UI,用来列出、搜索、删除、归档和改名 traex CLI 的会话,免去"resume 进选择器 → 切会话 → 敲斜杠命令"的繁琐流程。

## Language

**Session(会话)**:
traex 中一次对话,由 UUID 标识,在磁盘上对应一个 rollout `.jsonl` 文件,在 `state_5.sqlite` 的 `threads` 表中对应一行。
_Avoid_: conversation、thread(thread 是 traex 内部表名,用户语汇里统一说"会话/session")

**Project(项目)**:
会话创建时的工作目录 `cwd`,记录为 getcwd 物理路径(符号链接已解析)。"当前项目会话" = 会话 `cwd` 与 tsm 启动目录**精确相等**(两端同为 getcwd 结果);子目录、仓库根、git worktree 一律不归一,均视为不同项目。判定细节见 ticket T-proj。
_Avoid_: workspace、repo、directory

**Rollout 文件**:
会话在磁盘上的记录 `~/.trae/cli/sessions/年/月/日/rollout-<时间>-<uuid>.jsonl`;第一行 `session_meta` 记录 `cwd`/`id`/`timestamp`/`git`。
_Avoid_: log、transcript、history file

**Store(数据源)**:
traex 的 `~/.trae/cli/state_5.sqlite`,其 `threads` 表是会话列表的权威来源。tsm 只读它,唯一例外是改名时写 `title`。
_Avoid_: database、db、index(避免与 `session_index.jsonl` 混淆)

**Lifecycle(生命周期)**:
会话的活跃/归档状态,由 `threads.archived`(0/1)表示。tsm 的列表按**两态**过滤:"活跃"(`archived=0`,默认)↔ "归档"(`archived=1`),用 `Tab` 切换。_非目标_:不提供"全部/活跃+归档混合"视图(T-filter 定,不做)。
_Avoid_: status、state(与 `state_5.sqlite` 撞词)

**Scope(范围)**:
列表的项目范围,**两态开关**(`p` 切换):"当前项目"(默认,`threads.cwd` 与启动目录精确相等)↔ "全部项目"(追加 CWD 列)。
_Avoid_: filter(filter 泛指所有过滤维度,scope 专指项目范围这一维)

**Search(搜索)**:
对会话 `title` + `first_user_message` 做的**大小写不敏感子串**匹配(非模糊匹配)。是一个**模式**:`/` 进入实时过滤,`Enter` 提交并保留过滤,`Esc` 清除。与 Scope、Lifecycle 三者 AND 叠加。
_Avoid_: fuzzy、query、grep
