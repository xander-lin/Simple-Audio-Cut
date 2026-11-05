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

## 2026-11-05 iced 的 `space()` 是 Shrink，整行文字会粘连

- **背景**：检查器里的 "Duration0: 06.00"、素材行的 "0:26 · -14.0 LUFS ClearVoice"
  全部贴在了一起，看起来像排版坏了。
- **根因**：`iced::widget::space()` 等价于 `Space::new()`，宽高都是 `Length::Shrink`，
  在 Row 里宽度为 0；真正可伸展的是 `space::horizontal()`。旧的 UI 到处用 `space()`
  期待它撑开，所以每个"左标签 … 右数值"的行都退化成了紧挨着。
- **纠正**：在 `ui.rs` 定义 `spacer()`（`Space::new().width(Length::Fill)`）并全部替换。
- **预防**：iced 里不要想当然认为 `space()` 会填满；写"两端对齐"的行时，
  父 Row 和父 Column 都要 `Length::Fill`，并且分隔符必须是 Fill 宽度的实体。

## 2026-11-05 `responsive` 拿到的宽度不等于音频可用宽度，默认缩放导致换行

- **背景**：新加轨道时整段 6 秒音频只占 288px（48px/s），右侧大片空白；把缩放
  改成 fit 后，最后又莫名多出一小行残影。
- **根因**：`layout_rows` 会在左右各留 edge 边距（24px 起），fit 用"画布宽度/时长"
  计算，正好把边距挤出去，于是判定为需要换行。
- **纠正**：fit 用 `(width - 2*edge - 2) / duration`，并让缩放从当前 fit 值起步。
- **预防**：凡是"把内容铺满容器"的换算，必须扣除容器自身的 padding/edge；
  写完再检查是否多出半行——这类错误只有截图能发现。

## 2026-11-05 用固定像素断点判断"宽屏/窄屏"是错的

- **背景**：用户要求"全屏左右布局、半屏上下布局"。原实现用 1280px 固定断点，
  在 4K 显示器上分屏后窗口仍有约 1900px，被误判为宽屏，三栏被挤爆。
- **根因**：把"绝对宽度"当成了"可用空间"，忽略了窗口相对于所在显示器的比例。
- **纠正**：启动时通过 `iced::window::monitor_size` 取显示器逻辑尺寸，
  窗口 ≥ 72% 显示器宽度视为全屏（三栏），否则堆叠；宽度用 `WindowResized` 维护。
- **预防**：任何"宽/窄"的布局判断都应相对容器或屏幕，而不是固定像素；
  该判断不要放在 `responsive` 闭包里——`responsive` 只在自己尺寸变化时重建，
  外部状态（显示器尺寸）变化会被静默忽略。

## 2026-11-05 "fit" 缩放与 `layout_rows` 的边距不一致，导致多出半行

- **背景**：轨道默认 fit 后仍会多出一小截第二行，波形被压缩在上半行。
- **根因**：`layout_rows` 的左右边距是 `max(pps * 0.3, 24)`，与缩放相关；
  而 fit 按"宽度 - 48"计算，pps 越大边距越大，算出来的行宽超出行宽。
- **纠正**：在 core 提供 `fit_pixels_per_second(width, duration)`，
  同时满足"固定 24px 边距"和"0.3×pps 边距"两种情形，并加单元测试
  断言所有常见宽度/时长组合都只占一行。
- **预防**：凡是"把内容缩放到容器"的计算，必须使用与绘制相同的边距公式；
  这类错误只能靠截图或几何断言发现，写完要立刻核对渲染结果。

## 2026-11-05 降噪配置的 `enabled` 同时承担了两种语义

- **背景**：用户要求"降噪可选：不可用也能用、可以 CPU 也可以 GPU"。
  原实现里 `DenoiseConfig.enabled=false` 会让 `prepare()` 直接报错，
  于是"关闭自动降噪"等于"任何素材都不能降噪"。
- **根因**：一个布尔字段同时表达了"是否自动处理新素材"和"是否允许降噪"。
- **纠正**：`enabled` 只表示自动处理新素材；显式请求（检查器里的
  `Denoise this take`）始终可用。provider 不可用时只影响降噪，不影响编辑/导出。
- **预防**：配置开关要写清"它控制什么、不控制什么"；用单测固定住
  "关闭自动降噪后仍可手动降噪"和"provider 不可用仍可导出"这两条契约。

## 2026-11-05 iced canvas 会把整个窗口的事件投递给每个 Program

- **背景**：用户报"选中哪条轨道都没用，永远播放最后一条"。截图看选中态正常，
  但播放日志显示始终是最末轨道的音频。
