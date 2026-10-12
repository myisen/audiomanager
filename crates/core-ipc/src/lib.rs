//! core-ipc: 本地控制通道协议
//!
//! 实现 FR-08：守护进程与 CLI 之间通过 Unix Socket 通信的消息协议。
//! 消息格式：JSON 行（每行一条 JSON 消息）。

use serde::{Deserialize, Serialize};

/// 客户端 → 守护进程的请求
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IpcRequest {
    /// 查询当前状态
    Status,
    /// 列出所有清单
    ListPlaylists,
    /// 选择清单播放
    SelectPlaylist { directory: String },
    /// 播放/暂停
    PlayPause,
    /// 跳转到指定位置（秒）
    Seek { position_secs: f64 },
    /// 下一曲
    Next,
    /// 上一曲
    Prev,
    /// 设置音量（0.0 ~ 1.0）
    SetVolume { volume: f64 },
    /// 触发元数据同步
    SyncMetadata,
    /// 触发传输任务
    StartTransfer,
    /// 查询传输任务状态
    ListTransfers,
    /// 优雅关闭
    Shutdown,
}

/// 守护进程 → 客户端的响应
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum IpcResponse {
    /// 成功（无数据）
    Ok,
    /// 状态查询结果
    Status(StatusInfo),
    /// 清单列表
    PlaylistList(Vec<PlaylistInfo>),
    /// 传输任务列表
    TransferList(Vec<TransferInfo>),
    /// 错误
    Error { message: String },
}

/// 当前状态信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusInfo {
    /// 是否正在播放
    pub is_playing: bool,
    /// 当前清单目录
    pub current_playlist: Option<String>,
    /// 当前文件路径
    pub current_file: Option<String>,
    /// 当前位置（秒）
    pub position_secs: f64,
    /// 总时长（秒）
    pub duration_secs: Option<f64>,
    /// 音量
    pub volume: f64,
    /// 循环模式
    pub loop_mode: String,
    /// 音频设备是否可用
    pub audio_device_available: bool,
}

/// 清单摘要
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistInfo {
    pub directory: String,
    pub name: String,
    pub file_count: usize,
}

/// 传输任务摘要
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferInfo {
    pub id: String,
    pub file_name: String,
    pub state: String,
    pub progress: f64,
}

/// IPC 错误
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("序列化失败: {0}")]
    Serialize(String),
    #[error("反序列化失败: {0}")]
    Deserialize(String),
    #[error("I/O 错误: {0}")]
    Io(String),
}

/// 将请求序列化为 JSON 字符串
pub fn serialize_request(req: &IpcRequest) -> Result<String, IpcError> {
    serde_json::to_string(req).map_err(|e| IpcError::Serialize(e.to_string()))
}

/// 将响应序列化为 JSON 字符串
pub fn serialize_response(res: &IpcResponse) -> Result<String, IpcError> {
    serde_json::to_string(res).map_err(|e| IpcError::Serialize(e.to_string()))
}

/// 从 JSON 字符串反序列化请求
pub fn deserialize_request(s: &str) -> Result<IpcRequest, IpcError> {
    serde_json::from_str(s).map_err(|e| IpcError::Deserialize(e.to_string()))
}

/// 从 JSON 字符串反序列化响应
pub fn deserialize_response(s: &str) -> Result<IpcResponse, IpcError> {
    serde_json::from_str(s).map_err(|e| IpcError::Deserialize(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_roundtrip_status() {
        let req = IpcRequest::Status;
        let json = serialize_request(&req).unwrap();
        let parsed = deserialize_request(&json).unwrap();
        assert!(matches!(parsed, IpcRequest::Status));
    }

    #[test]
    fn request_roundtrip_select_playlist() {
        let req = IpcRequest::SelectPlaylist {
            directory: "/music/rock".into(),
        };
        let json = serialize_request(&req).unwrap();
        let parsed = deserialize_request(&json).unwrap();
        match parsed {
            IpcRequest::SelectPlaylist { directory } => {
                assert_eq!(directory, "/music/rock");
            }
            _ => panic!("unexpected variant"),
        }
    }

    #[test]
    fn request_roundtrip_seek() {
        let req = IpcRequest::Seek {
            position_secs: 42.5,
        };
        let json = serialize_request(&req).unwrap();
        let parsed = deserialize_request(&json).unwrap();
        match parsed {
            IpcRequest::Seek { position_secs } => {
                assert!((position_secs - 42.5).abs() < 0.01);
            }
            _ => panic!("unexpected variant"),
        }
    }

    #[test]
    fn response_roundtrip_status_info() {
        let res = IpcResponse::Status(StatusInfo {
            is_playing: true,
            current_playlist: Some("/music/rock".into()),
            current_file: Some("/music/rock/song.mp3".into()),
            position_secs: 30.0,
            duration_secs: Some(180.0),
            volume: 0.8,
            loop_mode: "off".into(),
            audio_device_available: true,
        });
        let json = serialize_response(&res).unwrap();
        let parsed = deserialize_response(&json).unwrap();
        match parsed {
            IpcResponse::Status(info) => {
                assert!(info.is_playing);
                assert_eq!(info.current_playlist.as_deref(), Some("/music/rock"));
                assert!((info.volume - 0.8).abs() < 0.01);
            }
            _ => panic!("unexpected variant"),
        }
    }

    #[test]
    fn response_roundtrip_error() {
        let res = IpcResponse::Error {
            message: "no audio device".into(),
        };
        let json = serialize_response(&res).unwrap();
        let parsed = deserialize_response(&json).unwrap();
        match parsed {
            IpcResponse::Error { message } => {
                assert_eq!(message, "no audio device");
            }
            _ => panic!("unexpected variant"),
        }
    }

    #[test]
    fn response_roundtrip_playlist_list() {
        let res = IpcResponse::PlaylistList(vec![
            PlaylistInfo {
                directory: "/music/rock".into(),
                name: "rock".into(),
                file_count: 10,
            },
            PlaylistInfo {
                directory: "/music/jazz".into(),
                name: "jazz".into(),
                file_count: 5,
            },
        ]);
        let json = serialize_response(&res).unwrap();
        let parsed = deserialize_response(&json).unwrap();
        match parsed {
            IpcResponse::PlaylistList(list) => {
                assert_eq!(list.len(), 2);
                assert_eq!(list[0].name, "rock");
            }
            _ => panic!("unexpected variant"),
        }
    }

    #[test]
    fn deserialize_invalid_json_returns_error() {
        let result = deserialize_request("not json");
        assert!(matches!(result, Err(IpcError::Deserialize(_))));
    }
}
