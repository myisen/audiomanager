//! FfiApi: 平台无关的 JSON 请求/响应 API 层
//!
//! T101: 供 Android JNI 薄封装调用的稳定 ABI 逻辑层。
//! 复用 core-ipc 的 IpcRequest/IpcResponse 协议，
//! 与桌面端 CLI / daemon 共用同一套消息定义。
//!
//! 本模块不依赖 JVM，可在任意平台单元测试。

use core_ipc::{deserialize_request, serialize_response, IpcRequest, IpcResponse};
use std::sync::Mutex;

/// Android 端共享状态
#[derive(Debug, Clone)]
pub struct AndroidState {
    pub is_playing: bool,
    pub current_playlist: Option<String>,
    pub current_file: Option<String>,
    pub position_secs: f64,
    pub duration_secs: Option<f64>,
    pub volume: f64,
    pub loop_mode: String,
    pub playlists: Vec<core_ipc::PlaylistInfo>,
    pub transfers: Vec<core_ipc::TransferInfo>,
    pub saf_uri: Option<String>,
    pub foreground_service_active: bool,
}

impl Default for AndroidState {
    fn default() -> Self {
        Self {
            is_playing: false,
            current_playlist: None,
            current_file: None,
            position_secs: 0.0,
            duration_secs: None,
            volume: 1.0,
            loop_mode: "off".into(),
            playlists: Vec::new(),
            transfers: Vec::new(),
            saf_uri: None,
            foreground_service_active: false,
        }
    }
}

/// 全局状态（JNI 层通过 `state()` 访问）
static STATE: Mutex<Option<AndroidState>> = Mutex::new(None);

/// 获取或初始化全局状态
pub fn with_state<R>(f: impl FnOnce(&mut AndroidState) -> R) -> R {
    let mut guard = STATE.lock().unwrap();
    let state = guard.get_or_insert_with(AndroidState::default);
    f(state)
}

/// 处理一条 JSON 请求，返回 JSON 响应字符串
///
/// 这是 JNI 层的唯一入口：`Java_com_audiosync_JniBridge_handleRequest`。
pub fn handle_request(request_json: &str) -> String {
    let request = match deserialize_request(request_json) {
        Ok(r) => r,
        Err(e) => {
            return response_to_string(&IpcResponse::Error {
                message: format!("请求解析失败: {}", e),
            });
        }
    };

    let response = dispatch(request);
    response_to_string(&response)
}

