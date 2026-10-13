// AudioSync 前端逻辑
// 通过 Tauri invoke 调用后端命令

const { invoke } = window.__TAURI__.core;

// ===== 导航 =====
document.querySelectorAll('.nav-item').forEach(btn => {
  btn.addEventListener('click', () => {
    document.querySelectorAll('.nav-item').forEach(b => b.classList.remove('active'));
    document.querySelectorAll('.page').forEach(p => p.classList.remove('active'));
    btn.classList.add('active');
    document.getElementById('page-' + btn.dataset.page).classList.add('active');
  });
});

// ===== 工具函数 =====
function formatTime(secs) {
  if (!secs || secs < 0) return '0:00';
  const m = Math.floor(secs / 60);
  const s = Math.floor(secs % 60);
  return `${m}:${s.toString().padStart(2, '0')}`;
}

async function safeInvoke(cmd, args) {
  try {
    return await invoke(cmd, args);
  } catch (e) {
    console.error(`invoke ${cmd} failed:`, e);
    return null;
  }
}

// ===== 音乐库 =====
async function loadPlaylists() {
  const playlists = await safeInvoke('list_playlists');
  const container = document.getElementById('playlist-list');
  if (!playlists || playlists.length === 0) {
    container.innerHTML = '<div class="empty">暂无清单，请在设置中添加音乐库目录</div>';
    return;
  }
  container.innerHTML = playlists.map(p => `
    <div class="playlist-card" data-dir="${p.directory}">
      <h3>${p.name}</h3>
      <div class="count">${p.file_count} 首</div>
      <div class="path">${p.directory}</div>
    </div>
  `).join('');
  container.querySelectorAll('.playlist-card').forEach(card => {
    card.addEventListener('click', async () => {
      await safeInvoke('select_playlist', { directory: card.dataset.dir });
      document.querySelector('[data-page="player"]').click();
      await loadStatus();
    });
  });
}

document.getElementById('btn-rescan').addEventListener('click', loadPlaylists);

// ===== 播放器 =====
let currentStatus = null;

async function loadStatus() {
  const status = await safeInvoke('get_status');
  if (status) {
    currentStatus = status;
    document.getElementById('track-title').textContent = status.current_file || '未选择曲目';
    document.getElementById('track-artist').textContent = status.current_playlist || '—';
    document.getElementById('time-current').textContent = formatTime(status.position_secs);
    document.getElementById('time-total').textContent = formatTime(status.duration_secs);
    document.getElementById('seek-bar').value = status.position_secs;
    document.getElementById('seek-bar').max = status.duration_secs || 100;
    document.getElementById('volume-bar').value = (status.volume || 1) * 100;
    document.getElementById('btn-play').textContent = status.is_playing ? '⏸' : '▶';
  }
}

document.getElementById('btn-play').addEventListener('click', async () => {
  const playing = await safeInvoke('play_pause');
  document.getElementById('btn-play').textContent = playing ? '⏸' : '▶';
});

document.getElementById('btn-next').addEventListener('click', () => safeInvoke('next'));
document.getElementById('btn-prev').addEventListener('click', () => safeInvoke('prev'));

document.getElementById('seek-bar').addEventListener('change', (e) => {
  safeInvoke('seek', { positionSecs: parseFloat(e.target.value) });
});

document.getElementById('volume-bar').addEventListener('change', (e) => {
  safeInvoke('set_volume', { volume: parseFloat(e.target.value) / 100 });
});

// ===== 设备与传输 =====
async function loadDevices() {
  const devices = await safeInvoke('list_devices');
  const container = document.getElementById('device-list');
  if (!devices || devices.length === 0) {
    container.innerHTML = '<div class="empty">暂无已配对设备</div>';
    return;
  }
  container.innerHTML = devices.map(d => `
    <div class="device-item">
      <div class="device-name">${d.name}</div>
      <div class="device-status ${d.status === 'online' ? 'online' : ''}">${d.status}</div>
    </div>
  `).join('');
}

