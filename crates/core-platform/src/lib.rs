//! core-platform: 跨平台适配层
//!
//! T91: Windows WASAPI 输出抽象、路径处理、注册表自启动、托盘
//! T92: 防火墙例外引导与网络发现适配
//!
//! 在非 Windows 平台上提供空实现或模拟实现，保证跨平台编译。

pub mod audio_output;
pub mod autostart;
pub mod firewall;
pub mod paths;

/// 当前平台名称
pub fn platform_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "windows"
    }
    #[cfg(target_os = "linux")]
    {
        "linux"
    }
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        "unknown"
    }
}

/// 平台适配错误
#[derive(thiserror::Error, Debug)]
pub enum PlatformError {
    #[error("不支持的平台: {0}")]
    Unsupported(String),
    #[error("系统调用失败: {0}")]
    System(String),
    #[error("注册表操作失败: {0}")]
    Registry(String),
    #[error("I/O 错误: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_name_returns_known() {
        let name = platform_name();
        assert!(["windows", "linux", "macos", "unknown"].contains(&name));
    }
}
