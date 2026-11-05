# 已知缺陷清单（iced 版本）

审计方式：逐行对照旧版 `src/`（React）实现、阅读 iced 版 `crates/app`、并用
`seed_preview` 实机截图 + 集成测试复核。以下全部是我在代码里确认过的问题，
按严重程度分组。状态列：`open` 未修，`fixed` 已修。

---

## 一、功能性缺陷（会让操作无效或结果错误）

| # | 问题 | 位置 | 状态 |
| --- | --- | --- | --- |
| 1 | **Esc 无效**：旧版 Esc 关闭轨道右键菜单、关闭设置抽屉；新版完全没处理 Esc，只能点遮罩。 | `crates/app/src/app.rs` `keyboard_shortcut` | **fixed**（2026-11-05） |
| 2 | **后台状态常驻不清**：`Session::denoise_note` 一旦写入（如 "take-1: denoising complete."）就永远显示在状态栏，越积越怪。 | `crates/app/src/update.rs` `apply_event` | **fixed**（2026-11-05） |
| 3 | **重命名时反复重建折叠缓冲**：`Message::RenameMedia` 每敲一个字符都调用 `track.touch()`，而 `touch()` 在折叠状态下会 `rebuild_collapsed()`（重算整段音频 + RMS）。长音频卡顿。 | `crates/app/src/update.rs` `RenameMedia` | **fixed**（2026-11-05） |
| 4 | **`RenameMedia` 无法改成空名/无法取消**：输入框每字符即提交，中途清空会把名字变成空串，旧版是 `onBlur` 才提交且拒绝空值。 | 同上 | **fixed**（2026-11-05） |
| 5 | **删除正在录制的占位素材后仍提示成功**：`RecordingStopped` 的 Ok 分支无条件设置 "Recording is ready in the media pool."，即使该 clip 已被删除（`apply_decoded` 静默 no-op）。 | `crates/app/src/update.rs` | **fixed**（2026-11-05） |
| 6 | **导入完成/失败消息使用空名字**：clip 被删除时 `clip_by_id` 返回 None，消息变成 "` is ready in the media pool.`"。 | 同上 | **fixed**（2026-11-05） |
| 7 | **导出刚开始时的签名可能过期**：`start_export` 在开始时抓 `signature`；若导出期间降噪完成导致音频被替换，导出成功后仍按旧签名标记"已导出"，实际文件与内容不符。 | `crates/app/src/update.rs` `start_export`/`ExportCompleted` | **fixed**（2026-11-05） |
| 8 | **`pending_export` 被塞空 edits**：`export_to_folder` 为了保留 `single` 而填入 `edits: Vec::new()`，是误导性结构，容易在后续改动中读到空 edits。应拆成独立字段。 | `crates/app/src/update.rs` `export_to_folder` | **fixed**（2026-11-05） |
| 9 | **没有拖拽入轨**：旧版可从素材架拖到时间线（`onDragStart`/`handleDrop`），新版只能点 `+`。属功能回归。 | `crates/app/src/ui.rs` | **won't fix**（产品决定：改为显式 `+` / `Enter`，理由见 DESIGN.md） |
| 10 | **`•••` 菜单定位写死**：`y: 140.0` 硬编码，素材架高度或窗口滚动后菜单位置漂移。 | `crates/app/src/ui.rs` `inspector` | **fixed**（2026-11-05：改为检查器工具栏锚点，不再依赖固定 140px） |
| 11 | **J/L 行为与旧版不同**：旧版按 L 即使未播放也会开始播放；新版仅移动播放头。需确认这是否是想要的行为（README 已按新版描述）。 | `crates/app/src/update.rs` `shortcut` | open（待定） |
| 12 | **`has_unsaved_work` 只看时间线**：旧版把素材架里未导出的素材也算作未保存；新版只看 tracks。关窗提示会不一致。 | `crates/app/src/model.rs` | **fixed**（2026-11-05） |
| 13 | **`ui.blade_mode` 是全局的**，切轨不重置，与旧版一致但容易误触；且折叠轨道上刀片被静默禁用（无提示）。 | `crates/app/src/update.rs` | 部分修复（折叠且刀片模式时检查器会显示提示；刀片仍为全局） |

## 二、波形 / 画布

