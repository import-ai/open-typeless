# Open Typeless
Open Typeless, deploy once, using everywhere.

## 自动构建

GitHub Actions 在 PR、`main` 推送、`v*` 标签推送和手动触发时运行：

- **Desktop build**：使用 GitHub 托管 runner，构建 macOS Apple Silicon / Intel 的
  DMG，以及 Windows x64 的 NSIS EXE / MSI。在对应运行的 **Artifacts** 中下载
  `open-typeless-macos-arm64`、`open-typeless-macos-x64` 或 `open-typeless-windows-x64`。
  当前未配置 Developer ID 签名、公证或 Windows 代码签名。
- **Server build**：先运行 Go 测试和 vet，再构建 `linux/amd64`、`linux/arm64`
  镜像。PR 只构建，其他触发使用 `GITHUB_TOKEN` 推送到
  `ghcr.io/lucienshui/open-typeless`，不需要 Docker Hub 凭据。
  `main` 分支对应 `:main`；例如 `v0.1.0` 标签生成 `:0.1.0`、`:0.1` 和 `:latest`
  （预发布版本不更新 `latest`）。手动运行使用所选分支或版本标签。

发布客户端前同步更新 `tauri-client/tauri.conf.json`、`tauri-client/Cargo.toml`
和 `tauri-client/Cargo.lock` 中的应用版本；服务端健康检查版本在
`internal/buildinfo/version.go` 中维护。

服务端容器只包含 Business Server，ASR 服务需单独部署，例如：

```sh
docker run --rm -p 8080:8080 \
  -e INFERENCE_URL=http://your-asr-host:18080 \
  ghcr.io/lucienshui/open-typeless:main
```

`INFERENCE_URL` 必须是容器内可访问的 ASR 地址，不能用 `localhost` 指代宿主机。
本地镜像可通过 `docker build -t open-typeless .` 构建。

## 本地第一阶段

Business Server 是 `cmd/server`，把客户端的 multipart 上传转换为当前已验证的
Qwen3-ASR HTTP 接口调用。启动：

```sh
INFERENCE_URL=http://localhost:18080 go run ./cmd/server
```

接口为 `POST /api/v1/recognitions`，字段 `audio`（WAV/MP3/M4A/WebM）、可选
`language` 和 `hotwords`。响应包含 PRD 约定的 `raw_text`、`polished_text`、
`language` 和 `duration_ms`。当前没有接入润色模型，因此 `polished_text` 与
`raw_text` 相同。健康检查为 `GET /api/v1/health`，返回 `{"status":"ok","version":"v0.1.0"}`。调试服务使用相同路由，额外返回 `audio_dir`。

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

应用图标的唯一源文件是 `tauri-client/icons/icon.svg`，底图、声波和猫头为三个独立分组。
`tauri dev`、`tauri build` 和 GitHub 桌面构建会从该 SVG 生成 PNG、macOS ICNS 和 Windows ICO，
输出到已忽略的 `tauri-client/icons/generated/`，打包配置统一使用这些生成文件。
在 `tauri-client/` 中运行 `npm run generate:icons` 可单独生成；直接运行 Cargo 构建或测试前也需先生成图标。
旧 PNG、设计方案和历史导出文件保存在 [图标归档](docs/archive/icons/2026-09-29/README.md)，不参与构建。

浏览器访问 `http://localhost:5173/?view=pill-debug` 可单独调整 pill。

通过 `npm run tauri:debug` 启动时，主窗口的“开发者选项”可显示、隐藏原生 pill，并切换未连接、未就绪、已就绪、识别中四态。
点击状态会直接显示对应外观；该预览不启用麦克风、不调用 ASR，录音和识别期间不可用。
原生外观回归：检查“识别中 → 未连接”和“识别中 → 已就绪 → 未连接”，胶囊左右圆角及底边应完整。
胶囊和识别圆形保留各自固定尺寸的元素，通过透明度切换；避免复用同一个元素改变尺寸造成 macOS 裁切残留。

Pill 首次显示在主屏幕水平中央，底边距离屏幕底部 128 个逻辑像素。波形、右侧等待图标和识别中的圆形均可拖动，光标显示抓手；拖动位置保留至退出应用。
macOS 上 pill 接受首次鼠标点击且不接管键盘焦点。Pill 显示期间按 Esc 取消当前录音或识别，隐藏后释放 Esc；取消会终止客户端请求并禁止旧会话结果粘贴。

