# 前端开发

## 集成依据

2026-09-28 查阅官方文档后，采用 React + TypeScript + Vite + shadcn/ui。

- [React 从零构建应用](https://react.dev/learn/build-a-react-app-from-scratch)：Vite 支持 React TypeScript 模板。当前应用是本地桌面客户端，使用客户端渲染，不需要服务端渲染框架。
- [Tauri 的 Vite 集成](https://v2.tauri.app/start/frontend/vite/)：开发时通过 `beforeDevCommand` 启动 Vite、`devUrl` 加载页面，发布时通过 `beforeBuildCommand` 构建并嵌入 `frontendDist`。固定端口并启用 `strictPort`，避免端口自动变化导致 Tauri 连接错误。
- [shadcn 的 Vite 安装](https://ui.shadcn.com/docs/installation/vite)：使用 Tailwind Vite 插件，配置 TypeScript 和 Vite 的 `@/` 别名，通过官方 CLI 生成本地组件源码。
- [Vite 开发要求](https://vite.dev/guide/)：本项目使用 Node 22.12+。

## 目录与约定

保留现有 `tauri-client/src/main.rs` Rust 布局。JavaScript 依赖、脚本、锁文件均在 `tauri-client/`；Vite root 为 `frontend/`，构建产物为 `tauri-client/dist/`。

```text
tauri-client/
  src/main.rs                 Rust 命令与音频采集
  frontend/
    index.html                唯一 HTML 入口
    src/
      main.tsx                按窗口 label 选择界面
      windows/                主界面、pill、debug 预览
      components/
        recording-pill.tsx    正式与预览共用的 pill
        ui/                   shadcn CLI 生成的基础组件
      hooks/                  React 事件订阅与清理
      lib/                    Tauri 命令类型和工具函数
      styles.css              Tailwind 和 shadcn 主题
      pill.css                透明窗口和 pill 专用样式
  components.json             shadcn 配置
  vite.config.ts
  tsconfig.json
```

- 通过 `@tauri-apps/api` 模块调用 Rust；不依赖 `window.__TAURI__`。
- 使用 React StrictMode。异步事件监听必须在卸载、热更新时清理，包括卸载后才完成的订阅。
- 主窗口、正式 pill、debug 窗口只挂载各自的组件，不依赖 CSS 隐藏一整套其他窗口的 DOM。
- 透明背景只作用于 pill 窗口，避免 shadcn 默认 body 背景覆盖桌面。
- Debug 使用模拟音量驱动同一个 pill 组件，不进行录音、上传、粘贴。
- 浏览器预览显示 UI，但录音操作必须在 Tauri 中执行。
- 依赖版本写入 `package-lock.json`；使用 `npm ci` 复现安装。

## 验证

运行 `npm run build` 检查 TypeScript 和生产资源，再运行 `npm run tauri dev` 检查桌面集成。
外观修改需额外打开 `npm run tauri:debug` 并截图验证；浏览器截图不能证明原生窗口透明效果。
