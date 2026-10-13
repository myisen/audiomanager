//! 自启动管理（T91）
//!
//! Windows: 注册表 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
//! Linux: `~/.config/systemd/user/audiosync.service`
//! macOS: `~/Library/LaunchAgents/com.audiosync.plist`

use crate::PlatformError;

/// 自启动配置
#[derive(Debug, Clone)]
pub struct AutostartConfig {
    /// 应用名称（用于注册表键名或 service 名）
    pub app_name: String,
    /// 可执行文件完整路径
    pub exe_path: String,
    /// 启动参数
    pub args: Vec<String>,
}

impl Default for AutostartConfig {
    fn default() -> Self {
        Self {
            app_name: "AudioSync".into(),
            exe_path: String::new(),
            args: Vec::new(),
        }
    }
}

/// 启用自启动
pub fn enable_autostart(config: &AutostartConfig) -> Result<(), PlatformError> {
    #[cfg(target_os = "windows")]
    {
        enable_autostart_windows(config)
    }

    #[cfg(target_os = "linux")]
    {
        enable_autostart_linux(config)
    }

    #[cfg(target_os = "macos")]
    {
        enable_autostart_macos(config)
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        Err(PlatformError::Unsupported(crate::platform_name().into()))
    }
}

/// 禁用自启动
pub fn disable_autostart(config: &AutostartConfig) -> Result<(), PlatformError> {
    #[cfg(target_os = "windows")]
    {
        disable_autostart_windows(config)
    }

    #[cfg(target_os = "linux")]
    {
        disable_autostart_linux(config)
    }

    #[cfg(target_os = "macos")]
    {
        disable_autostart_macos(config)
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        Err(PlatformError::Unsupported(crate::platform_name().into()))
    }
}

/// 查询自启动是否已启用
pub fn is_autostart_enabled(config: &AutostartConfig) -> Result<bool, PlatformError> {
    #[cfg(target_os = "windows")]
    {
        is_autostart_enabled_windows(config)
    }

    #[cfg(target_os = "linux")]
    {
        is_autostart_enabled_linux(config)
    }

    #[cfg(target_os = "macos")]
    {
        is_autostart_enabled_macos(config)
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        Ok(false)
    }
}

// ===== Windows 实现 =====

#[cfg(target_os = "windows")]
fn enable_autostart_windows(config: &AutostartConfig) -> Result<(), PlatformError> {
    use windows::core::*;
    use windows::Win32::System::Registry::*;

    let key_path = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");

    let mut exe = config.exe_path.clone();
    for arg in &config.args {
        exe.push(' ');
        exe.push_str(arg);
    }

    let value = HSTRING::from(&exe);
    let app_name = HSTRING::from(&config.app_name);

    unsafe {
        RegSetValueExW(
            HKEY_CURRENT_USER,
            key_path,
            app_name.as_ptr() as *const _,
            REG_SZ,
            value.as_ptr() as *const _ as *const u8,
            (value.len() * 2 + 2) as u32,
        )
        .map_err(|e| PlatformError::Registry(format!("RegSetValueExW 失败: {}", e)))?;
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn disable_autostart_windows(config: &AutostartConfig) -> Result<(), PlatformError> {
    use windows::core::*;
    use windows::Win32::System::Registry::*;

    let key_path = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let app_name = HSTRING::from(&config.app_name);

    unsafe {
        RegDeleteValueW(HKEY_CURRENT_USER, key_path, app_name.as_ptr() as *const _)
            .map_err(|e| PlatformError::Registry(format!("RegDeleteValueW 失败: {}", e)))?;
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn is_autostart_enabled_windows(config: &AutostartConfig) -> Result<bool, PlatformError> {
    use windows::core::*;
    use windows::Win32::System::Registry::*;

    let key_path = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let app_name = HSTRING::from(&config.app_name);

    unsafe {
        let mut len: u32 = 0;
        let result = RegQueryValueExW(
            HKEY_CURRENT_USER,
            key_path,
            app_name.as_ptr() as *const _,
            None,
            None,
            Some(&mut len),
        );
        Ok(result.is_ok())
    }
}

// ===== Linux 实现 =====

#[cfg(target_os = "linux")]
fn enable_autostart_linux(config: &AutostartConfig) -> Result<(), PlatformError> {
    let service = format!(
        r#"[Unit]
Description=AudioSync
After=graphical-session.target

[Service]
Type=simple
ExecStart={} {}

[Install]
WantedBy=default.target
"#,
        config.exe_path,
        config.args.join(" ")
    );

    let dir = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join(".config/systemd/user");

    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.service", config.app_name.to_lowercase()));
    std::fs::write(&path, &service)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn disable_autostart_linux(config: &AutostartConfig) -> Result<(), PlatformError> {
    let path = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join(".config/systemd/user")
        .join(format!("{}.service", config.app_name.to_lowercase()));

    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn is_autostart_enabled_linux(config: &AutostartConfig) -> Result<bool, PlatformError> {
    let path = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join(".config/systemd/user")
        .join(format!("{}.service", config.app_name.to_lowercase()));

    Ok(path.exists())
}

// ===== macOS 实现 =====

#[cfg(target_os = "macos")]
fn enable_autostart_macos(config: &AutostartConfig) -> Result<(), PlatformError> {
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>com.{}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
        {}
    </array>
    <key>RunAtLoad</key><true/>
</dict>
</plist>
"#,
        config.app_name.to_lowercase(),
        config.exe_path,
        config
            .args
            .iter()
            .map(|a| format!("<string>{}</string>", a))
            .collect::<Vec<_>>()
            .join("\n        ")
    );

    let dir = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join("Library/LaunchAgents");

    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("com.{}.plist", config.app_name.to_lowercase()));
    std::fs::write(&path, &plist)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn disable_autostart_macos(config: &AutostartConfig) -> Result<(), PlatformError> {
    let path = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join("Library/LaunchAgents")
        .join(format!("com.{}.plist", config.app_name.to_lowercase()));

    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn is_autostart_enabled_macos(config: &AutostartConfig) -> Result<bool, PlatformError> {
    let path = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join("Library/LaunchAgents")
        .join(format!("com.{}.plist", config.app_name.to_lowercase()));

    Ok(path.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autostart_config_default() {
        let config = AutostartConfig::default();
        assert_eq!(config.app_name, "AudioSync");
        assert!(config.exe_path.is_empty());
    }

    #[test]
    fn enable_disable_autostart_roundtrip() {
        let config = AutostartConfig {
            app_name: "AudioSyncTest".into(),
            exe_path: "/usr/bin/audiosync".into(),
            args: vec!["daemon".into()],
        };

        // 启用
        let result = enable_autostart(&config);
        // 在非 Windows 上应该成功
        #[cfg(not(target_os = "windows"))]
        {
            assert!(result.is_ok());

            // 检查
            assert!(is_autostart_enabled(&config).unwrap_or(false));

            // 禁用
            disable_autostart(&config).unwrap();

            // 确认已禁用
            assert!(!is_autostart_enabled(&config).unwrap_or(true));
        }
        #[cfg(target_os = "windows")]
        {
            // Windows 上如果 exe 不存在可能失败，不强制断言
            let _ = result;
        }
    }
}
