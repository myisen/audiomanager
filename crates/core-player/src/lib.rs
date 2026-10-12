//! core-player: 音频播放器
//!
//! 实现 FR-03：内置开源音频播放器（Symphonia，MIT 许可）。
//! T21: 验证 Symphonia 格式覆盖；T22: 播放器接口与不可播放标记。

use std::path::{Path, PathBuf};
use thiserror::Error;

/// 播放器错误
#[derive(Error, Debug)]
pub enum PlayerError {
    #[error("I/O 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("格式探测失败: {0}")]
    Probe(String),
    #[error("不支持的格式")]
    Unsupported,
}

/// Symphonia 支持的格式扩展名（基于启用的 feature）
///
/// T21 验证结论（Symphonia 0.5.5）：
/// 支持：mp3, aac, flac, ogg, oga, m4a, wav
/// 不支持：opus, ape, wv, mka（方案 A：纳入库但标记不可播放）
pub const SUPPORTED_EXTENSIONS: &[&str] = &["mp3", "aac", "flac", "ogg", "oga", "m4a", "wav"];

/// 白名单内但 Symphonia 不支持的格式（方案 A：不可播放）
pub const UNPLAYABLE_EXTENSIONS: &[&str] = &["opus", "ape", "wv", "mka"];

/// 判断扩展名是否可被 Symphonia 解码
pub fn is_playable_extension(ext: &str) -> bool {
    let ext = ext.to_lowercase();
    SUPPORTED_EXTENSIONS.contains(&ext.as_str())
}

/// 探测文件是否可播放
///
/// 使用 Symphonia 的 probe 接口验证文件格式是否可解码。
/// 探测失败（格式不识别、文件损坏等）返回 `PlayerError::Unsupported`。
pub fn probe_file<P: AsRef<Path>>(path: P) -> Result<(), PlayerError> {
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::probe::Hint;

    let path = path.as_ref();
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let file = std::fs::File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let probe_result = symphonia::default::get_probe().format(
        &hint,
        mss,
        &Default::default(),
        &Default::default(),
    );

    match probe_result {
        Ok(_) => Ok(()),
        Err(e) => {
            // 区分「不支持的格式」与其他探测错误
            if matches!(e, symphonia::core::errors::Error::Unsupported(_)) {
                Err(PlayerError::Unsupported)
            } else {
                Err(PlayerError::Probe(e.to_string()))
            }
        }
    }
}

/// 播放状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerState {
    /// 已停止
    Stopped,
    /// 播放中
    Playing,
    /// 已暂停
    Paused,
}

/// 播放器（状态机）
///
/// 管理播放状态与位置。实际音频解码与输出由平台层实现，
/// 此处仅负责状态转换与位置追踪。
pub struct Player {
    state: PlayerState,
    current_file: Option<PathBuf>,
    position: f64,
    duration: f64,
}

impl Default for Player {
    fn default() -> Self {
        Self::new()
    }
}

impl Player {
    pub fn new() -> Self {
        Self {
            state: PlayerState::Stopped,
            current_file: None,
            position: 0.0,
            duration: 0.0,
        }
    }

    /// 开始播放文件，从指定位置开始
    pub fn play<P: AsRef<Path>>(&mut self, file: P, start_position: f64, duration: f64) {
        self.current_file = Some(file.as_ref().to_path_buf());
        self.position = start_position.max(0.0);
        self.duration = duration;
        self.state = PlayerState::Playing;
    }

    /// 暂停
    pub fn pause(&mut self) {
        if self.state == PlayerState::Playing {
            self.state = PlayerState::Paused;
        }
    }

    /// 恢复播放
    pub fn resume(&mut self) {
        if self.state == PlayerState::Paused {
            self.state = PlayerState::Playing;
        }
    }

    /// 跳转到指定位置（秒）
    pub fn seek(&mut self, position: f64) {
        self.position = position.max(0.0);
        if self.duration > 0.0 {
            self.position = self.position.min(self.duration);
        }
    }

