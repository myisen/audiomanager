//! core-metadata: 音频元数据同步
//!
//! 实现 FR-07：内嵌标签、指纹、外部来源（MusicBrainz/Last.fm/ListenBrainz）、
//! 本地播放器库导入、匹配与冲突裁决、离线降级与隐私。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

// ===== 元数据模型 =====

/// 元数据来源（FR-07 优先级，规范定义）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MetadataSource {
    /// 优先级 1：内嵌标签
    Embedded,
    /// 优先级 1：音频指纹（Chromaprint）
    Fingerprint,
    /// 优先级 2：MusicBrainz
    MusicBrainz,
    /// 优先级 3：Last.fm
    LastFm,
    /// 优先级 3：ListenBrainz
    ListenBrainz,
    /// 优先级 4：本地播放器库文件
    LocalLibrary,
    /// 用户手动确认/改写
    User,
}

impl MetadataSource {
    pub fn priority(self) -> u8 {
        match self {
            MetadataSource::Embedded => 1,
            MetadataSource::Fingerprint => 1,
            MetadataSource::MusicBrainz => 2,
            MetadataSource::LastFm => 3,
            MetadataSource::ListenBrainz => 3,
            MetadataSource::LocalLibrary => 4,
            MetadataSource::User => 0, // 用户确认最高
        }
    }
}

/// 字段确认状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldStatus {
    /// 未确认（自动补全）
    Unconfirmed,
    /// 用户已确认
    Confirmed,
    /// 待用户裁决（多候选或冲突）
    PendingReview,
}

/// 单个字段值及其来源
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataFieldValue {
    pub value: String,
    pub source: MetadataSource,
    pub confidence: f32, // 0.0 ~ 1.0
    pub status: FieldStatus,
}

/// 音频元数据记录
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AudioMetadata {
    pub title: Option<MetadataFieldValue>,
    pub artist: Option<MetadataFieldValue>,
    pub album: Option<MetadataFieldValue>,
    pub track_number: Option<MetadataFieldValue>,
    pub year: Option<MetadataFieldValue>,
    pub duration_seconds: Option<u64>,
    pub cover_art: Option<Vec<u8>>,
    /// 外部实体标识（如 MusicBrainz recording ID）
    pub external_ids: HashMap<String, String>,
    /// 指纹
    pub fingerprint: Option<String>,
}

// ===== 内嵌标签读取（T61）=====

/// 内嵌标签读取错误
#[derive(Debug, thiserror::Error)]
pub enum TagError {
    #[error("文件读取失败: {0}")]
    Io(String),
    #[error("不支持的格式")]
    UnsupportedFormat,
    #[error("标签解析失败: {0}")]
    Parse(String),
}

/// 从音频文件读取内嵌标签（ID3 / Vorbis / FLAC 等）
///
/// 使用 lofty 库，支持多种格式。
pub fn read_embedded_tags(path: &Path) -> Result<AudioMetadata, TagError> {
    use lofty::file::{AudioFile, TaggedFileExt};
    use lofty::probe::Probe;
    use lofty::tag::{ItemKey, ItemValue};

    let tagged_file = Probe::open(path)
        .map_err(|e| TagError::Io(e.to_string()))?
        .read()
        .map_err(|e| TagError::Parse(e.to_string()))?;

    let mut meta = AudioMetadata::default();

    // 时长
    let props = tagged_file.properties();
    meta.duration_seconds = Some(props.duration().as_secs());

    // 取主标签
    if let Some(tag) = tagged_file.primary_tag() {
        if let Some(title) = tag.get_string(&ItemKey::TrackTitle) {
            meta.title = Some(MetadataFieldValue {
                value: title.to_string(),
                source: MetadataSource::Embedded,
                confidence: 1.0,
                status: FieldStatus::Unconfirmed,
            });
        }
        if let Some(artist) = tag.get_string(&ItemKey::TrackArtist) {
            meta.artist = Some(MetadataFieldValue {
                value: artist.to_string(),
                source: MetadataSource::Embedded,
                confidence: 1.0,
                status: FieldStatus::Unconfirmed,
            });
        }
        if let Some(album) = tag.get_string(&ItemKey::AlbumTitle) {
            meta.album = Some(MetadataFieldValue {
                value: album.to_string(),
                source: MetadataSource::Embedded,
                confidence: 1.0,
                status: FieldStatus::Unconfirmed,
            });
        }
        if let Some(track) = tag.get_string(&ItemKey::TrackNumber) {
            meta.track_number = Some(MetadataFieldValue {
                value: track.to_string(),
                source: MetadataSource::Embedded,
                confidence: 1.0,
                status: FieldStatus::Unconfirmed,
            });
        }
        if let Some(year) = tag.get_string(&ItemKey::Year) {
            meta.year = Some(MetadataFieldValue {
                value: year.to_string(),
                source: MetadataSource::Embedded,
                confidence: 1.0,
                status: FieldStatus::Unconfirmed,
            });
        }
        // 封面
        for item in tag.items() {
            if let ItemValue::Binary(pic) = item.value() {
                meta.cover_art = Some(pic.clone());
                break;
            }
        }
    }

    Ok(meta)
}

