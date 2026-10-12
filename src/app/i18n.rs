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
        "Preview · recommendations coming next",
        "预览 · 推荐功能即将推出",
        "プレビュー · おすすめ機能は近日公開",
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
        "Import failed: ",
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
}
