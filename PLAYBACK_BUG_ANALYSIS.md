# 播放异常代码分析

> **历史文档 / 已过时**：本文描述的是旧版 Tauri + React + WebKitGTK 前端的播放问题
> （`src/App.tsx`、`src/audio/audioContextManager.ts`）。2026-10-29 起编辑器已迁移到
> `crates/app` 的 iced 原生实现，解码、播放和渲染都在 Rust 进程内完成，WebAudio /
> `AudioContext` 路径已不复存在。本文仅作为迁移背景与调查记录保留，不代表当前代码。

初始分析：2026-08-22  
运行时取证与修复：2026-08-30 至 2026-08-31

## 2026-08-31 最终状态

本节取代下文初始静态分析中的“尚未捕捉”和“本次未实施”状态。下文保留作为调查过程和代码路径记录。

### 运行时证据

- 在问题实际发生的进程中，同时捕获了主进程和 `WebKitWebProcess` 的 core。压缩归档位于仓库本地的 `coredumps/`，原始 core 哈希已验证。
- 故障状态下，WebKit 音频流仍是 running/uncorked，但用户听到静音；`webaudioSrc`、`webaudioSrcTask` 和解码线程存在，渲染侧处于等待状态。
- 故障状态下点击播放后，按钮仍显示播放图标，状态栏没有进入播放态，也没有生成新的 `Unable to play audio` 动态错误。这说明请求停在 `source.start()` 之前的异步等待，而不是 source 已经正常启动。
- 当时的代码对 `AudioContext.resume()` 没有上限。该 Promise 可以长时间不结算；一旦以后结算，旧 `startPlayback()` 会继续执行。这是唯一能同时解释“播放点击静默等待”和“很久以后旧请求突然播放最后选中轨道”的代码路径。
- 恢复实验中，文件对话框关闭后旧音频流 serial `8601` 被销毁，新流 `11029` 创建，随后播放有声。这证明上下文重建路径有效。由于故障也会自行恢复，该实验不能单独证明旧上下文在实验瞬间仍处于故障状态。

### 已实施修复

- `AudioContext.resume()` 现在有 2 秒上限；超时或 resume 阶段失败会取消当前播放请求并异步重建上下文。
- 播放请求同时绑定请求号、轨道 ID、轨道对象和 AudioContext 生命周期版本。轨道变化、停止播放、上下文 reset 或 closed context 替换都会使旧请求失效。
- 超时后的旧 `resume()` 即使以后自行结算，也无法再进入 `source.start()`。
- `AudioBufferSourceNode` 在 `start()` 前登记；如果启动后发现请求已失效，会立即停止节点，避免留下静音的僵尸播放状态。
- 设置文件对话框关闭前先取消播放，再请求上下文重建，使设置、导入和导出三条对话框路径保持一致。
- 新增 manager 单元测试和 App 组件回归测试，覆盖 `resume()` 永不返回、超时重建、旧 Promise 迟到完成不播放、替代上下文下一次明确点击可播放。

### 验证与安装

- 前端逻辑测试：47 项通过。
- React 组件测试：14 项通过。
- TypeScript 检查和 Vite 生产构建通过。
- Arch `makepkg` 干净构建中重复运行上述测试；Rust 默认测试 18 项通过、1 项真实 ClearVoice 模型测试按设计忽略；无默认 feature 检查通过。
- 最终本地归档后缀为 `local-r37`，软件包 `simple-audio-cut 0.1.3-2` 已重新安装。

### 剩余风险

WebKit/PipeWire 内部究竟由什么事件使 `resume()` 长时间不结算、又自行恢复，仍未从 core 中确定。修复不依赖识别该环境触发器：它封死了旧请求延迟复活的路径，并在输出恢复失败时替换上下文。由于原问题需要长时间运行才能触发，最终仍需实际长期使用验证其环境层复现是否消失。

## 结论摘要

当前代码中没有定时器、后台 Rust 任务或 React `useEffect` 会直接自动播放音频。真正产生声音的唯一位置是 `src/App.tsx:712` 的 `AudioBufferSourceNode.start()`。因此，“软件开很久后自动播放最后一条选中的音频”在代码层面的含义是：某个已经发起的 `startPlayback()` 调用，最终在没有新的明确播放操作的时刻执行到了 `source.start()`。

