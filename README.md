# Simple Audio Cut

Simple Audio Cut 是一个面向录音口播的本地桌面剪辑工具。它适合不追求复杂混音和逐帧精修、但希望快速完成录音、降噪、去静音、删错句和导出的场景。

程序采用 Tauri 2、React 和 Rust 构建。录音、响度归一化、媒体转换与导出由本机 Rust/FFmpeg 链路完成；音频不会上传到网络。新素材会先按可调目标响度归一化（默认 `-14 LUFS`），再由用户配置的降噪提供方在后台处理，随后去除短促尖锐的毛刺噪声并再次归一化到目标响度，不阻塞继续录音和编辑。

## 适用场景

- 播客、视频旁白、课程和文章朗读。
- 快速删除停顿、口误、重录段和无效内容。
- 多条口播录音分别剪辑，再独立导出 WAV。

Simple Audio Cut 不是多轨混音、音乐制作或母带处理工作站。它优先保证操作直接、反馈及时和口播剪辑效率。

## 操作方法

### 1. 获取素材

- 点击 `Record voice` 开始录音；录音时长直接显示在按钮内，再次点击停止。
- 点击 `Import media` 可一次选择一个或多个 WAV、MP3、M4A、AAC、FLAC、OGG、Opus、AIFF 或常见视频文件导入；视频会抽取音频轨道作为素材。
- 在标题栏的 `Normalization` 数值框中设置之后录音和导入素材的响度归一化目标；范围为 `-70` 到 `-5 LUFS`，默认是 `-14 LUFS`。
- 录音优先使用设备支持的 `48 kHz`，不支持时回退到兼容的最高/默认规格。
- 素材按目标响度归一化、可选降噪、毛刺噪声滤除、再次目标响度归一化的顺序处理。后台降噪完成前素材仍可编辑和导出当前归一化版本；完成后会自动替换音频、保留已有剪辑，并让轨道重新进入增量导出队列。
- 点击素材名称可重命名。
- 点击素材右侧的 `+` 或将素材拖到时间线，即可开始编辑。选中素材后，点击标题栏的垃圾桶按钮立即删除当前素材，不弹出确认框。

### 2. 加入编辑区

- 上方是自动换行的素材架，下方是占满窗口宽度的时间线。每条素材加入后形成一条独立轨道，不覆盖已有轨道。
- 点击轨道选中它。静音检测参数和播放只作用于当前轨道；点击标题栏的垃圾桶按钮立即删除当前音轨，不弹出确认框。
- 轨道操作菜单只保留将当前轨道放回素材架和折叠已删除音频等低频操作；原始音频文件不会被改写。
- 鼠标位于波形或编辑区空白处时，滚轮用于纵向滚动编辑区。
- 将鼠标停在轨道左侧彩色条上并滚动滚轮，可独立调整该轨道的时间尺度；操作未选中的轨道时会同时选中它。短音频按实际长度显示，长音频自动换行。
- 左侧色条为黄色时，该轨道会被下一次 `Export changed` 导出；绿色表示与上次成功导出的内容一致，本次增量导出会跳过。
- 右键点击轨道左侧彩色条并选择 `Force export track`，可强制单独导出该轨道。

### 3. 直接编辑波形

- 左键点击波形移动播放位置；按 `Space` 播放或暂停，`J`/`K`/`L` 后退、停止或前进。
- 按住右键从左向右滑过波形可直接删除，按住右键从右向左滑动可恢复；两个方向都可跨行操作。
- 左键拖过波形只形成处理选区，不删除音频。按 `M` 或点击选区上的 `Mute range` 可将该范围替换为等长静音；再次选中已静音范围可恢复声音。
- 已删除区域直接显示在波形上，点击区域内的 `Restore` 也可恢复。
- 按 `B` 或 `C` 进入刀片模式并点击波形添加编辑点；按 `A` 或 `V` 返回直接编辑。
- 有编辑点时，单击某个片段会选中该片段，可继续执行静音等范围处理。
- 删除和静音都是非破坏性的，只保存时间区间，原始素材文件不会被改写。

### 4. 自动裁切静音

- 在轨道检查器中设置 dBFS 阈值和最短静音时长，点击 `Detect silence` 标记静音。
- `Silence cuts on` 表示静音切除已生效；再次点击可清除自动静音区，手动删除区不受影响。
- 绿色细线是采样峰值；蓝灰色轮廓是检测实际使用的短时 RMS；黄色虚线是当前静音阈值。
- 阈值和最短静音时长按轨道独立保存。重新调整参数会重新计算自动静音区，手动删除区不受影响。

### 5. 音量包络

- 选中轨道后，点击波形中的黄色响度曲线会创建一个局部关键帧；保持按下并拖动即可同时调整该点的时间与 dB 值。底部为静音，中线为原响度，顶部为两倍增益。
- 拖动已有关键帧可继续调整曲线，按住 `Alt` 点击关键帧可删除。
- 相邻控制点之间自动形成平滑曲线。

