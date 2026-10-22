# 桌面艺人图片与持久化任务队列

桌面端在导入专辑成功、启动曲库缓存发布和源文件刷新完成后，将当前可用歌曲的艺人按名字去重，加入后台图片队列。它也会补查旧曲库的图片，不需要重新导入。空白、Unknown Artist、Various Artists 不入队；大小写、Unicode NFC 和多余空白归一化，重音、标点和完整合作署名保留。

**Statistics → Artist Images** 显示查找中、排队中、待重试、已就绪的任务，未完成任务靠前，重试入口沿用现有列表选择和返回操作。页面支持 English／简体中文／日本語，以及 Device Mode／Screen Mode。失败详情尽可能本地化；未知技术错误显示通用提示，原始错误保存在队列文件中。

## 图片查找

1. 使用 MusicBrainz 搜索艺人名与别名，只接受精确归一化匹配的候选。
2. 唯一候选直接继续。存在同名候选时，只有配置 DeepSeek 且有专辑线索才请求辅助选择；模型只能返回本次真实候选中的 MBID。随后查询 MusicBrainz 专辑，核对精确专辑名及该 MBID 的署名。不能验证时保留失败任务，不猜测图片链接。
3. 经艺人的 Wikidata 关系获取 P18 图片，存在 P434 时反向验证 MusicBrainz 身份。再从 Wikimedia Commons 获取真实图片和作者、许可、来源页面。
4. 仅缓存通过 HTTPS 来源、响应大小、图片解码尺寸及许可检查的 PNG／JPEG。保留 CC BY、CC BY-SA、CC0 或 Public Domain 的有效署名和相符许可链接。请求 Commons 缩略图，缓存最大 640 像素边长的 PNG，不放大小图。

公开来源不需要 API 密钥。同名辅助复用 **设置 → AI 服务** 的 DeepSeek 配置及环境变量优先级；按需读取配置，使用量计入账户。艺人名与专辑名会发送到公开元数据服务，并在辅助识别时发送给 DeepSeek；不上传音频、歌词或本地文件路径。模型不负责生成照片，也不能提供任意下载 URL。

Statistics 的 Top Artists、Overview 艺人预览及艺人详情优先使用照片，找不到时继续使用现有封面／占位图。专辑、歌曲和普通曲库浏览保留原封面。艺人详情上方的图片署名可点击打开 Commons 文件页，查看完整作者与许可信息。封面异步加载不会覆盖已取得的艺人照。