目前最值得怀疑的是两个相互关联的生命周期/异步竞态：

1. `startPlayback()` 是异步函数，等待 `AudioContext` 获取和 `resume()`，但播放意图只用一个数字请求号取消，而且没有绑定到轨道 ID、音频缓冲区或上下文代数。旧调用可能在长时间等待后恢复，并播放调用当时捕获的“最后选中轨道”。
2. `AudioContextManager.requestReset()` 只设置标志；实际重置时，管理器可以关闭仍在播放的 `AudioContext`，但不会通知 `App` 停止播放、清理 `sourceNodeRef`、递增播放请求号或更新 `isPlaying`。这会留下“界面状态、播放节点和实际音频上下文不一致”的状态，能够解释随后点击播放按钮无声或需要额外操作才能恢复。

此外还有两个会放大问题的代码窗口：

- `source.start()` 在 `sourceNodeRef.current = source` 之前执行。接近音频末尾或剩余时长极短时，`onended` 可能在节点身份登记前触发，导致播放状态无法正常收尾。
- 键盘快捷键的 `useEffect` 没有依赖数组，每次渲染都会重绑一个捕获 `selectedTrack` 和 `isPlaying` 的闭包。正常情况下 React 会清理旧监听器，但渲染/被动 effect 切换期间仍存在旧事件处理器参与竞态的窗口。

这些结论解释了两个现象的共同机制，但由于问题尚未被运行时日志捕捉，暂时不能仅凭静态代码断言某一次用户报告对应的是哪一条具体触发路径。下面保留完整证据和复现时应观察的信号。

## 播放调用链

### 唯一的音频启动点

`src/App.tsx:691-716` 的流程是：

1. 捕获当前渲染中的 `selectedTrack`、删除区、包络和静音区。
2. 调用 `stopPlayback(false)`，使 `playbackRequestRef.current` 加一。
3. `await getAudioContext()`。
4. 如果上下文是 suspended，`await context.resume()`。
5. 创建编辑后的 `AudioBuffer` 和 `AudioBufferSourceNode`。
6. 在 `src/App.tsx:712` 调用 `source.start(0, editedOffset)`。
7. 设置 `isPlaying`、播放上下文、当前节点和动画帧。

### 当前所有已找到的调用入口

`startPlayback()` 只在以下入口被调用：

- `src/App.tsx:1041`：按 `Space` 或 `K`，且当前 `isPlaying` 为 false。
- `src/App.tsx:1049`：播放中按 `J`，跳回 5 秒。
- `src/App.tsx:1057`：按 `L`，向前 5 秒。
- `src/App.tsx:1154`：点击播放按钮。
- `src/App.tsx:1192`：点击/拖动波形定位，并且闭包中的 `isPlaying && isSelected` 为 true。

没有找到以下类型的自动播放入口：

- `setTimeout`/`setInterval` 调用 `startPlayback`。
- `requestAnimationFrame` 调用 `startPlayback`；它只更新进度。
- `AudioContext` 的 `statechange`、窗口 `focus`/`blur`、`visibilitychange` 回调调用播放。
- Rust 音频引擎输出播放；Rust 侧负责录音、归一化、导入、降噪和导出，没有音频输出 sink。
- HTML `<audio>`/`<video>` 元素或 `autoplay` 属性。

所以如果完全没有任何输入事件，浏览器本身不会凭现有代码凭空调用 `source.start()`；必须存在一个迟到的旧调用、一个被排队/重复派发的输入事件，或尚未观察到的 WebView/运行时行为。

## 发现一：播放请求没有完整的取消语义

**位置：** `src/App.tsx:315-345`、`src/App.tsx:673-731`

`playbackRequestRef` 的保护逻辑本身是有用的：

- `stopPlayback()` 会递增请求号。
- `startPlayback()` 在 `getAudioContext()` 返回后检查一次。
- `resume()` 返回后再检查一次。
- `onended` 和动画帧也检查请求号及当前 source 身份。

但它只在某些操作实际调用 `stopPlayback()` 时才生效，且没有验证播放目标是否仍然是当前目标。请求号没有表达以下信息：

