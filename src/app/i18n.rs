use serde::{Deserialize, Serialize};

/// Interface language. English is the default for existing and older settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    English,
    SimplifiedChinese,
    Japanese,
}

impl Language {
    pub const ALL: [Self; 3] = [Self::English, Self::SimplifiedChinese, Self::Japanese];

    pub const fn label(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::SimplifiedChinese => "简体中文",
            Self::Japanese => "日本語",
        }
    }

    pub const fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::SimplifiedChinese => "zh-CN",
            Self::Japanese => "ja",
        }
    }

    pub const fn index(self) -> usize {
        match self {
            Self::English => 0,
            Self::SimplifiedChinese => 1,
            Self::Japanese => 2,
        }
    }
}

// Keep English source strings as stable catalog keys so call sites can pass the
// existing UI text directly. Runtime metadata and user-provided text stay untouched.
const CATALOG: &[(&str, &str, &str)] = &[
    ("Add Subscription…", "添加订阅…", "購読を追加…"),
    ("{0} Episodes", "{0} 集", "{0} エピソード"),
    ("Continue Listening", "继续收听", "続きから聴く"),
    ("Latest Episodes", "最新单集", "最新エピソード"),
    ("Downloaded Episodes", "已下载", "ダウンロード済み"),
    ("Episode Details", "单集详情", "エピソード詳細"),
    ("Download and Play", "下载并播放", "ダウンロードして再生"),
    ("Replay Episode", "重新播放", "もう一度再生"),
    ("Play Episode", "播放单集", "エピソードを再生"),
    ("Cancel Download", "取消下载", "ダウンロードを中止"),
    ("Delete Download", "删除下载", "ダウンロードを削除"),
    ("Download Episode", "下载单集", "エピソードをダウンロード"),
    ("Mark as Unplayed", "标记未听", "未再生にする"),
    ("Mark as Played", "标记已听", "再生済みにする"),
    ("Play from Beginning", "从头播放", "最初から再生"),
    ("Refresh Episodes", "刷新节目", "エピソードを更新"),
    ("Unsubscribe", "取消订阅", "購読を解除"),
    ("Played", "已听", "再生済み"),
    ("Downloaded", "已下载", "ダウンロード済み"),
    ("Not Downloaded", "未下载", "未ダウンロード"),
    (
        "Paste the show's RSS feed URL",
        "粘贴节目的 RSS 订阅链接",
        "番組の RSS フィード URL を貼り付け",
    ),
    ("Subscribe", "订阅", "購読"),
    ("Preview Feed", "预览节目", "番組をプレビュー"),
    ("Loading RSS feed…", "正在获取 RSS…", "RSS を取得中…"),
    (
        "Refreshing episodes…",
        "正在刷新节目…",
        "エピソードを更新中…",
    ),
    (
        "Episodes refreshed",
        "节目已刷新",
        "エピソードを更新しました",
    ),
    (
        "Downloading before playback…",
        "下载完成后开始播放…",
        "ダウンロード後に再生します…",
    ),
    (
        "episodes · audio downloads on demand",
        "集 · 音频按需下载",
        "エピソード · 音声は必要時にダウンロード",
    ),
    (
        "Enter a valid RSS URL",
        "请输入有效的 RSS 链接",
        "有効な RSS URL を入力してください",
    ),
    (
        "RSS URLs must use HTTP or HTTPS",
        "RSS 链接必须使用 HTTP 或 HTTPS",
        "RSS URL は HTTP または HTTPS が必要です",
    ),
    (
        "Please provide a podcast RSS feed, not a webpage or audio URL",
        "请提供节目 RSS 链接，而非网页或音频地址",
        "Web ページや音声ではなく RSS URL を入力してください",
    ),
    (
        "This RSS feed has no show title",
        "该 RSS 没有节目名称",
        "この RSS に番組名がありません",
    ),
    (
        "Download canceled",
        "下载已取消",
        "ダウンロードを中止しました",
    ),
    (
        "This episode has no playable audio",
        "该单集没有可播放的音频",
        "このエピソードには再生可能な音声がありません",
    ),
    ("Sync to iPhone…", "同步到 iPhone…", "iPhone に同期…"),
    (
        "Audio worker stopped; tap Play to retry",
        "音频服务已停止，点播放重试",
        "音声処理が停止しました。再生をタップして再試行してください",
    ),
    // Desktop iPhone sync window and its structured CLI statuses.
    ("Sync to iPhone", "同步到 iPhone", "iPhone に同期"),
    ("USB Devices", "USB 设备", "USB デバイス"),
    (
        "Looking for USB devices…",
        "正在查找 USB 设备…",
        "USB デバイスを検索中…",
    ),
    (
        "Connect your iPhone with a USB cable, unlock it and trust this computer.",
        "用 USB 连接 iPhone，解锁后选择信任这台电脑。",
        "iPhone を USB ケーブルで接続し、ロックを解除してこのコンピュータを信頼してください。",
    ),
    ("Scanning…", "正在扫描…", "検索中…"),
    ("Refresh Devices", "刷新设备", "デバイスを更新"),
    (
        "A direct USB connection. Your music stays between this computer and your iPhone.",
        "通过 USB 直接连接，音乐只在这台电脑与 iPhone 之间传输。",
        "USB で直接接続します。音楽はこのコンピュータと iPhone の間だけで転送されます。",
    ),
    (
        "Choose a connected iPhone to review your sync settings.",
        "选择已连接的 iPhone，查看同步设置。",
        "接続した iPhone を選んで同期設定を確認してください。",
    ),
    (
        "Your music, on your iPhone",
        "把音乐带到 iPhone",
        "あなたの音楽を iPhone に",
    ),
    (
        "Bring your computer library, playlists and listening data with you. Preview changes before starting the transfer.",
        "把电脑上的音乐、歌单与聆听记录带到手机。开始传输前可先预览。",
        "パソコンの音楽、プレイリスト、再生履歴を持ち出せます。転送前に内容をプレビューしてください。",
    ),
    ("On this computer", "这台电脑", "このコンピュータ"),
    ("Music size", "音乐大小", "音楽の容量"),
    (
        "Use the Music tab to choose your sync range. Phone-only data is preserved.",
        "在“音乐”标签中选择同步范围，手机独有的数据会保留。",
        "「ミュージック」タブで同期する範囲を選んでください。iPhone だけにあるデータは保持されます。",
    ),
    ("Music sync settings", "音乐同步设置", "音楽の同期設定"),
    (
        "Entire music library",
        "整个音乐资料库",
        "音楽ライブラリ全体",
    ),
    ("Selected playlists", "所选播放列表", "選択したプレイリスト"),
    (
        "No playlists yet. Choose the entire library or create a playlist in Music.",
        "还没有播放列表。请选择整个资料库，或先在音乐中创建歌单。",
        "プレイリストがありません。ライブラリ全体を選ぶか、ミュージックでプレイリストを作成してください。",
    ),
    (
        "Preview the selection to calculate what will be transferred.",
        "预览所选内容，计算将传输的音乐。",
        "選択内容をプレビューすると転送する音楽を確認できます。",
    ),
    (
        "Include with your music",
        "随音乐一起同步",
        "音楽と一緒に同期",
    ),
    ("Lyrics and translations", "歌词与翻译", "歌詞と翻訳"),
    (
        "Keep available lyrics, translations and romaji alongside each song.",
        "保留每首歌曲已有的歌词、翻译和罗马音。",
        "各曲の歌詞、翻訳、ローマ字を一緒に転送します。",
    ),
    (
        "Ratings and listening history",
        "评分与聆听记录",
        "評価と再生履歴",
    ),
    (
        "Bring star ratings and recorded plays. Existing phone data is preserved.",
        "同步星级评分与播放记录，保留手机上已有的数据。",
        "星評価と再生記録を転送します。iPhone 上の既存データは保持されます。",
    ),
    (
        "Phone-only music and playlists are kept. Sync never removes songs from your iPhone.",
        "手机独有的音乐与歌单会保留，同步不会删除 iPhone 上的歌曲。",
        "iPhone だけにある音楽とプレイリストは保持されます。同期によって iPhone の曲が削除されることはありません。",
    ),
    ("Ready to preview", "可以开始预览", "プレビューできます"),
    (
        "Preview first. Nothing is transferred until you click Sync.",
        "请先预览，点击“同步”后才会传输。",
        "まずプレビューしてください。「同期」をクリックするまで転送は始まりません。",
    ),
    ("Conflicts", "冲突处理", "競合"),
    (
        "You can close this window. Sync will continue in the background.",
        "可以关闭此窗口，同步会在后台继续。",
        "このウィンドウを閉じても同期はバックグラウンドで続きます。",
    ),
    ("Hide Window", "隐藏窗口", "ウィンドウを隠す"),
    ("Preview Sync", "预览同步", "同期をプレビュー"),
    ("Sync", "同步", "同期"),
    ("Connected via USB", "已通过 USB 连接", "USB 接続済み"),
    (
        "USB · Music, playlists and listening data",
        "USB · 音乐、歌单与聆听记录",
        "USB · 音楽、プレイリスト、再生記録",
    ),
    (
        "Connect and unlock your iPhone, then select it in the sidebar.",
        "连接并解锁 iPhone，然后在侧栏中选择设备。",
        "iPhone を接続してロックを解除し、サイドバーで選択してください。",
    ),
    (
        "Reading your music selection…",
        "正在读取所选音乐…",
        "選択した音楽を確認中…",
    ),
    (
        "Preview ready. Click Sync to transfer this selection.",
        "预览已就绪，点击“同步”传输所选内容。",
        "プレビューができました。「同期」をクリックして選択内容を転送してください。",
    ),
    ("Preparing sync", "正在准备同步", "同期を準備中"),
    (
        "Preparing your selection. Keep the iPhone connected.",
        "正在准备所选内容，请保持 iPhone 连接。",
        "選択内容を準備しています。iPhone を接続したままにしてください。",
    ),
    ("Preparing music", "正在准备音乐", "音楽を準備中"),
    (
        "Backing up iPhone",
        "正在备份 iPhone",
        "iPhone をバックアップ中",
    ),
    ("Transferring over USB", "正在通过 USB 传输", "USB で転送中"),
    ("Finishing transfer", "正在完成传输", "転送を確定中"),
    (
        "Updating iPhone library",
        "正在更新 iPhone 曲库",
        "iPhone のライブラリを更新中",
    ),
    ("Sync complete", "同步完成", "同期完了"),
    ("Sync cancelled", "同步已取消", "同期をキャンセルしました"),
    (
        "Sync could not finish",
        "同步未能完成",
        "同期を完了できませんでした",
    ),
    (
        "Cancelling safely; please keep the USB cable connected…",
        "正在安全取消，请保持 USB 连接…",
        "安全にキャンセルしています。USB ケーブルは接続したままにしてください…",
    ),
    (
        "Could not request cancellation. Keep the app open until sync finishes.",
        "无法请求取消，请保持应用打开，等待同步结束。",
        "キャンセルを要求できませんでした。同期が終わるまでアプリを開いておいてください。",
    ),
    (
        "Keep Nano Music Player open on the iPhone while it updates the library.",
        "更新曲库期间，请保持 iPhone 上的 Nano Music Player 打开。",
        "ライブラリの更新中は、iPhone の Nano Music Player を開いたままにしてください。",
    ),
    (
        "Keep the USB cable connected. This step does not report a byte percentage.",
        "请保持 USB 连接，此步骤不提供字节传输百分比。",
        "USB ケーブルを接続したままにしてください。この処理では転送量の割合は表示されません。",
    ),
    (
        "No completion receipt was received. Keep the phone app open and retry later.",
        "尚未收到完成回执，请保持手机应用打开，稍后再试。",
        "完了通知を受信できませんでした。iPhone のアプリを開いたままにして、後でもう一度お試しください。",
    ),
    (
        "Could not read connected devices. Refresh and retry.",
        "无法读取已连接设备，请刷新后重试。",
        "接続したデバイスを読み取れませんでした。更新してもう一度お試しください。",
    ),
    (
        "Could not read the desktop library.",
        "无法读取电脑曲库。",
        "パソコンのライブラリを読み取れませんでした。",
    ),
    (
        "Could not read the sync preview.",
        "无法读取同步预览。",
        "同期のプレビューを読み取れませんでした。",
    ),
    (
        "Cannot prepare iPhone sync files. Check storage space and file permissions.",
        "无法准备 iPhone 同步文件，请检查可用空间和文件权限。",
        "iPhone の同期ファイルを準備できません。空き容量とファイルのアクセス権を確認してください。",
    ),
    (
        "Cannot start iPhone sync. Install Xcode and its command line tools.",
        "无法启动 iPhone 同步，请安装 Xcode 及其命令行工具。",
        "iPhone との同期を開始できません。Xcode とコマンドラインツールをインストールしてください。",
    ),
    (
        "Sync error output is unavailable",
        "无法读取同步错误信息",
        "同期エラー情報を読み取れません",
    ),
    (
        "Sync output is unavailable",
        "无法读取同步输出",
        "同期の出力を読み取れません",
    ),
    (
        "Cannot wait for iPhone sync",
        "无法等待 iPhone 同步结果",
        "iPhone の同期結果を待機できません",
    ),
    (
        "Could not read iPhone sync progress. Refresh and retry.",
        "无法读取 iPhone 同步进度，请刷新后重试。",
        "iPhone の同期状況を読み取れませんでした。更新してもう一度お試しください。",
    ),
    (
        "iPhone sync could not finish. Reconnect and unlock the phone, then retry.",
        "iPhone 同步未能完成，请重新连接并解锁手机后重试。",
        "iPhone との同期を完了できませんでした。接続し直してロックを解除し、もう一度お試しください。",
    ),
    (
        "USB sync requires macOS and Xcode with devicectl",
        "USB 同步需要 macOS 和包含 devicectl 的 Xcode",
        "USB 同期には macOS と devicectl を含む Xcode が必要です",
    ),
    (
        "Install Xcode and select its developer tools to enable USB sync",
        "请安装 Xcode 并选择其开发工具，以启用 USB 同步",
        "USB 同期を有効にするには、Xcode をインストールして開発ツールを選択してください",
    ),
    (
        "Unable to list USB devices; unlock and trust the iPhone, then retry",
        "无法列出 USB 设备，请解锁 iPhone 并信任这台电脑后重试",
        "USB デバイスを表示できません。iPhone のロックを解除し、このコンピュータを信頼してから再試行してください",
    ),
    (
        "Selected iPhone is not connected over USB; unlock, trust and reconnect it",
        "所选 iPhone 未通过 USB 连接，请解锁、信任电脑并重新连接",
        "選択した iPhone が USB で接続されていません。ロックを解除してコンピュータを信頼し、接続し直してください",
    ),
    (
        "USB device operation failed; unlock the iPhone and check its connection",
        "USB 设备操作失败，请解锁 iPhone 并检查连接",
        "USB デバイスの操作に失敗しました。iPhone のロックを解除して接続を確認してください",
    ),
    (
        "Unable to restart the phone app; open Nano Music Player on the iPhone",
        "无法重新启动手机应用，请在 iPhone 上打开 Nano Music Player",
        "iPhone のアプリを再起動できませんでした。iPhone で Nano Music Player を開いてください",
    ),
    (
        "The library, sync selection or package is invalid; refresh the panel and retry",
        "曲库、同步选择或同步包无效，请刷新面板后重试",
        "ライブラリ、同期の選択内容、または同期パッケージが無効です。パネルを更新して再試行してください",
    ),
    (
        "Unable to read the selected desktop music library",
        "无法读取所选电脑音乐资料库",
        "選択したパソコンの音楽ライブラリを読み取れません",
    ),
    (
        "The operation timed out; keep the phone app open and check the USB connection",
        "操作超时，请保持手机应用打开并检查 USB 连接",
        "処理がタイムアウトしました。iPhone のアプリを開いたままにして USB 接続を確認してください",
    ),
    (
        "Sync cancelled before the phone import was committed",
        "同步已在手机导入提交前取消",
        "同期は iPhone への読み込みが確定する前にキャンセルされました",
    ),
    (
        "Preparing desktop music",
        "正在准备电脑音乐",
        "パソコンの音楽を準備中",
    ),
    (
        "Prepared desktop audio",
        "已准备音频",
        "音声ファイルを準備しました",
    ),
    (
        "Skipped an unavailable audio file",
        "已跳过一个不可用的音频文件",
        "利用できない音声ファイルをスキップしました",
    ),
    (
        "Backing up the phone before copying",
        "正在传输前备份手机数据",
        "転送前に iPhone のデータをバックアップ中",
    ),
    ("{0} songs", "{0} 首歌曲", "{0} 曲"),
    ("{0} songs · {1}", "{0} 首歌曲 · {1}", "{0} 曲 · {1}"),
    (
        "{0} songs · {1} playlists · {2} new history records",
        "{0} 首歌曲 · {1} 个歌单 · 新增 {2} 条聆听记录",
        "{0} 曲 · {1} 個のプレイリスト · 新規再生記録 {2} 件",
    ),
    (
        "{0} unavailable files skipped",
        "已跳过 {0} 个不可用文件",
        "利用できないファイル {0} 個をスキップ",
    ),
    (
        "{0} phone edits preserved",
        "已保留手机上的 {0} 项修改",
        "iPhone 上の変更 {0} 件を保持しました",
    ),
    (
        "Kept phone audio for {0}",
        "已保留手机音频：{0}",
        "iPhone の音声ファイルを保持：{0}",
    ),
    (
        "Kept phone edits to playlist {0}",
        "已保留手机对歌单的修改：{0}",
        "iPhone でのプレイリストの編集を保持：{0}",
    ),
    (
        "Kept phone lyrics for {0}",
        "已保留手机歌词：{0}",
        "iPhone の歌詞を保持：{0}",
    ),
    (
        "Kept phone readings for {0}",
        "已保留手机注音：{0}",
        "iPhone の読み仮名を保持：{0}",
    ),
    (
        "Kept phone rating for {0}",
        "已保留手机评分：{0}",
        "iPhone の評価を保持：{0}",
    ),
    (
        "Kept phone lyric translation",
        "已保留手机上的歌词翻译",
        "iPhone の歌詞翻訳を保持しました",
    ),
    ("Computer Transfer", "电脑传歌", "パソコンから転送"),
    (
        "Import and transfer music",
        "导入与传歌",
        "音楽の読み込み・転送",
    ),
    (
        "Library refresh failed:",
        "曲库刷新失败：",
        "ライブラリの更新に失敗しました：",
    ),
    ("Refresh Library", "刷新曲库", "ライブラリを更新"),
    (
        "Preparing music library…",
        "正在准备音乐资料库…",
        "ライブラリを準備中…",
    ),
    (
        "Refreshing library…",
        "正在刷新曲库…",
        "ライブラリを更新中…",
    ),
    ("Show Lyrics", "显示歌词", "歌詞を表示"),
    (
        "Show or hide song lyrics",
        "显示或隐藏歌曲歌词",
        "曲の歌詞を表示・非表示",
    ),
    ("About nano", "关于 nano", "nano について"),
    (
        "Not available on iOS yet",
        "iOS 暂不支持此功能",
        "iOS ではまだ利用できません",
    ),
    (
        "Settings could not be saved",
        "无法保存设置",
        "設定を保存できませんでした",
    ),
    (
        "Library updated · {0} songs",
        "曲库已更新 · {0} 首歌曲",
        "ライブラリを更新しました · {0} 曲",
    ),
    ("Choose a song", "选择一首歌曲", "曲を選択"),
    (
        "Choose music to import",
        "请选择要导入的音乐",
        "読み込む音楽を選択",
    ),
    (
        "Copying and reading music…",
        "正在复制并读取音乐…",
        "音楽をコピー・読み込み中…",
    ),
    (
        "Imported {0} files",
        "已导入 {0} 个文件",
        "{0} 個のファイルを読み込みました",
    ),
    (
        "Imported {0} files; updating library…",
        "已导入 {0} 个文件，正在更新曲库…",
        "{0} 個のファイルを読み込みました。ライブラリを更新中…",
    ),
    (
        "Loaded {0} songs",
        "已载入 {0} 首歌曲",
        "{0} 曲を読み込みました",
    ),
    ("Untitled song", "未命名歌曲", "無題の曲"),
    (
        "Music received; updating library…",
        "音乐已收到，正在更新资料库…",
        "音楽を受信しました。ライブラリを更新中…",
    ),
    (
        "Computer transfer stopped",
        "电脑传歌已停止",
        "パソコンからの転送を停止しました",
    ),
    (
        "Computer transfer stopped; reopen it in the foreground",
        "电脑传歌已停止；回到前台后可重新开启",
        "転送を停止しました。アプリを開いて再開してください",
    ),
    (
        "Keep the app in the foreground while transferring. Tap Stop Computer Transfer when finished.",
        "传歌时请保持应用在前台。完成后轻点“停止电脑传歌”。",
        "転送中はこのアプリを開いたままにしてください。完了したら「転送を停止」をタップしてください。",
    ),
    (
        "Library worker stopped; import again",
        "曲库工作线程已停止，请重新导入",
        "ライブラリの処理が停止しました。もう一度読み込んでください",
    ),
    ("Start Computer Transfer", "开启电脑传歌", "転送を開始"),
    ("Stop Computer Transfer", "停止电脑传歌", "転送を停止"),
    ("Copy Address", "复制地址", "アドレスをコピー"),
    ("Address copied", "地址已复制", "アドレスをコピーしました"),
    (
        "You can switch apps briefly to share this address. If iOS pauses background transfer, return here to resume with the same address. Keep the app open for large uploads.",
        "可以短暂切换应用分享地址。若 iOS 暂停后台传输，回到这里即可用同一地址继续。上传大文件时请保持应用打开。",
        "アドレスの共有中は一時的にアプリを切り替えられます。iOS が転送を一時停止した場合、この画面に戻ると同じアドレスで再開します。大きなファイルの転送中はアプリを開いてください。",
    ),
    (
        "The address remains valid while you switch apps briefly. Background transfer pauses when the iOS allowance expires; returning here resumes the same address.",
        "短暂切换应用时地址仍然有效。iOS 后台时间用完后会暂停传输，回到这里即可用同一地址继续。",
        "一時的にアプリを切り替えてもアドレスは有効です。iOS のバックグラウンド時間が終了すると一時停止し、この画面に戻ると同じアドレスで再開します。",
    ),
    (
        "Computer transfer paused after the background allowance expired. Return to the app to resume with the same address.",
        "后台运行时间已用完，传歌已暂停。回到应用即可用同一地址继续。",
        "バックグラウンド時間が終了し、転送を一時停止しました。アプリに戻ると同じアドレスで再開します。",
    ),
    (
        "Computer transfer resumed with the same address. Keep the app open for large uploads.",
        "已用同一地址恢复传歌。上传大文件时请保持应用打开。",
        "同じアドレスで転送を再開しました。大きなファイルの転送中はアプリを開いてください。",
    ),
    (
        "Could not copy address",
        "无法复制地址",
        "アドレスをコピーできませんでした",
    ),
    (
        "Connect your computer and phone to the same Wi-Fi. Start transfer, then open the address below in your computer's browser to upload music.",
        "电脑与手机连接同一个 Wi-Fi。开启后，用电脑浏览器打开下方地址，选择音乐即可上传。",
        "パソコンとスマートフォンを同じ Wi-Fi に接続します。転送を開始し、パソコンのブラウザで下のアドレスを開いて音楽をアップロードしてください。",
    ),
    (
        "Open in your computer's browser:",
        "在电脑浏览器打开：",
        "パソコンのブラウザで開く：",
    ),
    (
        "Received music is added to your library automatically.\nKeep the app open while transferring, then stop when finished.",
        "收到的音乐会自动加入曲库。\n传歌时请保持应用在前台，完成后可停止。",
        "受信した音楽はライブラリに自動追加されます。\n転送中はアプリを開いたままにし、完了後に停止してください。",
    ),
    ("Display", "显示", "表示"),
    (
        "Mode, size, font & window",
        "模式、大小、字体与窗口",
        "モード・サイズ・フォント・ウィンドウ",
    ),
    (
        "Gapless & volume normalization",
        "无缝播放与音量均衡",
        "ギャップレス・音量調整",
    ),
    (
        "Track lyrics, romaji & AI",
        "歌曲歌词、罗马音与 AI",
        "歌詞・ローマ字・AI",
    ),
    ("Normalization Mode", "音量均衡模式", "音量調整モード"),
    (
        "Shuffle all songs",
        "随机播放全部歌曲",
        "全曲をシャッフル再生",
    ),
    (
        "Shuffle All Songs",
        "随机播放全部歌曲",
        "全曲をシャッフル再生",
    ),
    ("Song rating", "歌曲评分", "曲の評価"),
    (
        "Could not save rating",
        "无法保存评分",
        "評価を保存できませんでした",
    ),
    ("Recently Added", "最近添加", "最近追加した曲"),
    ("My Top Rated", "我的最高评分", "トップレート"),
    ("Genius Mixes", "Genius 混音", "Genius Mix"),
    ("Most Played", "播放次数最多", "再生回数の多い曲"),
    ("Recently Played", "最近播放", "最近再生した曲"),
    ("Never Played", "从未播放", "未再生の曲"),
    ("Gapless Playback", "无缝播放", "ギャップレス再生"),
    ("ReplayGain", "ReplayGain 音量均衡", "ReplayGain 音量調整"),
    (
        "ReplayGain Preamp",
        "ReplayGain 前置增益",
        "ReplayGain プリアンプ",
    ),
    ("Prevent Clipping", "防止削波", "クリッピングを防止"),
    ("Shuffle", "随机播放", "シャッフル"),
    (
        "Shuffle this list",
        "随机播放此列表",
        "このリストをシャッフル",
    ),
    ("Edit", "编辑", "編集"),
    ("Edit Playlist…", "编辑播放列表…", "プレイリストを編集…"),
    ("Edit Playlist", "编辑播放列表", "プレイリストを編集"),
    ("Changes not saved", "更改尚未保存", "変更は未保存です"),
    (
        "Done saves changes; Cancel discards",
        "完成保存更改，取消放弃更改",
        "完了で保存、キャンセルで破棄",
    ),
    ("Unavailable song", "歌曲不可用", "利用できない曲"),
    (
        "Playlist no longer exists",
        "播放列表已不存在",
        "プレイリストが見つかりません",
    ),
    ("Empty playlist", "空播放列表", "空のプレイリスト"),
    ("Add", "添加", "追加"),
    ("Delete", "删除", "削除"),
    ("Import Videos", "导入视频", "ビデオを読み込む"),
    ("Import Videos…", "导入视频…", "ビデオを読み込む…"),
    (
        "Choose local videos…",
        "请选择本地视频…",
        "ローカルビデオを選択…",
    ),
    (
        "Imported {0} videos",
        "已导入 {0} 个视频",
        "ビデオを{0}本読み込みました",
    ),
    ("{0} errors: {1}", "{0} 个错误：{1}", "{0}件のエラー：{1}"),
    (
        "Video library error:",
        "视频资料库错误：",
        "ビデオライブラリのエラー：",
    ),
    (
        "Video import failed:",
        "视频导入失败：",
        "ビデオの読み込みに失敗しました：",
    ),
    (
        "Video progress error:",
        "视频进度保存错误：",
        "ビデオ再生位置のエラー：",
    ),
    (
        "Removed missing video",
        "已移除失效视频",
        "見つからないビデオを削除しました",
    ),
    ("Next track", "下一首", "次の曲"),
    ("Previous track", "上一首", "前の曲"),
    (
        "Use lowercase ASCII letters, digits, punctuation or F1–F24",
        "请使用小写 ASCII 字母、数字、标点或 F1–F24",
        "小文字の ASCII 文字、数字、句読点、F1–F24 を使用してください",
    ),
    (
        "Conflicting shortcut: {0} is already assigned to {1}",
        "快捷键冲突：{0} 已分配给 {1}",
        "ショートカットの競合：{0} は {1} に割り当て済みです",
    ),
    (
        "Unsupported shortcut file version:",
        "不支持的快捷键文件版本：",
        "未対応のショートカットファイル形式：",
    ),
    (
        "Invalid settings.json:",
        "settings.json 无效：",
        "settings.json が無効です：",
    ),
    (
        "Invalid shortcuts.json:",
        "shortcuts.json 无效：",
        "shortcuts.json が無効です：",
    ),
    (
        "Choose how to edit this action.",
        "选择如何编辑此操作。",
        "この操作の編集方法を選択してください。",
    ),
    ("Lyrics updated", "歌词已更新", "歌詞を更新しました"),
    ("New Playlist…", "新建播放列表…", "新規プレイリスト…"),
    ("New Playlist", "新建播放列表", "新規プレイリスト"),
    ("Review Playlist", "检查播放列表", "プレイリストを確認"),
    ("Playlist Name", "播放列表名称", "プレイリスト名"),
    (
        "Enter a playlist name",
        "输入播放列表名称",
        "プレイリスト名を入力",
    ),
    ("Choose Songs", "选择歌曲", "曲を選択"),
    ("Add Playlist", "新建播放列表", "プレイリストを追加"),
    ("Saving…", "正在保存…", "保存中…"),
    (
        "Loading music library…",
        "正在加载音乐资料库…",
        "ライブラリを読み込み中…",
    ),
    (
        "Import music to choose songs.",
        "导入音乐后即可选择歌曲。",
        "音楽を読み込むと曲を選べます。",
    ),
    (
        "Saving playlist…",
        "正在保存播放列表…",
        "プレイリストを保存中…",
    ),
    (
        "Choose a playlist name",
        "请输入播放列表名称",
        "プレイリスト名を入力してください",
    ),
    (
        "Playlist name must be 100 characters or fewer",
        "播放列表名称不能超过 100 个字符",
        "名前は100文字以内にしてください",
    ),
    (
        "Choose at least one song",
        "请至少选择一首歌曲",
        "曲を1つ以上選択してください",
    ),
    (
        "A playlist with this name already exists",
        "已存在同名播放列表",
        "同じ名前のプレイリストがあります",
    ),
    (
        "Some selected songs are unavailable",
        "部分选中的歌曲已不可用",
        "選択した曲の一部が利用できません",
    ),
    (
        "Could not save playlist. Try again.",
        "无法保存播放列表，请重试。",
        "保存できませんでした。もう一度お試しください。",
    ),
    (
        "Could not load playlists",
        "无法加载播放列表",
        "プレイリストを読み込めませんでした",
    ),
    (" songs", " 首歌曲", " 曲"),
    (
        "Use a single-line playlist name",
        "播放列表名称应为单行文本",
        "プレイリスト名は1行で入力してください",
    ),
    (
        "Playlist saved",
        "播放列表已保存",
        "プレイリストを保存しました",
    ),
    (
        "Draft only · not saved",
        "仅为草稿 · 未保存",
        "下書き · 未保存",
    ),
    ("Name (optional)", "名称（可选）", "名前（任意）"),
    (
        "Select songs, then Review",
        "选择歌曲后检查",
        "曲を選択して確認へ",
    ),
    (
        "Review, then Create to save",
        "检查后点击创建以保存",
        "確認後、作成で保存",
    ),
    (
        "Create saves this playlist",
        "点击创建以保存播放列表",
        "作成でプレイリストを保存",
    ),
    (
        "Song selection is coming next.",
        "歌曲选择功能即将加入。",
        "曲の選択は今後対応予定です。",
    ),
    ("Move Up", "上移", "上へ"),
    ("Move Down", "下移", "下へ"),
    ("Remove", "移除", "削除"),
    ("Review", "检查", "確認"),
    ("Back", "返回", "戻る"),
    (
        "Draft ready · saving comes next",
        "草稿就绪 · 保存功能待加入",
        "下書き完成 · 保存は今後対応",
    ),
    ("Genius Preview…", "Genius 预览…", "Genius プレビュー…"),
    ("Genius", "Genius", "Genius"),
    ("Font", "字体", "フォント"),
    ("Always on Top", "始终置顶", "常に最前面"),
    ("Classic", "经典", "クラシック"),
    ("Maple Mono", "Maple Mono", "Maple Mono"),
    ("JetBrains Mono", "JetBrains Mono", "JetBrains Mono"),
    (
        "Create from local songs",
        "从本地歌曲创建",
        "ローカル曲から作成",
    ),
    (
        "Mock preview · example songs",
        "模拟预览 · 示例歌曲",
        "モックプレビュー · サンプル曲",
    ),
    (
        "Create an example playlist",
        "创建示例播放列表",
        "サンプルプレイリストを作成",
    ),
    (
        "Play preview only · audio unchanged",
        "仅预览播放 · 不改变实际音频",
        "再生プレビューのみ · 音声は変更されません",
    ),
    (
        "Save preview only · nothing stored",
        "仅预览保存 · 未实际存储",
        "保存プレビューのみ · 保存されません",
    ),
    (
        "Example playlist ready",
        "示例播放列表已准备好",
        "サンプルプレイリストの準備完了",
    ),
    (
        "Genius will build a playlist around this song.\n\nThis interface preview uses example songs. Recommendations are not connected yet.",
        "Genius 将根据这首歌曲创建播放列表。\n\n此界面预览使用示例歌曲，推荐功能尚未接入。",
        "Genius はこの曲をもとにプレイリストを作成します。\n\nこのプレビューではサンプル曲を使用します。おすすめ機能はまだ接続されていません。",
    ),
    ("Example Artist", "示例歌手", "サンプルアーティスト"),
    ("Create", "创建", "作成"),
    ("Refresh", "刷新", "更新"),
    ("Play", "播放", "再生"),
    ("Save", "保存", "保存"),
    ("Cancel", "取消", "キャンセル"),
    ("Close", "关闭", "閉じる"),
    ("On", "开启", "オン"),
    ("Off", "关闭", "オフ"),
    (
        "Lyrics, translation, romaji & Genius",
        "歌词、翻译、罗马音和 Genius",
        "歌詞・翻訳・ローマ字・Genius",
    ),
    (
        "Your key is required for DeepSeek requests.",
        "DeepSeek 请求需要您的密钥。",
        "DeepSeek のリクエストには API キーが必要です。",
    ),
    ("DeepSeek Settings", "DeepSeek 设置", "DeepSeek の設定"),
    (
        "Japanese lyrics → romaji",
        "日语歌词 → 罗马音",
        "日本語の歌詞 → ローマ字",
    ),
    ("API Key", "API 密钥", "API キー"),
    (
        "Saved key configured",
        "已配置保存的密钥",
        "保存済みキーを設定済み",
    ),
    ("Enter API key", "输入 API 密钥", "API キーを入力"),
    (
        "Leave blank to keep the saved key.",
        "留空以保留已保存的密钥。",
        "空欄にすると保存済みキーを保持します。",
    ),
    (
        "Your key is required to generate romaji.",
        "生成罗马音需要 API 密钥。",
        "ローマ字の生成には API キーが必要です。",
    ),
    ("Model", "模型", "モデル"),
    ("Generate Romaji", "生成罗马音", "ローマ字を生成"),
    (
        "Cancel Romaji Generation",
        "取消罗马音生成",
        "ローマ字生成を中止",
    ),
    ("Remove Romaji", "移除罗马音", "ローマ字を削除"),
    (
        "Show / Hide Romaji",
        "显示 / 隐藏罗马音",
        "ローマ字の表示 / 非表示",
    ),
    ("Show Romaji", "显示罗马音", "ローマ字を表示"),
    ("Smooth Slides", "平滑滑动", "スムーズなスライド"),
    (
        "On · Screens slide in the navigation direction",
        "开 · 屏幕按导航方向滑动",
        "オン · 画面がナビゲーション方向にスライドします",
    ),
    (
        "Off · Screens slide in the navigation direction",
        "关 · 屏幕按导航方向滑动",
        "オフ · 画面がナビゲーション方向にスライドします",
    ),
    ("DeepSeek API…", "DeepSeek API…", "DeepSeek API…"),
    ("Generating romaji…", "正在生成罗马音…", "ローマ字を生成中…"),
    ("Romaji generated", "罗马音已生成", "ローマ字を生成しました"),
    ("Romaji removed", "罗马音已移除", "ローマ字を削除しました"),
    (
        "Romaji generation cancelled",
        "罗马音生成已取消",
        "ローマ字生成を中止しました",
    ),
    (
        "Romaji generation is already running",
        "罗马音正在生成中",
        "ローマ字はすでに生成中です",
    ),
    (
        "Lyrics changed; romaji result discarded",
        "歌词已更改，罗马音结果已丢弃",
        "歌詞が変更されたため、ローマ字の結果を破棄しました",
    ),
    (
        "Romaji generation failed:",
        "罗马音生成失败：",
        "ローマ字生成に失敗：",
    ),
    (
        "Romaji storage failed:",
        "罗马音保存失败：",
        "ローマ字保存に失敗：",
    ),
    (
        "Romaji storage is unavailable",
        "罗马音存储不可用",
        "ローマ字の保存先を利用できません",
    ),
    (
        "Romaji does not match the lyric lines",
        "罗马音与歌词行不匹配",
        "ローマ字と歌詞の行が一致しません",
    ),
    (
        "Romaji changed a non-Japanese lyric line",
        "罗马音更改了非日语歌词行",
        "日本語以外の歌詞行が変更されました",
    ),
    (
        "No Japanese lyrics to romanize",
        "没有可转换的日语歌词",
        "ローマ字に変換できる日本語の歌詞がありません",
    ),
    (
        "Lyrics are too large for romaji generation",
        "歌词过长，无法生成罗马音",
        "歌詞が長すぎるためローマ字を生成できません",
    ),
    (
        "A lyric line is too long for romaji generation",
        "某行歌词过长，无法生成罗马音",
        "歌詞の一行が長すぎるためローマ字を生成できません",
    ),
    (
        "Configure a DeepSeek API key first",
        "请先配置 DeepSeek API 密钥",
        "先に DeepSeek API キーを設定してください",
    ),
    (
        "Enter a DeepSeek API key",
        "请输入 DeepSeek API 密钥",
        "DeepSeek API キーを入力してください",
    ),
    (
        "DeepSeek settings saved",
        "DeepSeek 设置已保存",
        "DeepSeek の設定を保存しました",
    ),
    (
        "Cannot read DeepSeek settings",
        "无法读取 DeepSeek 设置",
        "DeepSeek の設定を読み込めません",
    ),
    (
        "Invalid deepseek.json",
        "deepseek.json 无效",
        "deepseek.json が無効です",
    ),
    (
        "Invalid DeepSeek API key",
        "DeepSeek API 密钥无效",
        "DeepSeek API キーが無効です",
    ),
    (
        "Invalid DeepSeek model name",
        "DeepSeek 模型名称无效",
        "DeepSeek のモデル名が無効です",
    ),
    (
        "Cannot create DeepSeek settings directory",
        "无法创建 DeepSeek 设置目录",
        "DeepSeek 設定ディレクトリを作成できません",
    ),
    (
        "Cannot save DeepSeek settings",
        "无法保存 DeepSeek 设置",
        "DeepSeek の設定を保存できません",
    ),
    (
        "Cannot encode DeepSeek settings",
        "无法编码 DeepSeek 设置",
        "DeepSeek の設定をエンコードできません",
    ),
    (
        "Cannot initialize DeepSeek connection",
        "无法初始化 DeepSeek 连接",
        "DeepSeek 接続を初期化できません",
    ),
    (
        "Cannot connect to DeepSeek",
        "无法连接 DeepSeek",
        "DeepSeek に接続できません",
    ),
    (
        "DeepSeek request timed out",
        "DeepSeek 请求超时",
        "DeepSeek のリクエストがタイムアウトしました",
    ),
    (
        "DeepSeek rejected the API key",
        "DeepSeek 拒绝了 API 密钥",
        "DeepSeek が API キーを拒否しました",
    ),
    (
        "DeepSeek account has insufficient balance",
        "DeepSeek 账户余额不足",
        "DeepSeek アカウントの残高が不足しています",
    ),
    (
        "DeepSeek rate limit reached; try again later",
        "已达到 DeepSeek 请求限额，请稍后重试",
        "DeepSeek のリクエスト上限に達しました。後でもう一度お試しください",
    ),
    (
        "DeepSeek is temporarily unavailable",
        "DeepSeek 暂时不可用",
        "DeepSeek は一時的に利用できません",
    ),
    (
        "DeepSeek rejected the request; check the model setting",
        "DeepSeek 拒绝了请求，请检查模型设置",
        "DeepSeek がリクエストを拒否しました。モデルの設定を確認してください",
    ),
    (
        "Cannot read DeepSeek response",
        "无法读取 DeepSeek 响应",
        "DeepSeek の応答を読み込めません",
    ),
    (
        "DeepSeek response is too large",
        "DeepSeek 响应过大",
        "DeepSeek の応答が大きすぎます",
    ),
    (
        "DeepSeek returned an invalid response",
        "DeepSeek 返回了无效响应",
        "DeepSeek が無効な応答を返しました",
    ),
    (
        "DeepSeek returned an empty response",
        "DeepSeek 返回了空响应",
        "DeepSeek が空の応答を返しました",
    ),
    (
        "DeepSeek returned invalid JSON",
        "DeepSeek 返回了无效 JSON",
        "DeepSeek が無効な JSON を返しました",
    ),
    (
        "DeepSeek returned an incomplete response; nothing was saved",
        "DeepSeek 响应不完整，未保存任何结果",
        "DeepSeek の応答が不完全です。結果は保存されていません",
    ),
    (
        "DeepSeek returned invalid romaji; nothing was saved",
        "DeepSeek 返回了无效罗马音，未保存任何结果",
        "DeepSeek が無効なローマ字を返しました。結果は保存されていません",
    ),
    (
        "DeepSeek returned an invalid romaji format; nothing was saved",
        "DeepSeek 返回的罗马音格式无效，未保存任何结果",
        "DeepSeek のローマ字形式が無効です。結果は保存されていません",
    ),
    (
        "DeepSeek omitted lyric lines; nothing was saved",
        "DeepSeek 遗漏了歌词行，未保存任何结果",
        "DeepSeek が歌詞行を省略しました。結果は保存されていません",
    ),
    (
        "DeepSeek returned an unknown lyric line; nothing was saved",
        "DeepSeek 返回了未知歌词行，未保存任何结果",
        "DeepSeek が不明な歌詞行を返しました。結果は保存されていません",
    ),
    (
        "DeepSeek duplicated lyric lines; nothing was saved",
        "DeepSeek 返回了重复歌词行，未保存任何结果",
        "DeepSeek が歌詞行を重複させました。結果は保存されていません",
    ),
    ("{0} of {1}", "{0} / {1}", "{1} 曲中 {0}"),
    ("Language", "语言", "言語"),
    ("Lyrics for Track", "歌曲歌词", "曲の歌詞"),
    (
        "Import / Replace Lyrics…",
        "导入 / 替换歌词…",
        "歌詞をインポート / 置換…",
    ),
    ("Subtitles…", "字幕…", "字幕…"),
    ("Subtitles", "字幕", "字幕"),
    ("Import ASS/SSA…", "导入 ASS/SSA…", "ASS/SSA をインポート…"),
    ("Remove External Subtitle", "移除外部字幕", "外部字幕を削除"),
    ("External", "外部", "外部"),
    ("Track", "轨道", "トラック"),
    ("Subtitle error", "字幕错误", "字幕エラー"),
    (
        "Subtitle file is missing or unsupported",
        "字幕文件不存在或格式不受支持",
        "字幕ファイルが見つからないか、形式に対応していません",
    ),
    (
        "Could not open subtitle file",
        "无法打开字幕文件",
        "字幕ファイルを開けませんでした",
    ),
    (
        "Could not save subtitle preference",
        "无法保存字幕设置",
        "字幕設定を保存できませんでした",
    ),
    ("Remove Lyrics", "移除歌词", "歌詞を削除"),
    ("Use Automatic Lyrics", "使用自动歌词", "自動歌詞を使用"),
    ("Manual", "手动", "手動"),
    ("Automatic", "自动", "自動"),
    ("Removed", "已移除", "削除済み"),
    ("Jump to letter", "跳转到字母", "文字にジャンプ"),
    ("Press A–Z to jump", "按 A–Z 跳转", "A–Z キーでジャンプ"),
    ("Import Lyrics", "导入歌词", "歌詞をインポート"),
    ("iPod", "iPod", "iPod"),
    ("Music", "音乐", "ミュージック"),
    ("Videos", "视频", "ビデオ"),
    ("Fitness", "健身", "フィットネス"),
    ("Podcasts", "播客", "ポッドキャスト"),
    ("Photos", "照片", "写真"),
    ("Radio", "电台", "ラジオ"),
    ("Clock", "时钟", "時計"),
    ("Appearance", "外观", "外観"),
    (
        "Window style, size and font",
        "窗口样式、大小和字体",
        "ウィンドウの表示・サイズ・フォント",
    ),
    (
        "Track transitions and volume",
        "歌曲衔接与音量",
        "曲のつなぎ方と音量",
    ),
    (
        "Display, translation and song lyrics",
        "显示、翻译与单曲歌词",
        "表示・翻訳・曲の歌詞",
    ),
    (
        "Choose the app language",
        "选择界面使用的语言",
        "アプリの表示言語を選択",
    ),
    (
        "Change keys for any action",
        "自定义各项操作的按键",
        "各操作のキーを変更",
    ),
    ("AI Services", "AI 服务", "AI サービス"),
    (
        "DeepSeek key for lyrics and Genius",
        "配置歌词与推荐使用的 DeepSeek",
        "歌詞とおすすめ用の DeepSeek を設定",
    ),
    ("Window Style", "窗口样式", "ウィンドウ表示"),
    (
        "Show the iPod body or screen only",
        "显示 iPod 机身，或只显示屏幕",
        "iPod 本体付き・画面のみを選択",
    ),
    ("Window Size", "窗口大小", "ウィンドウサイズ"),
    (
        "Choose how large the player appears",
        "调整整个播放器的显示大小",
        "プレーヤー全体の大きさを選択",
    ),
    (
        "Text style throughout the app",
        "应用于整个界面的文字",
        "アプリ全体の文字スタイル",
    ),
    ("Page Animation", "页面动画", "ページアニメーション"),
    (
        "Slide between pages when navigating",
        "切换页面时使用滑动动画",
        "ページ移動時にスライド表示",
    ),
    ("Keep on Top", "窗口置顶", "最前面に表示"),
    (
        "Keep the player above other windows",
        "让播放器保持在其他窗口上方",
        "他のウィンドウより手前に表示",
    ),
    ("iPod Body", "iPod 机身", "iPod 本体"),
    (
        "Screen with bezel and Home button",
        "显示边框和 Home 按钮",
        "枠とホームボタン付きの画面",
    ),
    ("Screen Only", "仅屏幕", "画面のみ"),
    (
        "A plain window without the iPod body",
        "使用不带 iPod 机身的普通窗口",
        "iPod 本体のない通常のウィンドウ",
    ),
    ("Original size", "原始大小", "元のサイズ"),
    (
        "Scale the whole player",
        "等比例缩放整个播放器",
        "プレーヤー全体を拡大・縮小",
    ),
    (
        "Play consecutive tracks without pauses",
        "连续播放歌曲，减少曲间停顿",
        "曲間に間を入れずに連続再生",
    ),
    ("Volume Leveling", "音量均衡", "音量の均一化"),
    (
        "Balance loudness using ReplayGain tags",
        "根据 ReplayGain 标签平衡响度",
        "ReplayGain タグで音量差を調整",
    ),
    ("Volume Boost", "音量增益", "音量補正"),
    (
        "Adjust ReplayGain level before playback",
        "播放前调整 ReplayGain 的增益",
        "再生前に ReplayGain の音量を補正",
    ),
    ("Prevent Distortion", "防止失真", "音割れを防ぐ"),
    (
        "Limit peaks when volume leveling is on",
        "启用音量均衡时，限制过高峰值",
        "音量の均一化中にピークを抑える",
    ),
    (
        "Keep the original loudness",
        "保持歌曲原有的响度",
        "元の音量のまま再生",
    ),
    ("By Song", "按歌曲", "曲ごと"),
    (
        "Match loudness across individual songs",
        "让不同歌曲的响度更接近",
        "各曲の音量をそろえる",
    ),
    ("By Album", "按专辑", "アルバムごと"),
    (
        "Preserve volume differences in an album",
        "保留同一专辑内歌曲的音量差异",
        "アルバム内の曲の音量差を保つ",
    ),
    ("No extra boost", "不额外调整增益", "追加の音量補正なし"),
    (
        "Applied to audio with ReplayGain tags",
        "仅用于含 ReplayGain 标签的音频",
        "ReplayGain タグ付き音声に適用",
    ),
    (
        "Display a saved translation below lyrics",
        "在原歌词下方显示已生成的译文",
        "歌詞の下に保存済みの翻訳を表示",
    ),
    ("Translate To", "翻译目标语言", "翻訳先の言語"),
    (
        "Language used for new translations",
        "生成新译文时使用的语言",
        "新しい翻訳に使用する言語",
    ),
    ("Japanese Readings", "日语罗马音", "日本語の読み"),
    (
        "Show romaji when translation is hidden",
        "隐藏译文时，在日语歌词下显示罗马音",
        "翻訳が非表示のときローマ字を表示",
    ),
    ("Translate Current Song", "翻译当前歌曲", "再生中の曲を翻訳"),
    (
        "Generate or remove its translation",
        "为正在播放的歌曲生成或删除译文",
        "再生中の曲の翻訳を作成・削除",
    ),
    ("Find / Edit Lyrics", "查找或编辑歌词", "歌詞を検索・編集"),
    (
        "Choose any song from your library",
        "从曲库选择歌曲，无需开始播放",
        "再生せずにライブラリから曲を選択",
    ),
    ("Song Translation", "歌曲翻译", "曲の翻訳"),
    (
        "Play a song to translate its lyrics",
        "先播放一首歌，再翻译它的歌词",
        "曲を再生してから歌詞を翻訳",
    ),
    (
        "Replace the current song's translation",
        "重新生成当前歌曲的译文",
        "再生中の曲の翻訳を作り直す",
    ),
    (
        "Stop the pending lyric request",
        "停止正在进行的歌词请求",
        "処理中の歌詞リクエストを停止",
    ),
    (
        "Delete this song's saved translation",
        "删除当前歌曲已保存的译文",
        "再生中の曲の保存済み翻訳を削除",
    ),
    ("Settings", "设置", "設定"),
    ("Artists", "艺人", "アーティスト"),
    ("Albums", "专辑", "アルバム"),
    ("Genres", "流派", "ジャンル"),
    ("Playlists", "播放列表", "プレイリスト"),
    ("Songs", "歌曲", "曲"),
    ("Search", "搜索", "検索"),
    ("Now Playing", "正在播放", "再生中"),
    ("Shortcuts", "快捷键", "ショートカット"),
    (
        "Keyboard Shortcuts",
        "键盘快捷键",
        "キーボードショートカット",
    ),
    (
        "Customize keys and navigation",
        "自定义按键和导航",
        "キーとナビゲーションをカスタマイズ",
    ),
    ("Display Mode", "显示模式", "表示モード"),
    ("Device", "设备", "デバイス"),
    ("Screen", "屏幕", "画面"),
    ("Zoom In", "放大", "拡大"),
    ("Zoom Out", "缩小", "縮小"),
    (
        "Restore All Defaults",
        "恢复所有默认设置",
        "すべての既定値に戻す",
    ),
    ("All Songs", "所有歌曲", "すべての曲"),
    ("Import Music…", "导入音乐…", "音楽をインポート…"),
    (
        "Import Music Folder…",
        "导入音乐文件夹…",
        "音楽フォルダーをインポート…",
    ),
    ("Search Library…", "搜索曲库…", "ライブラリを検索…"),
    ("File", "文件", "ファイル"),
    ("View", "视图", "表示"),
    ("Help", "帮助", "ヘルプ"),
    ("Home", "主页", "ホーム"),
    ("Quit", "退出", "終了"),
    ("Playback", "播放", "再生"),
    ("Play / Pause", "播放 / 暂停", "再生 / 一時停止"),
    ("Next Track", "下一首", "次の曲"),
    ("Previous Track", "上一首", "前の曲"),
    ("Device Mode", "设备模式", "デバイスモード"),
    ("Screen Mode", "屏幕模式", "画面モード"),
    ("Lyrics", "歌词", "歌詞"),
    (
        "No matching songs",
        "没有匹配的歌曲",
        "一致する曲はありません",
    ),
    (
        "No music yet\nFile → Import Music Folder",
        "还没有音乐\n文件 → 导入音乐文件夹",
        "曲がありません\nファイル → 音楽フォルダーをインポート",
    ),
    (
        "Choose a song from Music",
        "从音乐中选择一首歌",
        "ミュージックから曲を選択",
    ),
    ("No lyrics available", "没有可用歌词", "歌詞がありません"),
    (
        "No matching synchronized lyrics found; existing lyrics kept",
        "未找到对应版本的同步歌词，已保留现有歌词",
        "この録音に一致する同期歌詞が見つからないため、既存の歌詞を保持しました",
    ),
    (
        "Press F to find lyrics",
        "按 F 自动查找歌词",
        "F キーで歌詞を検索",
    ),
    ("Press F to try again", "按 F 重新查找", "F キーで再検索"),
    ("No translation available", "暂无译文", "翻訳がありません"),
    (
        "Find Lyrics Online",
        "在线查找歌词",
        "オンラインで歌詞を検索",
    ),
    (
        "Searching online lyrics…",
        "正在在线查找歌词…",
        "オンラインで歌詞を検索中…",
    ),
    (
        "Saving online lyrics…",
        "正在保存在线歌词…",
        "オンライン歌詞を保存中…",
    ),
    (
        "A lyric request is already running",
        "歌词请求正在进行中",
        "歌詞のリクエストを処理中です",
    ),
    (
        "Lyrics are already available",
        "已有可用歌词",
        "歌詞はすでにあります",
    ),
    (
        "No matching online lyrics found",
        "未找到匹配的在线歌词",
        "一致するオンライン歌詞が見つかりません",
    ),
    (
        "Online lyrics did not match this recording",
        "在线歌词与此录音版本不匹配",
        "オンライン歌詞がこの録音と一致しません",
    ),
    (
        "This recording is instrumental",
        "此录音为纯音乐",
        "この録音はインストゥルメンタルです",
    ),
    (
        "Online lyric search cancelled",
        "在线歌词查找已取消",
        "オンライン歌詞の検索をキャンセルしました",
    ),
    (
        "Online lyric search failed",
        "在线歌词查找失败",
        "オンライン歌詞の検索に失敗しました",
    ),
    (
        "Song or lyrics changed; online result discarded",
        "歌曲或歌词已更改，已丢弃在线结果",
        "曲または歌詞が変更されたため、オンラインの結果を破棄しました",
    ),
    (
        "Lyrics were edited or removed; online result discarded",
        "歌词已编辑或移除，已丢弃在线结果",
        "歌詞が編集または削除されたため、オンラインの結果を破棄しました",
    ),
    (
        "Title, artist and duration are required for online lyrics",
        "在线查找歌词需要歌名、歌手和时长",
        "オンライン歌詞には曲名・アーティスト・長さが必要です",
    ),
    (
        "Cannot connect to online lyrics",
        "无法连接在线歌词服务",
        "オンライン歌詞に接続できません",
    ),
    (
        "Online lyrics rate limit reached; try again later",
        "在线歌词请求已达限额，请稍后重试",
        "オンライン歌詞の利用上限に達しました。後でもう一度お試しください",
    ),
    (
        "Online lyrics service is unavailable",
        "在线歌词服务不可用",
        "オンライン歌詞サービスが利用できません",
    ),
    (
        "Cannot read online lyrics",
        "无法读取在线歌词",
        "オンライン歌詞を読み込めません",
    ),
    (
        "Online lyrics response is too large",
        "在线歌词响应过大",
        "オンライン歌詞の応答が大きすぎます",
    ),
    (
        "Online lyrics returned an invalid response",
        "在线歌词返回无效响应",
        "オンライン歌詞の応答が無効です",
    ),
    (
        "DeepSeek returned an incomplete search response",
        "DeepSeek 搜索响应不完整",
        "DeepSeek の検索応答が不完全です",
    ),
    (
        "DeepSeek returned an invalid search response",
        "DeepSeek 搜索响应无效",
        "DeepSeek の検索応答が無効です",
    ),
    (
        "DeepSeek web search failed",
        "DeepSeek 网页搜索失败",
        "DeepSeek のウェブ検索に失敗しました",
    ),
    (
        "DeepSeek did not perform an online search",
        "DeepSeek 未执行在线搜索",
        "DeepSeek がオンライン検索を実行しませんでした",
    ),
    ("Find Lyrics", "查找歌词", "歌詞を探す"),
    (
        "Search online sources",
        "在线查找匹配歌词",
        "オンラインで一致する歌詞を検索",
    ),
    (
        "Could not verify lyrics for this recording",
        "暂未找到能确认匹配此录音的歌词",
        "この録音に一致する歌詞を確認できませんでした",
    ),
    (
        "DeepSeek web search rate limit reached; try again later",
        "DeepSeek 网页搜索已达限额，请稍后重试",
        "DeepSeek のウェブ検索上限に達しました。後でもう一度お試しください",
    ),
    (
        "DeepSeek search reached its tool-use limit",
        "DeepSeek 搜索已达到本次请求的工具使用次数上限",
        "DeepSeek 検索がこのリクエストのツール使用回数上限に達しました",
    ),
    ("Choose Song", "选择歌曲", "曲を選択"),
    (
        "Cancel Lyric Search",
        "取消歌词查找",
        "歌詞の検索をキャンセル",
    ),
    (
        "Search with DeepSeek",
        "使用 DeepSeek 在线搜索",
        "DeepSeek でオンライン検索",
    ),
    (
        "Found on LRCLIB",
        "已从 LRCLIB 找到歌词",
        "LRCLIB で歌詞が見つかりました",
    ),
    (
        "From audio tags and nearby files",
        "从音频标签及同目录歌词文件读取",
        "音声タグや同じフォルダのファイルから読み込み",
    ),
    (
        "Import, replace or choose file lyrics",
        "导入、替换或使用文件中的歌词",
        "歌詞ファイルを取り込む・置換する・選択する",
    ),
    ("Translation", "歌词翻译", "歌詞の翻訳"),
    ("Translation Language", "翻译语言", "翻訳先の言語"),
    ("Show Translation", "显示译文", "翻訳を表示"),
    ("Translate Lyrics", "翻译歌词", "歌詞を翻訳"),
    ("Regenerate Translation", "重新翻译", "翻訳を再生成"),
    ("Cancel Translation", "取消翻译", "翻訳をキャンセル"),
    (
        "Translation cancellation requested",
        "已请求取消翻译",
        "翻訳のキャンセルを要求しました",
    ),
    (
        "Translation cancelled",
        "翻译已取消",
        "翻訳をキャンセルしました",
    ),
    (
        "Translation failed; try again",
        "翻译失败，请重试",
        "翻訳に失敗しました。再試行してください",
    ),
    ("Generating translation…", "正在生成译文…", "翻訳を生成中…"),
    ("Translation ready", "译文已就绪", "翻訳の準備ができました"),
    (
        "Could not save translation cache",
        "无法保存译文缓存",
        "翻訳キャッシュを保存できません",
    ),
    (
        "Lyrics or target changed; translation discarded",
        "歌词或目标语言已更改，已丢弃译文",
        "歌詞または対象言語が変更されたため翻訳を破棄しました",
    ),
    (
        "Show / Hide Translation",
        "显示/隐藏译文",
        "翻訳を表示／非表示",
    ),
    ("Remove Translation", "移除译文", "翻訳を削除"),
    ("Translation: English", "译文：英语", "翻訳：英語"),
    (
        "Translation: 简体中文",
        "译文：简体中文",
        "翻訳：中国語（簡体字）",
    ),
    ("Translation: 日本語", "译文：日语", "翻訳：日本語"),
    ("Genius playlist", "Genius 播放列表", "Genius プレイリスト"),
    (
        "Choose a song first",
        "请先选择一首歌",
        "先に曲を選択してください",
    ),
    (
        "Create Genius playlist",
        "创建 Genius 播放列表",
        "Genius プレイリストを作成",
    ),
    (
        "Playing Genius playlist",
        "正在播放 Genius 播放列表",
        "Genius プレイリストを再生中",
    ),
    (
        "Genius playlist saved",
        "Genius 播放列表已保存",
        "Genius プレイリストを保存しました",
    ),
    (
        "Genius playlist ready",
        "Genius 播放列表已就绪",
        "Genius プレイリストの準備ができました",
    ),
    (
        "Create a playlist based on this song.",
        "根据这首歌创建播放列表。",
        "この曲をもとにプレイリストを作成します。",
    ),
    (
        "Not enough available local songs",
        "本地可用歌曲不足",
        "利用可能なローカル曲が不足しています",
    ),
    (
        "Library or seed changed; result discarded",
        "曲库或种子歌曲已更改，已丢弃结果",
        "ライブラリまたはシード曲が変更されたため結果を破棄しました",
    ),
    (
        "Short local playlist",
        "本地匹配歌曲较少",
        "ローカルの一致曲が少数です",
    ),
    (
        "Genius failed; retry is available",
        "Genius 生成失败，可重试",
        "Genius の生成に失敗しました。再試行できます",
    ),
    (
        "Could not save Genius playlist; retry",
        "无法保存 Genius 播放列表，请重试",
        "Genius プレイリストを保存できません。再試行してください",
    ),
    ("Generating…", "正在生成…", "生成中…"),
    ("No song selected", "未选择歌曲", "曲が選択されていません"),
    ("Add key", "添加按键", "キーを追加"),
    ("Cancel recording", "取消录制", "記録をキャンセル"),
    ("Default keys", "默认按键", "既定のキー"),
    ("Done", "完成", "完了"),
    ("Remove keys", "移除按键", "キーを削除"),
    ("Replace keys", "替换按键", "キーを置換"),
    (
        "Letters, arrows, function keys\nand modifier combinations.\nChanges save automatically.",
        "字母、方向键、功能键\n和修饰键组合。\n更改会自动保存。",
        "文字、矢印、ファンクションキー\nと修飾キーの組み合わせ。\n変更は自動保存されます。",
    ),
    (
        "Import a music folder to begin",
        "导入音乐文件夹以开始",
        "音楽フォルダーをインポートして開始",
    ),
    (
        "Import a music folder to play music",
        "导入音乐文件夹以播放音乐",
        "音楽を再生するにはフォルダーをインポート",
    ),
    (
        "Choose a music folder…",
        "选择音乐文件夹…",
        "音楽フォルダーを選択…",
    ),
    ("Muted", "已静音", "ミュート中"),
    ("Unmuted", "已取消静音", "ミュート解除"),
    (
        "Scanning music folders…",
        "正在扫描音乐文件夹…",
        "音楽フォルダーをスキャン中…",
    ),
    (
        "Import cancelled",
        "已取消导入",
        "インポートをキャンセルしました",
    ),
    (
        "Default shortcuts restored",
        "已恢复默认快捷键",
        "既定のショートカットを復元しました",
    ),
    (
        "Saved. Your shortcuts are active.",
        "已保存。快捷键已生效。",
        "保存しました。ショートカットが有効です。",
    ),
    (
        "Recording cancelled.",
        "已取消录制。",
        "記録をキャンセルしました。",
    ),
    (
        "Press a key combination. Escape cancels.",
        "请按下组合键。按 Escape 取消。",
        "キーの組み合わせを押してください。Escape でキャンセルします。",
    ),
    ("Unassigned", "未分配", "未割り当て"),
    ("Global playback", "全局播放", "全体の再生"),
    ("Screen navigation", "屏幕导航", "画面ナビゲーション"),
    ("Application management", "应用管理", "アプリ管理"),
    ("Move selection up", "向上移动选择", "選択項目を上へ移動"),
    ("Move selection down", "向下移动选择", "選択項目を下へ移動"),
    ("Activate selected item", "打开所选项目", "選択項目を開く"),
    ("Navigate back", "返回", "戻る"),
    ("Physical Home button", "实体 Home 键", "物理 Home ボタン"),
    (
        "Left arrow / seek back",
        "左方向键 / 后退",
        "左矢印 / 巻き戻し",
    ),
    (
        "Right arrow / seek forward",
        "右方向键 / 前进",
        "右矢印 / 早送り",
    ),
    ("Left / parent screen", "左 / 上级界面", "左 / 親画面"),
    ("Right / open item", "右 / 打开项目", "右 / 項目を開く"),
    (
        "Increase volume (hold to repeat)",
        "提高音量（按住连续调节）",
        "音量を上げる（長押しで連続）",
    ),
    (
        "Decrease volume (hold to repeat)",
        "降低音量（按住连续调节）",
        "音量を下げる（長押しで連続）",
    ),
    ("Toggle mute", "切换静音", "ミュート切り替え"),
    ("Toggle shuffle", "切换随机播放", "シャッフル切り替え"),
    (
        "Repeat: Off / All / One",
        "循环：关闭 / 全部 / 单曲",
        "リピート：オフ / 全曲 / 1 曲",
    ),
    ("Open Now Playing", "打开正在播放", "再生中を開く"),
    (
        "Import music folder",
        "导入音乐文件夹",
        "音楽フォルダーをインポート",
    ),
    ("Search library", "搜索曲库", "ライブラリを検索"),
    ("Zoom in", "放大", "拡大"),
    ("Zoom out", "缩小", "縮小"),
    (
        "Keyboard shortcuts",
        "键盘快捷键",
        "キーボードショートカット",
    ),
    (
        "Close keyboard shortcuts",
        "关闭键盘快捷键",
        "キーボードショートカットを閉じる",
    ),
    ("Browse search results", "浏览搜索结果", "検索結果を表示"),
    ("Seek back 5 seconds", "后退 5 秒", "5 秒戻る"),
    ("Seek forward 5 seconds", "前进 5 秒", "5 秒進む"),
    (
        "Show / hide lyrics",
        "显示 / 隐藏歌词",
        "歌詞を表示 / 非表示",
    ),
    (
        "Show / hide cover controls",
        "显示 / 隐藏封面控件",
        "カバー操作を表示 / 非表示",
    ),
    ("Open Settings", "打开设置", "設定を開く"),
    (
        "Customize shortcuts",
        "自定义快捷键",
        "ショートカットをカスタマイズ",
    ),
    (
        "Restore default shortcuts",
        "恢复默认快捷键",
        "既定のショートカットを復元",
    ),
    ("Album track list", "专辑曲目列表", "アルバムの曲一覧"),
    ("No entries", "没有条目", "項目がありません"),
    ("Press a key", "按下一个按键", "キーを押してください"),
    ("Search…", "搜索…", "検索…"),
    ("Type to search", "输入以搜索", "入力して検索"),
    (
        "This app is not available yet.",
        "此应用暂不可用。",
        "このアプリはまだ利用できません。",
    ),
    (
        "Press Shift + H to return Home.",
        "按 Shift + H 返回主页。",
        "Shift + H でホームに戻ります。",
    ),
    (
        "Shortcuts pause while editing search.\nEnter / Escape returns focus to results.\nDefaults: H/J/K/L = back/down/up/open.\nY: lyrics. Shift + H: Home.",
        "编辑搜索时快捷键会暂停。\n按 Enter / Escape 将焦点返回结果。\n默认：H/J/K/L = 返回/下/上/打开。\nY：歌词。Shift + H：主页。",
        "検索編集中はショートカットが一時停止します。\nEnter / Escape で結果に戻ります。\n既定：H/J/K/L = 戻る/下/上/開く。\nY：歌詞。Shift + H：ホーム。",
    ),
    ("Settings…", "设置…", "設定…"),
    (
        "Keyboard Shortcuts…",
        "键盘快捷键…",
        "キーボードショートカット…",
    ),
    ("Shuffle: On", "随机播放：开", "シャッフル：オン"),
    ("Shuffle: Off", "随机播放：关", "シャッフル：オフ"),
    ("Repeat: Off", "循环：关闭", "リピート：オフ"),
    ("Repeat: All", "循环：全部", "リピート：全曲"),
    ("Repeat: One", "循环：单曲", "リピート：1 曲"),
    ("Lyrics for Track…", "歌曲歌词…", "曲の歌詞…"),
    (
        "This part of the nano is coming next.\nMusic and keyboard controls are ready.",
        "iPod nano 的这一部分即将推出。\n音乐和键盘控制已就绪。",
        "この iPod nano の機能は近日公開予定です。\n音楽とキーボード操作は利用できます。",
    ),
    ("Playback error:", "播放错误：", "再生エラー："),
    ("Import failed:", "导入失败：", "インポートに失敗しました："),
    (
        "Lyrics import failed:",
        "歌词导入失败：",
        "歌詞のインポートに失敗しました：",
    ),
    (
        "Cannot save window settings:",
        "无法保存窗口设置：",
        "ウィンドウ設定を保存できません：",
    ),
    (
        "No entries for",
        "没有以下内容的条目：",
        "次の項目はありません：",
    ),
    ("Volume", "音量", "音量"),
    (" songs in library", " 首歌曲", " 曲"),
    (" · ", " · 跳过 ", " · スキップ "),
    (" files skipped", " 个文件", " 個のファイルをスキップ"),
    (
        "No music yet\nMusic → Import Music…",
        "还没有音乐\n音乐 → 导入音乐…",
        "曲がありません\nミュージック → 曲を読み込む…",
    ),
    ("Paste", "粘贴", "ペースト"),
    (
        "Clipboard has no text",
        "剪贴板里没有文本",
        "クリップボードにテキストがありません",
    ),
    (
        "The clipboard has no text or paste access was denied",
        "剪贴板没有文本，或粘贴权限未获允许",
        "クリップボードが空か、貼り付けが許可されていません",
    ),
    (
        "Paste plain text or LRC lyrics.",
        "粘贴纯文本或 LRC 歌词。",
        "テキストまたはLRC歌詞を貼り付けてください。",
    ),
    ("Delete Playlist", "删除播放列表", "プレイリストを削除"),
    (
        "Playlist deleted",
        "播放列表已删除",
        "プレイリストを削除しました",
    ),
    (
        "Could not delete playlist",
        "无法删除播放列表",
        "プレイリストを削除できませんでした",
    ),
    ("Edit Lyrics", "编辑歌词", "歌詞を編集"),
    ("Copy Lyrics", "复制歌词", "歌詞をコピー"),
    ("Lyrics copied", "歌词已复制", "歌詞をコピーしました"),
    (
        "Enter lyrics before saving",
        "请输入歌词后保存",
        "歌詞を入力してから保存してください",
    ),
    ("Lyrics are too large", "歌词文本过长", "歌詞が長すぎます"),
    ("Song Options", "歌曲选项", "曲のオプション"),
    ("Smart Playlists", "智能播放列表", "スマートプレイリスト"),
    ("Rate Current Song", "为当前歌曲评分", "再生中の曲を評価"),
];