| # | 问题 | 位置 | 状态 |
| --- | --- | --- | --- |
| 14 | **画布宽度与布局宽度不一致**（右侧留白或裁切）：此前用窗口宽度估算 `canvas_width()`，实际行宽由 `responsive` 解算，两者不等。 | `crates/app/src/ui.rs` `track_row` | **fixed**（28e69f2） |
| 15 | **静音阈值线用的是实线**：旧版是虚线 `setLineDash([5,4])`，新版 `fill_rectangle` 画两条实线。视觉与语义都不同。 | `crates/app/src/waveform.rs` `draw_base` | **fixed**（2026-11-05） |
| 16 | **RMS 包络画法不同**：旧版用「每像素 bin 的最大/最小幅度」画上下包络 + 白色最小幅度线；新版按采样点查单帧 `amplitude`，只得到一条对称轮廓，丢掉最小幅度细节。 | `crates/app/src/waveform.rs` `draw_rms` | **fixed**（2026-11-05） |
| 17 | **包络 hover 提示位置不对**：`envelope_hover` 存的是行内局部坐标，绘制时却按整块画布坐标使用，跨行时标签会跑到别的行。 | `crates/app/src/waveform.rs` `draw_overlays` | **fixed**（2026-11-05） |
| 18 | **`envelope_hover` 在行循环内重复绘制**：`draw_overlays` 每行都画一次 hover 文本，多行时出现多份。 | 同上 | **fixed**（2026-11-05） |
| 19 | **编辑点/包络点命中半径与旧版不一致**：旧版包络点 14px、线 8px、编辑点无方向限制；新版编辑点仅水平 6px。 | `crates/app/src/waveform.rs` | **fixed**（2026-11-05：包络点 14px、曲线 8px、编辑点 6px 与旧版一致，并改为在波形区内判定） |
| 20 | **删除区点击恢复用 `contains`（含右边界）**，边界上双击语义含糊。 | `crates/app/src/waveform.rs` `hit_deleted` | **fixed**（2026-11-05） |
| 21 | **`playhead_label` 是死代码**，`format_time_precise` 仅在 UI 用。 | `crates/app/src/waveform.rs` | **fixed**（2026-11-05） |

## 三、导出

| # | 问题 | 位置 | 状态 |
| --- | --- | --- | --- |
| 22 | **导出文件名冲突会加 `-2` 后缀**，属预期；但**同一目录重复导出同名轨道会不断堆积**，旧版行为一致，需确认。 | `crates/core/src/audio/engine.rs` `reserve_export_path` | **won't fix**（保留 `-2` 后缀，避免静默覆盖用户文件） |
| 23 | **导出失败后 `failed_exports` 只按 id 记，不清除已成功项**：`ExportCompleted` 里 `.failed_ids` 全量覆盖，逻辑正确；但 `start_export` 里 `retain` 与 `signatures` 混用，若签名列表为空会保留旧失败态。 | `crates/app/src/update.rs` | **fixed**（2026-11-05） |
| 24 | **并发导出被静默忽略**：`start_export` 在 `exporting` 非空时直接 `return`，用户没有任何反馈（旧版是按钮 disabled）。 | `crates/app/src/update.rs` | **fixed**（2026-11-05） |

## 四、设置 / 降噪

| # | 问题 | 位置 | 状态 |
| --- | --- | --- | --- |
| 25 | **`SettingsCheck` 会覆盖用户正在编辑的草稿**：`SettingsStatus` 里 `draft = status.configured`，清屏重建，光标位置也丢。 | `crates/app/src/update.rs` | **fixed**（2026-11-05） |
| 26 | **保存后自动关闭抽屉**：旧版保存后保留抽屉并刷新状态；新版 `settings_open = false`，用户看不到保存结果摘要。 | `crates/app/src/update.rs` `SettingsSaved` | **fixed**（2026-11-05） |
| 27 | **`pick_list` 选中值不在候选列表时显示空**：`high/low_sample_rate_model` 若不是已发现模型，下拉看起来"未选择"。 | `crates/app/src/ui.rs` `settings_overlay` | 已核实非缺陷（`selected` 值即使不在列表中也会直接显示） |
| 28 | **`busy` 期间遮罩点击是 `Noop`**，若后台任务异常不返回事件，抽屉永久卡住无法关闭。 | `crates/app/src/ui.rs` | **fixed**（2026-11-05） |
| 29 | **降噪状态徽标与 `denoise_note` 信息重复**，且 note 不随选中轨道变化。 | `crates/app/src/ui.rs` | **fixed**（2026-11-05：`denoise_note` 只出现在设置抽屉，轨道行显示状态徽标） |

