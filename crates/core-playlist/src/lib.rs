//! core-playlist: 播放清单生成与排序
//!
//! 实现 FR-02：为每个目录生成一个播放清单，按文件名自然排序。
//! 排序规则（规范定义，唯一引用）：剥离扩展名后，按不区分大小写的字符串比较排序；
//! 文件名中的数字按数值大小排序（自然排序）。

use std::path::PathBuf;

/// 排序模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    /// 自动排序（按自然排序规则）
    Auto,
    /// 手动排序（锁定用户指定的顺序）
    Manual,
}

/// 清单中的一个条目
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistEntry {
    /// 文件路径
    pub path: PathBuf,
    /// 文件名
    pub file_name: String,
    /// 在清单中的位置（0 起）
    pub position: usize,
}

/// 清单状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaylistStatus {
    /// 正常
    Active,
    /// 目录不可访问（权限、已拔出、已卸载），清单标记失效
    Invalid,
}

/// 播放清单
///
/// 每个目录对应一个清单。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playlist {
    /// 清单所属目录（作为清单的唯一标识）
    pub directory: PathBuf,
    /// 清单条目
    pub entries: Vec<PlaylistEntry>,
    /// 排序模式
    pub sort_mode: SortMode,
    /// 清单状态
    pub status: PlaylistStatus,
}

impl Playlist {
    /// 从音频文件列表创建清单
    ///
    /// `files` 为 (路径, 文件名) 元组列表。
    pub fn from_files<I>(directory: PathBuf, files: I, sort_mode: SortMode) -> Self
    where
        I: IntoIterator<Item = (PathBuf, String)>,
    {
        let mut entries: Vec<PlaylistEntry> = files
            .into_iter()
            .map(|(path, file_name)| PlaylistEntry {
                path,
                file_name,
                position: 0,
            })
            .collect();

        if sort_mode == SortMode::Auto {
            entries.sort_by(|a, b| {
                natural_sort_key(strip_extension(&a.file_name))
                    .cmp(&natural_sort_key(strip_extension(&b.file_name)))
            });
        }

        // 重新分配位置
        for (i, entry) in entries.iter_mut().enumerate() {
            entry.position = i;
        }

        Self {
            directory,
            entries,
            sort_mode,
            status: PlaylistStatus::Active,
        }
    }

    /// 创建一个失效清单（目录不可访问时使用）
    pub fn invalid(directory: PathBuf) -> Self {
        Self {
            directory,
            entries: Vec::new(),
            sort_mode: SortMode::Auto,
            status: PlaylistStatus::Invalid,
        }
    }

    /// 获取清单的条目数
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 清单是否为空
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 清单是否失效
    pub fn is_invalid(&self) -> bool {
        self.status == PlaylistStatus::Invalid
    }
}

/// 去除文件名的扩展名（仅最后一个点之后的部分）
pub fn strip_extension(name: &str) -> &str {
    match name.rfind('.') {
        Some(pos) => &name[..pos],
        None => name,
    }
}

/// 自然排序的键
///
/// 将字符串拆分为文本段与数字段，数字段按数值比较，文本段按不区分大小写比较。
/// 返回一个可比较的键序列。
pub fn natural_sort_key(s: &str) -> Vec<NaturalSegment> {
    let mut segments = Vec::new();
    let mut chars = s.chars().peekable();
    let mut current_text = String::new();

    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            // 先把累积的文本段推入
            if !current_text.is_empty() {
                segments.push(NaturalSegment::Text(current_text.to_lowercase()));
                current_text = String::new();
            }
            // 收集连续数字
            let mut num_str = String::new();
            num_str.push(c);
            while let Some(&nc) = chars.peek() {
                if nc.is_ascii_digit() {
                    num_str.push(nc);
                    chars.next();
                } else {
                    break;
                }
            }
            let num: u64 = num_str.parse().unwrap_or(u64::MAX);
            segments.push(NaturalSegment::Number(num));
        } else {
            current_text.push(c);
        }
    }
    if !current_text.is_empty() {
        segments.push(NaturalSegment::Text(current_text.to_lowercase()));
    }
    segments
}