- 发起请求时的 `selectedTrackId`。
- 发起请求时的 `AudioBuffer` 身份或版本。
- 发起请求时的 `AudioContext` 实例/代数。
- 请求是否仍然来自一次有效的用户手势。

`startPlayback()` 在 `await getAudioContext()` 或 `await context.resume()` 期间，闭包中的 `selectedTrack` 不会自动更新。只要期间没有路径调用 `stopPlayback()`，旧闭包就能在很久以后继续创建 source 并播放旧轨道。

特别需要注意的是，`onRangeSelect`（`src/App.tsx:1193-1194`）会直接设置新的 `selectedTrackId`，但没有调用 `stopPlayback(false)` 或递增播放请求号。它不是“自动播放”的直接入口，却证明“选择变化”和“取消旧播放意图”并非全局不变量。任何同时存在的延迟启动都可能继续使用旧的 `selectedTrack` 闭包。

### 可能的延迟播放时间线

```text
T0  某次按钮/快捷键/波形事件调用 startPlayback()。
T1  startPlayback() 调用 stopPlayback()，随后等待 getAudioContext() 或 resume()。
T2  文件对话框触发 requestReset()，或上下文/系统音频设备进入异常状态。
T3  上下文重置/恢复完成；如果期间没有 stopPlayback()，请求号没有变化。
T4  旧 startPlayback() 通过两次检查，使用旧闭包中的 selectedTrack 创建 source。
T5  source.start() 执行，用户看到/听到最后一次选中的音频突然播放。
```

这条链路是目前最符合“随机、难以捕捉、经过很长时间才发生”的代码级解释，但需要运行时日志确认 `T0` 是否确实早于异常发生很久。

## 发现二：上下文重置不会停止播放

**位置：** `src/audio/audioContextManager.ts:47-145`、`src/App.tsx:567`、`src/App.tsx:909`、`src/App.tsx:1233`

`AudioContextManager` 将解码操作通过 `run()` 计入 `activeOperations`，但播放通过 `get()` 获取上下文：

```ts
const context = await getAudioContext();
```

因此播放不是管理器眼中的 active operation。`requestReset()` 只做：

```ts
this.resetRequested = true;
```

下一次 `get()` 或 `run()` 看到这个标志后，`reset()` 会：

1. 等待被计数的解码操作空闲。
2. 关闭旧上下文。
3. 创建新上下文。

它不会调用 `stopPlayback()`，也不会改变 `playbackRequestRef`。当旧上下文仍有 `AudioBufferSourceNode` 时，App 侧仍可能保留：

- `sourceNodeRef.current` 指向旧节点。
- `playbackContextRef.current` 指向已关闭上下文。
- `isPlaying === true`。
- `startedAtRef`、`editedPlaybackOffsetRef` 和 `playbackKeptRegionsRef` 仍是旧播放数据。

### 直接导致“点击播放无声”的时间线

```text
T0  正常播放，isPlaying=true，sourceNodeRef 指向旧上下文中的 source。
T1  用户打开设置并使用文件选择器；DenoiseSettings.onDialogClosed() 请求上下文重置。
T2  之后任意一次解码/播放操作触发 manager.reset()，旧 AudioContext 被关闭。
T3  App 没有收到统一的 playback cancellation；isPlaying/source refs 可能仍显示旧状态。
T4  用户点击播放按钮。
T5  按钮依据 isPlaying=true 走 stopPlayback(true)，而不是启动新播放；这次点击没有声音。
T6  再次点击或进行其他会改变状态的操作后，按钮才可能重新走 startPlayback()。
```

即使旧 source 的 `onended` 在上下文关闭时最终触发，触发时序也不稳定；如果它在新 source 或请求号状态变化后触发，会被现有身份检查忽略。该问题高度符合“自动播放后，真正点击播放按钮没有声音”的第二个现象。

还有一个重要边界：`requestReset()` 目前在导入和导出前已经先调用了 `stopPlayback()`，所以这两个路径相对安全；设置抽屉的 `onDialogClosed` 只请求重置，没有先停止播放，是最明显的未闭合路径。

## 发现三：source 身份登记晚于 `source.start()`

**位置：** `src/App.tsx:691-716`

当前顺序是：

```ts
source.onended = ...;
startedAtRef.current = context.currentTime;
...
source.start(0, editedOffset);
sourceNodeRef.current = source;
playbackContextRef.current = context;
setIsPlaying(true);
```