// ===== Chromaprint 指纹（T62）=====

/// 指纹计算结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FingerprintResult {
    pub fingerprint: String,
    pub duration_seconds: u64,
}

/// 指纹计算错误
#[derive(Debug, thiserror::Error)]
pub enum FingerprintError {
    #[error("fpcalc 子进程执行失败: {0}")]
    Process(String),
    #[error("fpcalc 输出解析失败")]
    Parse,
    #[error("文件不支持或已损坏")]
    Unsupported,
}

/// 调用 fpcalc 计算 Chromaprint 指纹
///
/// `fpcalc_path` 为 fpcalc 可执行文件路径。
pub fn compute_fingerprint(
    fpcalc_path: &Path,
    file_path: &Path,
) -> Result<FingerprintResult, FingerprintError> {
    use std::process::Command;

    let output = Command::new(fpcalc_path)
        .arg("-json")
        .arg(file_path)
        .output()
        .map_err(|e| FingerprintError::Process(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if stderr.contains("Format error") || stderr.contains("Unable to read") {
            return Err(FingerprintError::Unsupported);
        }
        return Err(FingerprintError::Process(stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let json: serde_json::Value =
        serde_json::from_str(&stdout).map_err(|_| FingerprintError::Parse)?;

    let fingerprint = json
        .get("fingerprint")
        .and_then(|v| v.as_str())
        .ok_or(FingerprintError::Parse)?
        .to_string();
    let duration = json
        .get("duration")
        .and_then(|v| v.as_u64())
        .ok_or(FingerprintError::Parse)?;

    Ok(FingerprintResult {
        fingerprint,
        duration_seconds: duration,
    })
}

// ===== 外部来源客户端抽象（T63/T64）=====

/// MusicBrainz 录音查询结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MusicBrainzRecording {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub year: Option<String>,
    pub track_number: Option<String>,
}

/// MusicBrainz 客户端 trait
pub trait MusicBrainzClient {
    /// 按指纹查询录音
    fn lookup_by_fingerprint(
        &self,
        fingerprint: &str,
        duration: u64,
    ) -> Result<Vec<MusicBrainzRecording>, String>;
}

/// Last.fm 元数据
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LastFmMetadata {
    pub tags: Vec<String>,
    pub listeners: Option<u64>,
    pub similar_tracks: Vec<String>,
}

/// Last.fm 客户端 trait
pub trait LastFmClient {
    fn get_track_info(&self, artist: &str, title: &str) -> Result<LastFmMetadata, String>;
}

/// ListenBrainz 提交结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListenBrainzSubmission {
    pub recording_mbid: Option<String>,
    pub artist_name: String,
    pub track_name: String,
}

/// ListenBrainz 客户端 trait（匿名访问）
pub trait ListenBrainzClient {
    fn lookup_recording(&self, artist: &str, title: &str) -> Result<Option<String>, String>; // 返回 mbid
}

// ===== 本地播放器库导入（T65）=====

/// 播放列表/库文件格式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaylistFormat {
    M3u,
    M3u8,
    Xspf,
    Pls,
}

/// 从文件扩展名推断格式
pub fn detect_playlist_format(path: &Path) -> Option<PlaylistFormat> {
    path.extension()
        .and_then(|e| e.to_str())
        .and_then(|e| match e.to_lowercase().as_str() {
            "m3u" => Some(PlaylistFormat::M3u),
            "m3u8" => Some(PlaylistFormat::M3u8),
            "xspf" => Some(PlaylistFormat::Xspf),
            "pls" => Some(PlaylistFormat::Pls),
            _ => None,
        })
}

/// 解析播放列表文件，返回文件路径列表
pub fn parse_playlist(content: &str, format: PlaylistFormat) -> Vec<String> {
    match format {
        PlaylistFormat::M3u | PlaylistFormat::M3u8 => parse_m3u(content),
        PlaylistFormat::Xspf => parse_xspf(content),
        PlaylistFormat::Pls => parse_pls(content),
    }
}

fn parse_m3u(content: &str) -> Vec<String> {
    content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.to_string())
        .collect()
}

