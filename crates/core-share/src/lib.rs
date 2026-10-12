//! core-share: 播放列表与文件分享
//!
//! 实现 FR-11：两级分享（清单级/文件级）、对端浏览、下载、分享变更联动。
//!
//! 分享规则（规范定义，唯一引用）：
//! - 清单级开启 → 该清单所有文件对端可见
//! - 清单级关闭但文件级单独开启 → 仅该文件对端可见
//! - 未开启分享的内容对端完全不可见

use core_playlist::Playlist;
use core_transfer::{TransferDirection, TransferState, TransferTask, TransferTaskId};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// 分享配置（持久化）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShareConfig {
    /// 已开启清单级分享的目录集合
    shared_playlists: HashSet<PathBuf>,
    /// 已开启文件级分享的文件集合
    shared_files: HashSet<PathBuf>,
}

impl ShareConfig {
    pub fn new() -> Self {
        Self::default()
    }

    /// 开启清单级分享
    pub fn share_playlist(&mut self, directory: &Path) {
        self.shared_playlists.insert(directory.to_path_buf());
    }

    /// 关闭清单级分享
    pub fn unshare_playlist(&mut self, directory: &Path) {
        self.shared_playlists.remove(directory);
    }

    /// 清单是否已分享
    pub fn is_playlist_shared(&self, directory: &Path) -> bool {
        self.shared_playlists.contains(directory)
    }

    /// 开启文件级分享
    pub fn share_file(&mut self, file_path: impl AsRef<Path>) {
        self.shared_files.insert(file_path.as_ref().to_path_buf());
    }

    /// 关闭文件级分享
    pub fn unshare_file(&mut self, file_path: impl AsRef<Path>) {
        self.shared_files.remove(file_path.as_ref());
    }

    /// 文件是否对端可见（清单分享 OR 文件单独分享）
    ///
    /// `playlist_dir` 为文件所属清单目录，用于判断清单级分享。
    pub fn is_file_shared(&self, file_path: &Path, playlist_dir: &Path) -> bool {
        self.is_playlist_shared(playlist_dir) || self.shared_files.contains(file_path)
    }

    /// 获取所有已分享的清单目录
    pub fn shared_playlists(&self) -> impl Iterator<Item = &PathBuf> {
        self.shared_playlists.iter()
    }

    /// 获取所有单独分享的文件
    pub fn shared_files(&self) -> impl Iterator<Item = &PathBuf> {
        self.shared_files.iter()
    }

    /// 移除失效目录的分享记录（T55：目录失效联动）
    ///
    /// 当清单目录变为 Invalid 时，从分享列表中移除该目录。
    /// 注意：清单级分享被移除后，清单内文件是否仍可见取决于是否有文件级分享。
    pub fn on_playlist_invalid(&mut self, directory: &Path) {
        self.shared_playlists.remove(directory);
    }

    /// 文件从清单中移除时的联动（T55）
    ///
    /// 移除文件级分享记录。清单级分享不受影响。
    pub fn on_file_removed(&mut self, file_path: impl AsRef<Path>) {
        self.shared_files.remove(file_path.as_ref());
    }

    /// 判断新加入共享清单的文件是否自动可见（T55）
    ///
    /// 新文件加入已分享的清单 → 自动可见，无需手动开启文件级分享。
    pub fn is_new_file_visible(&self, playlist_dir: &Path) -> bool {
        self.is_playlist_shared(playlist_dir)
    }
}

/// 对端可浏览的内容摘要（T52）
///
/// 只包含已分享内容的元数据摘要，不暴露未分享内容。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareBrowseSummary {
    /// 已分享的清单列表
    pub playlists: Vec<SharedPlaylistInfo>,
    /// 单独分享的文件列表（不在已分享清单内的）
    pub files: Vec<SharedFileInfo>,
}

/// 已分享清单的摘要信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedPlaylistInfo {
    /// 清单目录（作为标识）
    pub directory: PathBuf,
    /// 清单名称（目录名）
    pub name: String,
    /// 文件数量
    pub file_count: usize,
    /// 总大小（字节）
    pub total_size: u64,
}

/// 单独分享文件的摘要信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedFileInfo {
    pub path: PathBuf,
    pub file_name: String,
    pub size: u64,
}