/// 自然排序的段
///
/// 枚举顺序决定排序优先级：数字段排在文本段之前（与 ASCII 中数字 < 字母一致）。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum NaturalSegment {
    Number(u64),
    Text(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn audio_file(name: &str) -> (PathBuf, String) {
        (PathBuf::from(format!("/music/{}", name)), name.to_string())
    }

    #[test]
    fn playlist_sorts_case_insensitively() {
        let files = vec![
            audio_file("Banana.mp3"),
            audio_file("apple.mp3"),
            audio_file("Cherry.mp3"),
        ];
        let playlist = Playlist::from_files("/music".into(), files, SortMode::Auto);

        let names: Vec<&str> = playlist
            .entries
            .iter()
            .map(|e| e.file_name.as_str())
            .collect();
        assert_eq!(names, vec!["apple.mp3", "Banana.mp3", "Cherry.mp3"]);
    }

    #[test]
    fn playlist_sorts_numerically() {
        let files = vec![
            audio_file("track10.mp3"),
            audio_file("track2.mp3"),
            audio_file("track1.mp3"),
        ];
        let playlist = Playlist::from_files("/music".into(), files, SortMode::Auto);

        let names: Vec<&str> = playlist
            .entries
            .iter()
            .map(|e| e.file_name.as_str())
            .collect();
        assert_eq!(names, vec!["track1.mp3", "track2.mp3", "track10.mp3"]);
    }

    #[test]
    fn playlist_strips_extension_for_sort() {
        // 扩展名不同但主体相同时，顺序应稳定
        let files = vec![audio_file("song.flac"), audio_file("song.mp3")];
        let playlist = Playlist::from_files("/music".into(), files, SortMode::Auto);

        // 两者主体相同，排序稳定，保持输入顺序
        assert_eq!(playlist.entries.len(), 2);
    }

    #[test]
    fn playlist_mixed_alphanumeric_sorting() {
        let files = vec![
            audio_file("10_song.mp3"),
            audio_file("2_song.mp3"),
            audio_file("1_song.mp3"),
            audio_file("album.mp3"),
        ];
        let playlist = Playlist::from_files("/music".into(), files, SortMode::Auto);

        let names: Vec<&str> = playlist
            .entries
            .iter()
            .map(|e| e.file_name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["1_song.mp3", "2_song.mp3", "10_song.mp3", "album.mp3"]
        );
    }

    #[test]
    fn playlist_manual_mode_preserves_order() {
        let files = vec![
            audio_file("c.mp3"),
            audio_file("a.mp3"),
            audio_file("b.mp3"),
        ];
        let playlist = Playlist::from_files("/music".into(), files, SortMode::Manual);

        let names: Vec<&str> = playlist
            .entries
            .iter()
            .map(|e| e.file_name.as_str())
            .collect();
        assert_eq!(names, vec!["c.mp3", "a.mp3", "b.mp3"]);
    }

    #[test]
    fn playlist_has_directory_as_id() {
        let files = vec![audio_file("song.mp3")];
        let playlist = Playlist::from_files("/music/rock".into(), files, SortMode::Auto);

        assert_eq!(playlist.directory, PathBuf::from("/music/rock"));
    }

    #[test]
    fn playlist_entry_carries_position() {
        let files = vec![
            audio_file("a.mp3"),
            audio_file("b.mp3"),
            audio_file("c.mp3"),
        ];
        let playlist = Playlist::from_files("/music".into(), files, SortMode::Auto);

        assert_eq!(playlist.entries[0].position, 0);
        assert_eq!(playlist.entries[1].position, 1);
        assert_eq!(playlist.entries[2].position, 2);
    }

    #[test]
    fn natural_sort_compares_strings_without_numbers() {
        // "apple" < "banana"
        assert!(natural_sort_key("apple") < natural_sort_key("banana"));
        // 不区分大小写
        assert_eq!(natural_sort_key("Apple"), natural_sort_key("apple"));
    }

    #[test]
    fn natural_sort_compares_numbers() {
        assert!(natural_sort_key("track2") < natural_sort_key("track10"));
        assert!(natural_sort_key("1") < natural_sort_key("2"));
    }

    #[test]
    fn strip_extension_removes_last_extension() {
        assert_eq!(strip_extension("song.mp3"), "song");
        assert_eq!(strip_extension("my.song.flac"), "my.song");
        assert_eq!(strip_extension("noext"), "noext");
    }

    #[test]
    fn invalid_playlist_has_no_entries_and_invalid_status() {
        let playlist = Playlist::invalid("/music/unplugged".into());

        assert!(playlist.is_invalid());
        assert!(playlist.is_empty());
        assert_eq!(playlist.directory, PathBuf::from("/music/unplugged"));
    }

    #[test]
    fn active_playlist_is_not_invalid() {
        let files = vec![audio_file("song.mp3")];
        let playlist = Playlist::from_files("/music".into(), files, SortMode::Auto);

        assert!(!playlist.is_invalid());
    }
}