### 6. 折叠与导出

- 在轨道操作菜单选择 `Collapse deleted audio`，可在视图中隐藏已删除区并拼接剩余波形；再次打开菜单选择 `Show all edits` 可恢复完整原时间轴。折叠不删除原始音频。
- `Export changed` 后的数字表示新增或自上次成功导出后有变化的时间线轨道数量；点击后只增量导出这些轨道，不重复导出未变化轨道。
- 导出结果会同时应用删除区、等长静音区和音量包络，格式为 24-bit WAV。改名、剪切、静音、音量包络或降噪源变化后，轨道会重新进入增量导出队列。
- 程序会记住上一次选择的导出目录；下次批量导出或单轨导出时，目录选择器会默认打开该位置。

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

点击标题栏的 `Denoising`/`ClearVoice` 打开设置抽屉：选择用户自己的 Python 可执行文件和模型库根目录，程序会扫描完整模型、列出可用设备，并让用户分别选择高采样率和低采样率模型。`Check setup` 会逐项验证环境、适配器、模型和设备，`Save settings` 将配置保存到应用配置目录。程序会自动迁移旧标识目录中可验证的 ClearVoice 环境与模型库路径，但不会安装、升级、下载或修改用户的 Python 环境和模型文件。

ClearVoice 环境中需安装 `clearvoice==0.1.2`。默认推荐 48 kHz 音频使用 `MossFormer2_SE_48K`，较低采样率使用 `FRCRN_SE_16K`；设置中也可选择扫描到的其他完整模型。模型根目录结构应为：

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

通常无需配置环境变量。以下变量仅作为启动时覆盖，适合调试或受管部署；界面会明确显示覆盖正在生效：

```bash
export SIMPLE_AUDIO_CUT_CLEARVOICE_PYTHON=/path/to/python
export SIMPLE_AUDIO_CUT_CLEARVOICE_MODEL_ROOT=/path/to/model-root
export SIMPLE_AUDIO_CUT_CLEARVOICE_DEVICE=auto
simple-audio-cut
```

从桌面菜单启动时，可将相同变量写入 `~/.config/environment.d/simple-audio-cut.conf`，重新登录后生效：

```ini
SIMPLE_AUDIO_CUT_CLEARVOICE_PYTHON=/path/to/python
SIMPLE_AUDIO_CUT_CLEARVOICE_MODEL_ROOT=/path/to/model-root
SIMPLE_AUDIO_CUT_CLEARVOICE_DEVICE=auto
```

`SIMPLE_AUDIO_CUT_CLEARVOICE_DEVICE` 可设为 `auto`、`cpu`、`cuda`、`cuda:N` 或 `gpu`。`auto` 会优先使用验证可用的 CUDA，否则回退 CPU；`cuda:N` 会通过 `CUDA_VISIBLE_DEVICES=N` 选择指定 GPU。

程序自带的 ClearVoice worker 在应用生命周期内保持运行，并按模型懒加载和缓存引擎；同一模型的后续音频只提交新任务，不会重复启动 Python 或重新加载权重。修改降噪配置、worker 异常退出或关闭应用时才会重启。48 kHz 与 16 kHz 模型首次使用时各加载一次，GPU 任务串行执行。

如需使用自定义适配器，可另设 `SIMPLE_AUDIO_CUT_CLEARVOICE_WORKER=/path/to/worker.py`。为兼容现有脚本，自定义适配器仍按单任务方式接收 `--model`、`--input`、`--output` 和 `--device` 参数。

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
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features
```

## 数据位置

录音和处理中间结果保存在本次应用会话专属的系统临时目录，退出后自动清理。降噪配置保存在用户应用配置目录：

```text
~/.config/io.github.xander-lin.simple-audio-cut/denoise.json
```

编辑操作是非破坏性的，但当前编辑会话尚未持久化；正在录音、正在处理或有未导出改动时，关闭应用会弹出二次确认。关闭前仍建议完成需要的导出。

## 许可证与来源

Simple Audio Cut 使用 [GNU General Public License v3](LICENSE)。向他人分发程序时，需要同时按 GPL v3 提供对应源码和许可证。

本项目基于 [LiRenTech/gap-gone](https://github.com/LiRenTech/gap-gone) 修改，保留 GPL v3 许可。2026 年的主要修改包括：原生录音与 FFmpeg 处理、可调目标 LUFS 归一化、ClearVoice 异步降噪与毛刺滤除、多轨独立编辑、多点音量包络、精细 RMS 静音检测、Arch Linux 源码打包，以及产品名称与图标更新。

ClearVoice 使用 Apache-2.0；各第三方依赖继续遵循各自许可证。
