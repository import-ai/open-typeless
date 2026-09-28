# 项目协作指南

## 项目现状与边界

Open Typeless 是桌面语音输入工具：快捷键开始录音，结束后一次性上传，识别结果通过剪贴板和模拟粘贴输入当前应用。当前不做流式识别，客户端不加载 ASR 模型。

- 桌面端：Tauri 2 + Rust，前端为 React + TypeScript + Vite + Tailwind CSS + shadcn/ui。
- 业务服务：Go HTTP 服务，接收 multipart 音频并转发给独立的 Python ASR 服务。
- 当前推理调用是 HTTP `/transcribe`，不是 gRPC；尚未接入文本润色，`polished_text` 与 `raw_text` 相同，客户端粘贴 `raw_text`。
- `prd.md` 描述产品目标，包含尚未实现的鉴权、gRPC、润色等能力。以源码和配置判断当前行为，不要把规划当作已实现功能。
- 使用说明见 `README.md`，前端约定见 `docs/frontend.md`，ASR 试验说明见 `deploy/asr/README.md`。部署文档含历史试验记录，当前仓库的 `deploy/asr/compose.yaml` 使用 R2T2；不要仅凭镜像名认定运行的是 Qwen，也不要把仓库配置当作远程运行状态的证明。

## 代码地图

| 路径 | 职责 |
| --- | --- |
| `cmd/server/` | 业务 API、推理转发及 Go 测试 |
| `cmd/debug-server/` | 保存上传音频并固定返回 `foo` 的本地调试服务，不调用 ASR |
| `internal/buildinfo/` | 服务端健康检查版本 |
| `tauri-client/src/main.rs` | Tauri 命令、录音、识别会话、取消、粘贴、窗口和设置集成 |
| `tauri-client/src/modifier_shortcut.rs` | macOS / Windows 原生修饰键监听与独立按键判断 |
| `tauri-client/src/settings_file.rs`、`dictionary.rs` | 设置和词典的校验、持久化及 Rust 测试 |
| `tauri-client/frontend/src/windows/` | 主窗口、pill 和 pill-debug 窗口 |
| `tauri-client/frontend/src/components/` | 业务组件；`ui/` 为 shadcn 基础组件 |
| `tauri-client/frontend/src/lib/desktop.ts` | Tauri 命令封装、跨端类型和浏览器预览分支 |
| `tauri-client/frontend/src/hooks/` | 事件订阅清理、输入期间快捷键暂挂 |
| `tauri-client/tests/` | 基于 Node 内置测试运行器的词典和快捷键测试 |
| `tauri-client/capabilities/`、`tauri.conf.json` | 窗口权限、应用和打包配置 |
| `deploy/asr/` | ASR 试验服务、Compose、探针和性能记录 |
| `.github/workflows/`、`Dockerfile` | 桌面安装包和业务服务容器构建 |

Rust 工程直接位于 `tauri-client/`，不是常见的 `src-tauri/` 布局。JavaScript 依赖和锁文件也位于 `tauri-client/`；Vite root 是 `frontend/`，输出为 `tauri-client/dist/`，`@/` 指向 `frontend/src/`。

## 开发与验证

Go 模块声明 Go 1.23，当前 CI 和 Docker 构建使用 Go 1.25。桌面开发需要 Node 22.12+、Rust 和对应平台的 Tauri 2 系统依赖；原生快捷键和粘贴主要面向 macOS / Windows。

在仓库根目录运行业务服务：

```sh
INFERENCE_URL=http://localhost:18080 go run ./cmd/server
go test ./...
go vet ./...
```

`HTTP_ADDR` 默认 `:8080`，`INFERENCE_URL` 默认 `http://localhost:18080`，`MAX_AUDIO_BYTES` 默认 12 MiB。`go run ./cmd/debug-server` 可替代业务服务调试上传，默认占用同一端口并将音频写入 `/tmp/open-typeless-debug`。

以下命令在 `tauri-client/` 运行：

```sh
npm ci
npm run tauri dev       # 原生桌面开发，自动启动 Vite
npm run dev             # 仅浏览器预览
npm run tauri:debug     # 原生 pill 四态预览和开发者选项
npm run typecheck
npm run build           # TypeScript 检查 + 前端生产构建
node --experimental-strip-types --test tests/*.test.mjs
cargo test --locked
npm run tauri build     # 桌面打包，自动执行前端构建
```

- 按改动范围验证：Go 改动运行 Go 测试和 vet；前端改动运行 build，涉及词典或快捷键逻辑时运行 Node 测试；Rust 改动运行 Cargo 测试。项目没有 `npm test` 脚本。
- Rust 单独构建或测试若提示缺少 `dist/`，先运行 `npm run build`。Cargo 测试也需要平台原生依赖。
- 外观改动需通过 `npm run tauri:debug` 检查原生窗口并截图；`http://localhost:5173/?view=pill-debug` 只用于浏览器预览，不能验证原生透明、焦点或裁切行为。
- 涉及录音、权限、快捷键或粘贴的改动还需原生端到端检查；macOS 需要麦克风和辅助功能权限。不能执行的检查应明确说明，不要将编译通过等同于交互验证通过。