- **根因**：轨道左侧的 `ScaleHandle`（缩放/导出状态色条）是 `canvas::Program`。
  iced 把窗口内的所有鼠标事件都交给每个 canvas，是否在控件内要 Program 自己判断。
  原实现没判断，于是窗口里任何一次左键按下都会让**每条轨道**各发一次
  `SelectTrack`，最后一条覆盖前面所有结果；右键同理，会同时弹出多份菜单。
- **纠正**：`update()` 开头 `let position = cursor.position_in(bounds)?;`，
  光标不在本控件上就直接返回。
- **预防**：所有 `canvas::Program` 的 `update` 第一行都必须做 bounds 判定
  （`position_in` 或 `is_over`）；滚动、点击、hover 都要。用"布局外的坐标"写单测
  固定住，别只测"点在控件内"。

## 2026-11-05 ClearVoice 下载失败仍会"成功"写出静音

- **背景**：一次真实降噪把素材变成了 -70 LUFS 的静音文件，应用随后把用户音频替换成它。
  同时那次任务耗时 4 分 39 秒。
- **根因**：两层叠加。(1) worker 里 `ClearVoice(...)` 在模型缺失时会尝试访问
  huggingface.co；本机不可达，于是先等网络超时（4:39），再打印
  "Downloading model ... is not successful" 却继续运行未加载的模型，写出静音。
  (2) 应用侧只看"进程返回成功"，没有校验输出是否真的是音频。
- **纠正**：worker 启动时强制 `HF_HUB_OFFLINE=1` / `TRANSFORMERS_OFFLINE=1` /
  `HF_DATASETS_OFFLINE=1`，模型缺失立刻失败而不是等网络；应用侧新增
  `verify_denoise_output`：输出低于 -60 LUFS（静音）或时长变化超过 2% 时
  判为失败，保留原音频并在状态栏说明。
- **预防**：任何"外部模型/工具产出文件"的步骤，都要校验产物本身（内容与时长），
  不能只看退出码；离线原则要用环境变量强制，而不是靠调用方自觉。

## 2026-11-05 `Check setup` 把草稿状态当成运行配置

- **背景**：设置抽屉里改一版（可能不可用的）草稿并点 `Check setup` 后，新导入的
  素材开始按草稿结果处理；关掉抽屉也不会恢复。
- **根因**：`SettingsInspected`（针对草稿的检查）把结果写进了
  `Session::denoise_status`，而那个字段的语义是"已保存配置的当前状态"，
  自动降噪策略据此运行。
- **纠正**：`SettingsInspected` 只更新 `ui.settings.status`；运行状态只由
  `SettingsStatus`（已保存配置）与 `SettingsSaved` 更新。
- **预防**：区分"草稿预览结果"和"生效配置"；任何会影响后台策略的字段，
  只能由保存成功的路径写入，并加测试固定住。

## 2026-11-05 自动降噪关闭时，用户显式选择"原始音频"没有被记录

- **背景**：自动降噪关闭时所有素材天然是 `Skipped`，用户点 `Use original audio`
  没有任何效果；之后打开自动降噪，该素材被重新降噪。
- **根因**：`set_denoise(false)` 看到 `status == Skipped` 就提前返回，
  没有写 `DenoiseState::opted_out`。"当前状态"与"用户意图"被当成了同一件事。
- **纠正**：只有 `Skipped && opted_out` 才提前返回；否则记录用户选择并给出反馈。
  恢复原始音频的回调也必须保留 `opted_out`。
- **预防**：凡是"幂等"判断，先区分状态是否已经表达用户意图；显式选择必须
  留下持久标记，自动策略不得覆盖。

## 2026-11-05 用 sleep 轮询等待后台任务，掩盖了调度问题

- **背景**：workflow/real-audio 测试靠 `sleep(5ms)` + 超时轮询；既慢又偶发失败，
  而且无法区分"任务没派发"和"任务很慢"。
- **根因**：后台任务的执行方式（直接 `thread::spawn`）不是可替换服务，
  测试只能猜它何时完成。
- **纠正**：引入 `JobRunner` 接口，测试使用 `InlineJobRunner`，任务在消息处理内
  同步完成；`Tick` 一次排空事件链；真实异步回调改用 `mpsc::recv_timeout`。
- **预防**：测试中不要用 sleep 等待自己控制的任务；先让执行器可注入，
  再断言状态。只有真正依赖外部时钟的行为（如 CPAL 播放推进）才允许短暂等待。

## 2026-11-05 固定尺寸按钮的文字贴在左上角

- **背景**：为避免标签变化推动相邻控件，按钮改成 `.width/.height(Length::Fixed)`；
  截图里 Record、Import、播放、Export 等文字全部贴在按钮左上角。
