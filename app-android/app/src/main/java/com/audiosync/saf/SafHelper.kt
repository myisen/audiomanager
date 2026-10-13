package com.audiosync.saf

import android.content.Context
import android.net.Uri
import android.provider.DocumentsContract
import androidx.documentfile.provider.DocumentFile

/**
 * T102: SAF 目录授权与库扫描适配
 *
 * Android 不提供全局文件系统访问，音频库目录通过
 * ACTION_OPEN_DOCUMENT_TREE 授权（无需存储权限）。
 * 授权结果持久化后每次启动自动恢复。
 */
object SafHelper {

    /** 发起目录授权选择；结果在 onActivityResult 中用 [onTreeGranted] 处理 */
    fun openTreePickerIntent(): android.content.Intent =
        android.content.Intent(android.content.Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
            addFlags(
                android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION
                        or android.content.Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                        or android.content.Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
                        or android.content.Intent.FLAG_GRANT_PREFIX_URI_PERMISSION
            )
        }

    /** 处理授权结果：持久化权限并返回 tree Uri（取消授权返回 null） */
    fun onTreeGranted(context: Context, resultCode: Int, data: android.content.Intent?): Uri? {
        val uri = data?.data ?: return null
        val flags =
            android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION
                    or android.content.Intent.FLAG_GRANT_WRITE_URI_PERMISSION
        try {
            context.contentResolver.takePersistableUriPermission(uri, flags)
        } catch (_: SecurityException) {
            return null
        }
        JniBridge.setSafUri(uri.toString())
        return uri
    }

    /** 恢复已授权的音频库目录（无授权返回 null） */
    fun restoreAuthorizedTree(context: Context): Uri? {
        val persisted = context.contentResolver.persistedUriPermissions
            .filter { it.isReadPermission }
            .maxByOrNull { it.persistedTime } ?: return null
        JniBridge.setSafUri(persisted.uri.toString())
        return persisted.uri
    }

    /** 判断 URI 对应目录是否仍然可访问（外部存储被移除等场景返回 false） */
    fun isTreeAccessible(context: Context, uri: Uri): Boolean =
        try {
            DocumentFile.fromTreeUri(context, uri)?.exists() == true
        } catch (_: SecurityException) {
            false
        }

    /**
     * 扫描目录下全部音频文件（递归），返回「显示名 + Uri」列表。
     * 音频扩展名白名单与 PRD FR-01 一致。
     */
    fun scanAudioFiles(
        context: Context,
        treeUri: Uri,
        supportedExtensions: Set<String> = DEFAULT_AUDIO_EXTENSIONS,
    ): List<Pair<String, Uri>> {
        val results = mutableListOf<Pair<String, Uri>>()
        val root = DocumentFile.fromTreeUri(context, treeUri) ?: return results
        walk(root, supportedExtensions, results)
        return results
    }

    private fun walk(
        dir: DocumentFile,
        extensions: Set<String>,
        results: MutableList<Pair<String, Uri>>,
    ) {
        for (file in dir.listFiles()) {
            if (file.isDirectory) {
                walk(file, extensions, results)
            } else {
                val ext = file.name?.substringAfterLast('.', "")?.lowercase() ?: ""
                if (ext in extensions) {
                    results.add(file.name ?: file.uri.toString() to file.uri)
                }
            }
        }
    }

    /** 将扫描结果回传 Rust 侧（T102 完成「SAF → 核心库」桥接） */
    fun reportScanToCore(displayNames: List<String>) {
        val playlistsJson = org.json.JSONArray().apply {
            displayNames.forEach { name ->
                put(
                    org.json.JSONObject()
                        .put("directory", "saf://scanned")
                        .put("name", name.substringBeforeLast('/'))
                        .put("file_count", displayNames.size)
                )
            }
        }
        JniBridge.setPlaylists(playlistsJson.toString())
    }

    /** 查询音频文件时长（元数据，FR-07 本地来源） */
    fun queryDuration(context: Context, uri: Uri): Long? =
        try {
            context.contentResolver.openFileDescriptor(uri, "r")?.use { pfd ->
                val extractor = android.media.MediaExtractor()
                extractor.setDataSource(pfd.fileDescriptor)
                var duration = -1L
                for (i in 0 until extractor.trackCount) {
                    val format = extractor.getTrackFormat(i)
                    if (format.containsKey(android.media.MediaFormat.KEY_DURATION)) {
                        duration = format.getLong(android.media.MediaFormat.KEY_DURATION)
                        break
                    }
                }
                extractor.release()
                if (duration > 0) duration / 1000 else null // µs → ms
            }
        } catch (_: Exception) {
            null
        }

    private val DEFAULT_AUDIO_EXTENSIONS = setOf(
        "mp3", "aac", "flac", "ogg", "oga", "m4a", "wav", "opus", "ape", "wv", "mka"
    )
}
