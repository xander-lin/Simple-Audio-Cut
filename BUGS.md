# 已知缺陷清单（native-gui-iced 迁移）

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
| 9 | **没有拖拽入轨**：旧版可从素材架拖到时间线（`onDragStart`/`handleDrop`），新版只能点 `+`。属功能回归。 | `crates/app/src/ui.rs` | open |
| 10 | **`•••` 菜单定位写死**：`y: 140.0` 硬编码，素材架高度或窗口滚动后菜单位置漂移。 | `crates/app/src/ui.rs` `inspector` | open |
| 11 | **J/L 行为与旧版不同**：旧版按 L 即使未播放也会开始播放；新版仅移动播放头。需确认这是否是想要的行为（README 已按新版描述）。 | `crates/app/src/update.rs` `shortcut` | open（待定） |
| 12 | **`has_unsaved_work` 只看时间线**：旧版把素材架里未导出的素材也算作未保存；新版只看 tracks。关窗提示会不一致。 | `crates/app/src/model.rs` | **fixed**（2026-11-05） |
| 13 | **`ui.blade_mode` 是全局的**，切轨不重置，与旧版一致但容易误触；且折叠轨道上刀片被静默禁用（无提示）。 | `crates/app/src/update.rs` | open（体验） |

## 二、波形 / 画布

| # | 问题 | 位置 | 状态 |
| --- | --- | --- | --- |
| 14 | **画布宽度与布局宽度不一致**（右侧留白或裁切）：此前用窗口宽度估算 `canvas_width()`，实际行宽由 `responsive` 解算，两者不等。 | `crates/app/src/ui.rs` `track_row` | **fixed**（28e69f2） |
| 15 | **静音阈值线用的是实线**：旧版是虚线 `setLineDash([5,4])`，新版 `fill_rectangle` 画两条实线。视觉与语义都不同。 | `crates/app/src/waveform.rs` `draw_base` | **fixed**（2026-11-05） |
| 16 | **RMS 包络画法不同**：旧版用「每像素 bin 的最大/最小幅度」画上下包络 + 白色最小幅度线；新版按采样点查单帧 `amplitude`，只得到一条对称轮廓，丢掉最小幅度细节。 | `crates/app/src/waveform.rs` `draw_rms` | **fixed**（2026-11-05） |
| 17 | **包络 hover 提示位置不对**：`envelope_hover` 存的是行内局部坐标，绘制时却按整块画布坐标使用，跨行时标签会跑到别的行。 | `crates/app/src/waveform.rs` `draw_overlays` | **fixed**（2026-11-05） |
| 18 | **`envelope_hover` 在行循环内重复绘制**：`draw_overlays` 每行都画一次 hover 文本，多行时出现多份。 | 同上 | **fixed**（2026-11-05） |
| 19 | **编辑点/包络点命中半径与旧版不一致**：旧版包络点 14px、线 8px、编辑点无方向限制；新版编辑点仅水平 6px。 | `crates/app/src/waveform.rs` | open（体验） |
| 20 | **删除区点击恢复用 `contains`（含右边界）**，边界上双击语义含糊。 | `crates/app/src/waveform.rs` `hit_deleted` | **fixed**（2026-11-05） |
| 21 | **`playhead_label` 是死代码**，`format_time_precise` 仅在 UI 用。 | `crates/app/src/waveform.rs` | **fixed**（2026-11-05） |

## 三、导出

| # | 问题 | 位置 | 状态 |
| --- | --- | --- | --- |
| 22 | **导出文件名冲突会加 `-2` 后缀**，属预期；但**同一目录重复导出同名轨道会不断堆积**，旧版行为一致，需确认。 | `crates/core/src/audio/engine.rs` `reserve_export_path` | open（待定） |
| 23 | **导出失败后 `failed_exports` 只按 id 记，不清除已成功项**：`ExportCompleted` 里 `.failed_ids` 全量覆盖，逻辑正确；但 `start_export` 里 `retain` 与 `signatures` 混用，若签名列表为空会保留旧失败态。 | `crates/app/src/update.rs` | **fixed**（2026-11-05） |
| 24 | **并发导出被静默忽略**：`start_export` 在 `exporting` 非空时直接 `return`，用户没有任何反馈（旧版是按钮 disabled）。 | `crates/app/src/update.rs` | **fixed**（2026-11-05） |

## 四、设置 / 降噪

| # | 问题 | 位置 | 状态 |
| --- | --- | --- | --- |
| 25 | **`SettingsCheck` 会覆盖用户正在编辑的草稿**：`SettingsStatus` 里 `draft = status.configured`，清屏重建，光标位置也丢。 | `crates/app/src/update.rs` | **fixed**（2026-11-05） |
| 26 | **保存后自动关闭抽屉**：旧版保存后保留抽屉并刷新状态；新版 `settings_open = false`，用户看不到保存结果摘要。 | `crates/app/src/update.rs` `SettingsSaved` | **fixed**（2026-11-05） |
| 27 | **`pick_list` 选中值不在候选列表时显示空**：`high/low_sample_rate_model` 若不是已发现模型，下拉看起来"未选择"。 | `crates/app/src/ui.rs` `settings_overlay` | 已核实非缺陷（`selected` 值即使不在列表中也会直接显示） |
| 28 | **`busy` 期间遮罩点击是 `Noop`**，若后台任务异常不返回事件，抽屉永久卡住无法关闭。 | `crates/app/src/ui.rs` | **fixed**（2026-11-05） |
| 29 | **降噪状态徽标与 `denoise_note` 信息重复**，且 note 不随选中轨道变化。 | `crates/app/src/ui.rs` | open |

## 五、打包 / 运行环境

| # | 问题 | 位置 | 状态 |
| --- | --- | --- | --- |
| 30 | **`v0.2.0` 标签未打**：PKGBUILD 默认走 `git+$url.git#tag=v$pkgver`，当前仓库打不出这个包，只有用 `SIMPLE_AUDIO_CUT_SOURCE_URL` 才能构建。 | `packaging/arch/PKGBUILD` | open |
| 31 | **`gtk3` 可能多余**：rfd 0.16 默认走 xdg-portal（`ashpd`），只有选 `gtk3` feature 才需要 GTK3。依赖列表未验证精简。 | `packaging/arch/PKGBUILD` | open |
| 32 | **会话音频写在 `/tmp/simple-audio-cut-recordings-*`**，进程崩溃不清理；`Drop` 只在正常退出时删。旧版也是 temp，但迁移后没有恢复"上次会话"的能力。 | `crates/core/src/audio/native.rs` | open |
| 33 | **`makepkg` 打印 `libfakeroot internal error: payload not recognized!`**，包仍生成成功；未实际 `pacman -U` 安装验证过。 | 打包环境 | open（待验证） |
| 34 | **`PLAYBACK_BUG_ANALYSIS.md` 仍描述 WebKit 问题**，未更新或移除。 | 仓库根 | **fixed**（2026-11-05：已标注为历史文档） |
| 35 | **`src/`、`src-tauri/` 仍留在仓库**（约 1.1 MB），未决定是否删除。 | 仓库根 | open（待决定） |

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
