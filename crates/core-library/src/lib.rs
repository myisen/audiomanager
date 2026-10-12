//! core-library: 音频库扫描与目录管理
//!
//! 实现 FR-01：按扩展名白名单扫描音频文件，支持递归开关，
//! 忽略隐藏文件与系统目录，以文件路径为唯一标识。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use core_playlist::{Playlist, SortMode};
use thiserror::Error;

/// 扫描结果：一个音频文件
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioFile {
    /// 文件完整路径（作为唯一标识，FR-01 §5）
    pub path: PathBuf,
    /// 文件名
    pub file_name: String,
    /// 扩展名（小写，不含点）
    pub extension: String,
}

/// 目录扫描错误
#[derive(Error, Debug)]
pub enum ScanError {
    #[error("I/O 错误: {0}")]
    Io(#[from] std::io::Error),
}

/// 音频文件扫描器
///
/// 按扩展名白名单识别音频文件，支持递归与隐藏文件过滤。
pub struct Scanner {
    extensions: HashSet<String>,
}

impl Scanner {
    /// 创建扫描器，传入允许的扩展名（不含点，大小写不敏感）
    pub fn new<I, S>(extensions: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let extensions = extensions
            .into_iter()
            .map(|s| s.into().to_lowercase())
            .collect();
        Self { extensions }
    }

    /// 扫描目录，返回匹配的音频文件列表
    ///
    /// - `recursive` 为 false 时仅扫描顶层目录
    /// - 隐藏文件与隐藏目录（以 `.` 开头）被跳过
    /// - 目录不存在时返回空列表
    pub fn scan<P: AsRef<Path>>(
        &self,
        root: P,
        recursive: bool,
    ) -> Result<Vec<AudioFile>, ScanError> {
        let root = root.as_ref();
        if !root.exists() {
            return Ok(Vec::new());
        }

        let mut results = Vec::new();
        self.scan_dir(root, recursive, &mut results)?;
        Ok(results)
    }

    fn scan_dir(
        &self,
        dir: &Path,
        recursive: bool,
        results: &mut Vec<AudioFile>,
    ) -> Result<(), ScanError> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();

            // 跳过隐藏文件/目录
            if name.starts_with('.') {
                continue;
            }

            let file_type = entry.file_type()?;

            if file_type.is_dir() {
                if recursive {
                    self.scan_dir(&path, recursive, results)?;
                }
            } else if file_type.is_file() {
                if let Some(audio_file) = self.try_parse_file(&path) {
                    results.push(audio_file);
                }
            }
            // 符号链接与其他类型跳过
        }
        Ok(())
    }

    fn try_parse_file(&self, path: &Path) -> Option<AudioFile> {
        let extension = path.extension()?.to_str()?.to_lowercase();
        if !self.extensions.contains(&extension) {
            return None;
        }
        let file_name = path.file_name()?.to_str()?.to_string();
        Some(AudioFile {
            path: path.to_path_buf(),
            file_name,
            extension,
        })
    }
}

/// 库根目录配置
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryRoot {
    pub path: PathBuf,
    pub recursive: bool,
}

/// 音频库管理器
///
/// 协调扫描器与清单生成，管理所有库根目录对应的清单。
/// 支持手动重扫描（T12 回退方案），目录不可访问时清单标记失效（T16）。
pub struct LibraryManager {
    scanner: Scanner,
    roots: Vec<LibraryRoot>,
}

impl LibraryManager {
    pub fn new(scanner: Scanner, roots: Vec<LibraryRoot>) -> Self {
        Self { scanner, roots }
    }

    /// 更新库根目录列表
    pub fn set_roots(&mut self, roots: Vec<LibraryRoot>) {
        self.roots = roots;
    }

    /// 执行全量扫描，为每个根目录生成一个清单
    ///
    /// 目录不可访问时生成失效清单，不影响其他目录。
    pub fn scan_all(&self) -> Result<Vec<Playlist>, ScanError> {
        let mut playlists = Vec::new();
        for root in &self.roots {
            let playlist = self.scan_root(root)?;
            playlists.push(playlist);
        }
        Ok(playlists)
    }

    /// 扫描单个根目录
    fn scan_root(&self, root: &LibraryRoot) -> Result<Playlist, ScanError> {
        if !root.path.exists() || !root.path.is_dir() {
            return Ok(Playlist::invalid(root.path.clone()));
        }

        let files = self.scanner.scan(&root.path, root.recursive)?;
        let file_tuples: Vec<(PathBuf, String)> =
            files.into_iter().map(|f| (f.path, f.file_name)).collect();

        Ok(Playlist::from_files(
            root.path.clone(),
            file_tuples,
            SortMode::Auto,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// PRD FR-01 §1 扩展名白名单
    const DEFAULT_EXTENSIONS: &[&str] = &[
        "mp3", "flac", "ogg", "oga", "opus", "m4a", "aac", "wav", "ape", "wv", "mka",
    ];

    fn temp_dir() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("core-library-test-{}-{}", std::process::id(), id));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn create_file(dir: &std::path::Path, name: &str) {
        fs::write(dir.join(name), b"fake audio data").unwrap();
    }

    #[test]
    fn scanner_finds_audio_files_with_whitelist_extensions() {
        let dir = temp_dir();
        create_file(&dir, "song1.mp3");
        create_file(&dir, "song2.flac");
        create_file(&dir, "notes.txt"); // 非音频
        create_file(&dir, "image.jpg"); // 非音频

        let scanner = Scanner::new(DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()));
        let files = scanner.scan(&dir, false).unwrap();

        let names: Vec<&str> = files.iter().map(|f| f.file_name.as_str()).collect();
        assert!(names.contains(&"song1.mp3"));
        assert!(names.contains(&"song2.flac"));
        assert!(!names.contains(&"notes.txt"));
        assert!(!names.contains(&"image.jpg"));
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn scanner_ignores_hidden_files() {
        let dir = temp_dir();
        create_file(&dir, "visible.mp3");
        create_file(&dir, ".hidden.mp3"); // 隐藏文件

        let scanner = Scanner::new(DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()));
        let files = scanner.scan(&dir, false).unwrap();

        let names: Vec<&str> = files.iter().map(|f| f.file_name.as_str()).collect();
        assert!(names.contains(&"visible.mp3"));
        assert!(!names.contains(&".hidden.mp3"));
    }

    #[test]
    fn scanner_ignores_hidden_directories() {
        let dir = temp_dir();
        create_file(&dir, "root.mp3");
        let hidden_dir = dir.join(".secret");
        fs::create_dir_all(&hidden_dir).unwrap();
        create_file(&hidden_dir, "inside.mp3");

        let scanner = Scanner::new(DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()));
        let files = scanner.scan(&dir, true).unwrap();

        let names: Vec<&str> = files.iter().map(|f| f.file_name.as_str()).collect();
        assert!(names.contains(&"root.mp3"));
        assert!(!names.contains(&"inside.mp3"));
    }