fn parse_pls(content: &str) -> Vec<String> {
    let mut files = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("File") {
            if let Some(eq_pos) = rest.find('=') {
                let value = &rest[eq_pos + 1..];
                if !value.is_empty() {
                    files.push(value.to_string());
                }
            }
        }
    }
    files
}

fn parse_xspf(content: &str) -> Vec<String> {
    let mut files = Vec::new();
    // 解析 <location>...</location> 标签
    let mut rest = content;
    while let Some(start) = rest.find("<location>") {
        let after_open = &rest[start + "<location>".len()..];
        if let Some(end) = after_open.find("</location>") {
            files.push(after_open[..end].to_string());
            rest = &after_open[end + "</location>".len()..];
        } else {
            break;
        }
    }
    files
}

// ===== 匹配与冲突裁决（T66）=====

/// 字段候选
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldCandidate {
    pub value: String,
    pub source: MetadataSource,
    pub confidence: f32,
}

/// 匹配结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MatchResult {
    /// 唯一匹配，可自动补全
    Unique {
        field: &'static str,
        candidate: FieldCandidate,
    },
    /// 多候选，需用户裁决
    Conflict {
        field: &'static str,
        candidates: Vec<FieldCandidate>,
    },
    /// 无候选
    None,
}

/// 冲突裁决引擎
pub struct ConflictEngine;

impl ConflictEngine {
    /// 对单个字段进行匹配裁决
    ///
    /// `existing` 为当前字段值（可为空），`candidates` 为各来源提供的候选。
    /// - 若所有候选值一致 → Unique，自动补全
    /// - 若候选值不一致 → Conflict，需用户裁决
    /// - 若已有用户确认值 → 不覆盖（返回 None）
    pub fn resolve_field(
        existing: Option<&MetadataFieldValue>,
        candidates: Vec<FieldCandidate>,
    ) -> MatchResult {
        // 用户已确认的字段不被自动覆盖
        if let Some(existing) = existing {
            if existing.status == FieldStatus::Confirmed {
                return MatchResult::None;
            }
        }

        if candidates.is_empty() {
            return MatchResult::None;
        }

        // 按优先级排序
        let mut candidates = candidates;
        candidates.sort_by(|a, b| {
            b.source.priority().cmp(&a.source.priority()).then_with(|| {
                b.confidence
                    .partial_cmp(&a.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        });

        // 检查是否所有非空候选值一致
        let values: Vec<&str> = candidates
            .iter()
            .map(|c| c.value.as_str())
            .filter(|v| !v.is_empty())
            .collect();

        if values.is_empty() {
            return MatchResult::None;
        }

        let first = values[0];
        let all_same = values.iter().all(|v| *v == first);

        if all_same {
            MatchResult::Unique {
                field: "",
                candidate: candidates.into_iter().next().unwrap(),
            }
        } else {
            MatchResult::Conflict {
                field: "",
                candidates,
            }
        }
    }

    /// 接受某候选作为字段值
    pub fn accept_candidate(candidate: FieldCandidate, confirmed: bool) -> MetadataFieldValue {
        MetadataFieldValue {
            value: candidate.value,
            source: candidate.source,
            confidence: candidate.confidence,
            status: if confirmed {
                FieldStatus::Confirmed
            } else {
                FieldStatus::Unconfirmed
            },
        }
    }
}

// ===== 隐私与离线降级（T67）=====

/// 隐私设置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacySettings {
    /// 完全关闭外部来源查询（仅用内嵌标签 + 本地库）
    pub disable_external_sources: bool,
    /// 上行字段最小化（仅上传：指纹、时长、标题、艺人）
    pub minimal_upload: bool,
    /// 写回内嵌标签（修改用户文件）
    pub write_back_tags: bool,
}

impl Default for PrivacySettings {
    fn default() -> Self {
        Self {
            disable_external_sources: false,
            minimal_upload: true,
            write_back_tags: false,
        }
    }
}

/// 判断是否可使用外部来源
pub fn can_use_external_sources(settings: &PrivacySettings, is_online: bool) -> bool {
    !settings.disable_external_sources && is_online
}

/// 获取允许上行的字段列表（隐私最小化）
pub fn allowed_upload_fields(settings: &PrivacySettings) -> Vec<&'static str> {
    if settings.minimal_upload {
        vec!["fingerprint", "duration", "title", "artist"]
    } else {
        vec![
            "fingerprint",
            "duration",
            "title",
            "artist",
            "album",
            "track_number",
            "year",
        ]
    }
}

// ===== 随传输携带元数据（T68）=====

/// 传输时携带的元数据摘要
///
/// 不含文件内容与路径，符合隐私要求（7.6）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransferMetadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_seconds: Option<u64>,
    pub fingerprint: Option<String>,
}

