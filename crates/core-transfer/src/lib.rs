//! core-transfer: P2P 文件传输
//!
//! 实现 FR-06：直连传输通道、多文件、双向推送/拉取、断点续传、SHA-256 校验。

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;
use thiserror::Error;
use uuid::Uuid;

/// 默认分块大小：256 KB
pub const DEFAULT_CHUNK_SIZE: u64 = 256 * 1024;

/// 传输任务唯一 ID
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransferTaskId(Uuid);

impl TransferTaskId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for TransferTaskId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TransferTaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 传输状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferState {
    /// 等待开始
    Pending,
    /// 传输中
    Transferring,
    /// 已暂停
    Paused,
    /// 已完成
    Completed,
    /// 失败
    Failed,
}

/// 传输方向
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferDirection {
    /// 推送（发送方主动发起）
    Push,
    /// 拉取（接收方主动请求）
    Pull,
}

/// 传输任务
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferTask {
    pub id: TransferTaskId,
    pub file_name: String,
    pub file_size: u64,
    pub transferred_bytes: u64,
    pub state: TransferState,
    pub direction: TransferDirection,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl TransferTask {
    /// 测试用构造
    pub fn new_test() -> Self {
        Self {
            id: TransferTaskId::new(),
            file_name: "test.mp3".into(),
            file_size: 1000,
            transferred_bytes: 0,
            state: TransferState::Pending,
            direction: TransferDirection::Push,
            error: None,
        }
    }

    /// 开始传输
    pub fn start(&mut self) {
        if self.state == TransferState::Pending || self.state == TransferState::Paused {
            self.state = TransferState::Transferring;
            self.error = None;
        }
    }

    /// 暂停
    pub fn pause(&mut self) {
        if self.state == TransferState::Transferring {
            self.state = TransferState::Paused;
        }
    }

    /// 恢复
    pub fn resume(&mut self) {
        if self.state == TransferState::Paused {
            self.state = TransferState::Transferring;
        }
    }

    /// 更新进度（new_bytes 为本次新增字节数）
    pub fn update_progress(&mut self, new_bytes: u64) {
        if self.state != TransferState::Transferring {
            return;
        }
        self.transferred_bytes += new_bytes;
        if self.transferred_bytes >= self.file_size {
            self.transferred_bytes = self.file_size;
            self.state = TransferState::Completed;
        }
    }

    /// 标记失败
    pub fn fail(&mut self, reason: String) {
        self.state = TransferState::Failed;
        self.error = Some(reason);
    }

    /// 进度百分比（0.0 ~ 100.0）
    pub fn progress(&self) -> f64 {
        if self.file_size == 0 {
            return 100.0;
        }
        (self.transferred_bytes as f64 / self.file_size as f64) * 100.0
    }

    pub fn is_completed(&self) -> bool {
        self.state == TransferState::Completed
    }

    pub fn is_failed(&self) -> bool {
        self.state == TransferState::Failed
    }
}

/// 文件分块信息
#[derive(Debug, Clone, Copy)]
pub struct ChunkInfo {
    pub file_size: u64,
    pub chunk_size: u64,
    pub total_chunks: u64,
}

impl ChunkInfo {
    pub fn new(file_size: u64, chunk_size: u64) -> Self {
        let chunk_size = chunk_size.max(1);
        let total_chunks = if file_size == 0 {
            0
        } else {
            file_size.div_ceil(chunk_size)
        };
        Self {
            file_size,
            chunk_size,
            total_chunks,
        }
    }

    /// 第 index 块的字节偏移
    pub fn chunk_offset(&self, index: u64) -> u64 {
        index * self.chunk_size
    }

    /// 第 index 块的实际大小
    pub fn chunk_size_at(&self, index: u64) -> u64 {
        if index >= self.total_chunks {
            return 0;
        }
        let offset = self.chunk_offset(index);
        let remaining = self.file_size - offset;
        remaining.min(self.chunk_size)
    }

    /// 字节偏移所属的块索引
    pub fn chunk_index(&self, byte_offset: u64) -> u64 {
        byte_offset / self.chunk_size
    }
}

/// 计算数据的 SHA-256 哈希（十六进制字符串）
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// 计算文件的 SHA-256 哈希
pub fn hash_file<P: AsRef<Path>>(path: P) -> std::io::Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

// ===== 传输协议消息 =====

/// 传输请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferRequest {
    pub task_id: TransferTaskId,
    pub file_name: String,
    pub file_size: u64,
    pub chunk_size: u64,
    pub file_hash: String,
}