- **根因**：iced 的 `button` 不会居中内容，固定尺寸后多出的空间全部落在右下。
- **纠正**：`ui.rs` 新增 `centered()`（`container(..).center(Length::Fill)`），
  所有固定尺寸按钮的内容经它包裹。
- **预防**：给按钮设固定宽高时同时包 `centered()`；改完必须截图核对。

## 2026-11-05 niri 下 `import`/`magick x:` 截不到窗口

- **背景**：用 ImageMagick 在 XWayland 上截预览窗口，`import` 报
  "missing an image filename"，`magick x:root` 无图像。
- **根因**：niri 是 Wayland 合成器，XWayland 根窗口不可读。
- **纠正**：用 `niri msg -j windows` 找窗口 id，再
  `niri msg action screenshot-window --id <ID> --path /tmp/opencode/<dir>/x.png`。
- **预防**：在本机截图核对 UI 时直接走 niri 的截图命令，不要先试 X11 工具。

## 2026-11-05 只换配色的"美化"被用户判为仍然有 AI 味

- **背景**：第一轮美化只调了配色、圆角和阴影，保留了卡片式素材行、圆角轨道框、
  Transport 卡片和大量 8–10px 的小字。用户反馈"AI 味还是太重"，认为卡片感和碎小文字是根源。
- **根因**：把"精致"理解成给现有盒子加细节；没有质疑盒子本身。每个区域都套边框/圆角、
  每个状态都配一行小字，信息被切成碎块，层级反而不清。
- **纠正**：重写 `ui.rs`/`theme.rs`：去掉卡片与描边框，区域平铺并用 1px 细线分隔；
  走带并入顶栏；字号收敛为 `theme::size`（最小 11px，仅波形刻度 10px）；
  单一琥珀强调色，选区用亮度表达；每屏只有一个实心主按钮。
- **预防**：做视觉改版前先列出"每个区域为什么需要边框/底色/说明文字"，说不出理由就去掉；
  新增文字前检查是否已能从位置或状态点推断。字号只用 `theme::size` 中的档位。

## 2026-11-05 同一个手势不能同时按"状态"和"方向"判定

- **背景**：左键拖动原本是"这段已静音就恢复、否则静音"的状态切换；用户要求它
  与右键一致，向右静音、向左恢复。若只是把判断换成 `region_is_covered`，
  向一个已经静音但未完全覆盖的区间左滑会意外"恢复"而不是"静音"。
- **根因**：手势语义混用两个来源——方向（用户这次想做什么）和当前状态（这段现在是什么）。
- **纠正**：core 增加 `mute_region_edit(start, current, minimum)`，输出的 operation 只由
  位移方向决定；`Track::set_mute(range, mute)` 无条件写入，不再做 toggle 推断。
  状态推断只保留在检查器里的 `Mute`/`Unmute` 按钮这类显式切换入口（`M` 快捷键已按用户要求移除）。
- **预防**：拖动类手势一律按"起点到终点的方向"决定语义，并且写测试覆盖
  "从不静音处左滑恢复"和"从静音处反向滑动"两种组合；不要用当前数据状态反推意图。

## 2026-11-05 iced 的 `MouseArea` 双击与拖动可以共存，但要留意事件顺序

- **背景**：需要实现"双击轨道名改名"，同时保留"单击选中"和把素材拖到时间线的拖动。
- **根因（认识误区）**：担心拖动会误触发双击。实测 iced 的 `MouseArea` 在
  `on_press` 之外单独发出 `on_double_click`，只有两次按下在同一位置、间隔足够短时才触发；
  拖到别处不会进入双击分支。
- **纠正**：`mouse_area(label).on_press(SelectTrack).on_double_click(BeginRename)`；
  改名用无边框、无底色的 `text_input`（`theme::inline_input_style`），
  由 `BeginRename` 只负责建立 draft 并 focus + 全选，不弹任何对话框。
  后续按用户要求把**右键**也接到同一个 `BeginRename`（名称栏上的右键不会和时间线的
  右键拖动冲突），并在 `•••` 菜单加了 `Rename track`，保证只有一个入口定义。
- **预防**：把"单击 / 双击 / 拖动"三件事叠在同一区域时，先确认框架是否提供独立的
  双击事件；有就不要再自己用计时器判定。改名控件不要给边框和底色，否则会被读成弹框。
  另外：iced 的 `text_input` 会**吞掉** `Escape`（用于取消聚焦），全局快捷键映射看不到它；
  要让 `Esc` 放弃改名，必须在 `rename.is_some()` 时换用另一个 `listen_with` 映射，
  对 `Status::Captured` 的 `Escape` 也产出 `Dismiss`。
  最后一点：能自动验证的只有状态机（draft、提交、Esc），鼠标手势必须实机点一遍；
  无法实机点验时，优先选一条"即使框架事件顺序不同也不会出错"的入口（右键）。
