# Simple Audio Cut — 项目定义与工作原则

## 项目身份与定位

Simple Audio Cut 是一个面向录音口播的本地桌面剪辑工具，使用 **iced（Rust 原生 GUI）** 构建。
目标用户是不追求复杂混音、但希望快速完成录音、降噪、去静音、删错句和导出的口播创作者
（播客、旁白、课程朗读）。

- 编辑器 UI：`crates/app/`（iced 0.14 + wgpu，纯 Rust，无 WebView）。
- 音频与编辑模型：`crates/core/`（与 GUI 无关，可被任意前端复用）。
- 降噪：可选外接 ClearVoice Python 环境（`tools/clearvoice_denoise.py` worker），
  程序本身不安装、不下载、不修改用户的 Python 环境和模型。

### 迁移背景（2026-10-29）

旧版为 Tauri 2 + React + WebKitGTK。WebKit 的音频链路（GStreamer 解码
`decodeAudioData`、WebAudio 播放）出现无法从 core 定位的故障：
`AudioContext.resume()` 可能长时间不结算，播放静默卡死，且系统缺 GStreamer
插件时导入直接解码失败。因此迁移到 Rust 原生 GUI，把解码、播放、渲染全部
收回进程内：

- 解码：Rust `hound` + FFmpeg（导入先归一化成 24-bit WAV）。
- 播放：CPAL 输出流（`crates/core/src/audio/player.rs`），不再有 WebAudio。
- 波形：iced canvas 自绘（`crates/app/src/waveform.rs`），不再有 Canvas2D/DOM。

迁移分支保留了旧版 `src/`（React）与 `src-tauri/`（Tauri 壳），仅用于对比，
不参与 workspace 构建与打包；确认新实现完整后可在后续提交中移除。

## 产品哲学

- 操作直接、反馈及时、口播剪辑效率优先；不做多轨混音/音乐制作/母带工作站。
- 音频不离开本机；编辑全部非破坏性（只存时间区间，不改写原始素材）。
- 未配置降噪环境时核心功能（录音/导入/编辑/导出）必须完全可用（显示 `No denoise`）。

## 功能范围（完成标准）

录音、导入（WAV/MP3/M4A/AAC/FLAC/OGG/Opus/AIFF/视频抽轨）、可调 LUFS 归一化
（默认 -14）、可选 ClearVoice 降噪 + 毛刺滤除、多轨独立编辑（删除/静音/刀片/音量包络）、
RMS 静音检测、折叠已删除区、24-bit WAV 增量导出（`Export changed` 只导有变化的轨道）。
详细操作说明以 `README.md` 为准。

## 架构与可插拔约定

每个外部依赖都在 `crates/app/src/services.rs` 中以 trait 暴露，并提供真实实现与 mock：

| 接口 | 生产实现 | Mock |
| --- | --- | --- |
| `AudioEngine`（录音/归一化/降噪/导出） | `NativeAudioEngine` | `MockAudioEngine` |
| `AudioPlayer`（播放输出） | `CpalAudioPlayer` | `MockAudioPlayer` |
| `DenoiseProvider`（降噪提供方） | `ClearVoiceProvider` / `DisabledDenoiseProvider` | `MockDenoiseProvider` |
| `DialogService`（文件对话框） | `RfdDialogService` | `MockDialogService` |
| `ExportDirectoryStore`（导出目录记忆） | `FileExportDirectoryStore` | `MemoryExportDirectoryStore` |
| `EventSink`（后台任务事件） | `EventBus` | `RecordingEventSink` |

- 状态只在 `crates/app/src/update.rs` 的 `App::update` 中变更；后台线程只通过
  `EventSink` 投递 `AppEvent`，UI 线程在 `Tick` 中消费。
- 视图（`ui.rs`、`waveform.rs`）是纯函数，不持有可变全局状态。
- 构建可通过 feature 关闭原生音频：`--no-default-features` 得到纯 mock 运行版。

## 构建与验证命令

```bash
cargo run -p simple-audio-cut-iced          # 开发版
cargo test                                  # 全部测试（core + app）
cargo test -p simple-audio-cut-core         # 编辑模型、分析、音频引擎
cargo clippy --workspace --all-targets -- -D warnings
cargo check -p simple-audio-cut-iced --no-default-features
cargo run -p simple-audio-cut-iced --example seed_preview   # 合成素材界面预览
```

`crates/core/tests/native_pipeline.rs` 跑真实 FFmpeg 归一化 + 解码 + CPAL 播放；
没有输出设备时会跳过播放断言。

## 打包与安装（Arch Linux，唯一目标平台）

- 源码打包位于 `packaging/arch/PKGBUILD`，只面向 Arch Linux，不生成其他平台安装包。
- 默认从 `git+$url.git#tag=v$pkgver` 构建；设置环境变量
  `SIMPLE_AUDIO_CUT_SOURCE_URL=<tarball-url>` 时改用本地源码包
  （tarball 顶层目录须为 `Simple-Audio-Cut-$pkgver/`）。
- 本地源码包命名约定：`simple-audio-cut-$pkgver-local-rNN.tar.gz`，
  源码有新修订时递增 NN，并同步更新 PKGBUILD 中引用的 rNN 与 `pkgrel`。
- 构建：在 `packaging/arch/` 下 `makepkg`；安装：`sudo pacman -U ./<pkg>.tar.zst`。
- makedepends：`cargo git pkgconf`；depends：`alsa-lib ffmpeg gtk3 libxkbcommon
  vulkan-icd-loader wayland`。
- 修改源码后需要发版时：升 `crates/app/Cargo.toml`、`crates/core/Cargo.toml` 与
  PKGBUILD 的 `pkgver`，打 `v$pkgver` 标签。

## 项目原则

1. **变更前后必须跑验证**：至少 `cargo test` 与 `cargo clippy --workspace
   --all-targets -- -D warnings`；UI/音频改动额外跑真实链路测试与
   `--no-default-features` 检查。不得删除或绕过测试来制造成功。
2. **架构可插拔**：新增外部依赖必须先进 `services.rs` 的 trait，并同时提供 mock；
   禁止在 `update.rs` 或视图里直接调用系统 API。
3. **打包即交付形态**：运行时改动需同步检查 `PKGBUILD` 的 `package()` 是否覆盖
   新增的安装产物（bin、desktop、icons、worker、doc、license）。
4. **用户数据不可破坏**：不写用户 Python 环境/模型；原始音频文件永不改写。
5. **版本一致性**：`crates/app/Cargo.toml`、`crates/core/Cargo.toml`、PKGBUILD 的
   `pkgver` 三处保持同步。
6. **无 WebKit 依赖**：新代码不得引入 webview；导入/播放/渲染必须留在 Rust 进程内。
