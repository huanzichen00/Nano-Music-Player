# 性能与职责优化（2026-10-28）

基于 `8b8dd70`，在 `refactor/desktop-controller` 上渐进实施。
用户已有的 `docs/FEATURE_PLAN.md` 修改独立保留，不纳入提交。

## 已完成的独立步骤

| 本地提交 | 内容 |
| --- | --- |
| `aef3304` | 修复 Settings 测试缺少 cover_wallpaper，并验证该设置持久化 |
| `d834274` | 普通 PR/main 增加桌面测试、Clippy、mobile/iOS 编译工作流 |
| `ea06a66` | 音乐导入通道断开后报告一次错误并释放 busy |
| `1dd8ad4` | 合并曲库使用临时路径索引；选中项计数不再构建排序 rows |
| `e802076` | 歌单任务断开保留编辑草稿，解除 saving，允许重试 |
| `3c9ca9d` | 串行构造 Carbon 热键测试事件，修复并行测试原生崩溃 |
| `85a49cc` | 未变化的可信缓存不再写库，原始歌词与手工关联分离 |
| `f37344a` | 翻译缓存通过 FIFO worker 保存，确认落盘后发布快照 |
| `1e52c6e` | 视频/字幕任务断开恢复及导入 picker 所有权保护 |
| `a6fb452` | Genius 歌单后台保存，关闭预览后仍完成明确保存意图 |
| `466599d` | 缓存 revision 独立驱动 UI 更新，旧完成事件保留新显示/取消意图 |
| `9f28ea7` | 音乐导入结束不会释放后来打开的原生 picker |
| `28175aa` | 两个前端统一依赖 services 和 shared/thumbnails |
| `4ae7d25` | 旧歌词缓存迁移保留手机自选封面和显示元数据 |

所有提交仅保存在本地，未推送。

## 性能与一致性边界

- 大批合并用本次调用的路径索引，已有 vector slot、播放队列 ID 和当前歌曲不变。
  单曲更新仍用一次线性查询，避免逐曲刷新时反复分配全库索引。
- `State` 字段仍公开；不引入可能被直接字段修改绕过的长期投影缓存。
  计数与 rows 使用同一过滤逻辑，排序 key 每行计算一次。
- scanner 只缓存原始来源歌词，手工导入、移除和在线歌词仍通过独立关联覆盖。
  scanner 专用 JSON provenance 标记与原 Track 反序列化兼容，无 SQL schema 迁移。
  未变化的可信曲目在完整刷新中零写入，由 SQLite 审计 trigger 回归验证。
- 旧的无标记缓存可能含有历史手工歌词覆盖值；来源可读时重建一次，之后跳过重写。
  来源暂时不可读时保留旧快照，待恢复后修复。普通 Database::save 不冒充可信来源。
  iPhone 同步补封面使用比较原记录的条件更新，并保留原 provenance。
- 没有把整个扫描包进长事务；慢文件、外置盘读取不持有整轮数据库写锁。
- 翻译 JSON 的快照复制、序列化、同步与原子替换在单个 FIFO worker 执行。
  失败保留已确认缓存；显式移除排在既有保存之后；退出排空已接受的本地写入。
  存储 revision 与状态提示分离，跨歌曲的静默完成也能刷新当前歌词。
- Genius 捕获保存时的预览顺序并在后台写入；保存成功才发布歌单。
  旧预览的完成事件不会改变新预览提示，关闭窗口仍等待明确保存意图完成。
- 曲库、视频、字幕、歌单、翻译和 Genius 的断开路径区分 Empty/Disconnected。
  视频 worker 下次操作可重建；原生 picker 仅由打开它的任务释放模态门禁。

## 共享模块

`src/services/{lyrics,genius,playlists,statistics}.rs` 保存不依赖 UI 的请求和存储生命周期。
`src/shared/thumbnails.rs` 保存共同缩略图实现，解码使用共同 artwork 模块。
桌面与移动端均正常导入这些模块，不再通过 path 引用桌面 Controller 目录编译服务。
UI model、导航、播放会话和媒体交接仍由各前端协调，不引入另一套全局状态。

## 合成性能采样

