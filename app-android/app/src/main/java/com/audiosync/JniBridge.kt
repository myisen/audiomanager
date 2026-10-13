package com.audiosync

/**
 * T101: Rust 核心 JNI 绑定
 *
 * 与 crates/core-jni 的 extern "system" 函数一一对应。
 * 所有调用使用 core-ipc 的 JSON 协议（与桌面端 CLI/daemon 共用）。
 */
object JniBridge {
    init {
        System.loadLibrary("audiosync_core")
    }

    /** 发送一条 JSON 请求，返回 JSON 响应（协议见 core-ipc） */
    external fun handleRequest(requestJson: String): String

    /** T102: 写入 SAF 授权 URI */
    external fun setSafUri(uri: String)

    /** T102: 写入扫描结果（PlaylistInfo JSON 数组） */
    external fun setPlaylists(playlistsJson: String)

    /** T103: 查询前台服务是否应保持活跃 */
    external fun isForegroundServiceActive(): Boolean

    // ===== 便捷封装（内部解析 JSON，供 UI 直接调用） =====

    internal fun request(json: String): org.json.JSONObject {
        val resp = handleRequest(json)
        return org.json.JSONObject(resp)
    }

    fun status(): org.json.JSONObject = request("""{"type":"status"}""")
    fun listPlaylists(): org.json.JSONArray =
        request("""{"type":"list_playlists"}""").optJSONArray("data") ?: org.json.JSONArray()
    fun listTransfers(): org.json.JSONArray =
        request("""{"type":"list_transfers"}""").optJSONArray("data") ?: org.json.JSONArray()
    fun selectPlaylist(directory: String) =
        request(org.json.JSONObject().put("type", "select_playlist").put("directory", directory).toString())
    fun playPause() = request("""{"type":"play_pause"}""")
    fun seek(positionSecs: Double) =
        request(org.json.JSONObject().put("type", "seek").put("position_secs", positionSecs).toString())
    fun next() = request("""{"type":"next"}""")
    fun prev() = request("""{"type":"prev"}""")
    fun setVolume(volume: Double) =
        request(org.json.JSONObject().put("type", "set_volume").put("volume", volume).toString())
    fun shutdown() = request("""{"type":"shutdown"}""")
}
