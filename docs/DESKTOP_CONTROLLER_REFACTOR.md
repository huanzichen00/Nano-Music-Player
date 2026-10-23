# 桌面 Controller 渐进式重构（2026-10-23）

## 基线与范围

从 `f715684` 开始；执行 `git fetch origin` 后确认与 `origin/main` 一致。
分支为 `refactor/desktop-controller`。用户已有的 `docs/FEATURE_PLAN.md`
未提交修改独立保留。本记录续接此前的 `CONTROLLER_REFACTOR.md`，不改写历史结果。

重构前 `src/app/controller.rs` 为 **5,149 行 / 211,214 字节**（含测试），
`state.rs` 为 **1,868 行 / 75,605 字节**。目标是明确协调职责，文件大小仅作记录。

审计覆盖 Controller、State、全部现有 Controller 子模块、Statistics State/投影、
Navigation、Artist Images worker/缓存，以及相关 Slint 页面和回归测试。
由主 Agent 集成，A 审查 UI/窗口，B 审查播放/媒体，C 只读审查状态和生命周期。
共享文件 `controller.rs` 串行修改；所有 Git 提交由主 Agent 完成。

## 职责与边界审计

| 职责 | 当前边界与本次决策 |
| --- | --- |
| UI 初始化、回调与 Action/Effect 分发 | `new`、`connect`、输入分发与 Effect 路由留主文件；回调捕获的 Rc/Weak 与注册生命周期不变 |
| UI 状态同步、页面切换、列表/选中/状态栏 | 集中于 `sync` 和缩略图/快捷键/详情投影，直接迁至 `ui_sync.rs` |
| 窗口尺寸、方向、设置持久化 | 迁至 `window.rs`；原 DPI、菜单高度、resize guard/debounce 和保存顺序保持 |
| 音频命令、会话、有效播放、Gapless/ReplayGain | 现有协调方法与 PendingMedia 迁至 `playback.rs`；音频引擎和资格规则不改 |
| 音视频事件、系统媒体命令、状态快照 | 事件消费/分发迁至 `media_events.rs`；原 `native_status.rs` 保留设备状态与命令幂等处理 |
| 曲库加载、导入、刷新结果与可用性 | `library.rs` 已合理拥有 worker/channel；结果应用迁至 `library_ui.rs`，不增加另一套曲库服务 |
| 缩略图与艺术家图片 | 缩略图仍由既有 loader/cache 执行；可见区域投影随 UI 同步移动；`artist_images.rs` 和后台下载 worker 保留 |
| 歌词、翻译、罗马音 | `lyrics.rs` 已拥有请求、缓存、取消与来源验证；主文件保留现有协调，不改变调度 |
| Genius 与播放列表 | 已有独立服务拥有请求/草稿/保存生命周期；本轮保留 UI 命令和队列协调原处 |
| 播放统计 | `statistics.rs`、`statistics_state.rs`、`statistics_projection.rs` 已分离查询/状态/格式化；仅已有 Slint 投影随 UI 方法迁移 |
| 视频任务、字幕、帧泵 | `video.rs` 已拥有加载/导入/picker；引擎事件单独迁移，其他协调及 16 ms 帧泵不改 |
| 后台任务轮询、退出 | 主文件保留明确的 50 ms 调度顺序及 Drop，worker 状态和字段所有权不迁出 Controller |

### 必须保留的交叉依赖

- `sync` 会触发统计查询、翻译状态回写、方向 resize、可见缩略图请求；它不是纯函数。
  整块迁移，保留 dirty 标记、model 身份、outgoing 页面快照与选中/滚动更新顺序。
- 音乐、播客和视频共享音频引擎与交接屏障。旧 handoff 不得消费新 pending 状态；
  ack 时读取最新暂停、音量、Seek、播客速度；播客删除的 release barrier 仍先处理。
- Gapless 的 Transitioned 经引擎 generation/epoch 过滤后是权威事件，不能新增
  Controller token 过滤；重复队列项优先按 successor cursor 处理。
- ListeningSession 使用 worker 的 `observed_at`；Pause/Seek 立即调整会话基线，
  仅自然结束/过渡走 complete。播放资格、去重和持久化失效规则全部保留。
