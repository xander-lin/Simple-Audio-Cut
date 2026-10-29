# Simple Audio Cut — 项目定义与工作原则

## 项目身份与定位

Simple Audio Cut 是一个面向录音口播的本地桌面剪辑工具（Tauri 2 + React + Rust + FFmpeg）。
目标用户是不追求复杂混音、但希望快速完成录音、降噪、去静音、删错句和导出的口播创作者
（播客、旁白、课程朗读）。

- 主进程：`src-tauri/`（Rust），负责原生录音、响度归一化、FFmpeg 转换与导出。
- 前端：`src/`（React + TypeScript + Vite），编辑工作台 UI。
- 降噪：可选外接 ClearVoice Python 环境（`tools/clearvoice_denoise.py` worker），
  程序本身不安装、不下载、不修改用户的 Python 环境和模型。

## 产品哲学

- 操作直接、反馈及时、口播剪辑效率优先；不做多轨混音/音乐制作/母带工作站。
- 音频不离开本机；编辑全部非破坏性（只存时间区间，不改写原始素材）。
- 未配置降噪环境时核心功能（录音/导入/编辑/导出）必须完全可用（显示 `No denoise`）。

## 功能范围（完成标准）

录音、导入（WAV/MP3/M4A/AAC/FLAC/OGG/Opus/AIFF/视频抽轨）、可调 LUFS 归一化
（默认 -14）、可选 ClearVoice 降噪 + 毛刺滤除、多轨独立编辑（删除/静音/刀片/音量包络）、
RMS 静音检测、折叠已删除区、24-bit WAV 增量导出（`Export changed` 只导有变化的轨道）。
详细操作说明以 `README.md` 为准。

## 构建与验证命令

```bash
pnpm tauri dev                 # 开发版
pnpm test                      # 前端/node 测试（vitest + node:test）
pnpm build                     # tsc + vite build
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features
```

## 打包与安装（Arch Linux，唯一目标平台）

- 源码打包位于 `packaging/arch/PKGBUILD`，只面向 Arch Linux，不生成其他平台安装包。
- 默认从 `git+$url.git#tag=v$pkgver` 构建；设置环境变量
  `SIMPLE_AUDIO_CUT_SOURCE_URL=<tarball-url>` 时改用本地源码包
  （tarball 顶层目录须为 `Simple-Audio-Cut-$pkgver/`）。
- 本地源码包命名约定：`simple-audio-cut-$pkgver-local-rNN.tar.gz`，
  源码有新修订时递增 NN，并同步更新 PKGBUILD 中引用的 rNN 与 `pkgrel`。
- 构建：在 `packaging/arch/` 下 `makepkg`；安装：`sudo pacman -U ./<pkg>.tar.zst`。
- makedepends：`cargo git nodejs pnpm pkgconf`；depends：`alsa-lib ffmpeg gtk3 webkit2gtk-4.1`。
- 修改源码后需要发版时：升 `package.json` 与 `src-tauri/tauri.conf.json` 的版本、
  打 `v$pkgver` 标签。

## 项目原则

1. **变更前后必须跑验证**：至少 `pnpm test` 与 `cargo test`；UI/Rust 改动额外跑
   clippy 与 `pnpm build`。不得删除或绕过测试来制造通过。
2. **架构可插拔**：降噪提供方通过 worker 接口对接（可换成自定义 worker：
   `SIMPLE_AUDIO_CUT_CLEARVOICE_WORKER`），主程序不硬绑 ClearVoice 实现。
3. **打包即交付形态**：运行时改动需同步检查 `PKGBUILD` 的 `package()` 是否覆盖
   新增的安装产物（bin、desktop、icons、worker、doc、license）。
4. **用户数据不可破坏**：不写用户 Python 环境/模型；原始音频文件永不改写。
5. **版本一致性**：`package.json`、`tauri.conf.json`、PKGBUILD 的 `pkgver` 三处保持同步。
