# Windows 任务栏 Peek 材质调查报告

> 2026-07-26 · 结论:在当前窗口架构(透明 tao 窗口 + WebView2 子窗口宿主)下,
> 「实时 peek 材质」与「live 自绘材质不动」**不可兼得**。本文记录全部实验数据、
> 三条已验证的约束定律、被排除的方案,以及唯一的真实时路线(composition hosting)。

## 背景

主窗口的 "system-shell" 材质是自绘 Host Mica Alt:DComp 桌面采样(未文档化
`DwmpCreateSharedMultiWindowVisual`/ordinal 163/164)+ 34px 高斯模糊 + 自定 tint;
采样构建失败的机器回退 SWCA acrylic accent。问题:任务栏 hover 触发的 **peek
(实际窗口预览)完全透明** —— 材质所在的每一层都不参与 DWM 的 peek 合成。

## 三条定律(全部单变量实验验证)

1. **系统 backdrop(DWMWA_SYSTEMBACKDROP_TYPE)需要移除 blur-behind 玻璃纸才会合成。**
   - 玻璃纸在 + SBT 钉 acrylic + 无 accent → live 无任何模糊(SBT 未渲染);
   - 玻璃纸摘 + SBT → backdrop 在所有 pass 合成(peek 有材质)。
2. **WebView2(windowed 宿主)的透明背景依赖父窗口的 blur-behind 玻璃纸。**
   - 摘玻璃纸后,webview 子窗口覆盖区域渲染为不透明黑;resize 暴露出的
     子窗口外区域反而显示原生 backdrop。
3. **SWCA accent(acrylic/gradient)永不参与非 live pass(peek、部分动画场景),
   且 accent 需要玻璃纸才能作为 live 模糊工作。**

三者合取:live 材质(需要玻璃纸)与 peek 材质(需要摘玻璃纸)互斥。∎

## 实验矩阵(时序)

| # | 配置 | live | peek | 结论贡献 |
|---|------|------|------|----------|
| 基线 | 玻璃纸 + accent(SWCA 回退) | ✓ | ✗ 透明 | 问题现场 |
| v1-v2 | +常驻 gradient accent / +SBT Tabbed+扩展 | ✓ | ✗ | accent 不进 peek |
| v3 | iconic bitmap(纯色底) | ✓ | ✓ 快照 | iconic 机制可行 |
| #8 | 摘玻璃纸+扩展+SBT Tabbed+accent | ✗ solid | ✓ solid | 摘玻璃纸→accent 死、SBT 活 |
| #12 | 玻璃纸+SBT pin(accent живой) | ✓ | ✗ | — |
| #13 | 玻璃纸+SBT+整窗扩展 | ✓ | ✗ | 扩展无害也无效 |
| Route A | 摘玻璃纸+SBT+DComp tint-only | ✗ webview 黑 | 部分 | 定律 2 现场 |
| #15 | iconic bitmap(模糊壁纸底,150ms) | ✓ | ✓ 快照 | 被否决:非实时 |
| #16 | 玻璃纸+SBT+无 accent+DComp tint | ✗ 无模糊 | — | **定律 1 定谳** |

## 已排除的方案

- **常驻 accent / SBT / 整窗框架扩展(Electron 三件套)**:被定律 1/2/3 全灭。
  Electron 能行是因为 Chromium 是顶层窗口本身(WS_EX_NOREDIRECTIONBITMAP,
  内容即合成树),没有"父窗口玻璃纸 + 子窗口 webview"这一层。
- **Tauri `Effect::Acrylic` / window-vibrancy**:Tauri 的 Acrylic 实现走 SWCA
  accent(证据:API 接受 tint color,SBT 无颜色参数),peek 空洞与自绘方案相同;
  Mica/Tabbed 类 SBT 效果在本机构建上同样被玻璃纸压制。
- **iconic bitmap(DWMWA_FORCE_ICONIC_REPRESENTATION + WM_DWMSENDICONIC*)**:
  技术上完全可行(v3/v15 已实现:窗口截图 + 模糊壁纸底 + live tint,150ms
  失效刷新),但本质是快照流,刷新上限 ~30fps 且非 DWM 原生实时,被产品层面否决。
  实现代码可从本调查会话的历史中恢复(peek_preview.rs + minimize_guard 接线)。
- **9 参数 ordinal-164 变体**:在本机上返回 S_OK 但不驱动共享视觉(假成功),
  会把采样层"演活"导致 backing+tint 纯色。不要盲用;真正的参数形态需在目标机
  运行 `cargo test -- --ignored probe_update_shared_variants --nocapture` 定型。

## 唯一的真实时路线(未来工作)

**WebView2 composition hosting(CoreWebView2CompositionController)**:webview
内容作为 DComp 视觉进入应用自己的合成树,消灭子窗口与玻璃纸依赖 —— 与 Electron
同构。窗口可改为非透明 + SBT acrylic 底 + 自绘层,live/peek/动画全部 DWM 原生
实时。代价:wry 层大改(输入转发、命中测试、IME),是独立立项的工程。

## 附:本机(调查环境)特征

- `DwmpUpdateSharedMultiWindowVisual`(ordinal 164,8 参数)返回 E_INVALIDARG
  (0x80070057),自绘采样层不可用,基线材质实际为 SWCA acrylic 回退;
- 最小化后的 peek 显示不透明遮罩,来源是 DWM 缓存的最小化前帧,并非新鲜合成。