- 曲库合并保持 vector slot/队列 ID；刷新结果保护期间完成的手工歌词编辑/导入，
  缺失歌曲处理同时影响 availability、播放会话、prefetch 和 handoff，必须整块迁移。
- startup 每轮最多一个事件，import 可排空；video events 在 audio events 前，
  statistics 在 library refresh 前。不同 channel 的关闭语义已有测试，不能统一处理。
- `mobile/mod.rs` 通过 path 复用 Lyrics/Genius/Playlists/Statistics 服务，新的桌面
  `impl Controller` 不放进这些共享源文件。Controller 字段、构造、Drop 顺序不改。

### 风险从低到高的实施顺序

1. 补充 headless UI 行为基线：重复同步保留 model、导航正确捕获 outgoing 页面。
2. 整块提取 UI 同步/投影，保留刷新频率；验证 UI、歌词、统计、图片与导航回归。
3. 提取窗口协调/设置，验证 DPI/resize、显示模式、设置与页面切换。
4. 提取播放协调和 PendingMedia；验证现有音频/队列/会话/播客/增益测试。
5. 提取音视频事件分发，补 stale event、handoff、prefetch、会话持久化回归测试。
6. 提取曲库结果应用，验证刷新/缺失/恢复/歌词并发与 startup backpressure。
7. 复审耦合；仅有明确等价证据才进一步抽取状态语义，不制造字段 getter/setter。
8. 完整 fmt/check/test/clippy、mobile/iOS 编译与 Release App 交付。

每一职责独立检查 diff、编译/测试并本地提交。行为/时序/缓存优化不混入迁移。

## 保留的架构问题与验证限制

- Controller 仍共享 State 和 UI 投影缓存。多个 `impl Controller` 明确代码职责，
  不代表引入独立运行时状态所有者；本次不拆锁、不增加 trait/框架/事件总线。
- 已有设置、视频进度、翻译缓存保存及 Genius 播放列表保存涉及同步
  磁盘访问；`cached_translation` 的 provider version 也读取 DeepSeek 配置。
  改异步调度会改变错误/完成语义，留作独立后续工作，本次不扩大阻塞路径。
- Statistics 保留 revision/date/offset 缓存键、request 过滤、失败抑制和 FIFO shutdown。
  Artist Images 保留单 worker、bounded event channel、请求去重与既有 detached 生命周期，
  退出不 join 网络请求。图片错误不能覆盖播放状态/notice。
- 部分旧 one-shot/import channel 断开后保留 busy 状态是现有行为；不在纯重构中修复。
- 真实输出听感、Gapless 连续性、ReplayGain 音量/削波、实体媒体键、耳机热插拔、
  原生窗口与真实视频解码/设备关闭须另行人工验证。软件渲染/事件测试不替代硬件验证。
- Windows/Linux 原生构建和 iOS 真机运行不能由本机 macOS 编译检查推定通过。

## 实际验证记录

日志位于 `/tmp/nano-controller-refactor-20261023/`。
忽略测试不计入已执行测试。

### 重构前基线（生产源码 `f715684`）

