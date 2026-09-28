# OpenTypeless
Open Typeless, deploy once, using everywhere.

## 本地第一阶段

Business Server 是 `cmd/server`，把客户端的 multipart 上传转换为当前已验证的
Qwen3-ASR HTTP 接口调用。启动：

```sh
INFERENCE_URL=http://localhost:18080 go run ./cmd/server
```

接口为 `POST /v1/recognitions`，字段 `audio`（WAV/MP3/M4A/WebM）、可选
`language` 和 `hotwords`。响应包含 PRD 约定的 `raw_text`、`polished_text`、
`language` 和 `duration_ms`。当前没有接入润色模型，因此 `polished_text` 与
`raw_text` 相同。

调试客户端录音时可以临时启动 `cmd/debug-server` 接管 8080。它不会调用 ASR，
会把收到的音频保存到 `/tmp/open-typeless-debug`，并始终返回 `raw_text: "foo"`：

```sh
go run ./cmd/debug-server
ls -lh /tmp/open-typeless-debug
```

Tauri 客户端在 `tauri-client/`，React + TypeScript 前端在
`tauri-client/frontend/`，使用 Vite、Tailwind CSS 和 shadcn/ui。
安装 Node 22.12+、Rust 和 Tauri 2 系统依赖后运行：

```sh
cd tauri-client
npm install
npm run tauri dev
```

`tauri dev` 自动启动 Vite，前端修改通过 HMR 更新。其他开发命令：

```sh
npm run tauri:debug  # 桌面常驻展示 pill 四态
npm run dev          # 仅启动浏览器预览，不调用麦克风
npm run typecheck    # TypeScript 严格检查
npm run build        # 检查类型并生成 dist/
npm run tauri build  # 构建桌面程序（自动运行前端构建）
```

浏览器访问 `http://localhost:5173/?view=pill-debug` 可单独调整 pill。

主窗口的“开发者选项”可显示、隐藏原生 pill，并切换未连接、未就绪、已就绪、识别中四态。
点击状态会直接显示对应外观；该预览不启用麦克风、不调用 ASR，录音和识别期间不可用。
原生外观回归：检查“识别中 → 未连接”和“识别中 → 已就绪 → 未连接”，胶囊左右圆角及底边应完整。
胶囊和识别圆形保留各自固定尺寸的元素，通过透明度切换；避免复用同一个元素改变尺寸造成 macOS 裁切残留。

新增 shadcn 组件：在 `tauri-client/` 运行 `npx shadcn@latest add <组件名>`。
组件源码保存在 `frontend/src/components/ui/`。
集成依据和目录约定见 [前端开发说明](docs/frontend.md)。

客户端默认注册 `Command+Shift+Space`（macOS）或 `Control+Shift+Space`（Windows）全局热键。
设置界面可以修改快捷键，保存后立即生效。当前 Tauri 的 `global-hotkey` 库要求快捷键包含一个主键，
因此暂不支持只有右 Command/右 Control 的单修饰键热键。
第一次按下开始采集麦克风，再次按下停止并生成临时 WAV，上传到 Business Server，收到 `raw_text` 后写入
剪贴板并模拟 `Ctrl/Command+V` 粘贴到当前窗口。服务端地址默认是
`http://127.0.0.1:8080`，可在 Rust 状态中通过 `set_server_url` 调整。

macOS 首次运行需要在“隐私与安全性”中允许麦克风，并给应用辅助功能权限，
这样系统才允许全局热键和向当前窗口发送粘贴按键。
