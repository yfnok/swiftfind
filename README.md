# SwiftFind

基于 **Everything** 的 Windows 快速启动器，Rust 编写：会学习你的选择、能搜 Windows 设置、支持自定义命令。

## 构建

需要 Windows + [Rust](https://rustup.rs)（stable，MSVC 工具链）。所有依赖都是纯 Rust（剪贴板数据库用 redb），没有任何需要额外编译的 C 库。**完全不想在本机装东西**也可以：把项目推到 GitHub，Actions 会在云端构建并产出 exe，下载即用。

```
cargo build --release
```

产物：`target\release\swiftfind.exe`（单文件，无需 DLL）。也可以把项目推到 GitHub，用 `.github/workflows/build.yml` 自动构建。

首次运行建议先 `cargo test`，纯逻辑部分（排序、学习、匹配、剪贴板、IPC 解析、热键解析）有 68 个单元测试。

## 使用

- 默认热键 **Alt+Space** 呼出/隐藏（可自定义，见下），托盘图标左键同样切换；`--background` 参数启动时不显示窗口。
- 直接输入：文件（Everything）、开始菜单/商店应用、Windows 设置、自定义命令、计算器（`2*(3+4)`）、网页搜索（`g rust`、`bd 关键词`）。
- Everything 语法原样透传：`ext:pdf 报告`、`size:>100mb`、`*.log`、`path:D:\work foo`。

| 按键 | 作用 |
|---|---|
| ↑ ↓ / Ctrl+J K | 选择 |
| Enter | 打开 |
| Ctrl+Enter | 以管理员身份运行 |
| Shift+Enter | 打开所在文件夹 |
| Ctrl+1…9 | 直接启动第 N 项 |
| Ctrl+Shift+C | 复制路径 |
| Ctrl+T | 在所选文件/文件夹所在目录打开终端（优先 Windows Terminal） |
| Ctrl+D | 不再显示此项（加入排除） |
| Ctrl+Delete | 忘记该项的学习记录（剪贴板项：删除该条） |
| Ctrl+, | 设置 |
| Esc | 清空输入 / 隐藏 |

右键结果也有同样的菜单。

## 自定义快捷键

设置（Ctrl+,）里点击快捷键按钮，直接**按下新的组合键**即可录制，Esc 取消；需要 Win 键时勾选“含 Win 键”。录制期间全局热键会暂时释放，所以可以直接按当前的组合键来替换它。如果组合键被其他程序占用，会立刻在设置里提示。两个独立快捷键：

- **呼出快捷键**：默认 `Alt+Space`，呼出/隐藏启动器。
- **剪贴板快捷键**：默认 `Ctrl+Alt+V`，直接打开剪贴板历史；可关闭。

也可以直接改 `config.json` 里的 `hotkey` / `clipboard_hotkey`（如 `Ctrl+Alt+F`、`Win+Shift+F12`）。

## 剪贴板历史

记录在程序目录下 `data\clipboard.redb`（纯 Rust 嵌入式数据库 [redb](https://github.com/cberner/redb)，ACID、崩溃安全，不需要安装任何东西），重启后仍在。启动时自动压缩，删除的内容会真正释放磁盘空间。

- **文本和图片**都记录。图片存成 PNG（保留透明度），列表里显示缩略图；复制截图、网页图片、文件管理器里的图片都可以。文本和图片同时存在时（如 Excel）优先记文本。
- **打开**：按剪贴板快捷键（默认 `Ctrl+Alt+V`），或在主窗口输入 `cb`（也可 `clip`）。
- **搜索**：`cb 关键词` 搜内容；也能搜**来源程序**（`cb chrome`、`cb wechat`）；多个词同时满足；中文、大小写都支持。特殊词：`img`（只看图片）、`text`（只看文本）、`pin`（只看置顶），可以组合，如 `cb chrome img`。
- **粘贴**：Enter 直接粘贴到打开启动器之前的窗口（图片同样）；Shift+Enter 仅复制；**Ctrl+P 置顶/取消置顶**（置顶项排最前、永不被自动清理）；Ctrl+Delete 删除这一条。右键菜单也有这些操作。
- **去重**：重复复制同一内容只会把它移到最前，不会产生重复记录。
- **自动清理**（设置里可调）：最多 2000 条、保留 90 天、图片总量 300 MB，超出时先删最旧的未置顶项。
- **忽略的程序**：可以填写不想记录的程序（如密码管理器 `keepass.exe`），另外程序主动声明“不要记录”的内容也会被跳过。
- 只记录纯文本与图片；富文本格式、文件列表（在资源管理器里复制文件）不记录。

## 自学习排序

每次启动一个结果，记录 **(项目, 当时输入的文字)**，排序分 = 文本匹配分 + 学习加分：

- 使用频率（对数增长，半衰期 45 天，不会降到 0）
- 同样输入 / 输入前缀的关联加分（选过一次就足以越过普通匹配，多选几次越稳）

- 被你**跳过**的结果（排在你选中项上面的前 3 项）对同样的输入会轻微下沉；之后再选中它会清除这个惩罚。
- 选过的文件/文件夹会记下完整路径，之后即使 Everything 返回的前 N 条里没有它，也会作为候选出现（已删除的自动隐藏）。

输入为空时，显示最常用的项目。数据在 `data\learn.json`。

## Everything

- 用 WM_COPYDATA IPC 直接和 Everything 通信，不需要 SDK DLL；同时尝试 1.4 与 1.5 的窗口类名。
- 路径：设置里填写，或留空自动检测（Program Files、LocalAppData、PATH）；未运行时自动用 `-startup` 启动。
- 查询在后台线程执行，旧请求自动丢弃，只处理最新输入。
- 排除目录会以 `!"目录\"` 的形式一并交给 Everything 过滤，不再占用结果名额。

## Windows 设置

内置 190+ 条目：Win11 全部设置页（ms-settings）、环境变量、防火墙（简易/高级规则/允许应用）、设备管理器、磁盘管理、服务、任务计划、事件查看器、组策略、注册表、网络适配器、电源、声音、`shell:` 常用文件夹（启动、AppData、下载…）等。支持中文、英文、拼音首字母（如 `hjbl` → 环境变量）。

## 配置与数据（便携）

所有文件都在**程序同目录的 `data` 文件夹**：`config.json`、`learn.json`、`clipboard.redb`、`apps_cache.json`。整个文件夹拷走就是完整迁移。若程序放在不可写的位置（如 Program Files），自动回退到 `%APPDATA%\SwiftFind`；也可用环境变量 `SWIFTFIND_DATA` 指定目录。

## 稳定性

- **日志**：`data\swiftfind.log`（超过 256 KB 自动轮转，只保留一份 `.old`）。程序出错、窗口创建失败（如远程桌面/虚拟机缺少 OpenGL）、热键注册失败都会记录；发生内部错误时会弹出提示框。**遇到问题请把这个文件发给我。**
- **后台线程隔离**：剪贴板监听、Everything 查询、热键线程里出现的异常只会被记录并恢复，不会让整个程序退出（所以发布版用 unwind 而非 abort）。
- **数据不丢**：配置、学习记录、应用缓存都是“先写临时文件再改名”，断电不会写坏。文件格式损坏时（比如手改 `config.json` 少了个逗号）不会被默认值悄悄覆盖，而是改名为 `config.json.bad` 保留，你可以修好后改回来。
- **异常内容防护**：超大/畸形图片在分配内存前就被拒绝，超长文本只读取上限内的部分；读取剪贴板时即使出错也一定会关闭剪贴板，不会卡住其他程序的复制粘贴。

## 配置：`data\config.json`

```json
{
  "everything_path": "",
  "hotkey": "Alt+Space",
  "excluded_dirs": ["C:\\$Recycle.Bin"],
  "commands": [
    { "name": "cmd", "title": "命令提示符 (管理员)", "program": "cmd.exe", "args": "", "admin": true },
    { "name": "ping", "title": "Ping 主机", "program": "cmd.exe", "args": "/k ping {q}" }
  ],
  "web_searches": [
    { "key": "g", "name": "Google", "url": "https://www.google.com/search?q={q}" }
  ]
}
```

- `name` 是触发词；`{q}` 会替换为触发词之后输入的内容（`ping 8.8.8.8`）。
- `admin: true` 会弹出 UAC 并以管理员运行。
- `confirm: true` 的命令（默认的 shutdown / restart / logoff / kill / emptybin）需要按两次 Enter 才执行。
- 改完后在设置面板点“重新加载配置”。
- 已有的旧 `config.json` 不会自动获得新的默认命令确认；给想保护的命令手动加上 `"confirm": true` 即可。

## 性能与内存

- 本地数据（设置/应用/命令）全部常驻内存，每次按键同步匹配，无分配热路径。
- 隐藏后 1.5 秒收缩工作集（EmptyWorkingSet 效果），后台常驻时内存很小。
- 中文字体用内存映射加载系统微软雅黑，不把 ~20 MB 字体读进堆。
- Release 开启 LTO、`opt-level=3`、`panic=abort`、strip。

## 目录结构

```
src/engine.rs     结果合并与排序（本地源 + Everything + 学习加分）
src/learn.rs      自学习（频率 / 查询关联 / 跳过降权 / 文件路径记忆）
src/clipboard.rs  剪贴板监听线程（文本 / 图片 / 来源程序）
src/clipdb.rs     剪贴板历史的 redb 存储（搜索、置顶、自动清理）
src/clipimg.rs    图片转换：DIB / PNG / 缩略图
src/log.rs        日志与 panic 钩子
src/matcher.rs    模糊匹配（前缀/词首/拼音首字母/子序列）
src/everything.rs Everything IPC（报文构造与解析可跨平台测试）
src/settings_db.rs Windows 设置索引表
src/apps.rs       开始菜单 + 商店应用索引（带磁盘缓存）
src/platform.rs   热键、托盘、启动、窗口显隐、自启动、单实例
src/winffi.rs     手写 Win32 绑定（无大依赖）
src/ui.rs         界面
```