## 五、打包 / 运行环境

| # | 问题 | 位置 | 状态 |
| --- | --- | --- | --- |
| 30 | **`v0.2.0` 标签未打**：PKGBUILD 默认走 `git+$url.git#tag=v$pkgver`，当前仓库打不出这个包，只有用 `SIMPLE_AUDIO_CUT_SOURCE_URL` 才能构建。 | `packaging/arch/PKGBUILD` | open |
| 31 | **`gtk3` 可能多余**：rfd 0.16 默认走 xdg-portal（`ashpd`），只有选 `gtk3` feature 才需要 GTK3。依赖列表未验证精简。 | `packaging/arch/PKGBUILD` | open |
| 32 | **会话音频写在 `/tmp/simple-audio-cut-recordings-*`**，进程崩溃不清理；`Drop` 只在正常退出时删。旧版也是 temp，但迁移后没有恢复"上次会话"的能力。 | `crates/core/src/audio/native.rs` | open |
| 33 | **`makepkg` 打印 `libfakeroot internal error: payload not recognized!`**，包仍生成成功；未实际 `pacman -U` 安装验证过。 | 打包环境 | open（待验证） |
| 34 | **`PLAYBACK_BUG_ANALYSIS.md` 仍描述 WebKit 问题**，未更新或移除。 | 仓库根 | **fixed**（2026-11-05：已标注为历史文档） |
| 35 | **`src/`、`src-tauri/` 仍留在仓库**（约 1.1 MB）。`main` 已是 iced 版本，这两个目录只剩历史参考价值；`src-tauri` 被 workspace 排除，不再构建或打包。 | 仓库根 | open（保留作对照；删除需显式确认） |

## 六、测试覆盖缺口

- 没有覆盖 **折叠 + 播放 + 编辑点** 组合的端到端测试。
- `workflow.rs` 里的导出用例依赖 mock 引擎的线程时序，曾出现偶发失败（已通过把后台状态移出 `ui.message` 消除，但根因是共享可变状态，仍应改成确定性事件注入）。
- 没有任何 **UI 快照/布局** 测试，画布宽度这类问题只能靠肉眼截图发现（#14 就是这样漏掉的）。
- 输入框聚焦时按 Esc 的行为没有组件级测试（`app.rs` 只测了 Esc 的映射）。
- RMS 每像素 bin 的取帧逻辑已有单元测试，但虚线阈值、hover 定位等绘制结果仍只能靠肉眼确认。

### 2026-11-05 新增覆盖

- `app::keyboard_shortcut`：Esc 映射。
- `update`：Esc 关闭设置/菜单、重命名提交与取消、空名保护、折叠缓冲不被重建、删除后录制/导入消息、导出签名过期、失败集合合并、并发导出提示、设置检查不覆盖草稿、保存后不关抽屉。
- `waveform`：RMS 列的 min/max 分箱、右边界半开、空输入。
- `model`：素材架中的 ready 素材计入未保存工作。

---

## 已知的行为差异（不一定是 bug，需要拍板）

1. 拖拽入轨缺失（#9）。
2. J/L 语义（#11）。
3. 关闭窗口时"未保存"的判定范围（#12）。
4. 导出同名文件是否覆盖而非加后缀（#22）。
5. 折叠视图下不允许右拖删除/选区（旧版也是，但新版没有任何提示文字）。

---

## 2026-11-05 修复批次

本轮修复 #1–#8、#12、#15–#18、#20、#21、#23–#26、#28，并把 #27 核实为非缺陷、#34 标注为历史文档。

验证：

```text
cargo test --workspace
  core lib:        57 passed, 1 ignored (real ClearVoice model test, by design)
  core integration: 2 passed
  app unit:        39 passed
  app workflow:     8 passed
  real audio:       1 passed

cargo clippy --workspace --all-targets --all-features -- -D warnings   passed
cargo check --workspace --no-default-features                          passed
```

未做：本轮没有重新运行 `seed_preview` 截图（当前环境无法在不截取整个桌面的前提下定位 iced 窗口），
波形绘制改动由单元测试和代码审查确认；如需视觉复核，请在有桌面的环境运行
`cargo run -p simple-audio-cut-iced --example seed_preview`。

---

