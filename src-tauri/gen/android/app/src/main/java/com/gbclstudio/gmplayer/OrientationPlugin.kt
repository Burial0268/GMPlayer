package com.gbclstudio.gmplayer

import android.app.Activity
import android.content.pm.ActivityInfo
import android.util.Log
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin

@InvokeArg
class SetOrientationArgs {
    lateinit var orientation: String
}

/**
 * 屏幕方向策略。
 *
 * 默认不允许系统传感器随意翻转我们：manifest 里 activity 起手就是 `nosensor`，
 * 只有显式调用本插件的位置（目前只有视频全屏）才会换向。所以这里没有
 * 「跟随传感器」的隐式回退——未知取值一律按 [DEFAULT] 处理，宁可不转，
 * 也不要把控制权交还给系统。
 */
internal object OrientationPolicy {
    /** 设备自然方向，传感器关闭。手机上是竖屏，TV / 车机上是横屏。 */
    const val DEFAULT = "default"
    const val PORTRAIT = "portrait"
    const val LANDSCAPE = "landscape"
    /** 唯一一个交还控制权的取值，且尊重系统的「自动旋转」开关。 */
    const val AUTO = "auto"

    fun toActivityInfo(orientation: String): Int? = when (orientation) {
        // 用自然方向而不是硬锁 PORTRAIT：AndroidTV（manifest 里声明了 leanback）
        // 的自然方向是横屏，硬锁竖屏会把整个 UI 转 90 度。
        DEFAULT -> ActivityInfo.SCREEN_ORIENTATION_NOSENSOR
        PORTRAIT -> ActivityInfo.SCREEN_ORIENTATION_PORTRAIT
        // SENSOR_LANDSCAPE 而不是 LANDSCAPE：允许在两个横向之间翻转，
        // 但永远不会掉回竖屏——旋转仍然被限制在我们允许的范围内。
        LANDSCAPE -> ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE
        // USER 而不是 SENSOR：SENSOR 会无视用户关掉的「自动旋转」强行跟随传感器。
        AUTO -> ActivityInfo.SCREEN_ORIENTATION_USER
        else -> null
    }
}

@TauriPlugin
class OrientationPlugin(private val activity: Activity) : Plugin(activity) {

    @Command
    fun setOrientation(invoke: Invoke) {
        val args = invoke.parseArgs(SetOrientationArgs::class.java)
        val requested = OrientationPolicy.toActivityInfo(args.orientation)

        if (requested == null) {
            invoke.reject("Unknown orientation: ${args.orientation}")
            return
        }

        activity.runOnUiThread {
            activity.requestedOrientation = requested
            Log.d(TAG, "requestedOrientation = ${args.orientation} ($requested)")
            invoke.resolve()
        }
    }

    private companion object {
        const val TAG = "OrientationPlugin"
    }
}
