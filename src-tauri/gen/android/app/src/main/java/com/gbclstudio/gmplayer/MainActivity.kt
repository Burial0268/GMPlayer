package com.gbclstudio.gmplayer

import android.Manifest
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.os.Build
import android.os.Bundle
import android.os.SystemClock
import android.util.Log
import android.view.View
import android.webkit.JavascriptInterface
import android.webkit.RenderProcessGoneDetail
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import java.util.Locale

class MainActivity : TauriActivity() {

    companion object {
        /** 崩溃恢复配额的观察窗口。 */
        private const val RENDER_GUARD_WINDOW_MS = 60_000L
        /** 窗口内最多自愈几次，超出则交回系统。 */
        private const val RENDER_GUARD_MAX_RECOVERIES = 3

        @Volatile
        private var renderRecoveryCount = 0

        @Volatile
        private var renderRecoveryWindowStart = 0L
    }

    private var webView: WebView? = null
    private var lastInsetsCss: String? = null

    // 申请通知权限（Android 13+ 必需）
    private val requestPermissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) { isGranted: Boolean ->
        if (isGranted) {
            Log.d("MainActivity", "Notification permission granted")
        } else {
            Log.w("MainActivity", "Notification permission denied")
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        // 启用全屏设计
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)

        val rootView = findViewById<View>(R.id.main) ?: findViewById<View>(android.R.id.content)

        // Edge-to-edge 安全区处理。
        //
        // 这里必须把 inset 主动推给 WebView：Android WebView 的 env(safe-area-inset-*)
        // 只反映 display cutout（刘海/挖孔），从不包含状态栏与底部手势条。所以在
        // enableEdgeToEdge() 之下，底部手势条实际占着位置，而 CSS 侧
        // env(safe-area-inset-bottom) 仍然是 0 —— 这就是移动端底栏偏移量的来源。
        // 用 systemBars + displayCutout 的并集覆盖 --app-safe-area-*，让 CSS 拿到真值。
        ViewCompat.setOnApplyWindowInsetsListener(rootView) { v, insets ->
            publishSafeAreaInsets(insets)
            ViewCompat.onApplyWindowInsets(v, insets)
        }

        checkAndRequestPermissions()
    }

    override fun onWebViewCreate(webView: WebView) {
        this.webView = webView
        installRenderProcessGuard(webView)
        // 拉取通道。推送依赖 document 已经存在，而首次 inset 回调可能早于文档解析完成，
        // 那一次推送会静默丢失。暴露一个同步 getter，让前端启动时能主动兜底读一次。
        webView.addJavascriptInterface(SafeAreaBridge(), "AndroidSafeArea")
        lastInsetsCss?.let { pushSafeAreaCss(it) }
    }

    /**
     * 把 WebView 渲染进程崩溃从「杀掉整个进程」降级为「重建 Activity」。
     *
     * wry 没有覆盖 onRenderProcessGone，而 Android 对 targetSdk >= N 的默认行为是
     * 终止整个应用进程 —— 那会把 Rust 音频线程和前台服务一起带走，正在放的歌直接断。
     * 返回 true 表示我们自己处理，进程得以存活；随后 recreate() 沿 Tauri 正常的
     * Activity 路径重建 WebView，前端把它当作一次普通重载，会通过
     * audio_get_session 接管仍在播放的会话（见 NativeSessionAdopt.ts）。
     *
     * 【维护约定】RustWebViewClient 是 final 类，且位于被 gitignore 的 generated/
     * 目录（由 tauri 重新生成），所以包一层是唯一可用的挂载点。下面转发的是 wry
     * 当前覆盖的全部五个方法。**升级 tauri/wry 时必须核对这个列表**：若上游新增了
     * 覆盖而这里没跟上，该方法会静默退回 WebViewClient 的默认实现 —— 其中
     * shouldInterceptRequest 一旦丢失，资源加载会整个失效。
     */
    private fun installRenderProcessGuard(webView: WebView) {
        // getWebViewClient() 与 onRenderProcessGone 均为 API 26+；低版本无从挂载，
        // 但那里也不存在这个回调，行为与改动前一致。
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val delegate = try {
            webView.webViewClient
        } catch (e: Exception) {
            Log.w("MainActivity", "Cannot read WebViewClient, render guard not installed: ${e.message}")
            return
        }
        if (delegate is RenderProcessGuardClient) return
        webView.webViewClient = RenderProcessGuardClient(delegate) { detail ->
            if (!shouldRecoverFromRenderCrash()) {
                // 连续崩溃：如果渲染进程每次起来都立刻死（WebView 数据损坏之类），
                // 无限 recreate() 会变成一个既不出界面又耗电的死循环，比原先「崩一次」
                // 更糟。超过配额就交回系统 —— 即改动前的行为。
                Log.e("MainActivity", "Render process crash loop detected — giving up recovery")
                return@RenderProcessGuardClient false
            }
            Log.e(
                "MainActivity",
                "WebView render process gone (didCrash=${detail.didCrash()}) — recreating activity",
            )
            // post 到主循环：回调返回前不能碰这个已死的 WebView。
            window.decorView.post { recreate() }
            true
        }
    }

    /**
     * 崩溃恢复配额。Activity 会被 recreate()，实例字段随之重置，所以计数必须放在
     * companion object 里（进程级）。
     */
    private fun shouldRecoverFromRenderCrash(): Boolean {
        val now = SystemClock.elapsedRealtime()
        if (now - renderRecoveryWindowStart > RENDER_GUARD_WINDOW_MS) {
            renderRecoveryWindowStart = now
            renderRecoveryCount = 0
        }
        renderRecoveryCount++
        return renderRecoveryCount <= RENDER_GUARD_MAX_RECOVERIES
    }

    private class RenderProcessGuardClient(
        private val delegate: WebViewClient,
        /** 返回 true 表示已接管（进程存活）；false 表示放弃，交回系统默认行为。 */
        private val onGone: (RenderProcessGoneDetail) -> Boolean,
    ) : WebViewClient() {
        override fun shouldInterceptRequest(
            view: WebView,
            request: WebResourceRequest,
        ): WebResourceResponse? = delegate.shouldInterceptRequest(view, request)

        override fun shouldOverrideUrlLoading(
            view: WebView,
            request: WebResourceRequest,
        ): Boolean = delegate.shouldOverrideUrlLoading(view, request)

        override fun onPageStarted(view: WebView, url: String, favicon: Bitmap?) =
            delegate.onPageStarted(view, url, favicon)

        override fun onPageFinished(view: WebView, url: String) =
            delegate.onPageFinished(view, url)

        override fun onReceivedError(
            view: WebView,
            request: WebResourceRequest,
            error: WebResourceError,
        ) = delegate.onReceivedError(view, request, error)

        override fun onRenderProcessGone(view: WebView, detail: RenderProcessGoneDetail): Boolean {
            // true = 已处理，别杀进程；false = 退回系统默认（终止进程）。
            return onGone(detail)
        }
    }

    private inner class SafeAreaBridge {
        /** "top,right,bottom,left"，单位 CSS px；尚无数据时返回空串。 */
        @JavascriptInterface
        fun get(): String = lastInsetsCss ?: ""
    }

    private fun publishSafeAreaInsets(insets: WindowInsetsCompat) {
        val bars = insets.getInsets(
            WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
        )
        val density = resources.displayMetrics.density.takeIf { it > 0f } ?: 1f
        // px -> CSS px。WebView 的 CSS 像素跟 density 挂钩，直接用原始 px 会在
        // 高 DPI 设备上放大好几倍。
        val css = String.format(
            Locale.US,
            "%.2f,%.2f,%.2f,%.2f",
            bars.top / density,
            bars.right / density,
            bars.bottom / density,
            bars.left / density,
        )
        if (css == lastInsetsCss) return
        lastInsetsCss = css
        pushSafeAreaCss(css)
    }

    private fun pushSafeAreaCss(css: String) {
        val view = webView ?: return
        val parts = css.split(",")
        if (parts.size != 4) return
        val script = """
            (function(){
              var r = document.documentElement;
              if (!r) return;
              r.style.setProperty('--app-safe-area-top', '${parts[0]}px');
              r.style.setProperty('--app-safe-area-right', '${parts[1]}px');
              r.style.setProperty('--app-safe-area-bottom', '${parts[2]}px');
              r.style.setProperty('--app-safe-area-left', '${parts[3]}px');
            })();
        """.trimIndent()
        view.post { view.evaluateJavascript(script, null) }
    }

    private fun checkAndRequestPermissions() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            if (ContextCompat.checkSelfPermission(
                    this,
                    Manifest.permission.POST_NOTIFICATIONS
                ) != PackageManager.PERMISSION_GRANTED
            ) {
                requestPermissionLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
            }
        }
    }
}