`onended` 的第一行要求：

```ts
if (sourceNodeRef.current !== source || playbackRequest !== playbackRequestRef.current) return;
```

如果 `editedOffset` 非常接近 `editedBuffer.duration`，source 剩余播放时间可能极短。某些 WebAudio/WebKit 时序下，`onended` 有机会在 `sourceNodeRef.current` 登记前执行。此时回调直接返回，之后代码又把已经结束的 source 写入 ref 并把 `isPlaying` 设为 true，形成：

- 实际没有声音。
- 播放按钮显示暂停状态。
- 点击一次只会停止/清理这个“僵尸” source。
- 再点击一次才真正启动新的播放。

这条路径也能独立解释“点击播放无声，需要多做一步”，尤其当延迟旧请求把 offset 带到音频末尾时。它不太可能单独解释长时间后自动播放，因此更像是第二阶段的状态放大器。

## 发现四：上下文挂起、关闭和设备变化没有统一观测

**位置：** `src/audio/audioContextManager.ts`、`src/App.tsx:678-681`

播放只在启动时检查一次：

```ts
if (context.state === "suspended") await context.resume();
```

代码没有监听：

- `AudioContext` 的 `statechange`。
- 窗口 `visibilitychange`、`pagehide`、`focus`、`blur` 后的音频状态。
- 系统默认输出设备改变或 WebKit 音频后端重置。

长时间运行后，浏览器/WebKit 可能将上下文挂起，或系统音频设备发生变化。若 `resume()` 在非用户手势的迟到异步回调中执行，可能失败、拒绝或恢复到无有效输出的状态。当前 catch 会设置错误消息，但如果请求号已经被其他操作递增，则会静默返回（`src/App.tsx:727-730`），这会让用户只看到“点击后没声音”而没有错误提示。

这更像环境触发条件，不是单独的自动播放源；它会显著提高前述竞态发生的概率。

## 发现五：键盘监听每次渲染重绑旧闭包

**位置：** `src/App.tsx:1026-1079`

快捷键 effect 没有依赖数组：

```ts
useEffect(() => {
  ...
  window.addEventListener("keydown", handleShortcut);
  return () => window.removeEventListener("keydown", handleShortcut);
});
```

这不等同于必然的永久监听器泄漏，因为 React 会在下一次 effect 建立时清理前一次监听器。但每个监听器都捕获当次渲染的：

- `selectedTrack`。
- `isPlaying`。
- `settingsOpen`。

在高频进度更新、异步状态提交或被动 effect 尚未完成切换的窗口内，旧 handler 仍可能处理键盘事件。一个旧 handler 可能认为仍未播放并再次调用 `startPlayback()`，另一个新 handler 可能认为正在播放并调用 `stopPlayback()`。请求号通常会取消其中一个，但这会制造随机的启动/停止竞态。

这条路径仍然需要键盘事件，不能单独证明“完全无用户输入时”的自动播放；它是现有代码中最明显的隐式输入竞态之一。

## 已排除或暂未发现的路径

静态搜索覆盖 `src/**/*.{ts,tsx}` 和 Rust 音频代码后，未发现：

- 任何 `autoplay` 属性或媒体元素播放。
- 任何后台轮询/延时任务调用播放。
- 任何录音、归一化、降噪、导出完成回调调用播放。
- Rust 输出播放线程、`rodio`/`cpal` 输出 sink 或系统媒体控制回调。
- `requestAnimationFrame` 中创建新的 source；现有 RAF 只更新 `currentTime`。

因此不能把问题归因于“最后一条音频被 React 重新选中后自动播放”。选中状态本身只影响渲染和后续调用，真正的声音仍必须经过 `source.start()`。

## 为什么现有测试没有暴露问题

当前测试全部通过，但覆盖不足：

- `tests/playbackUtils.test.mjs` 只测试纯函数 `savedPlaybackPosition()`。
- `tests/audioContextManager.test.mjs` 测试了上下文创建、重置串行化和解码 operation 的等待，但没有把正在播放的 source 纳入 manager 生命周期。
- `tests/components/App.test.tsx` 只测试导入后删除源/轨道；其中 `FakeAudioContext` 没有实现 `createBufferSource()`，所以没有真正跑过播放流程。
- `tests/components/WaveformRow.test.tsx` 测试波形手势回调，但没有挂载 App 验证 `onSeek` 到 `startPlayback` 的异步竞态。

