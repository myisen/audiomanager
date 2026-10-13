package com.audiosync.playback

import android.app.Notification
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.os.Build
import android.support.v4.media.MediaMetadataCompat
import android.support.v4.media.session.MediaSessionCompat
import android.support.v4.media.session.PlaybackStateCompat
import androidx.core.app.NotificationCompat
import androidx.media.session.MediaButtonReceiver
import com.audiosync.AudioSyncApp
import com.audiosync.JniBridge
import com.audiosync.R

/**
 * T103: 播放前台服务（foregroundServiceType="mediaPlayback"）
 * T105: 媒体通知 + 耳机/media 按键（MediaSession）
 *
 * 生命周期：开始播放 → startForeground；停止 → stopForeground + 停止自身。
 * 电池优化白名单被拒绝时服务仍可运行（仅前台期间存活），不崩溃。
 */
class PlaybackService : Service() {

    private lateinit var mediaSession: MediaSessionCompat

    override fun onCreate() {
        super.onCreate()
        // T105: MediaSession —— 处理媒体通知按钮与耳机按键
        mediaSession = MediaSessionCompat(this, "AudioSync").apply {
            setCallback(object : MediaSessionCompat.Callback() {
                override fun onPlay() {
                    JniBridge.playPause()
                    updatePlaybackState(true)
                }

                override fun onPause() {
                    JniBridge.playPause()
                    updatePlaybackState(false)
                }

                override fun onSkipToNext() {
                    JniBridge.next()
                }

                override fun onSkipToPrevious() {
                    JniBridge.prev()
                }

                override fun onSeekTo(pos: Long) {
                    JniBridge.seek(pos / 1000.0)
                    updatePlaybackState(true)
                }
            })
            // T105: 接收耳机/media 按键
            setMediaButtonReceiver(
                PendingIntent.getBroadcast(
                    this@PlaybackService,
                    0,
                    Intent(this@PlaybackService, MediaButtonReceiver::class.java),
                    PendingIntent.FLAG_IMMUTABLE
                )
            )
            isActive = true
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            // T105: 耳机/media 按键（HeadsetButtonReceiver 转发）→ MediaSession 处理
            Intent.ACTION_MEDIA_BUTTON -> {
                androidx.media.session.MediaButtonReceiver.handleIntent(mediaSession, intent)
            }
            ACTION_START -> {
                startForeground(NOTIFICATION_ID, buildNotification())
                updatePlaybackState(true)
            }
            ACTION_STOP -> {
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
            }
        }
        return START_NOT_STICKY
    }

    /** 同步当前曲目信息到媒体通知（T105） */
    fun updatePlaybackState(playing: Boolean) {
        val status = JniBridge.status()
        val title = status.optString("current_file", getString(R.string.no_track))
        val playlist = status.optString("current_playlist", "")

        mediaSession.setMetadata(
            MediaMetadataCompat.Builder()
                .putString(MediaMetadataCompat.METADATA_KEY_TITLE, title.substringAfterLast('/'))
                .putString(MediaMetadataCompat.METADATA_KEY_ALBUM, playlist)
                .putLong(
                    MediaMetadataCompat.METADATA_KEY_DURATION,
                    (status.optDouble("duration_secs", 0.0) * 1000).toLong()
                )
                .build()
        )

        mediaSession.setPlaybackState(
            PlaybackStateCompat.Builder()
                .setActions(
                    PlaybackStateCompat.ACTION_PLAY or
                            PlaybackStateCompat.ACTION_PAUSE or
                            PlaybackStateCompat.ACTION_PLAY_PAUSE or
                            PlaybackStateCompat.ACTION_SKIP_TO_NEXT or
                            PlaybackStateCompat.ACTION_SKIP_TO_PREVIOUS or
                            PlaybackStateCompat.ACTION_SEEK_TO
                )
                .setState(
                    if (playing) PlaybackStateCompat.STATE_PLAYING
                    else PlaybackStateCompat.STATE_PAUSED,
                    (status.optDouble("position_secs", 0.0) * 1000).toLong(),
                    1.0f
                )
                .build()
        )

        val manager = androidx.core.app.NotificationManagerCompat.from(this)
        manager.notify(NOTIFICATION_ID, buildNotification())
    }

    private fun buildNotification(): Notification {
        val openApp = PendingIntent.getActivity(
            this, 0,
            Intent(this, com.audiosync.MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE
        )
        return NotificationCompat.Builder(this, AudioSyncApp.CHANNEL_PLAYBACK)
            .setSmallIcon(android.R.drawable.ic_media_play)
            .setContentTitle(getString(R.string.app_name))
            .setContentText(getString(R.string.playback_notification_text))
            .setContentIntent(openApp)
            .setOngoing(true)
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setCategory(NotificationCompat.CATEGORY_TRANSPORT)
            .build()
    }

    override fun onBind(intent: Intent?) = null

    override fun onDestroy() {
        mediaSession.isActive = false
        mediaSession.release()
        super.onDestroy()
    }

    companion object {
        private const val NOTIFICATION_ID = 1001
        const val ACTION_START = "com.audiosync.action.START_PLAYBACK"
        const val ACTION_STOP = "com.audiosync.action.STOP_PLAYBACK"

        /** 启动播放前台服务（T103） */
        fun start(context: Context) {
            val intent = Intent(context, PlaybackService::class.java).apply {
                action = ACTION_START
            }
            context.startForegroundService(intent)
        }

        fun stop(context: Context) {
            context.startService(
                Intent(context, PlaybackService::class.java).apply { action = ACTION_STOP }
            )
        }
    }
}
