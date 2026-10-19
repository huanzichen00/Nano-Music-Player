# iOS 版本开发方案

更新日期：2026-10-17。

## 目标与技术路线

继续使用 Rust + Slint，不整体改写成 Swift。桌面版本保持现有入口与行为，iOS 直接复用桌面 iPod nano 的主页、列表、播放组件及机身素材，新增安全区域和触摸适配入口。音频引擎、元数据解析、歌词、播放队列、导航栈和翻译目录继续共享，通过少量 Objective-C 桥接接入 UIKit、AVAudioSession 和 MediaPlayer。

iOS 使用 Slint 1.18.1、Winit + Skia。最低部署版本暂定 iOS 15.0，同时支持 arm64 真机和 Apple Silicon 模拟器。构建和签名交由 Xcode，Cargo 负责 Rust 可执行文件及原生桥接库。

## 首版范围

- 从“文件”选择多个音频，复制到应用管理的 Music 目录，避免长期依赖外部文件权限。
- 读取标签、封面、时长与内嵌歌词，持久化曲库；再次启动可继续浏览。
- 触摸浏览曲库、搜索、选择歌曲；播放/暂停、前后切歌、进度跳转、随机与循环。
- 播放页显示封面、歌曲信息及同步歌词，直接复用桌面 nano 界面，适配手机安全区域。
- 配置后台音频，接入锁屏/控制中心元数据与播放命令；处理中断及耳机断开。
- 提供独立桌面预览入口，便于在没有 iOS SDK 时检查界面与共享逻辑。
- 电脑传歌：手机主动开启临时局域网上传服务，电脑浏览器批量拖入音乐、查看逐项进度与结果，上传后进入同一曲库；USB 文件共享作为补充。

按后续要求，已补充播放列表编辑、AI 配置、歌词查找/编辑/翻译/罗马音、Genius、评分与智能歌单、Gapless 和 ReplayGain。设置项目、歌词/歌单/统计服务直接复用桌面实现，移动层负责触摸入口和异步结果展示。

本轮范围不纳入 libmpv 视频、外部 ffmpeg/ffprobe、文件系统监控、桌面快捷键/窗口管理、系统 Apple Music 曲库/DRM 播放，以及主页原机外观中的照片、健身和广播功能。AirPlay 专项体验、商店发布和完整生命周期设备验收仍属后续工作。

## 代码边界

| 模块 | 位置 | 责任 |
| --- | --- | --- |
| 共享 Rust 核心 | `src/audio/`、`src/library/` | 解码播放、队列、元数据、数据库、歌词；桌面对话框及监控从移动构建中隔离 |
| 共享应用基础 | `src/shared/`、`src/platform/mod.rs` | 原桌面动作、翻译目录和导航实现通过共享模块导出，统一渲染后端选择 |
| 移动存储 | `src/mobile/storage.rs` | 应用内曲库目录、导入复制、扫描、缓存、路径持久化 |
| 移动控制器 | `src/mobile/mod.rs` | 异步扫描、UI 状态、播放队列、事件处理、歌词同步 |
| 移动高级功能适配 | `src/mobile/{annotations,playlists,genius,statistics,gapless}.rs` | 调用同一桌面服务、音频事件和数据库，提供移动编辑与持久化入口 |
| 移动导航适配 | `src/mobile/navigation.rs` | 在共享导航栈上生成移动功能菜单及歌曲作用域，保留返回时的选中行 |
| 电脑传歌 | `src/mobile/transfer.rs`、`web/transfer.html` | 手动开启的局域网接口、临时配对、流式批量上传、电脑端进度与曲库列表 |
| 共享界面 | `ui/home.slint`、`ui/library.slint`、`ui/now-playing.slint`、`ui/components.slint` | 桌面与移动共用原始主页、列表、播放页和按钮组件 |
| 移动界面外壳 | `ui/mobile.slint`、`ui/mobile/transfer.slint` | 手机尺寸、安全区域、原机身布局和新增电脑传歌页；不复制已有页面 |
| iOS 适配 | `src/platform/ios.rs`、`ios/NativeBridge.m` | 文件选择器、音频会话、系统媒体控制、中断、窗口安全区域 |
| 构建入口 | `Cargo.toml`、`build.rs`、`ios/`、`scripts/` | target/feature 隔离、Xcode 工程定义、构建与环境检查 |