fn response_to_string(response: &IpcResponse) -> String {
    serialize_response(response)
        .unwrap_or_else(|_| r#"{"type":"error","data":{"message":"响应序列化失败"}}"#.into())
}

fn dispatch(request: IpcRequest) -> IpcResponse {
    with_state(|state| match request {
        IpcRequest::Status => IpcResponse::Status(core_ipc::StatusInfo {
            is_playing: state.is_playing,
            current_playlist: state.current_playlist.clone(),
            current_file: state.current_file.clone(),
            position_secs: state.position_secs,
            duration_secs: state.duration_secs,
            volume: state.volume,
            loop_mode: state.loop_mode.clone(),
            audio_device_available: true,
        }),

        IpcRequest::ListPlaylists => IpcResponse::PlaylistList(state.playlists.clone()),

        IpcRequest::SelectPlaylist { directory } => {
            state.current_playlist = Some(directory);
            state.position_secs = 0.0;
            IpcResponse::Ok
        }

        IpcRequest::PlayPause => {
            state.is_playing = !state.is_playing;
            IpcResponse::Ok
        }

        IpcRequest::Seek { position_secs } => {
            state.position_secs = position_secs.max(0.0);
            IpcResponse::Ok
        }

        IpcRequest::Next | IpcRequest::Prev => {
            state.position_secs = 0.0;
            IpcResponse::Ok
        }

        IpcRequest::SetVolume { volume } => {
            state.volume = volume.clamp(0.0, 1.0);
            IpcResponse::Ok
        }

        IpcRequest::SyncMetadata => IpcResponse::Ok,

        IpcRequest::StartTransfer => IpcResponse::Ok,

        IpcRequest::ListTransfers => IpcResponse::TransferList(state.transfers.clone()),

        IpcRequest::Shutdown => {
            // Android 上不真正退出进程，只停止前台服务
            state.is_playing = false;
            state.foreground_service_active = false;
            IpcResponse::Ok
        }
    })
}

/// SAF 授权结果写入（T102：Kotlin 侧回调存入）
pub fn set_saf_uri(uri: String) {
    with_state(|state| {
        state.saf_uri = Some(uri);
    });
}

/// 注册扫描结果（T102：Kotlin 侧扫描 SAF 目录后回填清单）
pub fn set_playlists(playlists: Vec<core_ipc::PlaylistInfo>) {
    with_state(|state| {
        state.playlists = playlists;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_ipc::{deserialize_response, IpcResponse};

    #[test]
    fn status_request_returns_status() {
        with_state(|s| *s = AndroidState::default());
        let resp_json = handle_request(r#"{"type":"status"}"#);
        let resp = deserialize_response(&resp_json).unwrap();
        match resp {
            IpcResponse::Status(info) => {
                assert!(!info.is_playing);
                assert_eq!(info.volume, 1.0);
                assert!(info.audio_device_available);
            }
            _ => panic!("expected status"),
        }
    }

    #[test]
    fn play_pause_toggles() {
        with_state(|s| *s = AndroidState::default());
        handle_request(r#"{"type":"play_pause"}"#);
        let resp_json = handle_request(r#"{"type":"status"}"#);
        match deserialize_response(&resp_json).unwrap() {
            IpcResponse::Status(info) => assert!(info.is_playing),
            _ => panic!("expected status"),
        }
    }

    #[test]
    fn select_playlist_updates_state() {
        with_state(|s| *s = AndroidState::default());
        handle_request(r#"{"type":"select_playlist","directory":"/music"}"#);
        let resp_json = handle_request(r#"{"type":"status"}"#);
        match deserialize_response(&resp_json).unwrap() {
            IpcResponse::Status(info) => {
                assert_eq!(info.current_playlist.as_deref(), Some("/music"));
            }
            _ => panic!("expected status"),
        }
    }

    #[test]
    fn seek_clamps_negative() {
        with_state(|s| *s = AndroidState::default());
        handle_request(r#"{"type":"seek","position_secs":-5.0}"#);
        let resp_json = handle_request(r#"{"type":"status"}"#);
        match deserialize_response(&resp_json).unwrap() {
            IpcResponse::Status(info) => assert!(info.position_secs >= 0.0),
            _ => panic!("expected status"),
        }
    }

    #[test]
    fn set_volume_clamps() {
        with_state(|s| *s = AndroidState::default());
        handle_request(r#"{"type":"set_volume","volume":1.5}"#);
        let resp_json = handle_request(r#"{"type":"status"}"#);
        match deserialize_response(&resp_json).unwrap() {
            IpcResponse::Status(info) => assert!((info.volume - 1.0).abs() < 0.001),
            _ => panic!("expected status"),
        }
    }

    #[test]
    fn invalid_json_returns_error_response() {
        let resp_json = handle_request("not json");
        match deserialize_response(&resp_json).unwrap() {
            IpcResponse::Error { message } => assert!(message.contains("请求解析失败")),
            _ => panic!("expected error"),
        }
    }

    #[test]
    fn shutdown_resets_playing_and_service() {
        with_state(|s| {
            *s = AndroidState::default();
            s.is_playing = true;
            s.foreground_service_active = true;
        });
        handle_request(r#"{"type":"shutdown"}"#);
        with_state(|s| {
            assert!(!s.is_playing);
            assert!(!s.foreground_service_active);
        });
    }

    #[test]
    fn set_saf_uri_and_playlists() {
        with_state(|s| *s = AndroidState::default());
        set_saf_uri("content://com.android.externalstorage/tree/music".into());
        set_playlists(vec![core_ipc::PlaylistInfo {
            directory: "content://com.android.externalstorage/tree/music".into(),
            name: "music".into(),
            file_count: 12,
        }]);

        let resp_json = handle_request(r#"{"type":"list_playlists"}"#);
        match deserialize_response(&resp_json).unwrap() {
            IpcResponse::PlaylistList(list) => {
                assert_eq!(list.len(), 1);
                assert_eq!(list[0].file_count, 12);
            }
            _ => panic!("expected playlist list"),
        }

        with_state(|s| {
            assert!(s.saf_uri.is_some());
        });
    }

    #[test]
    fn list_transfers_empty_by_default() {
        with_state(|s| *s = AndroidState::default());
        let resp_json = handle_request(r#"{"type":"list_transfers"}"#);
        match deserialize_response(&resp_json).unwrap() {
            IpcResponse::TransferList(list) => assert!(list.is_empty()),
            _ => panic!("expected transfer list"),
        }
    }
}
