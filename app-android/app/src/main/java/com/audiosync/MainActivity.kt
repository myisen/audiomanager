package com.audiosync

import android.Manifest
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.view.View
import android.widget.ArrayAdapter
import android.widget.ListView
import android.widget.TextView
import android.widget.Toast
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import com.audiosync.playback.PlaybackService
import com.audiosync.saf.SafHelper
import com.audiosync.transfer.TransferService

/**
 * T104: Android 主界面，功能对齐桌面端
 * （音乐库 / 播放控制 / 设备与传输 / 设置）
 *
 * T102: 首次启动引导 SAF 授权音频库目录。
 * T103: 拒绝电池白名单时降级为仅前台运行，不崩溃。
 */
class MainActivity : AppCompatActivity() {

    private val playlists = mutableListOf<String>()

    // ===== 生命周期 =====

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        bindViews()
        requestNotificationPermissionIfNeeded()
        restoreOrRequestLibrary()
        refreshAll()
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: android.content.Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == RC_SAF_TREE) {
            val uri = SafHelper.onTreeGranted(this, resultCode, data)
            if (uri != null) {
                scanAuthorizedLibrary(uri)
            } else {
                toast(getString(R.string.saf_denied))
            }
        }
    }

    // ===== 视图绑定与事件 =====

    private fun bindViews() {
        findViewById<View>(R.id.btn_grant_library).setOnClickListener {
            startActivityForResult(SafHelper.openTreePickerIntent(), RC_SAF_TREE)
        }
        findViewById<View>(R.id.btn_play_pause).setOnClickListener {
            JniBridge.playPause()
            PlaybackService.start(this)
            refreshStatus()
        }
        findViewById<View>(R.id.btn_next).setOnClickListener { JniBridge.next(); refreshStatus() }
        findViewById<View>(R.id.btn_prev).setOnClickListener { JniBridge.prev(); refreshStatus() }
        findViewById<View>(R.id.btn_start_transfer).setOnClickListener {
            TransferService.start(this)
            refreshTransfers()
        }
        findViewById<View>(R.id.btn_rescan).setOnClickListener {
            SafHelper.restoreAuthorizedTree(this)?.let { scanAuthorizedLibrary(it) }
                ?: toast(getString(R.string.need_grant_first))
        }
        findViewById<ListView>(R.id.playlist_list).setOnItemClickListener { _, _, pos, _ ->
            JniBridge.selectPlaylist(playlists[pos])
            refreshStatus()
        }
    }

    // ===== T102: SAF 授权与扫描 =====

    private fun restoreOrRequestLibrary() {
        val restored = SafHelper.restoreAuthorizedTree(this)
        when {
            restored != null && SafHelper.isTreeAccessible(this, restored) ->
                scanAuthorizedLibrary(restored)

            restored != null ->
                // 目录不可访问（如存储被移除）：标记失效但不影响其他功能（FR-01 边界）
                toast(getString(R.string.library_unavailable))

            else ->
                AlertDialog.Builder(this)
                    .setTitle(R.string.saf_title)
                    .setMessage(R.string.saf_message)
                    .setPositiveButton(R.string.saf_grant) { _, _ ->
                        startActivityForResult(SafHelper.openTreePickerIntent(), RC_SAF_TREE)
                    }
                    .setNegativeButton(R.string.saf_later, null)
                    .show()
        }
    }

    private fun scanAuthorizedLibrary(treeUri: Uri) {
        Thread {
            val files = SafHelper.scanAudioFiles(this, treeUri)
            SafHelper.reportScanToCore(files.map { it.first })
            runOnUiThread {
                toast(getString(R.string.scan_done, files.size))
                refreshPlaylists()
            }
        }.start()
    }

    // ===== 刷新 =====

    private fun refreshAll() {
        refreshPlaylists()
        refreshStatus()
        refreshTransfers()
    }

    private fun refreshPlaylists() {
        playlists.clear()
        val arr = JniBridge.listPlaylists()
        for (i in 0 until arr.length()) {
            playlists.add(arr.getJSONObject(i).optString("name"))
        }
        val list = findViewById<ListView>(R.id.playlist_list)
        list.adapter = ArrayAdapter(
            this, android.R.layout.simple_list_item_1, playlists
        )
        findViewById<View>(R.id.empty_playlists).visibility =
            if (playlists.isEmpty()) View.VISIBLE else View.GONE
    }

    private fun refreshStatus() {
        val status = JniBridge.status()
        findViewById<TextView>(R.id.track_title).text =
            status.optString("current_file", getString(R.string.no_track))
                .substringAfterLast('/')
        findViewById<TextView>(R.id.track_playlist).text =
            status.optString("current_playlist", "—")
        findViewById<TextView>(R.id.track_position).text = String.format(
            "%d:%02d / %s",
            status.optDouble("position_secs", 0.0).toInt() / 60,
            status.optDouble("position_secs", 0.0).toInt() % 60,
            status.opt("duration_secs")?.let { d ->
                val s = (d as Double).toInt()
                "%d:%02d".format(s / 60, s % 60)
            } ?: "--:--"
        )
    }

    private fun refreshTransfers() {
        val arr = JniBridge.listTransfers()
        findViewById<TextView>(R.id.transfer_summary).text =
            getString(R.string.transfer_summary, arr.length())
    }

    private fun toast(msg: String) =
        Toast.makeText(this, msg, Toast.LENGTH_SHORT).show()

    // ===== T105: Android 13 通知权限（媒体通知需要） =====

    private fun requestNotificationPermissionIfNeeded() {
        if (Build.VERSION.SDK_INT >= 33 &&
            ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS)
            != PackageManager.PERMISSION_GRANTED
        ) {
            ActivityCompat.requestPermissions(
                this, arrayOf(Manifest.permission.POST_NOTIFICATIONS), RC_NOTIFICATION
            )
        }
    }

    override fun onRequestPermissionsResult(
        requestCode: Int, permissions: Array<out String>, grantResults: IntArray,
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        // 通知权限被拒绝：媒体通知不可见，播放功能不受影响（降级而非崩溃）
    }

    private companion object {
        const val RC_SAF_TREE = 100
        const val RC_NOTIFICATION = 101
    }
}