/// 生成对端浏览摘要（T52）
///
/// 遍历所有清单，只返回已分享内容。
pub fn build_browse_summary(
    config: &ShareConfig,
    playlists: &[Playlist],
    file_sizes: &impl Fn(&Path) -> Option<u64>,
) -> ShareBrowseSummary {
    let mut playlists_summary = Vec::new();
    let mut standalone_files: Vec<SharedFileInfo> = Vec::new();

    // 收集所有已分享清单的文件路径，用于区分单独分享文件
    let mut files_in_shared_playlists: HashSet<PathBuf> = HashSet::new();

    for playlist in playlists {
        if config.is_playlist_shared(&playlist.directory) {
            let file_count = playlist.entries.len();
            let total_size: u64 = playlist
                .entries
                .iter()
                .map(|e| file_sizes(&e.path).unwrap_or(0))
                .sum();

            playlists_summary.push(SharedPlaylistInfo {
                directory: playlist.directory.clone(),
                name: playlist
                    .directory
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                file_count,
                total_size,
            });

            for entry in &playlist.entries {
                files_in_shared_playlists.insert(entry.path.clone());
            }
        }
    }

    // 单独分享的文件（不在已分享清单内）
    for file_path in config.shared_files() {
        if !files_in_shared_playlists.contains(file_path) {
            let file_name = file_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let size = file_sizes(file_path).unwrap_or(0);
            standalone_files.push(SharedFileInfo {
                path: file_path.clone(),
                file_name,
                size,
            });
        }
    }

    ShareBrowseSummary {
        playlists: playlists_summary,
        files: standalone_files,
    }
}

// ===== 下载（T53）=====

/// 创建下载整个清单的传输任务列表
///
/// 为清单中每个已分享文件创建一个 Pull 方向的传输任务。
pub fn create_playlist_download_tasks(
    playlist: &Playlist,
    config: &ShareConfig,
) -> Vec<TransferTask> {
    playlist
        .entries
        .iter()
        .filter(|e| config.is_file_shared(&e.path, &playlist.directory))
        .map(|e| TransferTask {
            id: TransferTaskId::new(),
            file_name: e.file_name.clone(),
            file_size: 0, // 由对端提供
            transferred_bytes: 0,
            state: TransferState::Pending,
            direction: TransferDirection::Pull,
            error: None,
        })
        .collect()
}

/// 创建下载单个文件的传输任务
///
/// 仅当文件已分享时才创建任务。
pub fn create_file_download_task(
    file_path: &Path,
    playlist_dir: &Path,
    config: &ShareConfig,
) -> Option<TransferTask> {
    if !config.is_file_shared(file_path, playlist_dir) {
        return None;
    }
    let file_name = file_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    Some(TransferTask {
        id: TransferTaskId::new(),
        file_name,
        file_size: 0,
        transferred_bytes: 0,
        state: TransferState::Pending,
        direction: TransferDirection::Pull,
        error: None,
    })
}

// ===== 分享变更联动（T54）=====

/// 分享事件
#[derive(Debug, Clone)]
pub enum ShareEvent {
    /// 清单分享被关闭
    PlaylistUnshared(PathBuf),
    /// 文件分享被关闭
    FileUnshared(PathBuf),
    /// 源端离线
    SourceOffline,
    /// 接收端关闭
    ReceiverClosed,
    /// 源端删除文件
    SourceFileDeleted(PathBuf),
}

