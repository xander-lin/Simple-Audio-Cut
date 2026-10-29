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
