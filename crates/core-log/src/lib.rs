//! core-log: 日志与诊断基线
//!
//! 提供结构化日志初始化、日志级别配置与隐私感知日志工具。
//! 遵循 PRD 7.6：日志默认不记录完整文件路径与音频内容，仅记录必要的标识与结果。

use std::fmt;
use std::path::Path;
use std::str::FromStr;

use thiserror::Error;
use tracing::Level;
use tracing_subscriber::{fmt as tracing_fmt, EnvFilter};

/// 日志级别
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogLevel {
    Off,
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl FromStr for LogLevel {
    type Err = LogConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "off" => Ok(LogLevel::Off),
            "error" => Ok(LogLevel::Error),
            "warn" => Ok(LogLevel::Warn),
            "info" => Ok(LogLevel::Info),
            "debug" => Ok(LogLevel::Debug),
            "trace" => Ok(LogLevel::Trace),
            other => Err(LogConfigError::InvalidLevel(other.to_string())),
        }
    }
}

impl From<LogLevel> for Level {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Off => Level::ERROR, // tracing 无 Off，用最低级别 + filter 实现
            LogLevel::Error => Level::ERROR,
            LogLevel::Warn => Level::WARN,
            LogLevel::Info => Level::INFO,
            LogLevel::Debug => Level::DEBUG,
            LogLevel::Trace => Level::TRACE,
        }
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            LogLevel::Off => "off",
            LogLevel::Error => "error",
            LogLevel::Warn => "warn",
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
            LogLevel::Trace => "trace",
        };
        write!(f, "{}", s)
    }
}

/// 日志输出格式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogFormat {
    /// 人类可读格式（带颜色，适合终端）
    #[default]
    Human,
    /// JSON 结构化格式（适合服务模式与日志收集）
    Json,
}

impl FromStr for LogFormat {
    type Err = LogConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "human" => Ok(LogFormat::Human),
            "json" => Ok(LogFormat::Json),
            other => Err(LogConfigError::InvalidFormat(other.to_string())),
        }
    }
}

/// 日志配置
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LogConfig {
    pub level: LogLevel,
    pub format: LogFormat,
}

/// 日志配置错误
#[derive(Error, Debug, PartialEq)]
pub enum LogConfigError {
    #[error("无效的日志级别: {0}")]
    InvalidLevel(String),
    #[error("无效的日志格式: {0}")]
    InvalidFormat(String),
    #[error("日志初始化失败: {0}")]
    InitFailed(String),
}

/// 初始化全局日志订阅器
///
/// 根据配置设置日志级别与输出格式。
/// 支持 `RUST_LOG` 环境变量覆盖级别。
pub fn init(config: &LogConfig) -> Result<(), LogConfigError> {
    let level_str = config.level.to_string();
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level_str));

    let builder = tracing_fmt().with_env_filter(filter);

    match config.format {
        LogFormat::Human => {
            builder
                .try_init()
                .map_err(|e| LogConfigError::InitFailed(e.to_string()))?;
        }
        LogFormat::Json => {
            builder
                .json()
                .try_init()
                .map_err(|e| LogConfigError::InitFailed(e.to_string()))?;
        }
    }

    Ok(())
}

/// 隐私感知路径脱敏
///
/// 遵循 PRD 7.6：日志不记录完整文件路径。
/// 返回路径的最后一个非空组件（通常为文件名），去除目录前缀。
/// 同时处理 Unix (`/`) 与 Windows (`\\`) 路径分隔符，跨平台一致。
pub fn redact_path<P: AsRef<Path>>(path: P) -> String {
    let s = path.as_ref().to_string_lossy();
    // 同时按 `/` 和 `\` 分割，取最后一个非空组件
    s.split(['/', '\\'])
        .rfind(|seg| !seg.is_empty())
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_level_from_str_parses_valid_levels() {
        assert_eq!(LogLevel::from_str("off"), Ok(LogLevel::Off));
        assert_eq!(LogLevel::from_str("error"), Ok(LogLevel::Error));
        assert_eq!(LogLevel::from_str("warn"), Ok(LogLevel::Warn));
        assert_eq!(LogLevel::from_str("info"), Ok(LogLevel::Info));
        assert_eq!(LogLevel::from_str("debug"), Ok(LogLevel::Debug));
        assert_eq!(LogLevel::from_str("trace"), Ok(LogLevel::Trace));
    }

    #[test]
    fn log_level_from_str_is_case_insensitive() {
        assert_eq!(LogLevel::from_str("INFO"), Ok(LogLevel::Info));
        assert_eq!(LogLevel::from_str("Warn"), Ok(LogLevel::Warn));
    }

    #[test]
    fn log_level_from_str_rejects_invalid() {
        assert!(LogLevel::from_str("verbose").is_err());
        assert!(LogLevel::from_str("").is_err());
    }

    #[test]
    fn log_level_default_is_info() {
        assert_eq!(LogLevel::default(), LogLevel::Info);
    }

    #[test]
    fn log_config_default_is_human_info() {
        let cfg = LogConfig::default();
        assert_eq!(cfg.level, LogLevel::Info);
        assert_eq!(cfg.format, LogFormat::Human);
    }

    #[test]
    fn redact_path_removes_sensitive_prefix() {
        // 完整路径应被脱敏为仅文件名
        assert_eq!(redact_path("/home/user/Music/song.mp3"), "song.mp3");
        assert_eq!(
            redact_path("C:\\Users\\me\\Music\\album.flac"),
            "album.flac"
        );
        assert_eq!(redact_path("relative/path/a.wav"), "a.wav");
    }

    #[test]
    fn redact_path_handles_filename_only() {
        assert_eq!(redact_path("song.mp3"), "song.mp3");
    }

    #[test]
    fn redact_path_handles_empty() {
        assert_eq!(redact_path(""), "");
    }

    #[test]
    fn redact_path_handles_trailing_slash() {
        // 尾随斜杠被忽略，返回最后一个非空组件
        assert_eq!(redact_path("/home/user/Music/"), "Music");
    }

    #[test]
    fn log_format_from_str() {
        assert_eq!(LogFormat::from_str("json"), Ok(LogFormat::Json));
        assert_eq!(LogFormat::from_str("human"), Ok(LogFormat::Human));
        assert!(LogFormat::from_str("xml").is_err());
    }
}