## 2026-11-05 界面重做（产品 + 视觉）

本轮把界面按 `DESIGN.md` 重做，并补齐随之暴露的交互缺陷：

| 变更 | 位置 |
| --- | --- |
| 素材栏从顶部横条改为左侧固定栏（窄窗口降级为顶部横条） | `ui.rs` `library_sidebar` / `library_strip` |
| 新增右侧检查器面板（源信息 / 静音 / 选区 / 包络 / 导出 / 轨道操作） | `ui.rs` `inspector_panel` |
| 走带条独立成时间线顶部一条（播放 / 停止 / 时间码 / 缩放 / 导出） | `ui.rs` `transport` |
| 三档响应式断点 1280 / 940 | `ui.rs` `body` |
| 波形每行新增 17px 时间刻度、刻度线、悬停时间读数，点击刻度只定位不选区 | `waveform.rs` |
| 新轨道默认 "fit" 缩放，手动缩放后才锁定比例 | `model.rs` `effective_pixels_per_second` |
| `Delete` / `Backspace` 剪掉选中范围（时长变短），`Home`/`End` 跳转，`Enter` 入轨，`[`/`]` 缩放 | `app.rs` / `update.rs` |
| 选区改为青色边界 + 刻度条色带，在已删除区之上也清晰可读 | `waveform.rs` |
| 已删除区改用斜线阴影 + `↺` 标记，不再在每处写 "Restore" 文字 | `waveform.rs` |
| 右键菜单去掉全屏变暗，只保留点击外部关闭 | `ui.rs` `context_menu_overlay` |
| 主题重做：石墨灰 + 琥珀走带 + 青绿选中，新增对比度单元测试 | `theme.rs` |
| `space()` 全部替换为真正可伸展的 `spacer()`（原实现是 Shrink，导致整行文字粘连） | `ui.rs` |

实机核对（`seed_preview` + X11 截图）：1400×940 / 1000×800 / 860×700 三种布局、
设置抽屉、右键菜单、选区状态、空会话均逐张确认。

验证：

```text
cargo test --workspace                    全部通过（app 48 unit + 9 workflow + 1 real + 6 ui_view）
cargo clippy --workspace --all-targets --all-features -- -D warnings   passed
cargo check --workspace --no-default-features                          passed
```

---

## 2026-11-05 布局与降噪的可选性

### 布局按显示器而不是固定宽度切换

旧实现用 1280 / 940 两个固定像素断点。在 4K 显示器上分屏后窗口仍有 ~1900px，
会被误判为"宽屏"而挤进三栏。现在比较**窗口宽度与所在显示器宽度**：
≥ 72% 视为全屏（左右三栏），否则堆叠（素材条在上、时间线在下）。
显示器尺寸在启动时通过 `iced::window::monitor_size` 取得，窗口尺寸变化即时重算。
堆叠布局缺少右侧检查器，因此 `•••` 菜单补齐了降噪、清空包络、折叠、强制导出、
移回素材库、删除轨道这些检查器操作。

### 降噪可选

| 场景 | 行为 |
| --- | --- |
| 未配置降噪环境 / 构建未包含 provider | 素材显示 `No denoise`，其余功能完全可用 |
| 检查未通过 | 素材保留归一化原始音频，检查器显示失败原因 |
| 全局自动降噪关闭 | 新素材不自动降噪，标记 `Original audio`，可手动开启 |
| 单条素材选择不降噪 | `Use original audio`；已降噪的轨道会切回归一化原始音频 |
| 已降噪后重新开启 | 从归一化原始音频重新排队，剪辑保留 |

实现要点：

- `DenoiseConfig.enabled` 的语义从"是否允许降噪"改为"是否自动处理新素材"；
  `prepare()` 不再因 `enabled=false` 拒绝显式请求。
- 每条素材新增 `DenoiseStatus::Skipped`（`Original audio`）与 `denoise.reason`，
  并在 `MediaClip.normalized_path` 保留归一化源文件路径，以便随时切回。
- `DenoiseState::skipped()` 不会被自动队列或设置保存后的重扫覆盖。
- 设备列表固定包含 `Automatic`、`CPU`、`GPU (default CUDA device)`，再追加探测到的
  每块 GPU（`cuda:N`）；`gpu` 作为 `cuda` 的别名。

新增测试：

