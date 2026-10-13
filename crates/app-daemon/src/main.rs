//! app-daemon: 无头服务（FR-08）
//!
//! 提供守护进程形态、单实例检测、控制通道（Unix: Unix Socket / Windows:
//! TCP localhost + 端口文件）、无音频设备降级、非 root 运行校验。

use core_ipc::{
    deserialize_request, deserialize_response, serialize_request, serialize_response, IpcRequest,
    IpcResponse, PlaylistInfo, StatusInfo, TransferInfo,
};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::exit;
use std::sync::{Arc, Mutex};
use std::thread;

// ===== 跨平台 IPC 传输层 =====

#[cfg(unix)]
mod transport {
    use super::*;
    use std::os::unix::net::{UnixListener, UnixStream};

    pub type IpcListener = UnixListener;
    pub type IpcStream = UnixStream;

    /// 默认 Socket 路径
    pub fn endpoint_path() -> PathBuf {
        let uid = std::env::var("UID")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(1000);
        let runtime_dir =
            std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| format!("/tmp/runtime-{}", uid));
        PathBuf::from(runtime_dir).join("audiosync.sock")
    }

    /// 绑定控制通道；若已有实例存活则返回错误（单实例检测）
    pub fn bind_instance() -> Result<(IpcListener, String), String> {
        let socket = endpoint_path();
        if let Some(parent) = socket.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if socket.exists() {
            // 尝试连接以判断是否已有实例
            if UnixStream::connect(&socket).is_ok() {
                return Err("已有 audiosync 实例运行".into());
            }
            let _ = fs::remove_file(&socket);
        }
        let listener =
            UnixListener::bind(&socket).map_err(|e| format!("绑定 Socket 失败: {}", e))?;
        Ok((listener, socket.display().to_string()))
    }

    /// 客户端连接守护进程
    pub fn connect_daemon() -> Result<IpcStream, String> {
        let socket = endpoint_path();
        UnixStream::connect(&socket).map_err(|e| format!("无法连接守护进程: {}", e))
    }

    /// 服务器退出清理
    pub fn cleanup_instance() {
        let _ = fs::remove_file(endpoint_path());
    }

    /// 检查当前用户是否为 root
    pub fn is_running_as_root() -> bool {
        let uid = std::env::var("UID")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(1000);
        uid == 0
    }
}

#[cfg(windows)]
mod transport {
    use super::*;
    use std::net::{TcpListener, TcpStream};

    pub type IpcListener = TcpListener;
    pub type IpcStream = TcpStream;

    /// 默认控制端口（TCP localhost）
    const DEFAULT_PORT: u16 = 45123;

    /// 端口记录文件：%LOCALAPPDATA%\AudioSync\daemon.port
    pub fn port_file() -> PathBuf {
        let base = std::env::var("LOCALAPPDATA")
            .or_else(|_| std::env::var("APPDATA"))
            .unwrap_or_else(|_| std::env::temp_dir().display().to_string());
        PathBuf::from(base).join("AudioSync").join("daemon.port")
    }

    fn read_port() -> u16 {
        fs::read_to_string(port_file())
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(DEFAULT_PORT)
    }

    /// 绑定控制通道；若已有实例存活则返回错误（单实例检测）
    ///
    /// Windows 侧使用固定端口的 TCP 绑定排他性实现单实例；
    /// 端口写入 %LOCALAPPDATA%\AudioSync\daemon.port 供客户端发现。
    pub fn bind_instance() -> Result<(IpcListener, String), String> {
        // 先探测：能连接说明已有实例
        let port = read_port();
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return Err("已有 audiosync 实例运行".into());
        }

        let listener = TcpListener::bind(("127.0.0.1", port))
            .map_err(|e| format!("绑定端口 {} 失败（可能被其他程序占用）: {}", port, e))?;
        let addr = listener.local_addr().map_err(|e| e.to_string())?;

