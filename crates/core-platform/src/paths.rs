//! 路径处理（T91）
//!
//! Windows 路径规范化、AppData 目录定位、UNC 路径处理。
//! 在所有平台上提供统一接口，非 Windows 返回合理默认。

use std::path::{Path, PathBuf};

/// 规范化路径分隔符
///
/// Windows 上将正斜杠转为反斜杠，其他平台保持原样。
pub fn normalize_path(path: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        path.replace('/', "\\")
    }
    #[cfg(not(target_os = "windows"))]
    {
        path.to_string()
    }
}

/// 获取应用数据目录
///
/// Windows: `%APPDATA%\AudioSync`
/// Linux: `$XDG_DATA_HOME/audiosync` 或 `~/.local/share/audiosync`
/// macOS: `~/Library/Application Support/AudioSync`
pub fn app_data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return PathBuf::from(appdata).join("AudioSync");
        }
        PathBuf::from(r"C:\Users\Public\AudioSync")
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(xdg).join("audiosync");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("audiosync");
        }
        PathBuf::from("/tmp/audiosync")
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("AudioSync");
        }
        PathBuf::from("/tmp/AudioSync")
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        PathBuf::from("./audiosync")
    }
}

/// 获取配置目录
pub fn config_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        app_data_dir()
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(xdg).join("audiosync");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(".config").join("audiosync");
        }
        PathBuf::from("/tmp/audiosync-config")
    }

    #[cfg(target_os = "macos")]
    {
        app_data_dir()
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        PathBuf::from("./audiosync-config")
    }
}

/// 获取运行时目录（Socket 等临时文件）
pub fn runtime_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        app_data_dir().join("runtime")
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Some(xdg) = std::env::var_os("XDG_RUNTIME_DIR") {
            return PathBuf::from(xdg).join("audiosync");
        }
        std::env::temp_dir().join(format!("audiosync-{}", unsafe {
            // 不引入 libc，使用 UID 环境变量
            uid()
        }))
    }
}

#[cfg(not(target_os = "windows"))]
fn uid() -> u32 {
    std::env::var("UID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000)
}

/// 判断路径是否为 UNC 路径（Windows 网络路径）
pub fn is_unc_path(path: &Path) -> bool {
    let s = path.to_string_lossy();
    #[cfg(target_os = "windows")]
    {
        s.starts_with(r"\\") || s.starts_with("//")
    }
    #[cfg(not(target_os = "windows"))]
    {
        // 在非 Windows 上也可以检测 // 开头的路径
        s.starts_with("//")
    }
}

/// 将路径转换为平台原生格式
pub fn to_native_path(path: &str) -> PathBuf {
    let normalized = normalize_path(path);
    PathBuf::from(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_path_on_current_platform() {
        let p = normalize_path("/a/b/c");
        // 所有平台都应该返回非空字符串
        assert!(!p.is_empty());
    }

    #[test]
    fn app_data_dir_returns_non_empty() {
        let dir = app_data_dir();
        assert!(!dir.as_os_str().is_empty());
    }

    #[test]
    fn config_dir_returns_non_empty() {
        let dir = config_dir();
        assert!(!dir.as_os_str().is_empty());
    }

    #[test]
    fn runtime_dir_returns_non_empty() {
        let dir = runtime_dir();
        assert!(!dir.as_os_str().is_empty());
    }

    #[test]
    fn unc_path_detection() {
        assert!(is_unc_path(Path::new("//server/share")));
        assert!(is_unc_path(Path::new("\\\\server\\share")));
        assert!(!is_unc_path(Path::new("/home/user/music")));
    }

    #[test]
    fn to_native_path_returns_pathbuf() {
        let p = to_native_path("/music/test.mp3");
        assert!(p.to_string_lossy().contains("music"));
    }
}