| 命令 | 结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo check` | 通过 |
| `cargo test --no-fail-fast` | 527 通过，0 失败，29 忽略 |
| `cargo clippy --all-targets` | 通过 |

保留 Genius/translation 两处已有 `fetch_update` 弃用警告，未改无关代码。
本次不运行需要原生音频输出的忽略测试，以免与正在运行的用户播放会话混用系统输出。

### UI 同步阶段

迁移前新增的 `ui_sync_tests` 两项通过：进度同步保留 Slint model 身份/页面动画代次，
导航先冻结源页面 rows/selection/scroll，再同步目的页面。
`cargo fmt --all -- --check`、`cargo check` 通过。

`ui_sync.rs` 接收 `refresh_shortcuts`、`text`、`toggle_track_details`、
`sync_statistics_view`、`sync`、`refresh_thumbnails`、`refresh_video_thumbnails`、
`sync_editor` 和局部 `format_time`；没有新增查询或刷新规则。
主文件为 4,233 行（含新增测试注册）。逐函数 token 比对和独立审查确认实现等价，
主文件其余 97 个方法体逐字保持原状。

验证：`cargo test --lib app::controller::` 为 78 通过、1 忽略；
`cargo test --test screen_motion --test home_motion --test modal_navigation
--test statistics_details --test listening_statistics --test artist_images_ui
--test ratings_ui --test reference_library_ui --test translation_settings`
为 23 通过、0 失败。fmt 与 `git diff --check` 通过。

### 窗口协调阶段

`window.rs` 原样接收 `initialize`、`prepare_portrait_dialog`、`base_size`、`resize`、
`maintain_aspect_ratio`、`save_settings`。6 个方法逐 token 等价；主文件为 4,045 行。
Controller 字段、公开方法签名、native resize Rc/guard、保存顺序不变。

`cargo check`、`cargo fmt --all -- --check` 与 `git diff --check` 通过。
`cargo test --test window_resize --test settings_navigation --test video_ui
--test desktop_fill_alignment --test video_list_metrics`：10 通过、0 失败、1 忽略。
忽略项需 libmpv/ffmpeg/默认音频输出，没有启动用户的运行窗口或真实媒体。

### 播放协调阶段

`playback.rs` 接收 PendingMedia 和 12 个原有方法：`media_error`、`send_music`、
`gain_config`、`begin_playback_session`、`update_playback_session`、
`complete_playback_session`、`record_qualified_play`、`schedule_gapless_prefetch`、
`cancel_gapless_prefetch`、`send_video`、`finish_video_handoff`、`finish_music_handoff`。
除跨模块可见性外，方法体与枚举保持原文；主文件为 3,695 行。
字段所有权、构造、Drop、poll、原音频引擎和播放资格完全未改。

`cargo check`、fmt、`git diff --check` 通过。
`cargo test --lib`：350 通过、0 失败、9 忽略；
`cargo test --test core --test list_shuffle --test podcasts --test native_status_ui`：
61 通过、0 失败。

### 媒体事件阶段

`media_events.rs` 接收音频事件 drain/单事件分发、视频引擎事件/进度、系统媒体命令
轮询和媒体快照发布。仅为测试提取私有 `handle_audio_event`，原两处 `continue`
变为退出当前事件的 `return`，外层 drain 不变。逆向原位展开后 `poll`、`poll_videos`
逐 token 等价，独立审查通过。主文件为 3,503 行。

新增 5 条 headless 回归覆盖旧 owner/id/pending handoff、暂停意图、播客优先接管、
旧/错 owner ack、重复队列项与权威 late token、prefetch id/token、observed_at 计数、
Pause/Seek 和统计 shutdown 单次持久化。首次测试 fixture 漏设播客的默认暂停状态，
修正 fixture 后 5 项通过，生产行为未改。

`cargo check`、fmt、`git diff --check` 通过；
`cargo test --lib app::controller::`：83 通过、0 失败、1 忽略。
`cargo test --test video --test video_ui --test native_status_ui`：12 通过、0 失败、1 忽略。

### 曲库结果应用阶段

`library_ui.rs` 接收 `update_smart_playlist_library`、`start_library_refresh`、
`poll_library_refresh`、`apply_library_refresh`、`mark_library_missing`，并将原 poll 中
启动缓存/逐曲刷新应用、导入 drain 整块提为两个方法。5 个原方法的函数体不变，
helper 展开后原 poll 顺序保持；主文件为 3,277 行 / 130,188 字节。
现有 `library.rs` 的 receiver/ready/busy/断开处理与后台扫描服务完全未改。
当前实现仍是启动和显式的有限刷新，不恢复历史上的自动文件监控。

`cargo check`、fmt、`git diff --check` 通过；
`cargo test --lib app::controller::`：83 通过、0 失败、1 忽略。
`cargo test --test library_refresh_ui --test startup_cache --test library_order
--test feature_library --test lyrics_associations --test playlist_editing`：18 通过、0 失败。

## 最终结构与耦合复审

| 文件 | 重构前 | 重构后职责 / 行数 |
| --- | --- | --- |
| `src/app/controller.rs` | 5,149 行 / 211,214 字节 | 3,277 行 / 130,188 字节；构造/字段、输入与 Action/Effect 路由、明确轮询顺序、原有功能协调、回调绑定、Drop 及既有测试 |
| `controller/ui_sync.rs` | 在主文件 | 状态/列表/统计/歌词投影、导航快照、可见缩略图、快捷键/详情同步；932 行 |
| `controller/window.rs` | 在主文件 | 初始化、方向、DPI/尺寸保护、设置保存；196 行 |
| `controller/playback.rs` | 在主文件 | 音视频命令、ListeningSession、Gain/Gapless、handoff；359 行 |
| `controller/media_events.rs` | 在主文件 | 音视频引擎事件/进度、系统命令轮询与快照；220 行 |
| `controller/library_ui.rs` | 在主文件 | 加载/导入/刷新结果应用、可用性和既有缓存失效协调；246 行 |
| `controller/{statistics,lyrics,genius,playlists,podcasts,video,library,artist_images,native_status}.rs` | 已有模块 | 全部保持原文件和职责，不加入桌面实现至移动端共享服务 |
| `src/app/state.rs` | 1,868 行 | 未修改；继续复用 merge/availability/selection 等已有语义方法 |

主文件减少 **1,872 行（36.4%）/ 81,026 字节（38.4%）**，包含的原测试未为减行而搬走。
新增两条 UI 回归位于 `ui_sync_tests.rs`，五条媒体/会话回归位于 `media_events_tests.rs`。

独立审查确认主文件原 85 个方法全部保留（含 7 个 test-only 适配方法，
即 78 个生产方法），其中 14 个公开方法没有增减，未复制业务实现。
新增仅 `handle_audio_event`、`poll_audio_events`、`poll_video_events`、
`poll_library_loading`、`poll_library_imports` 五个局部辅助方法。
新模块是同一个 Controller 的私有 impl 分组，State/worker 仍由它统一持有，
没有引入服务间循环依赖、全局可变状态、锁、trait、泛型框架或事件总线。
`connect`、Controller struct/new/Drop 保持等价；所有迁移函数体通过原文/token 比较，
两个带调度提取的方法通过原位展开验证。

本轮没有调整刷新频率、全量 model 更新条件、请求去重或缓存规则。
新增 UI 测试验证既有 model 复用；现有曲库/统计/图片测试继续约束缓存失效和过期结果。
剩余跨媒体状态切换依赖会话、队列和 reducer 的共同语义，未强加字段 setter 或再次拆 owner；
同步 I/O、断开恢复和异步调度优化均留作独立任务。

最终差异确认音频引擎、数据库、State、Statistics State/投影、移动实现、共享服务、
Slint UI、Cargo manifest/lock 均未修改。用户 `docs/FEATURE_PLAN.md` 工作内容的
SHA-256 仍为 `9672422bb7abfc372232ab61400f7cc474b3d8bec4f2e40a40722984816df9fd`，
未暂存至本次提交。

## 最终完整验证

生产源码提交为 `a70c680`，以下命令全部实际执行且退出码为 0：

| 命令 | 结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo check` | 通过 |
| `cargo test --no-fail-fast` | **534 通过、0 失败、29 忽略**；基线为 527/0/29，新增 7 项回归 |
| `cargo clippy --all-targets` | 通过；仅原 Genius/translation 两处弃用警告 |
| `cargo check --features mobile` | 通过 |
| `cargo check --lib --target aarch64-apple-ios-sim` | 通过；保留未修改的 NativeBridge 弃用/unused 参数警告 |
| `git diff --check` | 通过 |