/// 数据块消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkMessage {
    pub task_id: TransferTaskId,
    pub seq: u64,
    pub offset: u64,
    pub data: Vec<u8>,
}

/// 传输响应（接受/拒绝）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferResponse {
    pub task_id: TransferTaskId,
    pub accepted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// 完成通知
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferComplete {
    pub task_id: TransferTaskId,
    pub file_hash: String,
}

// ===== 断点续传（T43）=====

/// 分块传输进度（断点续传用）
///
/// 用位图记录每个分块是否已传输完成。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferProgress {
    pub task_id: TransferTaskId,
    pub file_size: u64,
    pub chunk_size: u64,
    /// 已完成块的位图
    chunks_done: Vec<bool>,
}

impl TransferProgress {
    pub fn new(task_id: TransferTaskId, file_size: u64, chunk_size: u64) -> Self {
        let info = ChunkInfo::new(file_size, chunk_size);
        Self {
            task_id,
            file_size,
            chunk_size,
            chunks_done: vec![false; info.total_chunks as usize],
        }
    }

    pub fn total_chunks(&self) -> u64 {
        self.chunks_done.len() as u64
    }

    pub fn done_chunks(&self) -> u64 {
        self.chunks_done.iter().filter(|&&b| b).count() as u64
    }

    /// 标记某块已完成
    pub fn mark_done(&mut self, seq: u64) {
        if (seq as usize) < self.chunks_done.len() {
            self.chunks_done[seq as usize] = true;
        }
    }

    /// 某块是否已完成
    pub fn is_done(&self, seq: u64) -> bool {
        self.chunks_done.get(seq as usize).copied().unwrap_or(false)
    }

    /// 是否已全部完成
    pub fn is_complete(&self) -> bool {
        self.chunks_done.iter().all(|&b| b)
    }

    /// 已传输字节数
    pub fn transferred_bytes(&self) -> u64 {
        let info = ChunkInfo::new(self.file_size, self.chunk_size);
        let mut total = 0;
        for (i, &done) in self.chunks_done.iter().enumerate() {
            if done {
                total += info.chunk_size_at(i as u64);
            }
        }
        total
    }

    /// 获取下一个未完成的块索引（用于续传）
    pub fn next_undone(&self) -> Option<u64> {
        self.chunks_done.iter().position(|&b| !b).map(|i| i as u64)
    }
}

// ===== 离线排队（T44）=====

/// 离线排队队列最大长度（FR-06）
pub const MAX_QUEUE_SIZE: usize = 100;

/// 传输队列（离线排队，持久化）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransferQueue {
    tasks: Vec<TransferTask>,
}

impl TransferQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// 入队；队列满（100）时返回 false
    pub fn enqueue(&mut self, task: TransferTask) -> bool {
        if self.tasks.len() >= MAX_QUEUE_SIZE {
            return false;
        }
        self.tasks.push(task);
        true
    }

    /// 出队（取出队首）
    pub fn dequeue(&mut self) -> Option<TransferTask> {
        if self.tasks.is_empty() {
            None
        } else {
            Some(self.tasks.remove(0))
        }
    }

    /// 查看队首
    pub fn peek(&self) -> Option<&TransferTask> {
        self.tasks.first()
    }

    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.tasks.len() >= MAX_QUEUE_SIZE
    }

    pub fn iter(&self) -> impl Iterator<Item = &TransferTask> {
        self.tasks.iter()
    }
}

// ===== 传输层（T41/T42）=====

/// 传输模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportMode {
    /// 直连
    Direct,
    /// 中继（TURN/coturn）
    Relay,
}

/// 传输错误
#[derive(Error, Debug)]
pub enum TransportError {
    #[error("I/O 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("序列化错误: {0}")]
    Serialize(String),
    #[error("对方拒绝传输")]
    Rejected,
    #[error("校验失败")]
    HashMismatch,
    #[error("连接超时")]
    Timeout,
}