    /// 停止
    pub fn stop(&mut self) {
        self.state = PlayerState::Stopped;
        self.current_file = None;
        self.position = 0.0;
        self.duration = 0.0;
    }

    /// 推进播放位置（由音频输出回调调用）
    pub fn advance(&mut self, delta_secs: f64) {
        if self.state == PlayerState::Playing {
            self.position += delta_secs;
            if self.duration > 0.0 && self.position >= self.duration {
                self.position = self.duration;
                self.state = PlayerState::Stopped;
            }
        }
    }

    pub fn state(&self) -> PlayerState {
        self.state
    }

    pub fn position(&self) -> f64 {
        self.position
    }

    pub fn duration(&self) -> f64 {
        self.duration
    }

    pub fn current_file(&self) -> Option<&Path> {
        self.current_file.as_deref()
    }

    pub fn is_playing(&self) -> bool {
        self.state == PlayerState::Playing
    }

    pub fn is_paused(&self) -> bool {
        self.state == PlayerState::Paused
    }
}

/// 续播目标：从哪个文件的哪个位置开始播放
#[derive(Debug, Clone, PartialEq)]
pub struct ResumeTarget {
    pub file_path: std::path::PathBuf,
    pub start_position: f64,
}

/// 根据清单与位置记录决定续播目标（FR-04 / T25）
///
/// 规则：
/// 1. 清单中第一个有已保存位置的文件，从该位置继续播放
/// 2. 若无保存位置，从清单第一个文件的开头播放
/// 3. 清单为空则返回 None
pub fn resolve_resume_target(
    playlist: &core_playlist::Playlist,
    positions: &core_position::PositionStore,
) -> Option<ResumeTarget> {
    if playlist.is_empty() {
        return None;
    }

    // 查找第一个有保存位置的条目
    for entry in &playlist.entries {
        if let Some(pos) = positions.get(&entry.path) {
            if pos > 0.0 {
                return Some(ResumeTarget {
                    file_path: entry.path.clone(),
                    start_position: pos,
                });
            }
        }
    }

    // 无保存位置，从第一个文件开头播放
    let first = &playlist.entries[0];
    Some(ResumeTarget {
        file_path: first.path.clone(),
        start_position: 0.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("core-player-test-{}-{}", std::process::id(), id));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 生成一个最小有效的 WAV 文件（16-bit PCM，单声道，8kHz，1 秒静音）
    fn write_minimal_wav(path: &std::path::Path) {
        let sample_rate: u32 = 8000;
        let channels: u16 = 1;
        let bits_per_sample: u16 = 16;
        let byte_rate = sample_rate * channels as u32 * bits_per_sample as u32 / 8;
        let block_align = channels * bits_per_sample / 8;
        let data_size: u32 = sample_rate * block_align as u32;
        let file_size: u32 = 36 + data_size;

        let mut buf = Vec::new();
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&file_size.to_le_bytes());
        buf.extend_from_slice(b"WAVE");
        buf.extend_from_slice(b"fmt ");
        buf.extend_from_slice(&16u32.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
        buf.extend_from_slice(&channels.to_le_bytes());
        buf.extend_from_slice(&sample_rate.to_le_bytes());
        buf.extend_from_slice(&byte_rate.to_le_bytes());
        buf.extend_from_slice(&block_align.to_le_bytes());
        buf.extend_from_slice(&bits_per_sample.to_le_bytes());
        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&data_size.to_le_bytes());
        buf.extend_from_slice(&vec![0u8; data_size as usize]);
        fs::write(path, buf).unwrap();
    }

    #[test]
    fn supported_extensions_include_common_formats() {
        for ext in ["mp3", "flac", "ogg", "oga", "m4a", "wav", "aac"] {
            assert!(is_playable_extension(ext), "{} should be playable", ext);
        }
    }

    #[test]
    fn unplayable_formats_are_flagged() {
        // 方案 A：白名单内但 Symphonia 不支持
        for ext in ["opus", "ape", "wv", "mka"] {
            assert!(
                !is_playable_extension(ext),
                "{} should NOT be playable",
                ext
            );
        }
    }

    #[test]
    fn non_audio_extension_not_playable() {
        assert!(!is_playable_extension("txt"));
        assert!(!is_playable_extension("jpg"));
    }

    #[test]
    fn extension_check_is_case_insensitive() {
        assert!(is_playable_extension("MP3"));
        assert!(is_playable_extension("Flac"));
    }

    #[test]
    fn probe_valid_wav_succeeds() {
        let dir = temp_dir();
        let path = dir.join("test.wav");
        write_minimal_wav(&path);

        let result = probe_file(&path);
        assert!(
            result.is_ok(),
            "valid wav should probe ok, got {:?}",
            result
        );
    }

    #[test]
    fn probe_nonexistent_file_returns_io_error() {
        let dir = temp_dir();
        let path = dir.join("missing.wav");

        let result = probe_file(&path);
        assert!(matches!(result, Err(PlayerError::Io(_))));
    }

    #[test]
    fn probe_garbage_data_returns_unsupported_or_probe_error() {
        let dir = temp_dir();
        let path = dir.join("garbage.wav");
        fs::write(&path, b"this is not a wav file").unwrap();

        let result = probe_file(&path);
        // 无效数据应返回 Unsupported 或 Probe 错误，不能是 Ok
        assert!(result.is_err());
    }

    #[test]
    fn probe_text_file_with_mp3_extension_fails() {
        let dir = temp_dir();
        let path = dir.join("fake.mp3");
        fs::write(&path, b"not really an mp3").unwrap();

        let result = probe_file(&path);
        assert!(result.is_err());
    }

    // ===== Player 状态机测试 =====

    #[test]
    fn player_starts_stopped() {
        let player = Player::new();
        assert_eq!(player.state(), PlayerState::Stopped);
        assert!(player.current_file().is_none());
    }

    #[test]
    fn play_sets_state_to_playing_and_position() {
        let mut player = Player::new();
        player.play("/music/song.mp3", 30.0, 200.0);

        assert!(player.is_playing());
        assert_eq!(player.position(), 30.0);
        assert_eq!(player.duration(), 200.0);
        assert_eq!(
            player.current_file(),
            Some(std::path::Path::new("/music/song.mp3"))
        );
    }

    #[test]
    fn play_start_position_clamped_to_zero() {
        let mut player = Player::new();
        player.play("/music/song.mp3", -10.0, 200.0);
        assert_eq!(player.position(), 0.0);
    }

    #[test]
    fn pause_transitions_from_playing_to_paused() {
        let mut player = Player::new();
        player.play("/music/song.mp3", 0.0, 200.0);
        player.pause();

        assert!(player.is_paused());
        assert_eq!(player.position(), 0.0);
    }

    #[test]
    fn pause_when_stopped_does_nothing() {
        let mut player = Player::new();
        player.pause();
        assert_eq!(player.state(), PlayerState::Stopped);
    }

    #[test]
    fn resume_from_paused_returns_to_playing() {
        let mut player = Player::new();
        player.play("/music/song.mp3", 0.0, 200.0);
        player.pause();
        player.resume();

        assert!(player.is_playing());
    }

    #[test]
    fn seek_updates_position() {
        let mut player = Player::new();
        player.play("/music/song.mp3", 0.0, 200.0);
        player.seek(50.0);

        assert_eq!(player.position(), 50.0);
    }

    #[test]
    fn seek_clamped_to_duration() {
        let mut player = Player::new();
        player.play("/music/song.mp3", 0.0, 200.0);
        player.seek(300.0);

        assert_eq!(player.position(), 200.0);
    }

    #[test]
    fn stop_clears_everything() {
        let mut player = Player::new();
        player.play("/music/song.mp3", 50.0, 200.0);
        player.stop();

        assert_eq!(player.state(), PlayerState::Stopped);
        assert!(player.current_file().is_none());
        assert_eq!(player.position(), 0.0);
    }

    #[test]
    fn advance_increases_position_when_playing() {
        let mut player = Player::new();
        player.play("/music/song.mp3", 0.0, 200.0);
        player.advance(10.5);

        assert_eq!(player.position(), 10.5);
        assert!(player.is_playing());
    }

    #[test]
    fn advance_does_nothing_when_paused() {
        let mut player = Player::new();
        player.play("/music/song.mp3", 0.0, 200.0);
        player.pause();
        player.advance(10.0);

        assert_eq!(player.position(), 0.0);
    }

    #[test]
    fn advance_stops_when_reaching_duration() {
        let mut player = Player::new();
        player.play("/music/song.mp3", 195.0, 200.0);
        player.advance(10.0);

        assert_eq!(player.position(), 200.0);
        assert_eq!(player.state(), PlayerState::Stopped);
    }

    // ===== 续播目标测试 =====

    fn make_playlist() -> core_playlist::Playlist {
        let files: Vec<(std::path::PathBuf, String)> = vec![
            (std::path::PathBuf::from("/music/a.mp3"), "a.mp3".into()),
            (std::path::PathBuf::from("/music/b.mp3"), "b.mp3".into()),
            (std::path::PathBuf::from("/music/c.mp3"), "c.mp3".into()),
        ];
        core_playlist::Playlist::from_files(
            std::path::PathBuf::from("/music"),
            files,
            core_playlist::SortMode::Auto,
        )
    }

    #[test]
    fn resume_from_first_file_when_no_saved_position() {
        let playlist = make_playlist();
        let positions = core_position::PositionStore::default();

        let target = resolve_resume_target(&playlist, &positions).unwrap();
        assert_eq!(target.file_path, std::path::PathBuf::from("/music/a.mp3"));
        assert_eq!(target.start_position, 0.0);
    }

    #[test]
    fn resume_from_saved_position_of_first_marked_file() {
        let playlist = make_playlist();
        let mut positions = core_position::PositionStore::default();
        positions.set(&std::path::PathBuf::from("/music/b.mp3"), 42.0);

        let target = resolve_resume_target(&playlist, &positions).unwrap();
        assert_eq!(target.file_path, std::path::PathBuf::from("/music/b.mp3"));
        assert_eq!(target.start_position, 42.0);
    }

    #[test]
    fn resume_picks_first_file_with_saved_position() {
        let playlist = make_playlist();
        let mut positions = core_position::PositionStore::default();
        positions.set(&std::path::PathBuf::from("/music/a.mp3"), 10.0);
        positions.set(&std::path::PathBuf::from("/music/b.mp3"), 20.0);

        let target = resolve_resume_target(&playlist, &positions).unwrap();
        // a.mp3 是第一个有保存位置的文件
        assert_eq!(target.file_path, std::path::PathBuf::from("/music/a.mp3"));
        assert_eq!(target.start_position, 10.0);
    }

    #[test]
    fn resume_zero_position_treated_as_no_position() {
        let playlist = make_playlist();
        let mut positions = core_position::PositionStore::default();
        positions.set(&std::path::PathBuf::from("/music/a.mp3"), 0.0);

        let target = resolve_resume_target(&playlist, &positions).unwrap();
        // a.mp3 的位置为 0，视为无续播位置，从开头播放
        assert_eq!(target.file_path, std::path::PathBuf::from("/music/a.mp3"));
        assert_eq!(target.start_position, 0.0);
    }

    #[test]
    fn resume_returns_none_for_empty_playlist() {
        let playlist = core_playlist::Playlist::from_files(
            std::path::PathBuf::from("/music"),
            Vec::<(std::path::PathBuf, String)>::new(),
            core_playlist::SortMode::Auto,
        );
        let positions = core_position::PositionStore::default();

        assert!(resolve_resume_target(&playlist, &positions).is_none());
    }
}