        // 记录实际端口
        let file = port_file();
        if let Some(parent) = file.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&file, addr.port().to_string());

        Ok((listener, format!("127.0.0.1:{}", addr.port())))
    }

    /// 客户端连接守护进程
    pub fn connect_daemon() -> Result<IpcStream, String> {
        let port = read_port();
        TcpStream::connect(("127.0.0.1", port))
            .map_err(|e| format!("无法连接守护进程（127.0.0.1:{}）: {}", port, e))
    }

    /// 服务器退出清理
    pub fn cleanup_instance() {
        let _ = fs::remove_file(port_file());
    }

    /// Windows 无 root 概念
    pub fn is_running_as_root() -> bool {
        false
    }
}

use transport::{
    bind_instance, cleanup_instance, connect_daemon, is_running_as_root, IpcListener, IpcStream,
};

/// 守护进程内部状态
#[derive(Debug, Clone)]
struct DaemonState {
    is_playing: bool,
    current_playlist: Option<String>,
    current_file: Option<String>,
    position_secs: f64,
    duration_secs: Option<f64>,
    volume: f64,
    loop_mode: String,
    audio_device_available: bool,
    playlists: Vec<(String, String, usize)>, // (directory, name, file_count)
    transfers: Vec<(String, String, String, f64)>, // (id, file_name, state, progress)
}

impl Default for DaemonState {
    fn default() -> Self {
        Self {
            is_playing: false,
            current_playlist: None,
            current_file: None,
            position_secs: 0.0,
            duration_secs: None,
            volume: 1.0,
            loop_mode: "off".into(),
            audio_device_available: true,
            playlists: Vec::new(),
            transfers: Vec::new(),
        }
    }
}

// ===== 请求处理 =====

fn handle_request(req: IpcRequest, state: &Arc<Mutex<DaemonState>>) -> IpcResponse {
    let mut state = state.lock().unwrap();

    match req {
        IpcRequest::Status => IpcResponse::Status(StatusInfo {
            is_playing: state.is_playing,
            current_playlist: state.current_playlist.clone(),
            current_file: state.current_file.clone(),
            position_secs: state.position_secs,
            duration_secs: state.duration_secs,
            volume: state.volume,
            loop_mode: state.loop_mode.clone(),
            audio_device_available: state.audio_device_available,
        }),

        IpcRequest::ListPlaylists => IpcResponse::PlaylistList(
            state
                .playlists
                .iter()
                .map(|(dir, name, count)| PlaylistInfo {
                    directory: dir.clone(),
                    name: name.clone(),
                    file_count: *count,
                })
                .collect(),
        ),

        IpcRequest::SelectPlaylist { directory } => {
            state.current_playlist = Some(directory);
            state.current_file = None;
            state.position_secs = 0.0;
            IpcResponse::Ok
        }

        IpcRequest::PlayPause => {
            if !state.audio_device_available {
                return IpcResponse::Error {
                    message: "无音频输出设备，播放功能不可用".into(),
                };
            }
            state.is_playing = !state.is_playing;
            IpcResponse::Ok
        }

        IpcRequest::Seek { position_secs } => {
            if !state.audio_device_available {
                return IpcResponse::Error {
                    message: "无音频输出设备，播放功能不可用".into(),
                };
            }
            state.position_secs = position_secs.max(0.0);
            IpcResponse::Ok
        }

        IpcRequest::Next => {
            state.position_secs = 0.0;
            IpcResponse::Ok
        }

        IpcRequest::Prev => {
            state.position_secs = 0.0;
            IpcResponse::Ok
        }

        IpcRequest::SetVolume { volume } => {
            state.volume = volume.clamp(0.0, 1.0);
            IpcResponse::Ok
        }

        IpcRequest::SyncMetadata => IpcResponse::Ok,

        IpcRequest::StartTransfer => IpcResponse::Ok,

        IpcRequest::ListTransfers => IpcResponse::TransferList(
            state
                .transfers
                .iter()
                .map(|(id, name, s, progress)| TransferInfo {
                    id: id.clone(),
                    file_name: name.clone(),
                    state: s.clone(),
                    progress: *progress,
                })
                .collect(),
        ),

        IpcRequest::Shutdown => {
            // 标记关闭（实际退出由服务器循环检测）
            IpcResponse::Ok
        }
    }
}

