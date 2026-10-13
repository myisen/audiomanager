package com.audiosync

import android.app.Application
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Context

/** 应用入口：创建通知渠道（T103/T105 前台服务与媒体通知所需） */
class AudioSyncApp : Application() {
    override fun onCreate() {
        super.onCreate()
        createNotificationChannels()
    }

    private fun createNotificationChannels() {
        val manager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        manager.createNotificationChannel(
            NotificationChannel(
                CHANNEL_PLAYBACK,
                getString(R.string.channel_playback),
                NotificationManager.IMPORTANCE_LOW
            ).apply { setShowBadge(false) }
        )
        manager.createNotificationChannel(
            NotificationChannel(
                CHANNEL_TRANSFER,
                getString(R.string.channel_transfer),
                NotificationManager.IMPORTANCE_LOW
            ).apply { setShowBadge(false) }
        )
    }

    companion object {
        const val CHANNEL_PLAYBACK = "playback"
        const val CHANNEL_TRANSFER = "transfer"
    }
}