接口依据：[MusicBrainz API](https://musicbrainz.org/doc/MusicBrainz_API)、[请求限流](https://musicbrainz.org/doc/MusicBrainz_API/Rate_Limiting)、[MediaWiki imageinfo](https://www.mediawiki.org/wiki/API:Imageinfo)、[Commons 复用说明](https://commons.wikimedia.org/wiki/Commons:Reusing_content_outside_Wikimedia)。

## 存储与恢复

所有持久文件位于应用配置目录下的 `artist-images-v1/`：

- `queue.json`：版本、艺人键、显示名、专辑线索、状态、尝试次数、下次重试时间、失败原因、图片文件名及署名。
- `<sha256>.png`：按艺人键生成的固定缓存名，避免元数据成为文件路径。
- `queue.corrupt-*.json`：无效队列的原件备份，保留后才重建队列。

macOS 默认配置目录是 `~/Library/Application Support/Nano.Nano-Music-Player`；`--config-dir` 使用指定目录。队列不存 API 密钥，也不存音频文件路径。

Controller 通过 Rust channel 发送入队、重试、读取缓存指令。单个独立 worker 负责网络、磁盘和解码；Slint 回调仅发命令，UI 轮询结果。内存照片缓存同时限制为 48 项和 32 MiB。MusicBrainz 请求有进程级间隔控制，公开 HTTP 请求有连接及总超时。

每次任务状态变更写入临时文件，`sync_all` 后原子替换 `queue.json`。先保存 `Running` 再联网；存储失败时暂停新任务，间隔重试保存。存储恢复和容量恢复分别跟踪，确认恢复后才清除 UI 错误。磁盘故障时，尚未保存的新任务仅在内存中；异常退出前未落盘的数据不能保证保留，下次启动会从曲库重新补入。

任务状态为 `Pending → Running → Ready`；失败进入 `Failed`，从完成时刻计算 60 秒起的指数退避，最长 24 小时。手动重试把失败任务放回 `Pending`。重启会将中断的 `Running` 变回 `Pending`；仍在退避期的失败任务保留原计划。缓存丢失或损坏时重新排队。应用关闭时不会等待后台 HTTP 完成；退出后没有常驻服务继续下载，下次启动再继续。

队列文件限制为 8 MiB，并为未完成任务的结果预留空间；超量新增或扩展会被拒绝，已有任务继续处理。每位艺人至多保留 100 个专辑线索；Controller 每次按确定顺序提供至多 8 个。已就绪记录保留以避免重复查找；当前没有缓存淘汰的磁盘清理、任务删除或独立死信队列。已删除艺人的旧队列记录不会自动清除。

## 与常规消息队列的对比

| 概念 | 当前实现 | RabbitMQ 任务队列等系统 |
| --- | --- | --- |
| 生产者 | 曲库导入／启动发布后的 Controller | 发布消息的应用 |
| 持久存储 | 本机 JSON 任务账本 | 独立 Broker 存储 |
| 消费者 | App 内一个 worker | 可有多个进程／机器消费 |
| 成功确认 | 保存图片并持久化 `Ready` | 消费者 ACK |
| 中断恢复 | 重启将 `Running` 重新排队 | 未确认消息重新投递 |
| 去重 | 以规范化艺人名合并任务与专辑线索 | 通常需要应用幂等键及业务处理 |
| 顺序 | 确定的艺人键扫描，到期任务可执行；非严格 FIFO | 依产品、配置及并发方式而定 |
| 可用性 | 依赖当前 App 和本机磁盘 | 可配置独立服务、集群等能力 |

Rust channel 本身不持久化。入队指令发送成功，不等于已经写入磁盘；当前没有 Publisher Confirm 协议。RabbitMQ 的 Publisher Confirm 确认发布被 Broker 接收，Consumer ACK 确认消费处理完成，两者独立，见[官方确认机制说明](https://www.rabbitmq.com/docs/confirms)。

下载完成但 `Ready` 未落盘时崩溃，重启后可能重复请求。稳定任务键、固定缓存名和成功记录减少重复结果；这是可重试、尽量幂等的本机任务处理，不是严格 exactly-once，也不适合多个 App 进程共用一个配置目录并发写队列。它适合本机低吞吐、可重建的图片查找任务。

## 代码与验证

- `src/library/artist_images.rs`：队列状态机、原子保存、退避、缓存及 worker。
- `src/library/artist_images/provider.rs`：公开来源、DeepSeek 候选验证、下载与许可检查。
- `src/app/controller/artist_images.rs`：导入入队、结果事件、可见图片缓存和来源链接。
- `src/app/statistics_state.rs`、`ui/statistics.slint`：队列投影、图片署名、原列表交互。
- 后台离线 fixtures 覆盖去重、同名核验、Unicode、缓存、损坏恢复、容量、退避与恢复提示；Controller 测试覆盖照片不被封面覆盖、相同专辑名不误用照片、缓存内存边界及错误不影响播放。
- `tests/artist_images_ui.rs` 覆盖队列排序、重试不改变播放、三语言双模式渲染、页脚像素及署名真实点击；截图写入 `target/qa/artist-images/`，不纳入 Git。
- Blackfield 公共链路测试需要显式运行忽略测试，验证真实下载、署名、PNG 缓存及重启只读缓存。开发验证没有向 DeepSeek 发送付费请求。

查图不能保证每位艺人都有公开图片；元数据缺失、同名身份无法确认、没有符合许可的图片或网络故障时，会保留任务并继续封面兜底。此功能不修改 ListeningSession、播放次数规则、音频引擎、Gapless、ReplayGain 或播放队列；7／30 天播放统计仍受最近 1,000 条历史记录限制，与图片队列无关。

本次验证（2026-10-22）：`cargo fmt --all -- --check`、`cargo check`、`cargo clippy --all-targets` 均通过；`cargo test --no-fail-fast` 为 519 通过、0 失败、28 忽略。单独执行 Blackfield 真实公共链路和原生播放／Gapless／统计回归，各 1 项通过。`cargo check --features mobile` 与 `cargo check --lib --target aarch64-apple-ios-sim` 通过。保留既有 Genius／translation 的两处 `fetch_update` 弃用警告及 iOS NativeBridge 编译警告；其余忽略的原生／开发测试未执行，Windows／Linux 构建及真实付费 DeepSeek 请求未验证。
