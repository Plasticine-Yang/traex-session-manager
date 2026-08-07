# T-proj — "当前项目"判定与项目外启动行为

Type: grilling
Status: resolved
Blocked by: 01

## Question

"列出当前项目下的会话"这句话的精确语义,直接决定默认过滤:

1. **匹配规则**:`threads.cwd` 与 tsm 启动目录如何算"同一项目"?
   - 精确相等?
   - 前缀匹配(启动目录是某会话 cwd 的祖先,或反之)?
   - Git worktree / 仓库根归一化(同一 repo 的不同 worktree 算不算一个项目)?
2. **启动目录取哪个**:进程 CWD?还是向上找最近的 `.git` 作为项目根?你在 monorepo 子目录里启动时期望看到什么?
3. **项目外启动**:在一个没有任何会话的目录启动 tsm,默认空列表 + 提示"按 X 看全部",还是直接回退到全部?
4. **cwd 归一化**:路径尾斜杠、符号链接、大小写(macOS 大小写不敏感文件系统)如何归一以避免漏匹配。

## 交付

`## Answer` 记录:选定的匹配规则(精确/前缀/repo 根)、启动目录解析、项目外回退行为、cwd 归一化规则。用 `/grilling` + `/domain-modeling` 推进(会沉淀 CONTEXT.md 里 "Project" 的精确定义)。

## Answer

**1. 匹配规则 = 精确相等(方案 A)。** 会话属于当前项目 ⟺ `threads.cwd` 与 tsm 启动目录**逐字节相等**。不做前缀/子树匹配,不做仓库根归一化,不做 git worktree 归并 —— 子目录、repo 根、不同 worktree 一律算不同项目。理由:实现最简、零 git 依赖、对已删除的会话目录最稳(纯字符串比,目录没了照样匹配);"看得窄"的代价由旁边一键"全部项目"(T-filter)兜底。前缀(B)/repo 根(C)明确否掉。

**2. 启动目录锚点 = 进程 CWD(`std::env::current_dir()`)。** 是方案 A 的直接推论:要和 traex 存的值逐字节相等,tsm 必须用与 traex **相同的算法**算自身目录。traex 建会话时 cwd = `std::env::current_dir()`(= getcwd)。用"向上找 `.git`"当锚点等于偷偷退化成方案 C,已否。

**3. 项目外启动 = 空列表 + 提示(方案 a)。** 当前项目筛下来为空时,显示空列表并提示"本项目无会话,按 X 看全部",**不**自动回退到全部项目。理由:诚实——用户始终清楚自己看的是"本项目"还是"全部",不会把全机会话误当成当前项目。空状态的具体文案吊在 T-layout(键位/文案)。

**4. cwd 归一化 = 不做任何归一,直接精确比。** 关键事实(本 ticket 查证,并据此**更正 R1**):traex 存的 cwd 就是 `current_dir()`/getcwd 输出的**物理路径**(符号链接已由内核解析),traex **不读**逻辑 `$PWD`、**不额外** `canonicalize`(仅词法规范化)。tsm 也用 `current_dir()` 取自身目录,故:
   - **符号链接**:两端 getcwd 都已解析 → 天然一致,无需 canonicalize;
   - **大小写**(macOS APFS 大小写不敏感但保留):getcwd 返回磁盘规范大小写,不随 `cd` 敲法变 → 无需小写化;
   - **尾斜杠 / `//`**:getcwd 从不产生 → 非问题;
   - **会话目录已删**:纯字符串比,存的值还在 → 照样匹配(方案 A 的红利)。
   - **已知可接受漏网**:用 `traex --cd <符号链接路径>` 启动的会话可能存了未解析路径,与 tsm 物理 CWD 对不上 —— 按设计落到"全部项目",v1 不特殊处理。

**证据**:本机 10 个 distinct `threads.cwd` 全为干净物理绝对路径(无 `/tmp`、无尾斜杠、无 `//`、无 `/var/folders`);getcwd 实测 `/tmp`→`/private/tmp`;traex 二进制内嵌串证实 cwd 源自 `std::env::current_dir()`、无 `$PWD`、`AbsolutePathBuf` 文档明确"不保证 canonicalize"。

**领域规则一句话**:tsm 用与 traex 完全相同的方式(`current_dir()`/getcwd)算自己的项目目录,然后与 `threads.cwd` **原样逐字节比较**。已沉淀进 `CONTEXT.md` 的 "Project" 定义。