    #[test]
    fn scanner_no_recursion_by_default() {
        let dir = temp_dir();
        create_file(&dir, "root.mp3");
        let sub = dir.join("subdir");
        fs::create_dir_all(&sub).unwrap();
        create_file(&sub, "nested.mp3");

        let scanner = Scanner::new(DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()));
        let files = scanner.scan(&dir, false).unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_name, "root.mp3");
    }

    #[test]
    fn scanner_recursion_when_enabled() {
        let dir = temp_dir();
        create_file(&dir, "root.mp3");
        let sub = dir.join("subdir");
        fs::create_dir_all(&sub).unwrap();
        create_file(&sub, "nested.flac");

        let scanner = Scanner::new(DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()));
        let files = scanner.scan(&dir, true).unwrap();

        assert_eq!(files.len(), 2);
    }

    #[test]
    fn scanner_uses_file_path_as_identifier() {
        let dir = temp_dir();
        create_file(&dir, "song.mp3");

        let scanner = Scanner::new(DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()));
        let files = scanner.scan(&dir, false).unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, dir.join("song.mp3"));
        assert_eq!(files[0].extension, "mp3");
    }

    #[test]
    fn scanner_handles_case_insensitive_extensions() {
        let dir = temp_dir();
        create_file(&dir, "UPPER.MP3");
        create_file(&dir, "Mixed.Flac");

        let scanner = Scanner::new(DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()));
        let files = scanner.scan(&dir, false).unwrap();

        assert_eq!(files.len(), 2);
    }

    #[test]
    fn scanner_skips_non_file_entries() {
        let dir = temp_dir();
        create_file(&dir, "song.mp3");
        // 创建一个目录，确保不会被当作文件
        let sub = dir.join("not-a-file");
        fs::create_dir_all(&sub).unwrap();

        let scanner = Scanner::new(DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()));
        let files = scanner.scan(&dir, false).unwrap();

        assert_eq!(files.len(), 1);
    }

    #[test]
    fn scanner_returns_empty_for_nonexistent_directory() {
        let dir = temp_dir().join("does-not-exist");

        let scanner = Scanner::new(DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()));
        let files = scanner.scan(&dir, false).unwrap();

        assert!(files.is_empty());
    }

    #[test]
    fn library_manager_generates_playlist_per_root() {
        let dir1 = temp_dir();
        let dir2 = temp_dir();
        create_file(&dir1, "song1.mp3");
        create_file(&dir2, "song2.flac");

        let scanner = Scanner::new(DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()));
        let mgr = LibraryManager::new(
            scanner,
            vec![
                LibraryRoot {
                    path: dir1.clone(),
                    recursive: false,
                },
                LibraryRoot {
                    path: dir2.clone(),
                    recursive: false,
                },
            ],
        );

        let playlists = mgr.scan_all().unwrap();
        assert_eq!(playlists.len(), 2);
    }

    #[test]
    fn library_manager_marks_invalid_for_missing_directory() {
        let dir = temp_dir();
        let missing = temp_dir().join("does-not-exist");

        let scanner = Scanner::new(DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()));
        let mgr = LibraryManager::new(
            scanner,
            vec![
                LibraryRoot {
                    path: dir.clone(),
                    recursive: false,
                },
                LibraryRoot {
                    path: missing.clone(),
                    recursive: false,
                },
            ],
        );

        let playlists = mgr.scan_all().unwrap();
        assert_eq!(playlists.len(), 2);
        assert!(!playlists[0].is_invalid());
        assert!(playlists[1].is_invalid());
    }

    #[test]
    fn library_manager_invalid_playlist_does_not_affect_others() {
        let valid_dir = temp_dir();
        create_file(&valid_dir, "song.mp3");
        let missing = temp_dir().join("gone");

        let scanner = Scanner::new(DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()));
        let mgr = LibraryManager::new(
            scanner,
            vec![
                LibraryRoot {
                    path: missing,
                    recursive: false,
                },
                LibraryRoot {
                    path: valid_dir.clone(),
                    recursive: false,
                },
            ],
        );

        let playlists = mgr.scan_all().unwrap();
        // 第一个失效，第二个正常有内容
        assert!(playlists[0].is_invalid());
        assert!(!playlists[1].is_invalid());
        assert_eq!(playlists[1].len(), 1);
    }
}
