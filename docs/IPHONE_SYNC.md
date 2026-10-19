# 电脑资料同步到 iPhone

当前提供单向、合并式 USB 同步。电脑端导出可移植资料包，手机使用共享 Rust
库校验并导入。音乐内容无需经过 Wi-Fi、账号或云服务，适合局域网设备互访受限的场景。

## 同步范围与冲突规则

- 音频及封面、歌曲信息、歌单名称和顺序。
- 普通文本/LRC 歌词、显式移除歌词的选择、罗马音及已缓存的翻译。
- 评分、播放统计、播放历史。
- 手机原有的独立音乐与歌单不会删除，语言、窗口偏好、桌面快捷键和 API 密钥不随包迁移。

同一歌曲已有不同评分、歌词、罗马音或相同来源译文时，保留手机版本并在回执
中记录冲突。歌单保存上一次同步快照；如果手机改过名称或顺序，就保留其编辑，
跳过电脑对此歌单的更新。独立的同名歌单会使用不同名称保存。

播放历史按来源资料库及原始事件 ID 去重。播放次数只累加电脑相对上次已同步
高水位的增量，因此重复同步不会重复计数，也不会减掉手机新增的播放次数。
这不是双向同步：手机新增资料目前不会回传到电脑。

## 使用

### 桌面交互界面

在桌面版的 **File → Sync to iPhone…** 打开独立设备窗口。USB 连接并解锁 iPhone
后点 **Refresh Devices**，左侧选择手机。**Summary** 显示电脑曲库数量，**Music**
可以选择整个曲库或指定歌单，并选择是否带上歌词、翻译、评分及播放历史。

先点 **Preview Sync** 检查选择，再点 **Sync** 才会实际传输。窗口显示导出、备份、
复制、手机导入及最终回执；传输时仍可隐藏窗口。提交手机导入前可以取消，提交后
等待手机完成事务。失败可刷新、预览后重试，界面不会把复制完成当成导入成功。

也可仅打开设备窗口，不启动桌面音频播放器：

```sh
nano-music-player --iphone-sync
# 指定另一份桌面资料目录
nano-music-player --config-dir /path/to/config --iphone-sync
```

### 命令行

Mac 需要安装 Xcode 命令行工具，iPhone 通过 USB 连接并信任电脑，手机安装
支持 DesktopSync 的版本。默认桌面资料目录由脚本自动选取；也可用
`--config-dir` 指定。

```sh
# 导出并查看数量，不修改手机
python3 scripts/sync_iphone.py --output target/iphone-sync/my-library

# 备份手机资料、通过 USB 复制，最后提交导入
python3 scripts/sync_iphone.py \
  --package target/iphone-sync/my-library \
  --device <iPhone-UDID> --apply
```

同步会重新启动手机播放器，以便所有资料服务在导入完成后读取同一份数据。
导入期间请保持播放器在前台；界面会显示正在准备资料库。

脚本先用 SQLite backup 获取桌面库一致快照，原始音频只读。导出包使用稳定的
来源/歌曲 ID，包含独立的音频 SHA256；歌曲路径在手机上重建，不使用 Mac
绝对路径。封面按二进制保存，避免在 JSON 中重复膨胀。

只有 USB 文件复制完成后才写入含本次 export_id 的 `READY`。手机核对完整性，
暂存音频、使用数据库事务合并，再生成 `receipt.json` 并将标记改为 `APPLIED`。
复制失败时未提交的包不会导入；校验/导入失败保留可重试包并生成 `report.json`。

手机同步前的数据库、翻译缓存和偏好保存在导出目录旁边的
`<目录名>-phone-backup/phone-support`。完成回执也在备份目录中，包含数量、
冲突与警告。备份可能包含手机本来已有的私有配置，应留在本机。

## 验证

Python 导出/传输编排测试位于 `scripts/tests/test_iphone_sync.py`；共享导入测试
覆盖重复同步、手机编辑保护、统计增量、容器迁移、路径/哈希/READY 校验及发布失败回滚。
也可以将包放到本地 `Documents/DesktopSync/<source_id>`，写入正确的 READY 后
运行同一导入实现：

```sh
cargo run --locked --features mobile --example desktop-sync -- /path/to/Documents
```

USB 网页上传是另一种只传音频的方式，见 [电脑传歌](COMPUTER_TRANSFER.md)。
