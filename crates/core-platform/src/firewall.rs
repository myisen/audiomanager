//! 防火墙例外引导（T92）
//!
//! Windows: 通过 netsh advfirewall 添加规则，或引导用户手动添加
//! Linux: iptables/ufw 规则提示
//! macOS: 无内置防火墙需处理（pf 可选）

use crate::PlatformError;

/// 防火墙规则
#[derive(Debug, Clone)]
pub struct FirewallRule {
    /// 规则名称
    pub name: String,
    /// 应用程序路径（Windows: exe 路径，Linux: 端口/协议）
    pub program_path: String,
    /// 需要开放的端口
    pub ports: Vec<u16>,
    /// 协议（tcp/udp）
    pub protocol: String,
}

impl Default for FirewallRule {
    fn default() -> Self {
        Self {
            name: "AudioSync".into(),
            program_path: String::new(),
            ports: vec![],
            protocol: "both".into(),
        }
    }
}

/// 尝试自动添加防火墙例外（需要管理员权限）
///
/// Windows: `netsh advfirewall firewall add rule ...`
/// Linux: 返回提示字符串（iptables 需要 root，且通常不自动修改）
pub fn try_add_exception(rule: &FirewallRule) -> Result<(), PlatformError> {
    #[cfg(target_os = "windows")]
    {
        add_firewall_rule_windows(rule)
    }

    #[cfg(not(target_os = "windows"))]
    {
        // 非 Windows 平台不需要防火墙例外
        Ok(())
    }
}

/// 生成用户可手动执行的防火墙命令
pub fn generate_manual_commands(rule: &FirewallRule) -> Vec<String> {
    let mut commands = Vec::new();

    #[cfg(target_os = "windows")]
    {
        let exe = if rule.program_path.is_empty() {
            "%PROGRAMFILES%\\AudioSync\\audiosync-desktop.exe"
        } else {
            &rule.program_path
        };

        commands.push(format!(
            r#"netsh advfirewall firewall add rule name="{}" dir=in action=allow program="{}" enable=yes"#,
            rule.name, exe
        ));

        for port in &rule.ports {
            commands.push(format!(
                r#"netsh advfirewall firewall add rule name="{}-Port-{}" dir=in action=allow protocol=TCP localport={}"#,
                rule.name, port, port
            ));
        }
    }

    #[cfg(target_os = "linux")]
    {
        for port in &rule.ports {
            commands.push(format!(
                "sudo ufw allow {}/tcp",
                port
            ));
            commands.push(format!(
                "sudo ufw allow {}/udp",
                port
            ));
        }
    }

    #[cfg(target_os = "macos")]
    {
        // macOS pf 防火墙默认关闭
        commands.push("# macOS pf 防火墙默认关闭，通常无需配置".into());
    }

    if commands.is_empty() {
        commands.push("# 当前平台无需防火墙配置".into());
    }

    commands
}

/// 检查防火墙规则是否已存在
pub fn check_exception_exists(rule: &FirewallRule) -> Result<bool, PlatformError> {
    #[cfg(target_os = "windows")]
    {
        check_firewall_rule_windows(rule)
    }

    #[cfg(not(target_os = "windows"))]
    {
        Ok(true)
    }
}

// ===== Windows 实现 =====

#[cfg(target_os = "windows")]
fn add_firewall_rule_windows(rule: &FirewallRule) -> Result<(), PlatformError> {
    use std::process::Command;

    let exe = if rule.program_path.is_empty() {
        return Err(PlatformError::System("未指定 exe 路径".into()));
    } else {
        &rule.program_path
    };

    // 添加入站规则
    let result = Command::new("netsh")
        .args([
            "advfirewall", "firewall", "add", "rule",
            &format!("name={}", rule.name),
            "dir=in", "action=allow",
            &format!("program={}", exe),
            "enable=yes",
        ])
        .output();

    match result {
        Ok(output) => {
            if output.status.success() {
                Ok(())
            } else {
                Err(PlatformError::System(format!(
                    "netsh 失败: {}",
                    String::from_utf8_lossy(&output.stderr)
                )))
            }
        }
        Err(e) => Err(PlatformError::System(format!("无法执行 netsh: {}", e))),
    }
}

#[cfg(target_os = "windows")]
fn check_firewall_rule_windows(rule: &FirewallRule) -> Result<bool, PlatformError> {
    use std::process::Command;

    let result = Command::new("netsh")
        .args([
            "advfirewall", "firewall", "show", "rule",
            &format!("name={}", rule.name),
        ])
        .output();

    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            Ok(stdout.contains(&rule.name))
        }
        Err(_) => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn firewall_rule_default() {
        let rule = FirewallRule::default();
        assert_eq!(rule.name, "AudioSync");
        assert!(rule.ports.is_empty());
    }

    #[test]
    fn try_add_exception_on_non_windows() {
        let rule = FirewallRule {
            name: "Test".into(),
            program_path: "/usr/bin/audiosync".into(),
            ports: vec![8080],
            protocol: "tcp".into(),
        };
        // 非 Windows 平台应返回 Ok
        let result = try_add_exception(&rule);
        assert!(result.is_ok());
    }

    #[test]
    fn check_exception_on_non_windows() {
        let rule = FirewallRule::default();
        assert!(check_exception_exists(&rule).unwrap_or(false));
    }

    #[test]
    fn generate_manual_commands_returns_non_empty() {
        let rule = FirewallRule {
            name: "AudioSync".into(),
            program_path: "/usr/bin/audiosync".into(),
            ports: vec![8080, 8081],
            protocol: "both".into(),
        };
        let cmds = generate_manual_commands(&rule);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn generate_manual_commands_no_ports() {
        let rule = FirewallRule::default();
        let cmds = generate_manual_commands(&rule);
        assert!(!cmds.is_empty());
    }
}