新增 shadcn 组件：在 `tauri-client/` 运行 `npx shadcn@latest add <组件名>`。
组件源码保存在 `frontend/src/components/ui/`。
集成依据和目录约定见 [前端开发说明](docs/frontend.md)。

macOS 默认使用 `RCommand`：单独按下并松开右 Command 开始录音，再次单独松开停止并识别。
按住期间使用其他按键、修饰键或点击鼠标会取消本次快捷键触发，因此右 Command+C 等组合不触发录音。
通过原生 AppKit 本地及全局事件监听实现，需辅助功能权限；未授权时界面显示提示，授权后重启应用。
Windows 默认使用单独右 Control（`RControl`），通过原生键盘监听识别独立按下。
设置界面直接录入单键或常规组合键，松开后立即生效（快捷键和后端地址自动保存，下次启动恢复）。
停止录音后生成临时 WAV，上传到 Business Server，收到 `raw_text` 后写入
剪贴板并模拟 `Ctrl/Command+V` 粘贴到当前窗口。服务端地址默认是
空，在主窗口的“设置”页签填写后端地址，回车或移开焦点自动生效。本地服务可填写 `http://127.0.0.1:8080/api/v1`，反向代理可填写 `https://example.com/api/v1`。客户端将该地址作为完整 API base URL，仅追加 `/recognitions`。

macOS 首次运行需要在“隐私与安全性”中允许麦克风，并给应用辅助功能权限，
这样系统才允许全局热键和向当前窗口发送粘贴按键。

快捷键设置只响应输入框内的点击。点击后直接按键，松开后自动生效，无需保存按钮。
支持普通单键，以及左右 Command、Ctrl、Shift、Alt 的单独按下；修饰键与普通键组合继续可用。
录入时点击其他位置取消。Esc 也可作为激活键；Pill 显示期间 Esc 仍用于取消本次识别。
后端地址默认为空，未配置时不会启动录音。普通 `tauri dev` 不显示开发者选项。

客户端应用标识为 `pro.omnibox.open-typeless`，设置和词典统一保存在用户主目录下的
`.open-typeless`，不依赖应用标识。macOS 设置路径为 `~/.open-typeless/settings.json`，
Windows 为 `%USERPROFILE%\.open-typeless\settings.json`。
旧版本使用系统应用配置目录：macOS 为
`~/Library/Application Support/com.opentypeless.client/`，Windows 为
`%APPDATA%\com.opentypeless.client\`。退出应用后，将旧目录中的 `settings.json`
和 `dictionary.json` 复制到 `.open-typeless` 即可保留数据；不会自动迁移。
保存使用临时文件并原子替换，写入失败会显示错误并保留原设置。
启动时恢复设置；后端地址为空显示“后端地址未设置”，非空时请求 `<base_url>/health`，检查成功后才显示“就绪”。空闲时每 30 秒复查，开始录音前也会检查。

### 个人词典

主窗口的“词典”页签支持添加、编辑、搜索、单条删除和批量删除。词条按添加时间倒序排列，编辑保持原位置；重复判断忽略英文大小写。搜索后全选只选中当前结果，修改搜索内容会清空选择。添加和编辑支持回车提交、Esc 取消，中文输入法确认候选词的回车不会提交。输入词汇、搜索词典期间暂挂语音激活快捷键，避免单键快捷键误触。

词典保存在与设置同目录的 `dictionary.json`，使用版本化 JSON 和原子写入，重启后恢复。浏览器预览使用独立的 localStorage，不会修改桌面词典。

每次开始录音固定一份词典快照，停止后将所有词条以换行分隔的 `hotwords` 字段随音频上传，Go server 转发为 ASR 的 `context`。修改从下一次录音生效，继续原样返回并粘贴 `raw_text`，不做强制替换。词典是识别提示，实际效果取决于 ASR 模型。

当前完整词典含分隔符最多 **1000 UTF-8 字节**，与 Go 接口限制一致。添加、编辑时提前校验，超限明确提示，不静默截断。损坏的词典文件会报错并保留原文件。
