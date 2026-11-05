# found.md — 经验教训记录

记录已验证的错误与可执行教训：背景、根因、纠正、预防措施。不记录常规进度。

## 2026-10-26 卸载 cu13 同名 nvidia 包删掉了 cu12 共享路径里的 libcudnn

- **背景**：清理残留的 `nvidia-cudnn-cu13` 等包后，`import torch` 报
  `libcudnn.so.9: cannot open shared object file`，应用的 ClearVoice 检查随之失败。
- **根因**：`nvidia-cudnn-cu12` 与 `nvidia-cudnn-cu13` 装进**同一个**
  `site-packages/nvidia/cudnn/` 目录；卸载 cu13 按 其 RECORD 删文件，
  把 cu12 也"拥有"的同名共享库一并删除。此前 CUDA matmul 能通过，
  只是恰好由 cu13 包里的 libcudnn.so.9 兜底，掩盖了损坏。
- **纠正**：`uv pip install --reinstall nvidia-cudnn-cu12==9.5.1.17
  nvidia-cusparselt-cu12 nvidia-nccl-cu12` 重写共享文件。
- **预防**：卸载任何 `nvidia-*-cuNN` 系列包后，必须重新
  `import torch` 并跑一次真实 CUDA 内核验证；不要以一次"成功"的旧验证为准。

## 2026-10-26 huggingface.co 不可达导致 ClearVoice 模型下载失败

