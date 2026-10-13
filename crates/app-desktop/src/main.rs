//! app-desktop: Tauri 桌面应用后端
//!
//! 提供前端可调用的 Tauri 命令，封装核心 crate 的功能。

use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::State;

/// 应用共享状态（内存中，后续可对接 core-* crate）
pub struct AppState {
    inner: Mutex<InnerState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InnerState {
    is_playing: bool,
    current_playlist: Option<String>,
    current_file: Option<String>,
    position_secs: f64,
    duration_secs: Option<f64>,
    volume: f64,
    loop_mode: String,
    playlists: Vec<PlaylistItem>,
    devices: Vec<DeviceItem>,
    transfers: Vec<TransferItem>,
    settings: Settings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistItem {
    pub directory: String,
    pub name: String,
    pub file_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceItem {
    pub id: String,
    pub name: String,
    pub status: String,
    pub paired: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferItem {
    pub id: String,
    pub file_name: String,
    pub peer: String,
    pub state: String,
    pub progress: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub library_dirs: Vec<String>,
    pub recursive_scan: bool,
    pub default_sort: String,
    pub auto_maintain_playlists: bool,
    pub loop_mode: String,
    pub resume_enabled: bool,
    pub volume: f64,
    pub receive_dir: String,
    pub preserve_structure: bool,
    pub max_concurrent_transfers: u32,
    pub external_metadata_enabled: bool,
    pub write_back_tags: bool,
    pub theme: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            library_dirs: Vec::new(),
            recursive_scan: true,
            default_sort: "name".into(),
            auto_maintain_playlists: true,
            loop_mode: "off".into(),
            resume_enabled: true,
            volume: 1.0,
            receive_dir: "~/Music/AudioSync".into(),
            preserve_structure: true,
            max_concurrent_transfers: 3,
            external_metadata_enabled: true,
            write_back_tags: false,
            theme: "dark".into(),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            inner: Mutex::new(InnerState {
                is_playing: false,
                current_playlist: None,
                current_file: None,
                position_secs: 0.0,
                duration_secs: None,
                volume: 1.0,
                loop_mode: "off".into(),
                playlists: Vec::new(),
                devices: Vec::new(),
                transfers: Vec::new(),
                settings: Settings::default(),
            }),
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }
}

// ===== 命令（单独模块，避免与 generate_handler! 同模块宏冲突） =====

mod commands {
    use super::*;

    #[tauri::command]
    pub fn get_status(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
        let s = state.inner.lock().unwrap();
        Ok(serde_json::json!({
            "is_playing": s.is_playing,
            "current_playlist": s.current_playlist,
            "current_file": s.current_file,
            "position_secs": s.position_secs,
            "duration_secs": s.duration_secs,
            "volume": s.volume,
            "loop_mode": s.loop_mode,
        }))
    }

    #[tauri::command]
    pub fn list_playlists(state: State<'_, AppState>) -> Result<Vec<PlaylistItem>, String> {
        Ok(state.inner.lock().unwrap().playlists.clone())
    }

    #[tauri::command]
    pub fn select_playlist(state: State<'_, AppState>, directory: String) -> Result<(), String> {
        let mut s = state.inner.lock().unwrap();
        s.current_playlist = Some(directory);
        s.position_secs = 0.0;
        Ok(())
    }

    #[tauri::command]
    pub fn play_pause(state: State<'_, AppState>) -> Result<bool, String> {
        let mut s = state.inner.lock().unwrap();
        s.is_playing = !s.is_playing;
        Ok(s.is_playing)
    }

    #[tauri::command]
    pub fn seek(state: State<'_, AppState>, position_secs: f64) -> Result<f64, String> {
        let mut s = state.inner.lock().unwrap();
        s.position_secs = position_secs.max(0.0);
        Ok(s.position_secs)
    }

    #[tauri::command]
    pub fn next(state: State<'_, AppState>) -> Result<(), String> {
        let mut s = state.inner.lock().unwrap();
        s.position_secs = 0.0;
        Ok(())
    }

    #[tauri::command]
    pub fn prev(state: State<'_, AppState>) -> Result<(), String> {
        let mut s = state.inner.lock().unwrap();
        s.position_secs = 0.0;
        Ok(())
    }

    #[tauri::command]
    pub fn set_volume(state: State<'_, AppState>, volume: f64) -> Result<f64, String> {
        let mut s = state.inner.lock().unwrap();
        s.volume = volume.clamp(0.0, 1.0);
        Ok(s.volume)
    }

    #[tauri::command]
    pub fn list_devices(state: State<'_, AppState>) -> Result<Vec<DeviceItem>, String> {
        Ok(state.inner.lock().unwrap().devices.clone())
    }

    #[tauri::command]
    pub fn list_transfers(state: State<'_, AppState>) -> Result<Vec<TransferItem>, String> {
        Ok(state.inner.lock().unwrap().transfers.clone())
    }

    #[tauri::command]
    pub fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
        Ok(state.inner.lock().unwrap().settings.clone())
    }

    #[tauri::command]
    pub fn update_settings(state: State<'_, AppState>, settings: Settings) -> Result<(), String> {
        let mut s = state.inner.lock().unwrap();
        s.settings = settings;
        s.volume = s.settings.volume;
        Ok(())
    }

    #[tauri::command]
    pub fn add_library_dir(state: State<'_, AppState>, dir: String) -> Result<(), String> {
        let mut s = state.inner.lock().unwrap();
        if !s.settings.library_dirs.contains(&dir) {
            s.settings.library_dirs.push(dir);
        }
        Ok(())
    }

    #[tauri::command]
    pub fn remove_library_dir(state: State<'_, AppState>, dir: String) -> Result<(), String> {
        let mut s = state.inner.lock().unwrap();
        s.settings.library_dirs.retain(|d| d != &dir);
        Ok(())
    }
}

/// 构建 Tauri 应用并注册命令
fn run() {
    use commands::*;
    tauri::Builder::default()
        .manage(AppState::new())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            get_status,
            list_playlists,
            select_playlist,
            play_pause,
            seek,
            next,
            prev,
            set_volume,
            list_devices,
            list_transfers,
            get_settings,
            update_settings,
            add_library_dir,
            remove_library_dir,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn main() {
    run();
}