## 需要保持的行为约束

### API 与识别会话

- 客户端设置的是完整 API base URL，例如 `http://127.0.0.1:8080/api/v1`；只追加 `/health` 或 `/recognitions`，不要重复添加 `/api/v1`。
- 后端地址默认为空，未配置时不能录音。空闲时每 30 秒检查健康状态，开始录音前也检查。
- `POST /api/v1/recognitions` 接收 `audio`、可选 `language` 和 `hotwords`；Go 将音频作为原始请求体发送给 ASR `/transcribe`，将 `hotwords` 转为 `context` 查询参数。
- 修改接口时同步检查 Go 响应、Rust 序列化结构、`frontend/src/lib/desktop.ts` 和对应调用方。
- 取消必须终止客户端等待，并阻止已取消或被新会话取代的结果写入剪贴板或粘贴。保留会话 ID 校验和临时录音文件清理，不要只隐藏 UI。

### 快捷键与窗口

- macOS 默认单独右 Command，Windows 默认单独右 Control；独立修饰键在松开时触发，与其他键、修饰键或鼠标组合使用不能误触发录音。保留平台条件编译边界。
- 快捷键录入以及词典文本输入期间暂挂激活快捷键；保留中文输入法组合输入判断，确认候选词的回车不能提交词条。
- Pill 显示期间 Esc 取消录音或识别，隐藏后释放取消快捷键；即使激活键设为 Esc，也要保持该优先级。
- 正式 pill 不接管键盘焦点，支持首次点击与拖动；拖动位置在本次应用运行期间保留。
- Pill 胶囊和识别圆形使用各自固定尺寸的元素，通过透明度切换，避免 macOS 原生窗口裁切残留。回归检查“识别中 → 未连接”和“识别中 → 已就绪 → 未连接”。
- 主窗口、pill、pill-debug 按窗口 label 挂载各自组件；透明背景仅作用于 pill 窗口。预览共用正式 pill 组件，不启动麦克风、ASR 或粘贴。
- 使用 `@tauri-apps/api`，不依赖 `window.__TAURI__`。React StrictMode 下事件订阅须正确清理，包括卸载后才完成的异步订阅。

### 设置与词典

- 设置和词典保存在用户主目录 `~/.open-typeless/`（Windows 为 `%USERPROFILE%\.open-typeless\`），不依赖应用 identifier。旧系统应用目录不会自动迁移。
- 保留原子写入、失败反馈与原数据；损坏的词典不能静默覆盖。浏览器词典使用独立 localStorage，不修改桌面文件。
- 词条重复判断忽略英文大小写；编辑保留原位置，新增词条排在前面。搜索后全选只影响当前结果，修改搜索内容清空选择。
- 完整词典以换行拼接后的上限是 **1000 UTF-8 字节**，包含分隔符，不是 1000 个字符。前端、Rust 和 Go 校验需一致，不静默截断。
- 每次开始录音固定词典快照，修改从下一次录音生效。热词只是 ASR 提示，不强制替换识别结果。

## 构建与维护

- 应用图标的唯一源文件是 `tauri-client/icons/icon.svg`，保留底图、声波、猫头三个独立分组。`tauri dev`、`tauri build` 和桌面 CI 从该 SVG 生成 PNG、macOS ICNS、Windows ICO 到已被 Git 忽略的 `tauri-client/icons/generated/`；打包配置只引用该生成目录，不要手工维护或提交新的生成文件。需要单独生成或直接运行 Cargo 构建/测试前，在 `tauri-client/` 运行 `npm run generate:icons`。旧 PNG、设计方案及历史导出文件归档于 `docs/archive/icons/`，仅供回溯，不参与构建，也不要从归档恢复第二套图标源。
- 新增 shadcn 组件在 `tauri-client/` 运行 `npx shadcn@latest add <组件名>`，沿用现有主题和组件目录。
- 依赖改动同步维护 `package-lock.json` / `Cargo.lock`；不要手工修改 `dist/`、`target/`、`node_modules/` 或 Tauri 生成的 schema 来实现功能。
- 桌面 CI 构建 macOS arm64 / x64 DMG 和 Windows x64 NSIS / MSI；当前未配置代码签名或公证。
- 发布客户端时同步 `tauri.conf.json`、`Cargo.toml` 和 `Cargo.lock` 中的应用版本；服务端版本位于 `internal/buildinfo/version.go`。
- 根目录 Dockerfile 只打包 Go Business Server，不包含 ASR。容器的 `INFERENCE_URL` 必须可从容器内访问。
- 修改启动方式、配置或用户行为时同步相关文档。保留任务开始前的未提交修改，避免无关重构和格式化。

## Commit messages

Use the Conventional Commits format:

```text
feat(foo): bar
```

Keep the type and scope lowercase, and write a concise imperative summary after
the colon.