`mobile` Cargo feature 用于在桌面运行移动界面预览；iOS target 自动选择同一移动入口。默认 `cargo run` 继续运行桌面版。移动入口不编译桌面控制器、输入分发或视频模块；iOS target 排除 rfd/notify/libloading 等直接桌面依赖，桌面预览保留原生文件选择器。

## 阶段与验收

### P0：构建和架构基础（iOS SDK 真机构建及启动通过）

1. 隔离桌面依赖，建立移动 UI/控制器入口。
2. 建立 XcodeGen 工程配置、Info.plist、Cargo 构建脚本与环境检查。
3. 在桌面编译移动入口，共享核心回归验证通过。

验收：默认桌面构建不受影响；移动入口可以编译；构建脚本对缺失 Xcode、target、签名设置给出明确提示。

### P1：本地音乐闭环（原型已完成，桌面验证通过）

1. 应用内音乐目录、导入和持久化曲库。
2. 曲库/搜索/播放页面及播放控制。
3. 复用现有 Rodio 解码播放及 LRC 时间轴。

验收：导入两首真实音频后可浏览、播放、切歌和跳转；取消导入不改变曲库；重名文件不会被覆盖；重启后可恢复曲库。扫描和文件复制不阻塞 UI。

电脑传歌验收：开启后显示本地连接地址，电脑浏览器以临时令牌访问曲库/上传接口；拖入多文件可查看每项进度，成功文件立即进入曲库。未经配对的请求不能读取曲库或上传；大文件流式写入，取消/断传不留下半个音乐文件，重名不覆盖。短暂切后台保留会话，系统后台额度到期后暂停接收，回到前台沿用同一地址恢复；手动关闭后重新开启生成新令牌。不增加账号、云服务、远程删除或自动开放端口。

### P2：iOS 系统集成（真机基础 UI 已验证，音频与生命周期验收待完成）

1. UIKit 文件选择与应用沙盒目录。
2. AVAudioSession playback 类别、后台音频声明。
3. Now Playing 元数据与播放/暂停/切歌/跳转命令。
4. 来电中断、拔出耳机的暂停处理与安全区域适配。

验收：模拟器启动并完成导入；真机锁屏连续播放；控制中心操作反馈正确；中断/耳机断开后不会意外继续播放。上述行为必须在对应设备上实际验证，不能以配置文件或桌面测试代替。

### P3：发布准备（后续）

- 真机性能、发热、耗电、较大曲库和 iCloud 文件导入验证。
- 启动页、无障碍、本地化复核与隐私说明；iPod 机身图标及英/中/日语言选择已完成。
- Apple Developer 签名、Archive、TestFlight、App Store 素材。
- 较大曲库与联网服务的设备验收。

## 并行开发安排

| Agent | 负责范围 | 交付 |
| --- | --- | --- |
| 共享核心与存储 agent | `src/library/` 的移动兼容、`src/mobile/storage.rs` | 无桌面对话框的扫描能力、应用内导入/缓存、关键存储测试 |
| 共享核心与存储 agent（追加） | `src/mobile/transfer.rs`、`web/transfer.html` | 电脑浏览器批量传歌工具与受配对保护的本地接口 |
| 移动界面与播放 agent | `src/mobile/mod.rs`、`ui/mobile.slint` | 可触摸操作的音乐闭环、异步工作线程、歌词和播放状态 |
| iOS 原生桥接 agent | `src/platform/ios.rs`、`ios/NativeBridge.m` | 文档选择、音频会话、Now Playing、远程命令与中断事件 |
| 主 agent | 根构建文件、iOS 工程与脚本、文档、集成验证 | 接口协调、桌面回归、移动预览检查、交付与限制记录 |