/// 发送一个长度前缀的 JSON 消息
fn send_message<W: Write, T: Serialize>(writer: &mut W, msg: &T) -> Result<(), TransportError> {
    let json = serde_json::to_vec(msg).map_err(|e| TransportError::Serialize(e.to_string()))?;
    let len = json.len() as u32;
    writer.write_all(&len.to_be_bytes())?;
    writer.write_all(&json)?;
    writer.flush()?;
    Ok(())
}

/// 接收一个长度前缀的 JSON 消息
fn recv_message<R: Read, T: for<'de> Deserialize<'de>>(
    reader: &mut R,
) -> Result<T, TransportError> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    serde_json::from_slice(&buf).map_err(|e| TransportError::Serialize(e.to_string()))
}

/// 直连发送文件
///
/// 连接到目标地址，发送传输请求，等待响应，然后逐块发送文件。
pub fn send_file_direct(
    addr: impl ToSocketAddrs,
    file_path: &Path,
    chunk_size: u64,
) -> Result<(), TransportError> {
    let file_size = std::fs::metadata(file_path)?.len();
    let file_hash = hash_file(file_path)?;
    let file_name = file_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let task_id = TransferTaskId::new();
    let request = TransferRequest {
        task_id,
        file_name,
        file_size,
        chunk_size,
        file_hash: file_hash.clone(),
    };

    let mut stream = TcpStream::connect_timeout(
        &addr.to_socket_addrs()?.next().ok_or_else(|| {
            TransportError::Io(std::io::Error::new(
                std::io::ErrorKind::AddrNotAvailable,
                "no address",
            ))
        })?,
        Duration::from_secs(10),
    )?;

    // 1. 发送请求
    send_message(&mut stream, &request)?;

    // 2. 等待响应
    let response: TransferResponse = recv_message(&mut stream)?;
    if !response.accepted {
        return Err(TransportError::Rejected);
    }

    // 3. 逐块发送
    let mut file = std::fs::File::open(file_path)?;
    let info = ChunkInfo::new(file_size, chunk_size);
    let mut offset = 0u64;

    for seq in 0..info.total_chunks {
        let size = info.chunk_size_at(seq) as usize;
        let mut data = vec![0u8; size];
        file.read_exact(&mut data)?;

        let chunk = ChunkMessage {
            task_id,
            seq,
            offset,
            data,
        };
        send_message(&mut stream, &chunk)?;
        offset += size as u64;
    }

    // 4. 发送完成通知
    let complete = TransferComplete { task_id, file_hash };
    send_message(&mut stream, &complete)?;

    Ok(())
}

/// 直连接收文件（单连接）
///
/// 监听指定地址，接受一个连接，接收文件并保存到 output_dir。
/// 返回保存的文件路径。
pub fn recv_file_direct(
    listen_addr: impl ToSocketAddrs,
    output_dir: &Path,
) -> Result<std::path::PathBuf, TransportError> {
    let listener = TcpListener::bind(listen_addr)?;
    let (mut stream, _) = listener.accept()?;

    // 1. 接收请求
    let request: TransferRequest = recv_message(&mut stream)?;

    // 2. 接受传输
    let response = TransferResponse {
        task_id: request.task_id,
        accepted: true,
        reason: None,
    };
    send_message(&mut stream, &response)?;

    // 3. 接收所有块
    std::fs::create_dir_all(output_dir)?;
    let output_path = output_dir.join(&request.file_name);
    let mut file = std::fs::File::create(&output_path)?;

    let info = ChunkInfo::new(request.file_size, request.chunk_size);
    for _ in 0..info.total_chunks {
        let chunk: ChunkMessage = recv_message(&mut stream)?;
        file.write_all(&chunk.data)?;
    }

    // 4. 接收完成通知并校验
    let complete: TransferComplete = recv_message(&mut stream)?;
    drop(file);

    let actual_hash = hash_file(&output_path)?;
    if actual_hash != complete.file_hash {
        std::fs::remove_file(&output_path).ok();
        return Err(TransportError::HashMismatch);
    }

    Ok(output_path)
}

