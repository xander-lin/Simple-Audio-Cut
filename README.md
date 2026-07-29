# Simple Audio Cut

Simple Audio Cut 是一个面向录音口播的本地桌面剪辑工具。它适合不追求复杂混音和逐帧精修、但希望快速完成录音、降噪、去静音、删错句和导出的场景。

程序采用 Tauri 2、React 和 Rust 构建。录音、响度归一化、媒体转换与导出由本机 Rust/FFmpeg 链路完成；音频不会上传到网络。新素材会先按可调目标响度归一化（默认 `-15 LUFS`），再在后台使用 ClearVoice 降噪，最后去除短促尖锐的毛刺噪声，不阻塞继续录音和编辑。

## 适用场景

- 播客、视频旁白、课程和文章朗读。
- 快速删除停顿、口误、重录段和无效内容。
- 多条口播录音分别剪辑，再独立导出 WAV。

Simple Audio Cut 不是多轨混音、音乐制作或母带处理工作站。它优先保证操作直接、反馈及时和口播剪辑效率。

## 操作方法

### 1. 获取素材

- 点击 `Record` 开始录音；录音时长直接显示在按钮内，再次点击停止。
- 点击 `Import` 导入 WAV、MP3、M4A、AAC、FLAC、OGG、Opus 或 AIFF。
- 在顶部 `LUFS` 按钮上滚动鼠标滚轮，可调节之后录音和导入素材的响度归一化目标；范围为 `-70` 到 `-5 LUFS`，默认是 `-15 LUFS`。
- 录音优先使用设备支持的 `48 kHz`，不支持时回退到兼容的最高/默认规格。
- 素材按目标响度归一化、ClearVoice 降噪、毛刺噪声滤除的顺序处理。后台处理完成前素材仍可编辑；完成后会自动替换音频并保留已有剪辑。
- 点击素材名称可重命名。
- 右键点击素材并选择 `Delete`，可将其从当前素材库移除；该操作不会删除磁盘中的原始或处理后音频文件。

### 2. 加入编辑区

- 将上方素材拖到下方编辑区。每次拖入都会新增一条独立轨道，不覆盖已有轨道。
- 点击轨道选中它。顶部的播放、静音裁切、折叠和导出按钮只作用于当前轨道。
- 点击 `Return` 将当前轨道放回素材库，保留已有剪辑；点击 `Remove` 仅从编辑区移除该轨道，不删除磁盘中的原始音频。
- 鼠标位于波形或编辑区空白处时，滚轮用于纵向滚动编辑区。
- 将鼠标停在轨道左侧彩色条上并滚动滚轮，可独立调整该轨道的时间尺度；操作未选中的轨道时会同时选中它。短音频按实际长度显示，长音频自动换行。

### 3. 播放、删除与恢复

- 左键点击波形：移动播放位置。
- 点击 `Play`：播放当前轨道；播放时间显示在按钮内。
- 右键从左向右拖动：删除经过的区间。
- 右键从右向左拖动：恢复经过的区间。
- 删除是非破坏性的，只保存时间区间，原始素材文件不会被改写。

### 4. 自动裁切静音

- 左键点击 `Mark silence`：按当前参数重新检测并标记静音。
- 右键点击 `Mark silence`：按钮展开为 dBFS 阈值滚轮。
- 在阈值滚轮上滚动：调整当前轨道的静音阈值。
- 再次右键点击滚轮中心：从整数精度切换到 `0.1 dB`、`0.01 dB`；左键逐级返回粗精度。
- `200 ms` 按钮控制“连续低于阈值至少多久才裁切”。点击或右键展开，以 `10 ms` 步进滚动调节；较大值保留更多字词间停顿，`0 ms` 最激进。
- 鼠标离开滚轮后，控件自动收起。
- 绿色细线是采样峰值；蓝灰色轮廓是检测实际使用的短时 RMS；黄色虚线是当前静音阈值。
- 阈值和最短静音时长按轨道独立保存。重新调整参数会重新计算自动静音区，手动删除区不受影响。

### 5. 音量包络

- 中键点击波形：在该时间和音量位置增加包络点。
- 中键按住已有点拖动：横向调整时间，纵向调整增益；底部为静音，中线为原音量，顶部为两倍音量。
- 相邻控制点之间自动形成平滑曲线。
- 右键点击控制点：删除该点。右键点击空白区域仍用于删除/恢复区间。

### 6. 折叠与导出