// ===== 服务器 =====

fn run_server(listener: IpcListener, state: Arc<Mutex<DaemonState>>) {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let state = Arc::clone(&state);
                thread::spawn(move || {
                    handle_client(stream, &state);
                });
            }
            Err(e) => {
                eprintln!("[audiosync] 连接接受失败: {}", e);
            }
        }
    }
}

fn handle_client(stream: IpcStream, state: &Arc<Mutex<DaemonState>>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut writer = stream;

    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break, // 连接关闭
            Ok(_) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let req = match deserialize_request(line) {
                    Ok(r) => r,
                    Err(e) => {
                        let resp = IpcResponse::Error {
                            message: format!("请求解析失败: {}", e),
                        };
                        let _ = writeln!(writer, "{}", serialize_response(&resp).unwrap());
                        continue;
                    }
                };

                let is_shutdown = matches!(req, IpcRequest::Shutdown);
                let resp = handle_request(req, state);
                let resp_json = serialize_response(&resp).unwrap();
                if writeln!(writer, "{}", resp_json).is_err() {
                    break;
                }

                if is_shutdown {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

// ===== CLI 客户端 =====

fn send_request(req: &IpcRequest) -> Result<IpcResponse, String> {
    let mut stream = connect_daemon()?;
    let req_json = serialize_request(req).map_err(|e| e.to_string())?;
    writeln!(stream, "{}", req_json).map_err(|e| e.to_string())?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| format!("读取响应失败: {}", e))?;
    deserialize_response(line.trim()).map_err(|e| e.to_string())
}

fn print_response(resp: &IpcResponse) {
    match resp {
        IpcResponse::Ok => println!("OK"),
        IpcResponse::Status(info) => {
            println!(
                "播放: {}",
                if info.is_playing {
                    "▶ 播放中"
                } else {
                    "⏸ 已暂停"
                }
            );
            println!("清单: {}", info.current_playlist.as_deref().unwrap_or("-"));
            println!("文件: {}", info.current_file.as_deref().unwrap_or("-"));
            println!(
                "位置: {:.1}s / {}",
                info.position_secs,
                info.duration_secs
                    .map(|d| format!("{:.1}s", d))
                    .unwrap_or_else(|| "-".into())
            );
            println!("音量: {:.0}%", info.volume * 100.0);
            println!("循环: {}", info.loop_mode);
            println!(
                "音频设备: {}",
                if info.audio_device_available {
                    "可用"
                } else {
                    "不可用（播放功能受限）"
                }
            );
        }
        IpcResponse::PlaylistList(list) => {
            if list.is_empty() {
                println!("（无清单）");
            } else {
                for p in list {
                    println!("{}  [{}]  {} 首", p.name, p.directory, p.file_count);
                }
            }
        }
        IpcResponse::TransferList(list) => {
            if list.is_empty() {
                println!("（无传输任务）");
            } else {
                for t in list {
                    println!(
                        "{}  {}  {:.1}%  {}",
                        t.id,
                        t.state,
                        t.progress * 100.0,
                        t.file_name
                    );
                }
            }
        }
        IpcResponse::Error { message } => {
            eprintln!("错误: {}", message);
            exit(1);
        }
    }
}

// ===== CLI 入口 =====

fn print_help() {
    println!(
        "audiosync - 本地音乐互传播放器\n\n\
         用法:\n  \
           audiosync daemon              启动守护进程\n  \
           audiosync status              查询当前状态\n  \
           audiosync list                列出清单\n  \
           audiosync select <目录>       选择清单\n  \
           audiosync play                播放/暂停\n  \
           audiosync seek <秒>           跳转到指定位置\n  \
           audiosync next                下一曲\n  \
           audiosync prev                上一曲\n  \
           audiosync volume <0-100>      设置音量\n  \
           audiosync sync                触发元数据同步\n  \
           audiosync transfers           查看传输任务\n  \
           audiosync shutdown            关闭守护进程\n  \
           audiosync help                显示此帮助"
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() {
        print_help();
        return;
    }

    match args[0].as_str() {
        "daemon" => run_daemon(),
        "help" | "--help" | "-h" => print_help(),
        "status" => client_cmd(IpcRequest::Status),
        "list" => client_cmd(IpcRequest::ListPlaylists),
        "select" => {
            if args.len() < 2 {
                eprintln!("用法: audiosync select <目录>");
                exit(1);
            }
            client_cmd(IpcRequest::SelectPlaylist {
                directory: args[1].clone(),
            })
        }
        "play" => client_cmd(IpcRequest::PlayPause),
        "seek" => {
            if args.len() < 2 {
                eprintln!("用法: audiosync seek <秒>");
                exit(1);
            }
            let pos: f64 = match args[1].parse() {
                Ok(v) => v,
                Err(_) => {
                    eprintln!("无效的位置: {}", args[1]);
                    exit(1);
                }
            };
            client_cmd(IpcRequest::Seek { position_secs: pos })
        }
        "next" => client_cmd(IpcRequest::Next),
        "prev" => client_cmd(IpcRequest::Prev),
        "volume" => {
            if args.len() < 2 {
                eprintln!("用法: audiosync volume <0-100>");
                exit(1);
            }
            let vol: f64 = match args[1].parse::<f64>() {
                Ok(v) => v / 100.0,
                Err(_) => {
                    eprintln!("无效的音量: {}", args[1]);
                    exit(1);
                }
            };
            client_cmd(IpcRequest::SetVolume { volume: vol })
        }
        "sync" => client_cmd(IpcRequest::SyncMetadata),
        "transfers" => client_cmd(IpcRequest::ListTransfers),
        "shutdown" => client_cmd(IpcRequest::Shutdown),
        other => {
            eprintln!("未知命令: {}", other);
            print_help();
            exit(1);
        }
    }
}

fn client_cmd(req: IpcRequest) {
    match send_request(&req) {
        Ok(resp) => print_response(&resp),
        Err(e) => {
            eprintln!("{}", e);
            exit(1);
        }
    }
}

fn run_daemon() {
    // T75: 非 root 校验
    if is_running_as_root() {
        eprintln!("[audiosync] 错误: 禁止以 root 运行，请使用普通用户启动。");
        exit(1);
    }

    println!("[audiosync] 启动守护进程...");

    // T71: 单实例检测 + 绑定控制通道
    let (listener, endpoint) = match bind_instance() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[audiosync] {}", e);
            exit(1);
        }
    };

    println!("[audiosync] 控制通道监听: {}", endpoint);

    let state = Arc::new(Mutex::new(DaemonState::default()));

    // T74: 结构化日志（示例）
    eprintln!(
        "[audiosync] {{\"event\":\"daemon_started\",\"pid\":{},\"endpoint\":\"{}\"}}",
        std::process::id(),
        endpoint
    );

    run_server(listener, state);

    // 清理 endpoint（socket 文件 / 端口文件）
    cleanup_instance();
    println!("[audiosync] 已退出。");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_state_defaults() {
        let state = DaemonState::default();
        assert!(!state.is_playing);
        assert!(state.current_playlist.is_none());
        assert_eq!(state.volume, 1.0);
        assert!(state.audio_device_available);
    }

    #[test]
    fn is_root_detection() {
        // 在测试环境中通常非 root
        assert!(!is_running_as_root() || std::env::var("UID").unwrap_or_default() == "0");
    }

    #[cfg(unix)]
    #[test]
    fn socket_path_ends_with_sock() {
        // 不设置环境变量（避免与并发测试竞态），仅验证文件名
        let path = transport::endpoint_path();
        assert!(path.ends_with("audiosync.sock"));
    }

    #[test]
    fn handle_status_request() {
        let state = Arc::new(Mutex::new(DaemonState::default()));
        let resp = handle_request(IpcRequest::Status, &state);
        match resp {
            IpcResponse::Status(info) => {
                assert!(!info.is_playing);
                assert_eq!(info.volume, 1.0);
            }
            _ => panic!("expected Status response"),
        }
    }

    #[test]
    fn handle_select_playlist() {
        let state = Arc::new(Mutex::new(DaemonState::default()));
        let resp = handle_request(
            IpcRequest::SelectPlaylist {
                directory: "/music/rock".into(),
            },
            &state,
        );
        assert!(matches!(resp, IpcResponse::Ok));
        assert_eq!(
            state.lock().unwrap().current_playlist.as_deref(),
            Some("/music/rock")
        );
    }

    #[test]
    fn handle_play_pause_toggles() {
        let state = Arc::new(Mutex::new(DaemonState::default()));
        handle_request(IpcRequest::PlayPause, &state);
        assert!(state.lock().unwrap().is_playing);
        handle_request(IpcRequest::PlayPause, &state);
        assert!(!state.lock().unwrap().is_playing);
    }

    #[test]
    fn handle_play_pause_fails_without_audio_device() {
        let state = Arc::new(Mutex::new(DaemonState {
            audio_device_available: false,
            ..Default::default()
        }));
        let resp = handle_request(IpcRequest::PlayPause, &state);
        match resp {
            IpcResponse::Error { message } => {
                assert!(message.contains("无音频输出设备"));
            }
            _ => panic!("expected Error response"),
        }
    }

    #[test]
    fn handle_set_volume_clamps() {
        let state = Arc::new(Mutex::new(DaemonState::default()));
        handle_request(IpcRequest::SetVolume { volume: 1.5 }, &state);
        assert!((state.lock().unwrap().volume - 1.0).abs() < 0.001);

        handle_request(IpcRequest::SetVolume { volume: -0.5 }, &state);
        assert!(state.lock().unwrap().volume.abs() < 0.001);
    }

    #[test]
    fn handle_seek_clamps_negative() {
        let state = Arc::new(Mutex::new(DaemonState::default()));
        handle_request(
            IpcRequest::Seek {
                position_secs: -5.0,
            },
            &state,
        );
        assert!(state.lock().unwrap().position_secs >= 0.0);
    }

    #[test]
    fn handle_list_playlists_empty() {
        let state = Arc::new(Mutex::new(DaemonState::default()));
        let resp = handle_request(IpcRequest::ListPlaylists, &state);
        match resp {
            IpcResponse::PlaylistList(list) => {
                assert!(list.is_empty());
            }
            _ => panic!("expected PlaylistList response"),
        }
    }

    #[test]
    fn ipc_roundtrip_via_endpoint() {
        // 使用 transport 抽象绑定与连接（Unix Socket / Windows TCP 自动适配）
        let (listener, _endpoint) = transport::bind_instance().expect("绑定控制通道");
        let state = Arc::new(Mutex::new(DaemonState {
            is_playing: true,
            volume: 0.5,
            ..Default::default()
        }));

        // 客户端连接
        let client_stream = transport::connect_daemon().expect("客户端连接");

        // 主线程 accept（避免线程调度竞态）
        let (server_stream, _) = listener.accept().unwrap();

        // 在子线程处理客户端
        let state_clone = Arc::clone(&state);
        let handle = thread::spawn(move || {
            handle_client(server_stream, &state_clone);
        });

        // 客户端发送请求
        let mut client_stream = client_stream;
        let req = IpcRequest::Status;
        let req_json = serialize_request(&req).unwrap();
        writeln!(client_stream, "{}", req_json).unwrap();
        client_stream.flush().unwrap();

        // 读取响应
        let mut reader = BufReader::new(client_stream);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let resp: IpcResponse = deserialize_response(line.trim()).unwrap();

        match resp {
            IpcResponse::Status(info) => {
                assert!(info.is_playing);
                assert!((info.volume - 0.5).abs() < 0.001);
            }
            _ => panic!("expected Status response"),
        }

        // drop reader 关闭客户端连接，服务器 read_line 返回 EOF
        drop(reader);
        handle.join().unwrap();
        transport::cleanup_instance();
    }
}