检查了本轮生成的统计概览、彩色比例 fixture 艺人详情软件截图，未发现布局异常。
图片比例、页脚/图表像素、导航动画、窗口逻辑尺寸由现有自动断言进一步验证。

| 重点行为 | 已执行的自动验证 | 本轮未执行的真实环境验证 |
| --- | --- | --- |
| 播放/暂停/Seek/切歌、队列、随机/重复 | core、engine、list_shuffle、Controller owner/paused guard | 真实音频输出听感与快速操作 |
| Gapless、ReplayGain、播放资格 | 既有引擎/source/gain/ListeningSession 单元；重复队列 slot、late token、Pause/Seek、单次计数持久化新回归 | 设备/格式组合中的连续性、音量/削波 |
| 统计计数、排行/刷新、评分 | StatisticsService 缓存/请求/rollback/shutdown；listening_statistics/statistics_details/ratings_ui | 完整真人听歌会话与原生统计页面操作 |
| 艺人图片、缩略图缓存 | 离线 worker/provider/恢复/容量/缓存；晚到封面不盖照片；artist_images_ui/artist_photo_layout | 真实 HTTP 查图、生产缓存下载与恢复 |
| 歌词、翻译、罗马音 | Controller/service、ai_lyrics、translation_settings、romaji、来源/过期/存储回归 | 付费/真实网络 provider 和真人播放联动 |
| 曲库、缺失/恢复、重命名 | library_refresh_ui/startup_cache/library_order/歌词并发；既有数据库 rename/history 回归 | 用户真实曲库的文件改名、外置盘热插拔 |
| 视频/音乐/播客切换 | 媒体 owner/handoff、video/video_ui/podcasts/subtitle generation 回归 | libmpv/ffmpeg 真解码、音视频输出切换 |
| 播放列表、Genius | draft/edit/save/frozen snapshot/queue/offline provider 回归 | 真实 Genius provider 请求 |
| 导航、窗口、系统媒体、退出 | 软件 UI、DPI 测试适配器、media command 幂等、设备状态投影、FIFO 写入/关闭 | 原生窗口拖动、物理媒体键、耳机热插拔、真实引擎/设备退出 |

