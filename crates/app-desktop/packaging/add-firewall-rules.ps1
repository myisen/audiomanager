# AudioSync Windows 防火墙规则脚本
# T92: 安装时自动添加防火墙例外

$ErrorActionPreference = "Stop"
$exePath = "$env:PROGRAMFILES\AudioSync\audiosync-desktop.exe"
$daemonPath = "$env:PROGRAMFILES\AudioSync\audiosync.exe"
$ruleName = "AudioSync"

Write-Host "Adding firewall rules for AudioSync..."

# 检查是否已有规则
$existing = netsh advfirewall firewall show rule name=$ruleName 2>$null
if ($existing -and $existing.Contains($ruleName)) {
    Write-Host "Firewall rule '$ruleName' already exists, skipping."
} else {
    # 添加桌面应用规则
    netsh advfirewall firewall add rule name=$ruleName dir=in action=allow program=$exePath enable=yes
    Write-Host "Added firewall rule for desktop app."

    # 添加守护进程规则
    netsh advfirewall firewall add rule name="$ruleName-Daemon" dir=in action=allow program=$daemonPath enable=yes
    Write-Host "Added firewall rule for daemon."
}

# 添加 P2P 传输端口（动态端口范围）
$ports = @(8080, 8081)
foreach ($port in $ports) {
    $portRule = netsh advfirewall firewall show rule name="$ruleName-Port-$port" 2>$null
    if (-not ($portRule -and $portRule.Contains("$ruleName-Port-$port"))) {
        netsh advfirewall firewall add rule name="$ruleName-Port-$port" dir=in action=allow protocol=TCP localport=$port
        netsh advfirewall firewall add rule name="$ruleName-Port-$port-UDP" dir=in action=allow protocol=UDP localport=$port
        Write-Host "Added firewall rule for port $port."
    }
}

Write-Host "Firewall rules configured successfully."
