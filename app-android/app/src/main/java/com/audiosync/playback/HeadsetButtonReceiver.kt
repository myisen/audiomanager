package com.audiosync.playback

/**
 * T105: 耳机 / media 按键接收
 *
 * 继承 androidx MediaButtonReceiver：系统把 MEDIA_BUTTON intent 投递到这里，
 * 默认实现自动转发给声明了 MEDIA_BUTTON intent-filter 的服务
 * （PlaybackService），由其 MediaSession 处理播放/暂停/上下曲。
 */
class HeadsetButtonReceiver : androidx.media.session.MediaButtonReceiver()