- 点击 `Collapse cuts`：在视图中隐藏已删除区并拼接剩余波形。
- 点击 `Show cuts`：恢复完整原时间轴，继续修改删除区。折叠不删除原始音频。
- 点击 `Export`：导出当前选中轨道。导出结果会同时应用删除区和音量包络，格式为 24-bit WAV。
- 即使没有删除区或音量包络，也可直接导出当前轨道。

## Arch Linux 安装

仓库提供源码 `PKGBUILD`，只面向 Arch Linux，不生成其他平台安装包。

```bash
git clone git@github.com:xander-lin/Simple-Audio-Cut.git
cd Simple-Audio-Cut/packaging/arch
makepkg -si
```

安装后可从桌面菜单启动 `Simple Audio Cut`，或运行：

```bash
simple-audio-cut
```

### 配置 ClearVoice

主程序不安装 Python、PyTorch、ClearVoice 或模型。未配置降噪环境时，录音、导入、静音裁切、手动编辑和导出仍可正常使用，素材显示 `No denoise`。

用户可以自行选择 CPU、CUDA 或其他 PyTorch 环境。环境中需安装 `clearvoice==0.1.2`，并在模型根目录中准备所需模型：48 kHz 音频使用 `MossFormer2_SE_48K`，较低采样率使用 `FRCRN_SE_16K`。模型根目录结构应为：

```text
/path/to/model-root/checkpoints/MossFormer2_SE_48K/last_best_checkpoint
/path/to/model-root/checkpoints/MossFormer2_SE_48K/<checkpoint>.pt
/path/to/model-root/checkpoints/FRCRN_SE_16K/last_best_checkpoint
/path/to/model-root/checkpoints/FRCRN_SE_16K/<checkpoint>.pt
```

在用户自己选择并已安装 PyTorch 的 Python 环境中安装 ClearVoice，然后下载模型：

```bash
/path/to/python -m pip install 'clearvoice==0.1.2'
mkdir -p /path/to/model-root
cd /path/to/model-root
/path/to/python - <<'PY'
from clearvoice import ClearVoice

for model in ("MossFormer2_SE_48K", "FRCRN_SE_16K"):
    ClearVoice(task="speech_enhancement", model_names=[model])
PY
```

ClearVoice 会把模型下载到当前目录的 `checkpoints/`。程序不会修改该 Python 环境，也不会自行下载模型。

为终端启动配置环境变量：

```bash
export SIMPLE_AUDIO_CUT_CLEARVOICE_PYTHON=/path/to/python
export SIMPLE_AUDIO_CUT_CLEARVOICE_MODEL_ROOT=/path/to/model-root
simple-audio-cut
```

从桌面菜单启动时，可将相同变量写入 `~/.config/environment.d/simple-audio-cut.conf`，重新登录后生效：

```ini
SIMPLE_AUDIO_CUT_CLEARVOICE_PYTHON=/path/to/python
SIMPLE_AUDIO_CUT_CLEARVOICE_MODEL_ROOT=/path/to/model-root
```

程序自带 ClearVoice 调用脚本；如需使用自定义适配器，可另设 `SIMPLE_AUDIO_CUT_CLEARVOICE_WORKER=/path/to/worker.py`。

## 从源码开发

安装构建依赖：

```bash
sudo pacman -S --needed base-devel alsa-lib ffmpeg gtk3 webkit2gtk-4.1 nodejs pnpm rustup pkgconf
rustup default stable
```

安装前端依赖：

```bash
pnpm install
```

启动开发版：

```bash
pnpm tauri dev
```

验证：

```bash
pnpm test
pnpm build
cargo check --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features
```

## 数据位置

录音和处理结果保存在用户应用数据目录：

```text
~/.local/share/io.github.xander_lin.simple_audio_cut/
```

编辑操作是非破坏性的，但当前编辑会话尚未持久化；关闭应用前请完成需要的导出。

## 许可证与来源

Simple Audio Cut 使用 [GNU General Public License v3](LICENSE)。向他人分发程序时，需要同时按 GPL v3 提供对应源码和许可证。

本项目基于 [LiRenTech/gap-gone](https://github.com/LiRenTech/gap-gone) 修改，保留 GPL v3 许可。2026 年的主要修改包括：原生录音与 FFmpeg 处理、可调目标 LUFS 归一化、ClearVoice 异步降噪与毛刺滤除、多轨独立编辑、多点音量包络、精细 RMS 静音检测、Arch Linux 源码打包，以及产品名称与图标更新。

ClearVoice 使用 Apache-2.0；各第三方依赖继续遵循各自许可证。