/// Translate a catalog key. Unknown text is returned unchanged for forward compatibility.
pub fn translate(language: Language, english: &str) -> String {
    if language == Language::English {
        return english.to_owned();
    }
    CATALOG
        .iter()
        .find(|entry| entry.0 == english)
        .map(|entry| match language {
            Language::English => entry.0,
            Language::SimplifiedChinese => entry.1,
            Language::Japanese => entry.2,
        })
        .unwrap_or(english)
        .to_owned()
}

pub fn has_translation(english: &str) -> bool {
    CATALOG.iter().any(|entry| entry.0 == english)
}

/// Translate app notices, including common formatted notices whose values are runtime data.
/// Arbitrary system error details are retained after their localized label.
pub fn notice(language: Language, text: &str) -> String {
    if language == Language::English {
        return text.to_owned();
    }
    for (prefix, suffix, key) in [
        (
            "Library updated · ",
            " songs",
            "Library updated · {0} songs",
        ),
        ("Loaded ", " songs", "Loaded {0} songs"),
        (
            "Imported ",
            " files; updating library…",
            "Imported {0} files; updating library…",
        ),
        ("Imported ", " files", "Imported {0} files"),
    ] {
        if let Some(rest) = text.strip_prefix(prefix)
            && let Some((count, detail)) = rest.split_once(suffix)
            && count.parse::<usize>().is_ok()
            && (detail.is_empty() || detail.starts_with("; ") || detail.starts_with(" · "))
        {
            return format!("{}{detail}", translate(language, key).replace("{0}", count));
        }
    }
    for key in [
        "Settings could not be saved",
        "Could not copy address",
        "Could not delete playlist",
    ] {
        if let Some(detail) = text.strip_prefix(&format!("{key}: ")) {
            return format!("{}: {detail}", translate(language, key));
        }
    }
    if let Some(rest) = text.strip_prefix("Imported ")
        && let Some((count, errors)) = rest.split_once(" videos")
        && count.parse::<usize>().is_ok()
    {
        let imported = translate(language, "Imported {0} videos").replace("{0}", count);
        if let Some(errors) = errors.strip_prefix(" · ")
            && let Some((count, detail)) = errors.split_once(" errors: ")
        {
            return format!(
                "{imported} · {}",
                translate(language, "{0} errors: {1}")
                    .replace("{0}", count)
                    .replace("{1}", detail)
            );
        }
        return imported;
    }
    if let Some(count) = text.strip_suffix(" songs")
        && count.parse::<usize>().is_ok()
    {
        return format!("{count}{}", translate(language, " songs"));
    }
    for prefix in ["Romaji generation failed: ", "Romaji storage failed: "] {
        if let Some(detail) = text.strip_prefix(prefix) {
            return format!(
                "{} {}",
                translate(language, prefix.trim_end()),
                translate(language, detail)
            );
        }
    }
    if let Some(progress) = text.strip_prefix("Generating romaji… ") {
        return format!("{} {}", translate(language, "Generating romaji…"), progress);
    }
    for prefix in [
        "Unsupported shortcut file version: ",
        "Invalid settings.json: ",
        "Invalid shortcuts.json: ",
        "Playback error: ",
        "Subtitle error: ",
        "Import failed: ",
        "Video library error: ",
        "Library refresh failed: ",
        "Video progress error: ",
        "Video import failed: ",
        "Lyrics import failed: ",
        "Cannot save window settings: ",
    ] {
        if let Some(detail) = text.strip_prefix(prefix) {
            return format!(
                "{} {}",
                translate(language, prefix.trim_end_matches(' ')),
                detail
            );
        }
    }
    for prefix in [
        "Subtitle file is missing or unsupported: ",
        "Could not open subtitle file: ",
        "Could not save subtitle preference: ",
    ] {
        if let Some(detail) = text.strip_prefix(prefix) {
            return format!(
                "{}: {detail}",
                translate(language, prefix.trim_end_matches([':', ' ']))
            );
        }
    }
    if let Some(rest) = text.strip_prefix("Conflicting shortcut: ")
        && let Some((keys, action)) = rest.split_once(" is already assigned to ")
    {
        return translate(
            language,
            "Conflicting shortcut: {0} is already assigned to {1}",
        )
        .replace("{0}", keys)
        .replace("{1}", &translate(language, action));
    }
    if let Some(rest) = text.strip_prefix("No entries for ") {
        return format!("{} {}", translate(language, "No entries for"), rest);
    }
    if text.starts_with("Volume: ") {
        return format!(
            "{}{}",
            translate(language, "Volume"),
            &text["Volume".len()..]
        );
    }
    if let Some(value) = text.strip_prefix("Shuffle: ") {
        let key = match value {
            "On" => "Shuffle: On",
            "Off" => "Shuffle: Off",
            _ => return text.to_owned(),
        };
        return translate(language, key);
    }
    if let Some(value) = text.strip_prefix("Repeat: ") {
        let key = match value {
            "Off" => "Repeat: Off",
            "All" => "Repeat: All",
            "One" => "Repeat: One",
            _ => return text.to_owned(),
        };
        return translate(language, key);
    }
    if let Some((count, tail)) = text.split_once(" songs in library") {
        let mut out = format!(
            "{}{}{}",
            count,
            translate(language, " songs in library"),
            tail
        );
        if let Some((_, files)) = tail.split_once(" · ") {
            let number = files.split_whitespace().next().unwrap_or("");
            out = format!(
                "{}{}{}{}{}",
                count,
                translate(language, " songs in library"),
                translate(language, " · "),
                number,
                translate(language, " files skipped")
            );
        }
        return out;
    }
    translate(language, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn catalog_keys_are_unique_and_translations_are_present() {
        let mut keys = HashSet::new();
        for (en, zh, ja) in CATALOG {
            assert!(keys.insert(en), "duplicate catalog key: {en}");
            assert!(!en.is_empty() && !zh.is_empty() && !ja.is_empty());
        }
    }

    #[test]
    fn representative_translations_and_fallback() {
        assert_eq!(translate(Language::SimplifiedChinese, "Settings"), "设置");
        assert_eq!(translate(Language::Japanese, "Lyrics"), "歌詞");
        assert_eq!(
            translate(Language::English, "Smooth Slides"),
            "Smooth Slides"
        );
        assert_eq!(
            translate(Language::SimplifiedChinese, "Smooth Slides"),
            "平滑滑动"
        );
        assert_eq!(
            translate(Language::Japanese, "Smooth Slides"),
            "スムーズなスライド"
        );
        assert_eq!(
            translate(
                Language::SimplifiedChinese,
                "On · Screens slide in the navigation direction"
            ),
            "开 · 屏幕按导航方向滑动"
        );
        assert_eq!(
            translate(Language::Japanese, "runtime metadata"),
            "runtime metadata"
        );
    }

    #[test]
    fn interface_literals_and_customizable_actions_have_translations() {
        for action in super::super::action::Action::CUSTOMIZABLE {
            assert!(
                has_translation(action.title()),
                "missing action: {}",
                action.title()
            );
            assert!(has_translation(action.category().title()));
        }
        for (source, call) in [
            (include_str!("../../ui/app.slint"), "I18n.tr("),
            (include_str!("../../ui/library.slint"), "I18n.tr("),
            (include_str!("../../ui/now-playing.slint"), "I18n.tr("),
            (include_str!("../../ui/deepseek-settings.slint"), "I18n.tr("),
            (include_str!("../../ui/genius-preview.slint"), "I18n.tr("),
            (include_str!("../../ui/playlist-editor.slint"), "I18n.tr("),
            (include_str!("../../ui/shortcut-editor.slint"), "I18n.tr("),
            (include_str!("../../ui/mobile.slint"), "I18n.tr("),
            (include_str!("../../ui/mobile/transfer.slint"), "I18n.tr("),
            (include_str!("controller.rs"), "self.text("),
        ] {
            for tail in source.split(call).skip(1) {
                if tail.starts_with('"') {
                    let literal = tail.split_once("\")").unwrap().0.to_owned() + "\"";
                    let key: String = serde_json::from_str(&literal).unwrap();
                    assert!(has_translation(&key), "missing interface key: {key}");
                }
            }
        }
    }

    #[test]
    fn mobile_settings_and_transfer_keys_use_the_shared_catalog() {
        for key in [
            "Computer Transfer",
            "Import and transfer music",
            "Refresh Library",
            "Refreshing library…",
            "Show Lyrics",
            "Show or hide song lyrics",
            "About nano",
            "Not available on iOS yet",
            "Settings could not be saved",
            "Library updated · {0} songs",
            "Choose a song",
            "Start Computer Transfer",
            "Stop Computer Transfer",
            "Copy Address",
            "Address copied",
            "Could not copy address",
        ] {
            assert!(has_translation(key), "missing mobile key: {key}");
            for language in [Language::SimplifiedChinese, Language::Japanese] {
                assert_ne!(translate(language, key), key);
            }
        }
    }

    #[test]
    fn mobile_notice_templates_preserve_counts_and_error_details() {
        assert_eq!(
            notice(
                Language::SimplifiedChinese,
                "Library updated · 12 songs; Music/My Song.wav"
            ),
            "曲库已更新 · 12 首歌曲; Music/My Song.wav",
        );
        assert_eq!(
            notice(Language::Japanese, "Imported 2 files; updating library…"),
            "2 個のファイルを読み込みました。ライブラリを更新中…",
        );
        assert_eq!(
            notice(
                Language::SimplifiedChinese,
                "Settings could not be saved: database is locked"
            ),
            "无法保存设置: database is locked",
        );
        assert_eq!(
            notice(Language::English, "Library updated · 12 songs"),
            "Library updated · 12 songs",
        );
        assert_eq!(
            notice(Language::Japanese, "Library updated · demo songs"),
            "Library updated · demo songs",
        );
    }
}