/// 检查进行中的传输任务是否因分享事件而应失败（T54）
///
/// 返回应标记为失败的任务 ID 列表。
pub fn check_share_impact(
    event: &ShareEvent,
    active_tasks: &[TransferTask],
    config: &ShareConfig,
    playlist_dir: Option<&Path>,
) -> Vec<TransferTaskId> {
    let mut failed = Vec::new();

    for task in active_tasks {
        if task.state != TransferState::Transferring {
            continue;
        }

        let should_fail = match event {
            ShareEvent::PlaylistUnshared(dir) => {
                // 任务属于该清单，且文件无单独分享
                if let Some(pdir) = playlist_dir {
                    pdir == dir
                        && !config
                            .shared_files()
                            .any(|p| p.file_name() == Path::new(&task.file_name).file_name())
                } else {
                    false
                }
            }
            ShareEvent::FileUnshared(path) => {
                // 任务对应该文件
                Path::new(&task.file_name).file_name() == path.file_name()
                    && !playlist_dir
                        .map(|d| config.is_playlist_shared(d))
                        .unwrap_or(false)
            }
            ShareEvent::SourceOffline => true,
            ShareEvent::ReceiverClosed => true,
            ShareEvent::SourceFileDeleted(path) => {
                Path::new(&task.file_name).file_name() == path.file_name()
            }
        };

        if should_fail {
            failed.push(task.id);
        }
    }

    failed
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_playlist::{Playlist, SortMode};

    fn make_playlist(dir: &str, files: &[&str]) -> Playlist {
        let entries: Vec<(PathBuf, String)> = files
            .iter()
            .map(|f| (PathBuf::from(format!("{}/{}", dir, f)), f.to_string()))
            .collect();
        Playlist::from_files(PathBuf::from(dir), entries, SortMode::Auto)
    }

    fn file_sizes(_: &Path) -> Option<u64> {
        Some(1024)
    }

    // ===== T51 分享模型 =====

    #[test]
    fn playlist_share_toggle() {
        let mut config = ShareConfig::new();
        let dir = PathBuf::from("/music/rock");

        assert!(!config.is_playlist_shared(&dir));
        config.share_playlist(&dir);
        assert!(config.is_playlist_shared(&dir));
        config.unshare_playlist(&dir);
        assert!(!config.is_playlist_shared(&dir));
    }

    #[test]
    fn file_share_toggle() {
        let mut config = ShareConfig::new();
        let file = PathBuf::from("/music/rock/song.mp3");

        assert!(!config.is_file_shared(&file, Path::new("/music/rock")));
        config.share_file(&file);
        assert!(config.is_file_shared(&file, Path::new("/music/rock")));
        config.unshare_file(&file);
        assert!(!config.is_file_shared(&file, Path::new("/music/rock")));
    }

    #[test]
    fn playlist_shared_makes_all_files_visible() {
        let mut config = ShareConfig::new();
        let dir = PathBuf::from("/music/rock");
        config.share_playlist(&dir);

        // 清单内文件无需单独分享即可见
        assert!(config.is_file_shared(Path::new("/music/rock/a.mp3"), &dir));
        assert!(config.is_file_shared(Path::new("/music/rock/b.mp3"), &dir));
    }

    #[test]
    fn playlist_not_shared_only_individual_files_visible() {
        let mut config = ShareConfig::new();
        let dir = PathBuf::from("/music/rock");
        config.share_file(PathBuf::from("/music/rock/a.mp3"));

        assert!(config.is_file_shared(Path::new("/music/rock/a.mp3"), &dir));
        assert!(!config.is_file_shared(Path::new("/music/rock/b.mp3"), &dir));
    }

    // ===== T52 浏览接口 =====

    #[test]
    fn browse_summary_only_includes_shared() {
        let mut config = ShareConfig::new();
        config.share_playlist(Path::new("/music/rock"));

        let playlists = vec![
            make_playlist("/music/rock", &["a.mp3", "b.mp3"]),
            make_playlist("/music/jazz", &["c.mp3"]), // 未分享
        ];

        let summary = build_browse_summary(&config, &playlists, &file_sizes);

        assert_eq!(summary.playlists.len(), 1);
        assert_eq!(summary.playlists[0].name, "rock");
        assert_eq!(summary.playlists[0].file_count, 2);
    }

    #[test]
    fn browse_summary_includes_standalone_files() {
        let mut config = ShareConfig::new();
        config.share_file(PathBuf::from("/music/jazz/c.mp3"));

        let playlists = vec![make_playlist("/music/jazz", &["c.mp3", "d.mp3"])];

        let summary = build_browse_summary(&config, &playlists, &file_sizes);

        // 清单未分享，c.mp3 单独分享
        assert_eq!(summary.playlists.len(), 0);
        assert_eq!(summary.files.len(), 1);
        assert_eq!(summary.files[0].file_name, "c.mp3");
    }

    #[test]
    fn browse_summary_excludes_files_in_shared_playlists_from_standalone() {
        let mut config = ShareConfig::new();
        config.share_playlist(Path::new("/music/rock"));
        config.share_file(PathBuf::from("/music/rock/a.mp3")); // 已在分享清单内

        let playlists = vec![make_playlist("/music/rock", &["a.mp3", "b.mp3"])];

        let summary = build_browse_summary(&config, &playlists, &file_sizes);

        assert_eq!(summary.playlists.len(), 1);
        assert_eq!(summary.files.len(), 0); // a.mp3 不在单独文件列表中
    }

    // ===== T53 下载 =====

    #[test]
    fn download_playlist_creates_tasks_for_shared_files() {
        let mut config = ShareConfig::new();
        config.share_playlist(Path::new("/music/rock"));

        let playlist = make_playlist("/music/rock", &["a.mp3", "b.mp3"]);
        let tasks = create_playlist_download_tasks(&playlist, &config);

        assert_eq!(tasks.len(), 2);
        for t in &tasks {
            assert_eq!(t.direction, TransferDirection::Pull);
            assert_eq!(t.state, TransferState::Pending);
        }
    }

    #[test]
    fn download_playlist_skips_unshared_files() {
        let mut config = ShareConfig::new();
        // 清单未分享，只分享 a.mp3
        config.share_file(PathBuf::from("/music/rock/a.mp3"));

        let playlist = make_playlist("/music/rock", &["a.mp3", "b.mp3"]);
        let tasks = create_playlist_download_tasks(&playlist, &config);

        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].file_name, "a.mp3");
    }

    #[test]
    fn download_single_file_requires_share() {
        let config = ShareConfig::new();
        // 未分享 → None
        assert!(create_file_download_task(
            Path::new("/music/rock/a.mp3"),
            Path::new("/music/rock"),
            &config
        )
        .is_none());
    }

    #[test]
    fn download_single_shared_file_creates_task() {
        let mut config = ShareConfig::new();
        config.share_file(PathBuf::from("/music/rock/a.mp3"));

        let task = create_file_download_task(
            Path::new("/music/rock/a.mp3"),
            Path::new("/music/rock"),
            &config,
        )
        .unwrap();

        assert_eq!(task.file_name, "a.mp3");
        assert_eq!(task.direction, TransferDirection::Pull);
    }

    // ===== T54 分享变更联动 =====

    fn make_transfer_task(file_name: &str) -> TransferTask {
        TransferTask {
            id: TransferTaskId::new(),
            file_name: file_name.into(),
            file_size: 1000,
            transferred_bytes: 500,
            state: TransferState::Transferring,
            direction: TransferDirection::Pull,
            error: None,
        }
    }

    #[test]
    fn unshare_playlist_fails_its_tasks() {
        let config = ShareConfig::new();
        let tasks = vec![make_transfer_task("a.mp3")];

        let failed = check_share_impact(
            &ShareEvent::PlaylistUnshared(PathBuf::from("/music/rock")),
            &tasks,
            &config,
            Some(Path::new("/music/rock")),
        );

        assert_eq!(failed.len(), 1);
    }

    #[test]
    fn unshare_file_fails_its_task() {
        let config = ShareConfig::new();
        let tasks = vec![make_transfer_task("a.mp3"), make_transfer_task("b.mp3")];

        let failed = check_share_impact(
            &ShareEvent::FileUnshared(PathBuf::from("/music/rock/a.mp3")),
            &tasks,
            &config,
            None,
        );

        assert_eq!(failed.len(), 1);
    }

    #[test]
    fn source_offline_fails_all_active_tasks() {
        let config = ShareConfig::new();
        let tasks = vec![make_transfer_task("a.mp3"), make_transfer_task("b.mp3")];

        let failed = check_share_impact(&ShareEvent::SourceOffline, &tasks, &config, None);

        assert_eq!(failed.len(), 2);
    }

    #[test]
    fn pending_tasks_not_affected_by_share_change() {
        let config = ShareConfig::new();
        let mut pending = make_transfer_task("a.mp3");
        pending.state = TransferState::Pending;
        let tasks = vec![pending];

        let failed = check_share_impact(&ShareEvent::SourceOffline, &tasks, &config, None);

        assert_eq!(failed.len(), 0); // 非传输中不处理
    }

    // ===== T55 边界处理 =====

    #[test]
    fn playlist_invalid_removes_its_share() {
        let mut config = ShareConfig::new();
        config.share_playlist(Path::new("/music/rock"));
        assert!(config.is_playlist_shared(Path::new("/music/rock")));

        config.on_playlist_invalid(Path::new("/music/rock"));
        assert!(!config.is_playlist_shared(Path::new("/music/rock")));
    }

    #[test]
    fn file_removed_clears_file_share() {
        let mut config = ShareConfig::new();
        config.share_file(PathBuf::from("/music/rock/a.mp3"));
        assert!(config.is_file_shared(Path::new("/music/rock/a.mp3"), Path::new("/music/rock")));

        config.on_file_removed(Path::new("/music/rock/a.mp3"));
        assert!(!config.is_file_shared(Path::new("/music/rock/a.mp3"), Path::new("/music/rock")));
    }

    #[test]
    fn new_file_in_shared_playlist_is_visible() {
        let mut config = ShareConfig::new();
        config.share_playlist(Path::new("/music/rock"));

        // 新文件自动可见
        assert!(config.is_new_file_visible(Path::new("/music/rock")));
    }

    #[test]
    fn new_file_in_unshared_playlist_not_visible() {
        let config = ShareConfig::new();
        assert!(!config.is_new_file_visible(Path::new("/music/rock")));
    }

    #[test]
    fn share_config_serialization_roundtrip() {
        let mut config = ShareConfig::new();
        config.share_playlist(Path::new("/music/rock"));
        config.share_file(PathBuf::from("/music/jazz/c.mp3"));

        let json = serde_json::to_string(&config).unwrap();
        let parsed: ShareConfig = serde_json::from_str(&json).unwrap();

        assert!(parsed.is_playlist_shared(Path::new("/music/rock")));
        assert!(parsed.is_file_shared(Path::new("/music/jazz/c.mp3"), Path::new("/music/jazz")));
    }
}