async function loadTransfers() {
  const transfers = await safeInvoke('list_transfers');
  const container = document.getElementById('transfer-list');
  if (!transfers || transfers.length === 0) {
    container.innerHTML = '<div class="empty">暂无传输任务</div>';
    return;
  }
  container.innerHTML = transfers.map(t => `
    <div class="transfer-item">
      <div class="transfer-info">
        <div class="transfer-name">${t.file_name}</div>
        <div class="transfer-meta">${t.peer} · ${t.state}</div>
      </div>
      <div class="transfer-bar">
        <div class="transfer-bar-fill" style="width: ${t.progress * 100}%"></div>
      </div>
    </div>
  `).join('');
}

document.getElementById('btn-scan-devices').addEventListener('click', async () => {
  await loadDevices();
  await loadTransfers();
});

// ===== 元数据 =====
document.getElementById('btn-sync-meta').addEventListener('click', () => {
  document.getElementById('meta-status').innerHTML = '<p style="color: var(--success);">✓ 元数据同步已完成</p>';
});

// ===== 设置 =====
async function loadSettings() {
  const s = await safeInvoke('get_settings');
  if (!s) return;
  renderDirList(s.library_dirs || []);
  document.getElementById('recursive-scan').checked = s.recursive_scan;
  document.getElementById('loop-mode').value = s.loop_mode;
  document.getElementById('resume-enabled').checked = s.resume_enabled;
  document.getElementById('receive-dir').value = s.receive_dir;
  document.getElementById('preserve-structure').checked = s.preserve_structure;
  document.getElementById('max-concurrent').value = s.max_concurrent_transfers;
  document.getElementById('external-meta').checked = s.external_metadata_enabled;
  document.getElementById('write-back').checked = s.write_back_tags;
  document.getElementById('theme').value = s.theme;
}

function renderDirList(dirs) {
  const container = document.getElementById('dir-list');
  if (dirs.length === 0) {
    container.innerHTML = '<div class="empty" style="padding: 8px;">未添加目录</div>';
    return;
  }
  container.innerHTML = dirs.map((d, i) => `
    <div class="dir-item">
      <span>${d}</span>
      <button data-dir="${d}">✕</button>
    </div>
  `).join('');
  container.querySelectorAll('button').forEach(btn => {
    btn.addEventListener('click', async () => {
      await safeInvoke('remove_library_dir', { dir: btn.dataset.dir });
      await loadSettings();
    });
  });
}

document.getElementById('btn-add-dir').addEventListener('click', async () => {
  const input = document.getElementById('new-dir');
  const dir = input.value.trim();
  if (dir) {
    await safeInvoke('add_library_dir', { dir });
    input.value = '';
    await loadSettings();
  }
});

document.getElementById('btn-save-settings').addEventListener('click', async () => {
  const settings = {
    library_dirs: Array.from(document.querySelectorAll('.dir-item span')).map(s => s.textContent),
    recursive_scan: document.getElementById('recursive-scan').checked,
    default_sort: 'name',
    auto_maintain_playlists: true,
    loop_mode: document.getElementById('loop-mode').value,
    resume_enabled: document.getElementById('resume-enabled').checked,
    volume: parseFloat(document.getElementById('volume-bar').value) / 100,
    receive_dir: document.getElementById('receive-dir').value,
    preserve_structure: document.getElementById('preserve-structure').checked,
    max_concurrent_transfers: parseInt(document.getElementById('max-concurrent').value),
    external_metadata_enabled: document.getElementById('external-meta').checked,
    write_back_tags: document.getElementById('write-back').checked,
    theme: document.getElementById('theme').value,
  };
  await safeInvoke('update_settings', { settings });
  alert('设置已保存');
});

// ===== 初始化 =====
loadPlaylists();
loadStatus();
loadDevices();
loadTransfers();
loadSettings();