各 agent 只编辑分配文件，避免覆盖他人改动；共享接口变更先同步。主 agent 负责集成，不自动提交、商店发布或修改用户既有的未提交文件；真机签名与覆盖安装按用户授权执行。

## 当前环境与验证约束

2026-10-16 首轮开发时，工作机仅安装 Command Line Tools，没有完整 Xcode、iOS SDK、模拟器及 XcodeGen。2026-10-17 已安装 Xcode 27.1、iOS SDK、XcodeGen 和 Rust iOS targets；已成功构建签名并更新到连接的 iPhone 16 Pro Max，启动与真机中文渲染通过。

首轮完成源码、工程和脚本，以及桌面预览/共享逻辑验证。后续已按用户要求清理项目 `target/debug` 构建缓存，保留验证日志和预览。iOS 编译、签名安装及基础显示现已验证，后台音频、控制中心和真实导入等功能仍需逐项验收。

用户已有的 `docs/FEATURE_PLAN.md` 修改和 `tests/monitor_file_budget.rs` 新文件保持原样。

## 主要风险与应对

- Slint 的 iOS 渲染、键盘和生命周期需实机验证：提供移动入口，复用桌面组件并验证手机尺寸下的触摸与布局。
- CPAL/Rodio 在 iOS 上需与音频会话协同：原生桥接在加载音频前激活会话，中断和路由变化由控制器处理。
- iOS 应用容器路径可能变化：音乐由应用管理，存储层处理可迁移路径，避免依赖文件选择器临时 URL。
- 外部文档可能来自云端或无访问权限：原生选择器读取时持有临时授权，复制到本地后才进入曲库。
- 后台线程与系统回调：回调仅投递事件，Rust 控制器在 UI 线程更新状态；复制和扫描使用工作线程。
- 局域网传歌：默认关闭、手动开关、每次开启生成临时令牌；仅流式接收支持的本地音频，拒绝任意目标路径，限制请求体大小。后台运行有系统期限，到期暂停并取消当前请求，前台恢复同一会话。iOS 本地网络权限与实际后台额度需设备验证。
- 桌面回归：默认构建路径保留，新增移动 feature/target 配置单独验证。

## 官方参考