已运行验证：

```text
pnpm test
43 tests passed

pnpm build
TypeScript check and Vite production build passed
```

本次没有修改播放代码，也没有完成真实桌面 WebKit 音频设备上的运行时复现；因此测试通过只能说明现有纯函数、组件交互和构建没有回归，不能证明播放生命周期正确。

## 建议的运行时取证

在实际修复前，最有价值的是给每一次播放意图和上下文生命周期加结构化日志。日志应至少包含：

- `startPlayback` 的入口来源：播放按钮、键盘、波形 seek、J、L。
- 每次播放请求号、`selectedTrackId`、传入 offset。
- `getAudioContext` 返回时的上下文对象 ID、`state` 和是否发生过 reset。
- `resume()` 开始/结束/异常及结束后的 `state`。
- `source.start()` 前后的 source ID、轨道 ID、edited offset、剩余时长。
- `stopPlayback()` 的调用来源和是否保存位置。
- `requestReset()`、实际关闭旧上下文、创建新上下文的时间戳及上下文 ID。
- `source.onended` 是否触发，以及触发时 ref/source/request 是否匹配。
- `AudioContext.statechange`、窗口焦点/可见性变化和系统音频设备变化。

建议把日志中的“入口来源”和“请求号”作为同一条事件链输出。若自动播放事件前没有新的入口事件，说明是已有异步调用迟到；若有入口事件，则可以进一步判断是重复/延迟键盘或鼠标事件。

## 建议的修复方向（本次未实施）

1. 把播放控制收敛为一个明确的 playback controller/状态机；播放意图必须携带轨道 ID、buffer 版本和 context generation，并在真正 `source.start()` 前再次验证这些值仍然有效。
2. 上下文 reset 必须成为播放生命周期的一部分：重置前先取消播放并递增取消代数，或让 manager 通过明确接口通知播放控制器，不能在 source 仍然存在时直接关闭上下文。
3. 将 source 写入 `sourceNodeRef` 后再调用 `source.start()`，并对剩余时长为零/极短的 offset 做统一边界处理。
4. 不要用可能过期的 `isPlaying` 闭包决定 toggle；使用稳定的播放状态 ref/控制器状态，并确保快速连续点击只有一个有效启动请求。
5. 键盘 effect 使用稳定监听器读取 refs，或明确列出依赖并保证旧 handler 不会在异步播放操作中继续生效。
6. 对“选择轨道、上下文 reset、窗口失焦、音频上下文 statechange、组件卸载”统一调用取消播放意图，而不是只在部分 UI 操作中调用 `stopPlayback()`。
7. 为以下场景补充可控测试：延迟 `getAudioContext`、延迟 `resume`、reset 期间播放、切换轨道后旧请求恢复、source 在 ref 登记前结束、上下文关闭后再次点击播放、快速双击播放按钮。

## 当前判断的置信度

| 判断 | 置信度 | 说明 |
| --- | --- | --- |
| 声音只能由 `source.start()` 触发，代码没有独立自动播放定时器 | 高 | 已定位唯一启动点并搜索相关调用 |
| context reset 可留下失效播放状态并导致后续点击无声 | 高 | 当前 manager reset 不知道播放 source，设置对话框路径未先 stop |
| 长时间后迟到的旧 `startPlayback()` 可播放旧选中轨道 | 中高 | 异步闭包和取消代数不完整；需运行时日志确认是否存在长等待 |
| `source.start()` 前登记 ref 的时序可造成僵尸播放状态 | 中 | 依赖 source 极短播放时长和 WebKit 事件调度 |
| 键盘监听重绑是本次报告中自动播放的直接来源 | 中低 | 仍需要键盘事件，但可能制造重复/错序启动 |
| 单纯“AudioContext 使用时间太长”会自行调用播放 | 低 | 当前代码没有这样的调用路径；它更可能是触发上下文挂起/重置的条件 |

## 工作区状态

分析开始和结束时未发现已有未提交代码改动。本次新增文件只有本报告；未修改或删除其他文件。