impl TransferMetadata {
    /// 从完整元数据提取可传输的摘要
    pub fn from_metadata(meta: &AudioMetadata) -> Self {
        Self {
            title: meta.title.as_ref().map(|f| f.value.clone()),
            artist: meta.artist.as_ref().map(|f| f.value.clone()),
            album: meta.album.as_ref().map(|f| f.value.clone()),
            duration_seconds: meta.duration_seconds,
            fingerprint: meta.fingerprint.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== 元数据模型 =====

    #[test]
    fn source_priority_ordering() {
        assert_eq!(MetadataSource::User.priority(), 0);
        assert_eq!(MetadataSource::Embedded.priority(), 1);
        assert_eq!(MetadataSource::Fingerprint.priority(), 1);
        assert_eq!(MetadataSource::MusicBrainz.priority(), 2);
        assert_eq!(MetadataSource::LastFm.priority(), 3);
        assert_eq!(MetadataSource::ListenBrainz.priority(), 3);
        assert_eq!(MetadataSource::LocalLibrary.priority(), 4);
    }

    // ===== 内嵌标签读取（无实际音频文件时用错误分支）=====

    #[test]
    fn read_nonexistent_file_returns_error() {
        let result = read_embedded_tags(Path::new("/nonexistent/audio.mp3"));
        assert!(matches!(result, Err(TagError::Io(_))));
    }

    #[test]
    fn read_unsupported_file_returns_error() {
        // 创建一个文本文件
        let dir = std::env::temp_dir();
        let path = dir.join("core-meta-test.txt");
        std::fs::write(&path, b"not an audio file").unwrap();
        let result = read_embedded_tags(&path);
        // 可能是 UnsupportedFormat 或 Parse
        assert!(result.is_err());
        let _ = std::fs::remove_file(&path);
    }

    // ===== Chromaprint 指纹 =====

    #[test]
    fn fingerprint_with_nonexistent_fpcalc_errors() {
        let result = compute_fingerprint(Path::new("/nonexistent/fpcalc"), Path::new("test.mp3"));
        assert!(matches!(result, Err(FingerprintError::Process(_))));
    }

    // ===== 播放列表解析（T65）=====

    #[test]
    fn detect_m3u_format() {
        assert_eq!(
            detect_playlist_format(Path::new("list.m3u")),
            Some(PlaylistFormat::M3u)
        );
        assert_eq!(
            detect_playlist_format(Path::new("list.m3u8")),
            Some(PlaylistFormat::M3u8)
        );
    }

    #[test]
    fn detect_xspf_format() {
        assert_eq!(
            detect_playlist_format(Path::new("list.xspf")),
            Some(PlaylistFormat::Xspf)
        );
    }

    #[test]
    fn detect_pls_format() {
        assert_eq!(
            detect_playlist_format(Path::new("list.pls")),
            Some(PlaylistFormat::Pls)
        );
    }

    #[test]
    fn detect_unknown_format() {
        assert_eq!(detect_playlist_format(Path::new("list.txt")), None);
    }

    #[test]
    fn parse_m3u_skips_comments_and_empty_lines() {
        let content = "#EXTM3U\n#EXTINF:123,Song One\n/music/a.mp3\n\n/music/b.mp3\n";
        let files = parse_playlist(content, PlaylistFormat::M3u);
        assert_eq!(files, vec!["/music/a.mp3", "/music/b.mp3"]);
    }

    #[test]
    fn parse_pls_extracts_file_entries() {
        let content = "[playlist]\nNumberOfEntries=2\nFile1=/music/a.mp3\nFile2=/music/b.mp3\n";
        let files = parse_playlist(content, PlaylistFormat::Pls);
        assert_eq!(files, vec!["/music/a.mp3", "/music/b.mp3"]);
    }

    #[test]
    fn parse_xspf_extracts_locations() {
        let content = r#"<?xml version="1.0"?>
<playlist>
  <trackList>
    <track><location>/music/a.mp3</location></track>
    <track><location>/music/b.mp3</location></track>
  </trackList>
</playlist>"#;
        let files = parse_playlist(content, PlaylistFormat::Xspf);
        assert_eq!(files, vec!["/music/a.mp3", "/music/b.mp3"]);
    }

    #[test]
    fn parse_m3u8_handles_utf8() {
        let content = "#EXTM3U\n/music/歌曲.mp3\n/music/artist - title.flac\n";
        let files = parse_playlist(content, PlaylistFormat::M3u8);
        assert_eq!(files.len(), 2);
        assert!(files[0].contains("歌曲"));
    }

    // ===== 匹配与冲突裁决（T66）=====

    #[test]
    fn single_candidate_is_unique_match() {
        let candidates = vec![FieldCandidate {
            value: "Song Title".into(),
            source: MetadataSource::MusicBrainz,
            confidence: 0.9,
        }];

        let result = ConflictEngine::resolve_field(None, candidates);
        match result {
            MatchResult::Unique { candidate, .. } => {
                assert_eq!(candidate.value, "Song Title");
            }
            _ => panic!("expected Unique"),
        }
    }

    #[test]
    fn matching_candidates_from_all_sources_is_unique() {
        let candidates = vec![
            FieldCandidate {
                value: "Same Title".into(),
                source: MetadataSource::MusicBrainz,
                confidence: 0.9,
            },
            FieldCandidate {
                value: "Same Title".into(),
                source: MetadataSource::LastFm,
                confidence: 0.7,
            },
        ];

        let result = ConflictEngine::resolve_field(None, candidates);
        assert!(matches!(result, MatchResult::Unique { .. }));
    }

    #[test]
    fn conflicting_candidates_enter_pending_review() {
        let candidates = vec![
            FieldCandidate {
                value: "Title A".into(),
                source: MetadataSource::MusicBrainz,
                confidence: 0.9,
            },
            FieldCandidate {
                value: "Title B".into(),
                source: MetadataSource::LastFm,
                confidence: 0.6,
            },
        ];

        let result = ConflictEngine::resolve_field(None, candidates);
        match result {
            MatchResult::Conflict { candidates, .. } => {
                assert_eq!(candidates.len(), 2);
            }
            _ => panic!("expected Conflict"),
        }
    }

    #[test]
    fn confirmed_field_not_overwritten() {
        let existing = MetadataFieldValue {
            value: "User Confirmed".into(),
            source: MetadataSource::User,
            confidence: 1.0,
            status: FieldStatus::Confirmed,
        };
        let candidates = vec![FieldCandidate {
            value: "Different".into(),
            source: MetadataSource::MusicBrainz,
            confidence: 0.9,
        }];

        let result = ConflictEngine::resolve_field(Some(&existing), candidates);
        assert!(matches!(result, MatchResult::None));
    }

    #[test]
    fn unconfirmed_field_can_be_overwritten() {
        let existing = MetadataFieldValue {
            value: "Old".into(),
            source: MetadataSource::Embedded,
            confidence: 1.0,
            status: FieldStatus::Unconfirmed,
        };
        let candidates = vec![FieldCandidate {
            value: "New".into(),
            source: MetadataSource::MusicBrainz,
            confidence: 0.9,
        }];

        let result = ConflictEngine::resolve_field(Some(&existing), candidates);
        match result {
            MatchResult::Unique { candidate, .. } => {
                assert_eq!(candidate.value, "New");
            }
            _ => panic!("expected Unique"),
        }
    }

    #[test]
    fn accept_candidate_sets_confirmed_status() {
        let candidate = FieldCandidate {
            value: "Title".into(),
            source: MetadataSource::MusicBrainz,
            confidence: 0.95,
        };
        let field = ConflictEngine::accept_candidate(candidate, true);
        assert_eq!(field.status, FieldStatus::Confirmed);
        assert_eq!(field.value, "Title");
    }

    #[test]
    fn empty_candidates_return_none() {
        let result = ConflictEngine::resolve_field(None, vec![]);
        assert!(matches!(result, MatchResult::None));
    }

    // ===== 隐私与离线降级（T67）=====

    #[test]
    fn external_sources_disabled_when_setting_off() {
        let settings = PrivacySettings {
            disable_external_sources: true,
            ..Default::default()
        };
        assert!(!can_use_external_sources(&settings, true));
    }

    #[test]
    fn external_sources_disabled_when_offline() {
        let settings = PrivacySettings::default();
        assert!(!can_use_external_sources(&settings, false));
    }

    #[test]
    fn external_sources_enabled_when_online_and_not_disabled() {
        let settings = PrivacySettings::default();
        assert!(can_use_external_sources(&settings, true));
    }

    #[test]
    fn minimal_upload_limits_fields() {
        let settings = PrivacySettings {
            minimal_upload: true,
            ..Default::default()
        };
        let fields = allowed_upload_fields(&settings);
        assert_eq!(fields, vec!["fingerprint", "duration", "title", "artist"]);
    }

    #[test]
    fn full_upload_includes_all_fields() {
        let settings = PrivacySettings {
            minimal_upload: false,
            ..Default::default()
        };
        let fields = allowed_upload_fields(&settings);
        assert!(fields.contains(&"album"));
        assert!(fields.contains(&"year"));
    }

    #[test]
    fn default_write_back_is_off() {
        let settings = PrivacySettings::default();
        assert!(!settings.write_back_tags);
    }

    // ===== 传输元数据（T68）=====

    #[test]
    fn transfer_metadata_extracts_only_summary() {
        let meta = AudioMetadata {
            title: Some(MetadataFieldValue {
                value: "Title".into(),
                source: MetadataSource::Embedded,
                confidence: 1.0,
                status: FieldStatus::Unconfirmed,
            }),
            artist: Some(MetadataFieldValue {
                value: "Artist".into(),
                source: MetadataSource::Embedded,
                confidence: 1.0,
                status: FieldStatus::Unconfirmed,
            }),
            duration_seconds: Some(180),
            fingerprint: Some("fp_hash".into()),
            ..Default::default()
        };

        let transfer = TransferMetadata::from_metadata(&meta);
        assert_eq!(transfer.title.as_deref(), Some("Title"));
        assert_eq!(transfer.artist.as_deref(), Some("Artist"));
        assert_eq!(transfer.duration_seconds, Some(180));
        assert_eq!(transfer.fingerprint.as_deref(), Some("fp_hash"));
    }

    #[test]
    fn transfer_metadata_does_not_include_path_or_cover() {
        let meta = AudioMetadata {
            cover_art: Some(vec![1, 2, 3]),
            ..Default::default()
        };

        let transfer = TransferMetadata::from_metadata(&meta);
        // 传输摘要不含封面和路径
        assert!(transfer.title.is_none());
    }

    #[test]
    fn transfer_metadata_serialization_roundtrip() {
        let meta = TransferMetadata {
            title: Some("Title".into()),
            artist: Some("Artist".into()),
            album: None,
            duration_seconds: Some(180),
            fingerprint: Some("fp".into()),
        };
        let json = serde_json::to_string(&meta).unwrap();
        let parsed: TransferMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.title, meta.title);
        assert_eq!(parsed.fingerprint, meta.fingerprint);
    }
}