- [Slint iOS 部署](https://docs.slint.dev/latest/docs/slint/guide/platforms/mobile/ios/)
- [Rust iOS targets](https://doc.rust-lang.org/rustc/platform-support/apple-ios.html)
- [Apple：媒体播放与后台音频配置](https://developer.apple.com/documentation/avfoundation/configuring-your-app-for-media-playback)
- [Apple：文档和目录访问](https://developer.apple.com/documentation/uikit/providing-access-to-directories)

## 实施记录

- 已完成仓库和工具链调查、路线选择、首版范围及三个 agent 的并行开发与集成。
- 已实现移动入口、共享核心的条件编译、应用内存储、异步导入和扫描、搜索/播放/进度/歌词、播放会话及中断意图处理。
- 根据 UI 反馈恢复桌面 nano 设计：直接使用原 `HomeView`、`LibraryView`、`NowPlayingView`、`NanoActionButton` 和蓝色机身 SVG，删除先前重复的移动控件。共享原桌面动作、导航及翻译源文件；移动层只适配可用功能、触摸和安全区域。
- 已实现 iOS 原生文件选择、音频会话、Now Playing/远程命令、耳机路由事件、安全区域和前后台事件；原生 Objective-C 已由真实 iOS SDK 编译通过，功能性设备验收仍待完成。
- 已实现电脑传歌网页与临时局域网接口，批量进度/取消/错误结果/曲库浏览，存储锁内检查取消。后续根据真实反馈改为保留后台会话：申请有限 UIKit 后台任务，到期暂停，前台同地址恢复。服务前台开启期间临时防自动锁屏，离开前台或停止后恢复原设置。
- 已实现 USB 共享目录根部音频整理：验证后用同卷链接转入 Music，重名保留，失败原件不删除；不要求用户在 Finder 内操作子目录。
- 已添加 XcodeGen 工程、Info.plist、环境检查/构建脚本，以及只手动触发的 unsigned simulator CI。没有实际触发云构建或发布。
- 2026-10-17 修复真机中文方框：移动入口内置未裁剪的 Noto Sans CJK SC，统一共享页面的字体来源，覆盖菜单、动态歌名和歌词。字形映射检查覆盖 627 个界面及中日韩元数据样本字符；字体许可证随 iOS 包分发。真机更新启动后截图确认设置与状态文字正常，见 `iphone-cjk-fixed.png`。
- 2026-10-17 补齐共享设置及高级音乐功能：语言默认英文，可在 Settings → Language 切换；已有明确选择持久保存。手机不显示桌面快捷键、字体和窗口管理设置。
- 增加原生一键复制传歌地址与歌词，接入系统剪贴板。制作并接入 iPod 机身 AppIcon。编辑弹窗避让软键盘，保留可点击的保存/取消/Done；共享播放控件在触摸模式下扩大命中区域，桌面模式沿用原布局。
- 播放列表支持新建、改名、添加/移除/排序、保存与二次确认删除；删除仅处理歌单关联。歌词支持在线查找、粘贴普通文本/LRC、保存、复制、翻译和罗马音；AI 设置及 Genius 复用桌面服务。没有配置密钥时显示明确错误，不生成模拟结果。
- 增加实际播放时间统计、评分、智能歌单与自然 Gapless 切换；应用容器路径迁移同时处理歌单、歌词、评分/统计及翻译缓存。

### 高级功能验证（2026-10-17）

移动测试增加了键盘遮挡与触摸控件、复制按钮、歌单编辑/持久化、路径迁移、Gapless 状态和共享服务的回归。控制器闭环测试用两个真实音频文件完成创建/重排/改名歌单、保存 LRC、评分、重启恢复及删除确认，并确认音频文件未被删除。模拟器已完成 SDK 构建、安装和启动，显示英文主页并读取两个测试音频。

用户反馈后追加手机列表手势：左滑进入手指起始行的下级页面，右滑返回；普通歌曲左滑不会误播放。移动列表用同一个 TouchArea 立即处理点击/滑动，避开通用 SwipeGestureHandler 对子控件的 100ms 按下延迟。加入按下高亮、横向拖动反馈、短滑回弹和一次方向锁定；同类页面的不同专辑/歌单切换也会取消旧滚动状态。桌面仍使用原识别器和滚动物理。

后台传歌追加两项真实 TCP 回归：到期中止半上传、快速前台恢复仍取消过期请求、同 listener/URL/token 恢复，以及手动停止撤销。修正 Xcode 脚本阶段与输出声明，使打包资源完成后由 Xcode 重新签名；严格 codesign 验证通过。

最终源码验证结果：

| 验证 | 结果 |
| --- | --- |
| 移动 lib 全量 | 203 通过、1 忽略；`mobile-final-tests.log` |
| 桌面 lib/core/home_motion/library_order/settings_navigation/translation_settings | 297 通过、8 忽略；`desktop-mobile-final-tests.log` |
| 横滑与弹性滚动追加桌面回归 | back_scroll/overscroll/overscroll_ui/settings_navigation/translation_settings 共 17 通过；`desktop-gesture-final-tests.log` |
| 共享统计服务调整后回归 | 7 通过；`mobile-statistics-final-tests.log` |
| 移动 lib/bin/transfer 示例 Clippy `-D warnings` | 通过；`mobile-final-clippy.log` |
| 字形与构建脚本 Python 验证 | 12 通过；`python-final-tests.log` |
| iOS 真机 SDK 构建、签名校验 | 通过；`ios-final-build.log` |

最终包已覆盖安装到用户连接的 iPhone 16 Pro Max，`devicectl` 返回安装成功，启动命令成功，进程列表确认新的 app 容器内可执行文件正在运行。未卸载应用或清理手机数据。完整手势和后台额度的真实使用体验仍由手机实际操作验收，不能用合成输入测试替代。

统计服务原有的一个 `collapsible_if` 按 Clippy 建议合并条件，行为保持一致，移动静态检查现已通过。桌面 `state.rs` 原有同类提示不在本轮修改范围。

### 真机 Start Transfer 闪退修复（2026-10-17）

用户反馈点 Start Transfer 闪退。读取手机 `NanoMusicPlayer-2026-10-17-124946.ips`，主线程重复进入 `-[NMBridge setTransferActive:]`，最终 `EXC_BAD_ACCESS`/栈保护区访问失败。原因是 `transferActive` 属性的自动 setter 与管理系统资源的方法同名，方法内部 `self.transferActive = YES/NO` 再次调用自身。将存储属性改名 `transferEnabled`，保留原生入口方法名，消除递归。

修复后的真实 iOS SDK 构建和严格签名校验通过，已覆盖更新手机；用户再次点 Start Transfer，确认正常显示传歌地址。随后读取设备崩溃目录，没有新增本应用报告。

新增 `ios/tests/TransferLifecycleRegression.m` 与 `scripts/check-ios-transfer-lifecycle.sh`，在 iOS Simulator 进程直接执行实际桥接实现；100 轮原生启停、重复调用、关闭后禁止重启均通过。测试没有创建可见 UIApplication 窗口，故只验证入口和状态，不代表系统实际后台额度验收。传歌 Rust 定向回归 6/6 通过。日志：`native-transfer-regression.log`、`transfer-crash-fix-tests.log`、`ios-transfer-crash-fix-build.log`。

### 电脑整库资料同步（2026-10-17）

用户确认同步全部音乐资料。新增 Python 桌面快照/导出与 USB 传输工具，手机
使用共享 `src/library/desktop_sync.rs` 导入，复用原歌词、罗马音、翻译及数据库结构。
READY/export_id 最后提交，SHA256/大小/路径与符号链接预检，安全发布与 DB 事务、
回执、重试、来源映射和增量统计。发生冲突时手机资料优先；不复制桌面偏好或密钥。

移动启动改为先显示 UI，再后台构造 Storage/导入资料，完成后在主线程创建原生桥
和控制器，避免大曲库校验阻塞 UIKit 启动。Debug 单独优化 SHA256 依赖。

真实桌面资料包已离线导入验证：142 音频约 2.6 GB、1 歌单、115 条有效歌词关联
（包含桌面外置 LRC 缓存及显式选择）、10 罗马音、3 评分、116 播放历史、15 翻译。
完整移动测试 209 通过、1 忽略；移动 Clippy `-D warnings` 通过。Python 测试
40 通过，包括 USB 网页备用连接工具的 22 项测试；网页工具尚未完成真实 USB 上传验收。

真机已返回完整成功回执：142 首音乐、1 歌单、116 条历史、0 冲突；手机导入后数据库 `quick_check` 通过，142 首的原始封面完整（135,907,324 字节）。手机设置和新增资料不作整库替换。

使用、保护规则和验证命令见 [IPHONE_SYNC.md](IPHONE_SYNC.md)。日志包含
`desktop-sync-real-validation.log`、`mobile-desktop-sync-tests.log`、
`desktop-sync-clippy.log`、`desktop-sync-python-all-tests.log` 和
`ios-desktop-sync-final-build.log`。

AI 配置和异步服务状态已做离线验证；没有使用用户密钥发起真实翻译或 Genius 请求。锁屏长时间播放、电话中断、耳机路由及多设备 Wi-Fi 连通性仍需真实场景验证。开发工具 Device Hub 的屏幕镜像显示空白，模拟器截图通过 `simctl` 获取；本轮不能据此宣称已经手动验证完整软键盘流程。

### 首轮与中间验证记录

| 验证 | 结果 |
| --- | --- |
| 默认桌面 `cargo check --locked` | 通过 |
| 默认桌面 `cargo test --locked --lib` | 232 通过，8 忽略 |
| 移动 `cargo test --locked --features mobile --lib` | 139 通过，1 忽略；含共享导航、存储、真实 loopback 上传和 UI 指针/渲染验证 |
| 桌面所有 target 回归（跳过已确认基线失败项） | 377 通过，27 忽略，1 过滤 |
| UI 复用后的桌面定向回归（lib/core/home_motion/library_order） | 288 通过，8 忽略；覆盖共享导航、主页动画及曲库排序 |
| 移动 lib/bin/电脑传歌示例 Clippy `-D warnings` | 通过 |
| 桌面所有 target Clippy `-D warnings` | 未通过：两个未改动桌面文件已有 `collapsible_if` 提示，见下文 |
| 构建脚本的 mock Xcode/Cargo 测试 | 6 通过，覆盖真机/模拟器架构、空配置、路径空格、符号输出 |
| Rust 原生桥补充检查 | 2 事件解析测试及独立 Rust iOS 分支类型检查通过；不等同 SDK 构建 |
| 格式、diff 空白、shell 语法及 plist | 通过 |
| 电脑浏览器端到端 | 批量上传 2 个真实 WAV 成功；追加同名文件分别保留；无效 MP3 单项失败且不损坏已有曲库；关闭服务后页面提示连接中断 |
| 移动 UI | 软件渲染检查 320×568、390×844、844×390，包含原主页翻页、菜单/列表、中文搜索、播放/seek/音量、歌词、传歌和机身 Home 按钮；机身及纯屏幕两种模式 |
| iOS SDK 真机构建与签名安装（2026-10-17） | Xcode 27.1 Debug 构建通过，已更新到 iPhone 16 Pro Max；启动及中文界面显示通过 |
| iOS 模拟器、真机完整功能、Archive | 尚未完成；真机锁屏播放、系统命令、中断和导入仍需验证 |

完整默认 `cargo test --all-targets` 被用户此前已有的 `tests/monitor_file_budget.rs` 阻断：测试将 macOS 文件句柄上限设为 256，桌面目录监控报 `Too many open files`。已用 HEAD `8a71365` 的原始监控模块独立复现相同失败，确认不是本轮路径 helper 抽取引入；未修改该用户测试。跳过此项后的其余所有 target 回归通过。

此前桌面 Clippy 有两处未改动代码的 `collapsible_if`：`src/app/controller/statistics.rs:147`、`src/app/state.rs:1236`，完整输出见 `desktop-clippy.log`。高级功能现在通过路径引用复用统计服务，新增验证结果另见最终日志，不能沿用早期移动 Clippy 结果代表当前源码。

验证日志与截图保存在 `target/ios-preview/`（忽略的开发产物），含 `desktop-tests-excluding-known-fd.log`、`desktop-ui-reuse-tests.log`、`mobile-tests.log`、`baseline-fd.log`。还原后的 UI 截图包括 `home.png`、`music.png`、`library.png`、`player.png`、`lyrics.png`、`transfer.png`、`small-player.png` 和 `landscape.png`；电脑传歌成功截图为 `computer-transfer.jpg`。

### 后续第一步

继续用真机验收文件导入、Wi-Fi 上传、锁屏连续播放、控制中心、中断和真实软键盘操作。P2 完整设备验收及 P3 发布工作仍未完成。

### 触屏与显示验收补充（2026-10-17）

列表缩略图接入桌面worker/cache；移除轮询Track深拷贝和逐行队列全量复制；手机Debug启用opt-level=2。新增原生按需Paste、提示超时关闭、歌词手动滚动后恢复/切歌复位。96张跨尺寸截图及223项移动回归覆盖，详见 [IOS_UI_QA.md](IOS_UI_QA.md)。针对真机后台更新后字形缺损，回移Slint上游383c344修复，保留版本1.18.1及Metal GPU渲染。当前USB断开，最终锁屏/解锁字形验收和更新仍需手机连接。