29 项忽略测试保持原设置，未计为已执行。Windows/Linux 原生构建、iOS 模拟器运行、
真机和 App Store/签名发布未执行；iOS 本轮结果限于库编译兼容性。

## Release App 交付

- `scripts/package-macos.sh`：通过，Release/optimized，arm64；构建 App 位于
  `/Users/huanzichen/Documents/ChatGPT/rust-music-player/target/Nano Music Player.app`。
- 安装副本：`/Users/huanzichen/Applications/Nano Music Player.app`。
- 可恢复旧版：
  `/Users/huanzichen/Applications/.nano-controller-refactor-20261023-171109/previous.app`。
- 保留 Bundle ID `org.nano.music-player`、版本 `0.1.1`、原 Info.plist 内容和 ad-hoc 签名方式。
  对构建/安装副本执行 plist 检查；安装副本及备份通过 `codesign --verify --deep --strict`；
  安装副本为可执行的 Mach-O arm64。
- 构建 App 与安装副本主程序 SHA-256 完全一致：
  `3d8685508549f3943f13950461cbaaf0365984e8135db7a134f058e30669ccc2`。
- 旧版 SHA-256 保持
  `b74c488be1f261e8337e4e8e309b61b74eb40c3847988d9a8d359dee67ed5bbc`。
- 原进程 PID `13741` 仍运行；`lsof` 确认其仍映射备份中的旧可执行文件，没有截断运行文件、
  退出或重启。重新启动后才加载新版。没有打开另一个使用生产配置的 App 实例。
- 未执行新版原生窗口/真实音频运行验证；人工项目见上表。安装信息与完整校验输出保存在
  `/tmp/nano-controller-refactor-20261023/app-delivery.json`，不纳入 Git。

## Git 记录

仅做本地提交，未推送、未合并至 main。各职责提交前均检查 diff 并执行相应验证。

| 提交 | 内容 |
| --- | --- |
| `e624dfe` | 记录职责审计、风险和分阶段方案 |
| `130ba08` | 两条 UI 同步/导航行为基线测试及基线结果 |
| `72d5c26` | 提取 UI 同步 |
| `2a94559` | 提取窗口尺寸、DPI 和设置协调 |
| `c02b416` | 提取播放协调和 PendingMedia |
| `e91e9e8` | 提取媒体事件并增加五条生命周期/状态回归 |
| `a70c680` | 提取曲库结果应用 |

最终验证及 App 交付记录另以 docs 提交收尾。构建产物、备份、日志和个人配置不提交。
工作区仅保留用户原有的 `docs/FEATURE_PLAN.md` 修改。