- **背景**：`ClearVoice(...)` 下载 MossFormer2_SE_48K/FRCRN_SE_16K 时报
  "Downloading model ... is not successful"，指向 huggingface.co/alibabasglab/*。
- **根因**：本机网络到 huggingface.co 完全不可达（curl 000），hf-mirror.com 可达。
  另外 clearvoice 即使下载失败也照样打印 `ready:`，不能拿它的输出当成功依据。
- **纠正**：下载前 `export HF_ENDPOINT=https://hf-mirror.com`；
  大文件另加 `HF_HUB_DISABLE_XET=1`（镜像不支持 Xet 存储，否则权重下载会中途失败，
  且 429 限流需重试循环）。
  下载后以 `checkpoints/<模型>/last_best_checkpoint` 存在为准验证。
- **预防**：凡走 HuggingFace 的下载工具，先探测网络（`curl -sI -m 8`），
  不可达就设 `HF_ENDPOINT=https://hf-mirror.com`；脚本化验证只认产物文件，
  不认工具的"成功"输出。

## 2026-10-26 给已有 torch cu126 的 venv 装 clearvoice 时，torch 被换成 PyPI cu13 构建

- **背景**：在 ClearVoice 专用 venv（torch 2.7.1+cu126，V100）中
  `uv pip install clearvoice==0.1.2`，uv 重新解析依赖并安装 PyPI 的 torch 2.14.1
  （cu13 构建栈），替换了已装好的 cu126 版本。
- **根因**：`+cu126` 是本地版本号，PyPI 上更高的普通版本（如 2.14.1）在解析时"更新"；
  uv 默认按索引声明重装满足条件的最新版。CUDA 13 已删除 sm_70，该版本在 V100 上
  会报 "no kernel image"。
- **纠正**：用约束文件 `--constraint`（内容 `torch==2.7.1+cu126` 等）配合
  `--extra-index-url https://download.pytorch.org/whl/cu126` 重装，把 cu126 构建换回。
- **预防**：任何向含 `+cuXXX` 本地版本 torch 的环境追加包的操作，都必须同时传
  约束文件钉住 torch/torchaudio/torchvision；装完用真实 CUDA matmul 验证 sm_70 内核
  可运行，而不是只看 `torch.cuda.is_available()`。

## 2026-10-26 zsh 不拆分未引用变量导致 uv pip uninstall 解析失败

- **背景**：`C13=$(... | tr '\n' ' ')` 后执行 `uv pip uninstall ... $C13`，
  zsh 报 "Failed to parse ... Expected one of @, (, <..."。
- **根因**：zsh 默认关闭 `SH_WORD_SPLIT`，未引用的 `$C13` 仍是一个单词，
  整串被当成单个 requirement。
- **纠正**：显式列出包名（或用 zsh 的 `${=C13}` 强制拆分）。
- **预防**：在 zsh 中写命令时，凡期望"空格分隔多参数"的变量展开，
  一律用 `${=VAR}`、数组或直接写全参数；不要依赖 POSIX 式单词拆分。

## 2026-10-26 uv 缓存离线重装失败时，archive-v0 仍可手工恢复文件

- **背景**：cu12 库被误删后 `uv pip install --offline` 报
  "Network connectivity is disabled, but the requested data wasn't found in the cache"，
  尽管 `~/.cache/uv/archive-v0/` 里确有解包好的完整 wheel 内容。
- **根因**：uv 的下载缓存记录（URL→payload 映射）已失效，`--offline` 按键查找失败；
  但解包树按内容哈希仍保留在 archive-v0（12GB 中有 11GB 是它）。
- **纠正**：按 `*dist-info/METADATA` 的 `Version:` 找到对应版本的 archive-v0 树，
  用 `cp -a` 把包目录恢复进 site-packages；落点以该 wheel 的 RECORD 为准
  （例：nvidia-cusparselt-cu12 装到顶层 `cusparselt/lib/`，不是 `nvidia/cusparselt/`），
  恢复后立即 `import clearvoice` + 真实 CUDA matmul 验证。
- **预防**：慢网环境下优先找缓存而非重下；手工恢复时必须核对 dist-info 版本与
  RECORD 路径，恢复后用真实内核验证，而不是只看包存在。

## 2026-10-29 WebKitGTK 的 decodeAudioData 失败根因是系统缺 GStreamer 插件

- **背景**：应用录音后导入报 `EncodingError: Decoding failed`，但录音 WAV 文件本身
  完全有效（ffprobe 正常）。应用对所有 WAV 的导入路径都是 Rust 归一化成
  24-bit PCM WAV → 前端 `AudioContext.decodeAudioData`。
- **根因**：WebKitGTK 的 `decodeAudioData` 走 GStreamer decodebin；本机只装了
  `gst-plugins-ugly`，缺 `gst-plugins-base/good`（wavparse 在 good 中），
  页面实测报 "no suitable plugins found: Missing decoder: WAV (audio/x-wav)"，
  三种位深的 WAV 全部失败——不是应用 bug，是环境依赖不完整。
- **纠正**：安装 `gst-plugins-base gst-plugins-good gst-plugins-bad gst-libav`
  （ugly 已存在），实测三种 WAV 全部解码成功。
- **预防**：桌面应用依赖 WebKitGTK 解码时，先确认 GStreamer 插件集完整；
  诊断方法：用 python-gi 起一个最小 WebKit2 页面，通过
  `register_script_message_handler` 把 `decodeAudioData` 结果回传，
  直接区分"文件坏"与"环境缺解码器"。`gst-inspect-1.0 wavparse` 也可快速确认插件是否在。

## 2026-10-26 构建机缺少 makedepend（pnpm）

- **背景**：按 `packaging/arch/PKGBUILD` 执行 `makepkg` 前检查环境，发现 `pnpm`
  未安装（其余 makedepends `cargo/git/nodejs/pkgconf` 均在）。
- **根因**：README 的开发环境安装命令包含 `pnpm`，但该机器的包管理器状态未包含它；
  makepkg 在 prepare 阶段（`pnpm install --frozen-lockfile`）才会失败，提前不做检查
  会浪费一次完整构建周期。
- **纠正**：构建前用 `pacman -S --needed pnpm` 补齐。
- **预防**：打包/安装前先校验 `makedepends` 清单中每个命令是否可用
  （`command -v`），再启动 makepkg；不要等 makepkg 在中途报错。

## 2026-10-29 迁移到 iced 时 `cpal::Stream` 不能跨线程持有

- **背景**：为 iced 前端新增播放后端，最初把 `cpal::Stream` 放进
  `Mutex<Option<Stream>>` 字段，编译报 `*mut ()` cannot be sent between threads。
- **根因**：ALSA 后端的 `cpal::Stream` 内含 `NotSendSyncAcrossAllPlatforms`，
  因此整个播放器无法满足 `AudioPlayer: Send + Sync`。
- **纠正**：把流的创建/持有/销毁放在专用线程，播放器只保留命令通道
  （`Command::Play/Stop`）与共享游标（`crates/core/src/audio/player.rs`）。
- **预防**：凡在 GUI 状态里持有系统音频/窗口句柄，先把句柄限制在单一线程，
  对外只暴露 `Send + Sync` 的消息接口；不要用 `unsafe impl Send` 绕过。

## 2026-10-29 pkill -f 会杀掉正在执行命令的 shell 自身

- **背景**：用 `pkill -f seed_preview` 结束预览进程时，命令本身以 SIGTERM 结束，
  后续构建/截图步骤没有执行。
- **根因**：`-f` 匹配完整命令行，而当前 shell 的命令行包含 `seed_preview`。
- **纠正**：改用 `pkill -x <进程名>` 或匹配绝对路径并排除自身。
- **预防**：结束进程优先用 `-x`（精确进程名）；需要 `-f` 时确认模式不会匹配当前 shell。

## 2026-10-29 iced canvas 缓存必须放在应用状态里

- **背景**：波形 `canvas::Program` 若在视图函数里创建并持有 `Cache`，每次重建都会
  丢失缓存；若想在 `&self` 上清缓存又不可变。
- **根因**：`canvas::Program::draw` 接收 `&self`，无法直接 `clear()` 内部缓存。
- **纠正**：把 `WaveformCache` 放在 `Session.waveform_caches`，用 `Cell<u64>` 记录
  已缓存 revision，draw 时对比 revision 并清空（`crates/app/src/waveform.rs`）。
- **预防**：iced 中任何需要跨帧保留的绘制缓存都放进应用状态，而不是 widget 构造处。

## 2026-10-29 后台事件不能写进"操作反馈"那一行

- **背景**：`workflow` 测试偶发失败（10 次里 3~4 次），断言
  `ui.message == "Export cancelled"` 实际得到 `"Mock denoising is disabled."`。
- **根因**：降噪完成/失败、降噪不可用等后台事件与用户操作（导出、
  导入、选区静音）共用 `UiState::message` 这一个字段，异步事件到达顺序不定，
  会覆盖用户刚看到的结果；同时它也是测试不稳定的来源。
- **纠正**：后台事件改写 `Session::denoise_note`，只在设置抽屉里以灰色提示显示
  （2026-11-05 补：不再显示在状态栏，关闭抽屉时清空，避免常驻）；
  `ui.message` 只保留用户操作的结果。
- **预防**：区分"操作反馈"与"环境/后台状态"两类文本，不要共用一个字段；
  凡是后台线程可以异步写入的 UI 状态，都要考虑它是否会覆盖用户的即时反馈。

## 2026-11-04 用"窗口宽度估算"代替真实布局宽度

- **背景**：波形 canvas 的行布局用 `canvas_width(window_width)` 估算可用宽度，
  实际行宽由 iced 布局解算（`responsive`），两者不等，导致右侧约 24px 空白/裁切。
- **根因**：为了让 `canvas::Program::draw` 的 `bounds.width` 与 `layout_rows` 的输入
  一致，选了"自己算宽度"这条捷径，而不是用布局真实给出的宽度。
  另外 `responsive` 在纵向滚动列里高度是 `INFINITY`，直接 `Length::Fixed(inf)`
  会让画布不可见——这是早前放弃 `responsive` 的原因。
- **纠正**：继续用 `responsive`，宽度取 `size.width`（只在非有限时回退估算），
  高度用 `Length::Shrink`，并把画布高度按宽度算出来。
- **预防**：凡是"绘制尺寸必须等于布局尺寸"的 widget，不要自己推尺寸；
  用布局回调给出的值，并对 `INFINITY`/`NaN` 明确回退。
  这类问题没有任何测试能抓到，必须靠实机截图核对。

## 2026-11-04 迁移审计：先验证"看得见的行为"，不要先宣布完成

- **背景**：迁移汇报时列了测试全绿与打包成功，但用户实际使用后反馈"几乎各方面
  都有问题"。
- **根因**：验证只覆盖了状态机与核心算法，没有覆盖（a）布局尺寸、（b）键盘
  Esc/焦点拦截、（c）新旧行为逐条对照、（d）真实鼠标交互。
  自动化测试通过不代表产品可用。
- **纠正**：补充 `BUGS.md` 作为持续维护的缺陷清单，逐项对照旧版实现。
- **预防**：迁移类任务必须做"旧版行为清单 → 逐条勾选"的对照表，
  并对每个可见交互做实机验证；测试通过只能作为必要条件。

## 2026-11-05 在错误的工作树里"继续修复 bug"

- **背景**：用户要求"继续修复 bug"，当前会话目录是
  `/home/life/Work/Simple-Audio-Cut`（`main`），于是修复了旧的 React/Tauri
  播放代码并提交；用户随后指出"不是 iced 吗?"。实际开发在
  `native-gui-iced` 工作树（`/home/life/.local/share/opencode/worktree/...`），
  那里的 `BUGS.md` 才是当前缺陷清单。
- **根因**：开始前没有确认分支/工作树状态，也没有检查仓库里是否存在其他
  worktree；把"当前目录"默认当成了"用户正在做的产品"，
  而迁移分支明确保留 `src/`、`src-tauri/` 仅用于对照。
- **纠正**：切换到 `native-gui-iced` 工作树，按 `BUGS.md` 继续修复；
  `main` 上那次提交只涉及已退役的 React 前端，是否保留需用户决定。
- **预防**：接手"继续修复"类任务时，先跑 `git worktree list`、`git branch -vv`、
  `git log --all --oneline -10`，并读当前分支的 `BUGS.md`/`AGENTS.md`；
  确认目标产物（iced vs 遗留 Tauri）后再改代码。
  看到 `AGENTS.md` 写"迁移分支保留旧目录仅用于对比"时，不要改旧目录。

## 2026-11-05 导出完成时用"开始时的签名"标记已导出

- **背景**：`start_export` 在派发任务时抓取 `track.signature()`，
  `ExportCompleted` 直接用这份签名写回 `last_export_signature`。
  若导出期间降噪完成替换了音频，导出文件仍是旧音频，但轨道被标记为"已导出"。
- **根因**：把"请求参数"当成了"完成时的真相"。签名是内容的函数，
  必须在写回时用当前状态重新计算并比对，而不是复用派发时的快照。
- **纠正**：完成时只有当 `track.signature()` 仍等于派发时的签名才标记已导出；
  否则保持待导出，并在状态栏说明"内容在导出期间变化"。
- **预防**：任何"记录某个异步任务完成后的状态"的逻辑，都要在完成时
  重新校验"任务开始时假设的输入是否仍然是当前输入"；快照只用于比较，不能直接写回
  "已完成"标记。

## 2026-11-05 重命名调用了会重建音频的 `touch()`

- **背景**：`Message::RenameMedia` 每输入一个字符就 `track.touch()`；折叠轨道下
  `touch()` 会 `rebuild_collapsed()`，对长音频逐样本重建并重算 RMS。
- **根因**：把 `touch()` 当成"任何字段变更后都该调用"的通用刷新，而它实际语义是
  "输出音频相关的编辑已变化"。
- **纠正**：重命名只更新名称（签名会因名字变化自动判定为待导出），不调用 `touch()`。
- **预防**：给会做重计算的 `touch()`/`rebuild_*()` 明确文档语义：只用于影响
  输出音频或波形几何的变化；纯展示字段（名称、颜色、选中状态）不得触发重算，
  并用"折叠音频 Arc 指针不变"这类测试锁住。
