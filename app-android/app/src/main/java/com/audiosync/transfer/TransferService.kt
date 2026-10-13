package com.audiosync.transfer

import android.app.Notification
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import androidx.core.app.NotificationCompat
import com.audiosync.AudioSyncApp
import com.audiosync.JniBridge
import com.audiosync.MainActivity
import com.audiosync.R

/**
 * T103: 传输前台服务（foregroundServiceType="dataSync"）
 *
 * 后台 P2P 传输期间保持存活；任务完成后自行停止。
 * 电池优化白名单被拒绝时降级：仅在应用前台运行传输，不崩溃。
 */
class TransferService : Service() {

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START -> {
                startForeground(NOTIFICATION_ID, buildNotification())
                JniBridge.request("""{"type":"start_transfer"}""")
            }
            ACTION_STOP -> {
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
            }
        }
        return START_NOT_STICKY
    }

    private fun buildNotification(): Notification {
        val openApp = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE
        )
        return NotificationCompat.Builder(this, AudioSyncApp.CHANNEL_TRANSFER)
            .setSmallIcon(android.R.drawable.stat_sys_download)
            .setContentTitle(getString(R.string.app_name))
            .setContentText(getString(R.string.transfer_notification_text))
            .setContentIntent(openApp)
            .setOngoing(true)
            .build()
    }

    override fun onBind(intent: Intent?) = null

    companion object {
        private const val NOTIFICATION_ID = 1002
        const val ACTION_START = "com.audiosync.action.START_TRANSFER"
        const val ACTION_STOP = "com.audiosync.action.STOP_TRANSFER"

        fun start(context: Context) {
            context.startForegroundService(
                Intent(context, TransferService::class.java).apply { action = ACTION_START }
            )
        }

        fun stop(context: Context) {
            context.startService(
                Intent(context, TransferService::class.java).apply { action = ACTION_STOP }
            )
        }
    }
}