- core：`fit_pixels_per_second` 在所有常见宽度/时长下都能单行放下；
  `normalize_device` 的 CPU/GPU 别名；设备列表始终提供 CPU 与 GPU。
- app：单条素材关闭降噪后仍可编辑/导出；opt-out 不被自动重扫覆盖；
  重新开启会重新排队；provider 不可用时记录原因且不阻塞导出；
  已降噪素材切回原始音频。
- workflow：全局关闭自动降噪时导入→编辑→播放→导出整条链路可用。

---

## 2026-11-05 左键默认静音 + 测试与可维护性

### 交互变更

- **左键拖动波形 = 静音**：拖动时预览为琥珀色，松手后立即把区间替换为等长静音，
  选区保留；再次左键拖动同一段（或按 `M`）恢复声音。
- **右键拖动保持原样**：向右删除、向左恢复（红色/绿色预览）。
- 折叠视图下两种手势都会给出"Show all edits"提示，而不是静默无反应。

### 修复的缺陷

| 问题 | 位置 | 状态 |
| --- | --- | --- |
| **`Check setup` 会用未保存的草稿覆盖运行中的 provider 状态**：之后新素材会按草稿（可能不可用）处理，而不是已保存配置。 | `update.rs` `SettingsInspected` | **fixed** |
| **恢复原始音频后丢失 `opted_out`**：自动降噪开启时会把用户刚恢复的音频再次降噪。 | `update.rs` `OriginalAudioRestored` | **fixed** |
| **自动降噪关闭时 `Use original audio` 不记录用户选择**：`Skipped` 提前返回，`opted_out` 永远不置位，后续策略可能重新降噪。 | `update.rs` `set_denoise` | **fixed** |
| **按 `M` 静音后选区被清空**：README 承诺"再按一次恢复"实际做不到，必须重新拖选。 | `update.rs` `toggle_mute_range` | **fixed** |
| **折叠轨道上右键拖动完全无反馈**：手势被静默忽略。 | `update.rs` `waveform_event` | **fixed** |
| **左键拖动预览使用删除的红色**：非破坏性选区看起来像要删除音频。 | `waveform.rs` | **fixed** |
| **`select_track` 与 `select_track_for_interaction` 完全重复** | `update.rs` | **fixed**（合并） |
| **降噪失败后没有重试入口**：检查器按钮显示 `Use original audio`，点了只会关闭降噪；失败素材无法重试。 | `ui.rs` | **fixed**（`Failed` → `Retry denoising`） |
| **已知不可用的 provider 仍会被策略反复派发任务**：每次打开/保存设置都重新排队并立即失败。 | `update.rs` `apply_denoise_policy` | **fixed**（`ready` 为假时只记录具体阻塞项，不派发） |
| **失败任务的阻塞原因只有笼统 summary**：用户看不到到底是 Python、模型还是设备的问题。 | `denoise/mod.rs` `blocking_reason` | **fixed**（取第一条 Error 检查的标签与详情） |

### 测试与可维护性

- 新增 `JobRunner` 接口：生产实现 `ThreadJobRunner`，测试默认 `InlineJobRunner`。
  后台任务在消息处理内同步完成，**workflow 测试不再有 `sleep`、超时和轮询**。
- `Tick` 一次排空整条事件链（对话框 → 导出 → 完成），不再每帧只处理一批事件。
- `MockAudioEngine` 的录音/降噪改为同步且产出真实可解码 WAV；
  `MockDenoiseProvider` 的输出改到临时目录，不在用户素材旁写文件。
- `update.rs` / `ui.rs` / `waveform.rs` 的测试移到各自的 `tests.rs` 子模块。
- 新增波形手势测试：左键静音、已静音时恢复预览、右键删除/恢复、刻度点击、
  点击画布外忽略。
- 新增状态测试：显式降噪失败后保留原音频、显式降噪→恢复原音的往返、
  折叠轨道提示、`Check setup` 不改运行状态、自动关闭时 `Use original audio`
  仍被记录。
- `native_pipeline` 用 `mpsc::recv_timeout` 等待真实导入回调，替代 50ms 轮询。

### 验证

```text
cargo test --workspace
  core lib:         66 passed, 1 ignored
  core integration:  2 passed
  app unit:         79 passed
  real audio:        1 passed
  ui_view:           6 passed
  workflow:         15 passed
cargo clippy --workspace --all-targets --all-features -- -D warnings   passed
cargo check --workspace --no-default-features                          passed
```