Debug，10,000/20,000 首合成歌曲、无音频输出；共享编译负载影响采样。
两次采样范围如下，未测改前数据，不用于宣称提速倍数：

| 曲库大小 | 合并 | 100 次选中移动 | 构建排序投影 |
| --- | --- | --- | --- |
| 10,000 | 7.10–17.76 ms | 11.35–25.71 ms | 2.55–4.16 ms |
| 20,000 | 13.85–21.62 ms | 22.66–37.81 ms | 5.15–9.00 ms |

可重复执行：

```sh
cargo test --test library_state_performance profile_large_library_merge_and_navigation -- --ignored --nocapture
```

## 最终验证与 App 交付

桌面全量命令在 `28175aa` 启动，595 通过、0 失败、31 忽略。
随后移动端全量发现旧缓存迁移可能覆盖手机自选封面；`4ae7d25` 修复后，
scanner/storage 9 项和移动端全量 264 项通过（0 失败、1 忽略）。
最后补跑受缓存修改影响的桌面 core/曲库/歌词/导入/文件句柄回归，结果见下表。
忽略项不计为已执行，未将前一次移动端失败记为通过。

| 检查 | 结果 |
| --- | --- |
| `cargo fmt --all -- --check`、`git diff --check` | 通过 |
| `cargo test --locked --no-fail-fast` | 595 通过、0 失败、31 忽略（上述快照） |
| 修复后 scanner/storage 单元回归 | 9 通过 |
| 修复后桌面缓存相关集成回归 | 76 通过、0 失败、1 忽略 |
| `cargo test --locked --lib --features mobile --no-fail-fast` | 264 通过、0 失败、1 忽略 |
| `cargo clippy --locked --all-targets` | 通过（含最终兼容性修复） |
| `cargo check --locked --lib --features mobile` | 通过 |
| `cargo check --locked --lib --target aarch64-apple-ios-sim` | 通过（含最终兼容性修复） |
| Carbon 热键测试并行压力 | 3 项测试连续运行 20 次，全部通过 |
| Checks workflow YAML | 本地解析通过；尚未推送触发 GitHub 托管作业 |

Clippy 保留两处已有 fetch_update 弃用警告及 practice 的两处 collapsible_if 警告。
本机曾在全量测试中触发旧 Carbon 事件测试的并行初始化 SIGABRT，已由
`3c9ca9d` 串行保护，修复后全量及压力验证通过。

执行 `scripts/package-macos.sh`，Release 构建成功（1 分 24 秒），生产源码为
`4ae7d25`。对构建 App 重新应用与已有安装相同的 ad-hoc 签名后，先保留旧 App，
再以暂存副本和目录 rename 更新安装，未停止或重新启动现有 PID 60552。

| 产物 | 实际路径 / 校验 |
| --- | --- |
| 构建 App | `/Users/huanzichen/Documents/ChatGPT/rust-music-player/target/Nano Music Player.app` |
| 安装 App | `/Users/huanzichen/Applications/Nano Music Player.app` |
| 可恢复旧 App | `/Users/huanzichen/Applications/.Nano Music Player Backups/20261028-162355/Nano Music Player.app` |
| Bundle ID / 架构 / 签名 | `org.nano.music-player` / arm64 / ad-hoc |
| 新主程序 SHA-256 | `96470385550a74fc26fbbcec8fe4c9d0c035a018f8e753c0a0ba0a229d6127cd` |
| 旧主程序 SHA-256 | `9aff13bf108b7aa7d0eaaa3e4fcb19fce7049fb6e94961e261f59b0c8db64c98` |

构建和安装副本的主程序 SHA-256 完全一致；Info.plist 解析、可执行文件架构、
`codesign --verify --deep --strict` 和旧备份哈希均已检查。
安装产物、个人配置和备份均不加入 Git。重新启动 App 才会加载新版。

真实音频连续性、物理媒体键、外置盘热插拔、iOS 真机、Windows/Linux 原生运行、
真实付费 provider 与 GitHub 托管 CI 未由本机 headless/编译验证替代。
设置和视频进度等其余同步持久化路径仍在原有流程，未声称全部 UI I/O 已异步化。