/// 传输回退策略：直连失败后使用中继地址
///
/// `direct_addr` 为对端直连地址，`relay_addr` 为 coturn 中继地址。
/// 先尝试直连，失败则使用中继。
pub fn send_file_with_fallback(
    direct_addr: impl ToSocketAddrs + Clone,
    relay_addr: Option<impl ToSocketAddrs>,
    file_path: &Path,
    chunk_size: u64,
) -> Result<TransportMode, TransportError> {
    // 先尝试直连
    match send_file_direct(direct_addr.clone(), file_path, chunk_size) {
        Ok(()) => Ok(TransportMode::Direct),
        Err(TransportError::Rejected) => Err(TransportError::Rejected),
        Err(_) => {
            // 直连失败，尝试中继
            if let Some(relay) = relay_addr {
                send_file_direct(relay, file_path, chunk_size)?;
                Ok(TransportMode::Relay)
            } else {
                Err(TransportError::Io(std::io::Error::new(
                    std::io::ErrorKind::ConnectionRefused,
                    "direct connection failed and no relay configured",
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn temp_dir() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("core-transfer-test-{}-{}", std::process::id(), id));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn create_test_file(path: &std::path::Path, size: usize) {
        let mut f = fs::File::create(path).unwrap();
        let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
        f.write_all(&data).unwrap();
    }

    // ===== 任务状态机测试 =====

    #[test]
    fn task_starts_in_pending_state() {
        let task = TransferTask::new_test();
        assert_eq!(task.state, TransferState::Pending);
    }

    #[test]
    fn pending_to_transition_to_transferring() {
        let mut task = TransferTask::new_test();
        task.start();
        assert_eq!(task.state, TransferState::Transferring);
    }

    #[test]
    fn transfer_completes_when_all_bytes_transferred() {
        let mut task = TransferTask {
            id: TransferTaskId::new(),
            file_name: "test.mp3".into(),
            file_size: 1000,
            transferred_bytes: 1000,
            state: TransferState::Transferring,
            direction: TransferDirection::Push,
            error: None,
        };
        task.update_progress(0); // 0 新字节，但已传完
        assert_eq!(task.state, TransferState::Completed);
    }

    #[test]
    fn transfer_fails_on_error() {
        let mut task = TransferTask::new_test();
        task.start();
        task.fail("network error".into());
        assert_eq!(task.state, TransferState::Failed);
        assert_eq!(task.error.as_deref(), Some("network error"));
    }

    #[test]
    fn task_progress_percentage() {
        let mut task = TransferTask {
            id: TransferTaskId::new(),
            file_name: "test.mp3".into(),
            file_size: 1000,
            transferred_bytes: 250,
            state: TransferState::Transferring,
            direction: TransferDirection::Push,
            error: None,
        };
        assert!((task.progress() - 25.0).abs() < 0.01);

        task.update_progress(250);
        assert!((task.progress() - 50.0).abs() < 0.01);
    }

    #[test]
    fn pause_and_resume() {
        let mut task = TransferTask::new_test();
        task.start();
        task.pause();
        assert_eq!(task.state, TransferState::Paused);

        task.resume();
        assert_eq!(task.state, TransferState::Transferring);
    }

    // ===== 文件分块测试 =====

    #[test]
    fn chunk_count_calculation() {
        // 1000 字节，每块 256 字节 → 4 块
        let info = ChunkInfo::new(1000, 256);
        assert_eq!(info.total_chunks, 4);
        assert_eq!(info.chunk_size, 256);
    }

    #[test]
    fn chunk_count_with_remainder() {
        // 1000 字节，每块 300 字节 → 4 块（300+300+300+100）
        let info = ChunkInfo::new(1000, 300);
        assert_eq!(info.total_chunks, 4);
    }

    #[test]
    fn chunk_count_for_small_file() {
        let info = ChunkInfo::new(100, 256);
        assert_eq!(info.total_chunks, 1);
    }

    #[test]
    fn chunk_offset_for_index() {
        let info = ChunkInfo::new(1000, 256);
        assert_eq!(info.chunk_offset(0), 0);
        assert_eq!(info.chunk_offset(1), 256);
        assert_eq!(info.chunk_offset(2), 512);
        assert_eq!(info.chunk_offset(3), 768);
    }

    #[test]
    fn chunk_size_for_last_chunk() {
        let info = ChunkInfo::new(1000, 300);
        assert_eq!(info.chunk_size_at(0), 300);
        assert_eq!(info.chunk_size_at(1), 300);
        assert_eq!(info.chunk_size_at(2), 300);
        assert_eq!(info.chunk_size_at(3), 100); // 最后一块 100 字节
    }

    #[test]
    fn chunk_index_for_byte_offset() {
        let info = ChunkInfo::new(1000, 256);
        assert_eq!(info.chunk_index(0), 0);
        assert_eq!(info.chunk_index(255), 0);
        assert_eq!(info.chunk_index(256), 1);
        assert_eq!(info.chunk_index(999), 3);
    }

    // ===== SHA-256 校验测试 =====

    #[test]
    fn sha256_hash_matches_expected() {
        // "hello" 的 SHA-256 是已知值
        let hash = sha256_hex(b"hello");
        assert_eq!(
            hash,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn file_hash_is_deterministic() {
        let dir = temp_dir();
        let path = dir.join("test.bin");
        create_test_file(&path, 1000);

        let h1 = hash_file(&path).unwrap();
        let h2 = hash_file(&path).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn different_files_have_different_hashes() {
        let dir = temp_dir();
        let p1 = dir.join("a.bin");
        let p2 = dir.join("b.bin");
        create_test_file(&p1, 100);
        create_test_file(&p2, 101); // 不同大小

        assert_ne!(hash_file(&p1).unwrap(), hash_file(&p2).unwrap());
    }

    // ===== 传输消息测试 =====

    #[test]
    fn transfer_request_serialization() {
        let req = TransferRequest {
            task_id: TransferTaskId::new(),
            file_name: "song.mp3".into(),
            file_size: 5000,
            chunk_size: 1024,
            file_hash: "abc123".into(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: TransferRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.file_name, req.file_name);
        assert_eq!(parsed.file_size, req.file_size);
        assert_eq!(parsed.file_hash, req.file_hash);
    }

    #[test]
    fn chunk_message_serialization() {
        let chunk = ChunkMessage {
            task_id: TransferTaskId::new(),
            seq: 5,
            offset: 5120,
            data: vec![1, 2, 3, 4],
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let parsed: ChunkMessage = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.seq, 5);
        assert_eq!(parsed.offset, 5120);
        assert_eq!(parsed.data, vec![1, 2, 3, 4]);
    }

    // ===== 断点续传测试 =====

    #[test]
    fn progress_starts_all_undone() {
        let progress = TransferProgress::new(TransferTaskId::new(), 1000, 256);
        assert_eq!(progress.total_chunks(), 4);
        assert_eq!(progress.done_chunks(), 0);
        assert!(!progress.is_complete());
    }

    #[test]
    fn mark_done_tracks_completion() {
        let mut progress = TransferProgress::new(TransferTaskId::new(), 1000, 256);
        progress.mark_done(0);
        progress.mark_done(2);

        assert!(progress.is_done(0));
        assert!(!progress.is_done(1));
        assert!(progress.is_done(2));
        assert_eq!(progress.done_chunks(), 2);
    }

    #[test]
    fn all_done_marks_complete() {
        let mut progress = TransferProgress::new(TransferTaskId::new(), 600, 300);
        progress.mark_done(0);
        progress.mark_done(1);
        assert!(progress.is_complete());
    }

    #[test]
    fn next_undone_returns_first_pending() {
        let mut progress = TransferProgress::new(TransferTaskId::new(), 1000, 256);
        assert_eq!(progress.next_undone(), Some(0));

        progress.mark_done(0);
        assert_eq!(progress.next_undone(), Some(1));

        progress.mark_done(1);
        progress.mark_done(2);
        progress.mark_done(3);
        assert_eq!(progress.next_undone(), None);
    }

    #[test]
    fn transferred_bytes_sum_done_chunks() {
        let mut progress = TransferProgress::new(TransferTaskId::new(), 1000, 300);
        // chunks: 300, 300, 300, 100
        progress.mark_done(0);
        progress.mark_done(3); // 最后一块 100 字节

        assert_eq!(progress.transferred_bytes(), 400); // 300 + 100
    }

    #[test]
    fn progress_serialization_persists_chunk_state() {
        let mut progress = TransferProgress::new(TransferTaskId::new(), 1000, 256);
        progress.mark_done(1);
        let json = serde_json::to_string(&progress).unwrap();
        let parsed: TransferProgress = serde_json::from_str(&json).unwrap();

        assert!(!parsed.is_done(0));
        assert!(parsed.is_done(1));
        assert_eq!(parsed.done_chunks(), 1);
    }

    // ===== 离线排队测试 =====

    #[test]
    fn queue_starts_empty() {
        let q = TransferQueue::new();
        assert!(q.is_empty());
        assert_eq!(q.len(), 0);
    }

    #[test]
    fn enqueue_and_dequeue_fifo() {
        let mut q = TransferQueue::new();
        let t1 = TransferTask::new_test();
        let t2 = TransferTask::new_test();
        let id1 = t1.id;
        let id2 = t2.id;

        assert!(q.enqueue(t1));
        assert!(q.enqueue(t2));
        assert_eq!(q.len(), 2);

        let first = q.dequeue().unwrap();
        assert_eq!(first.id, id1);
        let second = q.dequeue().unwrap();
        assert_eq!(second.id, id2);
        assert!(q.is_empty());
    }

    #[test]
    fn dequeue_empty_returns_none() {
        let mut q = TransferQueue::new();
        assert!(q.dequeue().is_none());
    }

    #[test]
    fn queue_max_size_100() {
        let mut q = TransferQueue::new();
        for _ in 0..MAX_QUEUE_SIZE {
            assert!(q.enqueue(TransferTask::new_test()));
        }
        assert!(q.is_full());
        // 第 101 个被拒绝
        assert!(!q.enqueue(TransferTask::new_test()));
        assert_eq!(q.len(), MAX_QUEUE_SIZE);
    }

    #[test]
    fn peek_does_not_remove() {
        let mut q = TransferQueue::new();
        let t = TransferTask::new_test();
        let id = t.id;
        q.enqueue(t);

        assert_eq!(q.peek().unwrap().id, id);
        assert_eq!(q.len(), 1); // 未移除
    }

    #[test]
    fn queue_serialization_roundtrip() {
        let mut q = TransferQueue::new();
        q.enqueue(TransferTask::new_test());
        q.enqueue(TransferTask::new_test());

        let json = serde_json::to_string(&q).unwrap();
        let parsed: TransferQueue = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.len(), q.len());
    }

    // ===== 直连传输集成测试 =====

    #[test]
    fn direct_send_and_receive_file() {
        let dir = temp_dir();
        let src_path = dir.join("source.mp3");
        create_test_file(&src_path, 5000);

        let output_dir = dir.join("received");
        fs::create_dir_all(&output_dir).unwrap();

        // 绑定到端口 0，让 OS 分配端口
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let listen_addr = listener.local_addr().unwrap();

        // 接收端在后台线程运行
        let output_dir_clone = output_dir.clone();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request: TransferRequest = recv_message(&mut stream).unwrap();
            let response = TransferResponse {
                task_id: request.task_id,
                accepted: true,
                reason: None,
            };
            send_message(&mut stream, &response).unwrap();

            let out_path = output_dir_clone.join(&request.file_name);
            let mut file = fs::File::create(&out_path).unwrap();
            let info = ChunkInfo::new(request.file_size, request.chunk_size);
            for _ in 0..info.total_chunks {
                let chunk: ChunkMessage = recv_message(&mut stream).unwrap();
                file.write_all(&chunk.data).unwrap();
            }
            let complete: TransferComplete = recv_message(&mut stream).unwrap();
            drop(file);

            let actual_hash = hash_file(&out_path).unwrap();
            assert_eq!(actual_hash, complete.file_hash);
            out_path
        });

        // 发送端
        send_file_direct(listen_addr, &src_path, 1024).unwrap();

        let received_path = handle.join().unwrap();
        // 校验文件内容一致
        let src_hash = hash_file(&src_path).unwrap();
        let recv_hash = hash_file(&received_path).unwrap();
        assert_eq!(src_hash, recv_hash);
    }

    #[test]
    fn direct_transfer_rejects_when_receiver_declines() {
        let dir = temp_dir();
        let src_path = dir.join("source.mp3");
        create_test_file(&src_path, 1000);

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let listen_addr = listener.local_addr().unwrap();

        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request: TransferRequest = recv_message(&mut stream).unwrap();
            let response = TransferResponse {
                task_id: request.task_id,
                accepted: false,
                reason: Some("declined".into()),
            };
            send_message(&mut stream, &response).unwrap();
        });

        let result = send_file_direct(listen_addr, &src_path, 1024);
        assert!(matches!(result, Err(TransportError::Rejected)));
        handle.join().unwrap();
    }
}
