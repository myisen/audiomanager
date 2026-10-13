package com.audiosync

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.PowerManager
import android.provider.Settings

/**
 * T103: 电池优化白名单引导
 *
 * 退出条件要求：拒绝电池白名单时降级为仅前台运行而非崩溃。
 * 本类只做检测与引导，绝不强制申请：
 * - [isIgnoringBatteryOptimizations] 为 false 时，后台传输/播放
 *   由前台服务承载（应用可见期间），一切功能可用但不做后台保活。
 */
object BatteryOptimizationHelper {

    /** 是否已在电池优化白名单中 */
    fun isIgnoringBatteryOptimizations(context: Context): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.M) return true
        val pm = context.getSystemService(Context.POWER_SERVICE) as PowerManager
        return pm.isIgnoringBatteryOptimizations(context.packageName)
    }

    /**
     * 生成跳转系统「忽略电池优化」授权界面的 Intent。
     * 调用方用 startActivity 尝试启动；用户拒绝时 catch ActivityNotFoundException
     * 并降级（返回 null 表示无法引导，调用方直接进入前台运行模式）。
     */
    fun buildWhitelistIntent(context: Context): Intent? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.M) return null
        return try {
            Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS).apply {
                data = Uri.parse("package:${context.packageName}")
            }
        } catch (_: Exception) {
            null
        }
    }

    /**
     * 引导结果处理：无论用户是否同意，应用都继续运行。
     * @return true 表示用户同意（可后台保活），false 表示拒绝（降级为仅前台运行）
     */
    fun handleGuideResult(context: Context): Boolean =
        isIgnoringBatteryOptimizations(context)
}
