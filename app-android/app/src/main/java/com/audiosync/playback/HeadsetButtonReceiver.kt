package com.audiosync.playback

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.support.v4.media.session.MediaButtonReceiver
import android.support.v4.media.session.PlaybackStateCompat
import android.view.KeyEvent

/**
 * T105: 耳机 / media 按键接收
 *
 * 清单注册 MEDIA_BUTTON intent-filter，系统在耳机按键时投递到这里，
 * 转发给 androidx MediaButtonReceiver → PlaybackService 的 MediaSession 处理
 * （播放/暂停/上下曲）。
 */
class HeadsetButtonReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_MEDIA_BUTTON) return

        val event: KeyEvent =
            intent.getParcelableExtra(Intent.EXTRA_KEY_EVENT) ?: return
        when (event.keyCode) {
            KeyEvent.KEYCODE_MEDIA_PLAY,
            KeyEvent.KEYCODE_MEDIA_PAUSE,
            KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE,
            KeyEvent.KEYCODE_HEADSETHOOK ->
                MediaButtonReceiver.handleAction(context, PlaybackStateCompat.ACTION_PLAY_PAUSE)

            KeyEvent.KEYCODE_MEDIA_NEXT ->
                MediaButtonReceiver.handleAction(context, PlaybackStateCompat.ACTION_SKIP_TO_NEXT)

            KeyEvent.KEYCODE_MEDIA_PREVIOUS ->
                MediaButtonReceiver.handleAction(context, PlaybackStateCompat.ACTION_SKIP_TO_PREVIOUS)
        }
    }
}
