//! core-cast: 设备发现与配对
//!
//! 实现 FR-05：局域网自动发现、配对流程、设备状态机。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// 发现协议版本
pub const PROTOCOL_VERSION: u32 = 1;

/// 设备离线超时（秒）：超过此时间未收到 beacon 视为离线
pub const DEVICE_TIMEOUT_SECS: u64 = 30;

/// 设备唯一标识
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceId(Uuid);

impl DeviceId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl Default for DeviceId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for DeviceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 设备能力
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceCapabilities {
    /// 可发送文件
    pub can_send: bool,
    /// 可接收文件
    pub can_receive: bool,
}

impl Default for DeviceCapabilities {
    fn default() -> Self {
        Self {
            can_send: true,
            can_receive: true,
        }
    }
}

/// 设备信息
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// 设备唯一 ID
    pub id: DeviceId,
    /// 设备名称（用户可读）
    pub name: String,
    /// 设备监听地址
    pub addr: SocketAddr,
    /// 设备能力
    pub capabilities: DeviceCapabilities,
}

/// 发现广播包
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryBeacon {
    /// 协议版本
    pub protocol_version: u32,
    /// 设备信息
    pub device: DeviceInfo,
    /// 发送时间戳（Unix 毫秒）
    pub timestamp: u128,
}

impl DiscoveryBeacon {
    pub fn new(device: DeviceInfo) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            device,
            timestamp: current_timestamp_ms(),
        }
    }

    /// 是否过期（基于发送时间戳）
    pub fn is_expired(&self) -> bool {
        let now = current_timestamp_ms();
        now - self.timestamp > Duration::from_secs(DEVICE_TIMEOUT_SECS).as_millis()
    }

    /// 序列化为 JSON 字节
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// 从 JSON 字节反序列化
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        serde_json::from_slice(data).ok()
    }
}

/// 已发现的设备（带最后看到时间）
#[derive(Debug, Clone)]
pub struct DiscoveredDevice {
    pub device: DeviceInfo,
    pub last_seen: Instant,
}

impl DiscoveredDevice {
    pub fn from_beacon(beacon: DiscoveryBeacon) -> Self {
        Self {
            device: beacon.device,
            last_seen: Instant::now(),
        }
    }

    /// 更新最后看到时间
    pub fn touch(&mut self) {
        self.last_seen = Instant::now();
    }

    /// 是否过期
    pub fn is_expired(&self) -> bool {
        self.last_seen.elapsed() > Duration::from_secs(DEVICE_TIMEOUT_SECS)
    }
}

/// 发现表：维护所有已发现的设备
#[derive(Debug, Default)]
pub struct DiscoveryTable {
    devices: HashMap<DeviceId, DiscoveredDevice>,
}

impl DiscoveryTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// 根据 beacon 更新（新增或刷新）设备
    pub fn update(&mut self, beacon: &DiscoveryBeacon) {
        let id = beacon.device.id;
        if let Some(existing) = self.devices.get_mut(&id) {
            existing.device = beacon.device.clone();
            existing.touch();
        } else {
            self.devices
                .insert(id, DiscoveredDevice::from_beacon(beacon.clone()));
        }
    }

    pub fn get(&self, id: &DeviceId) -> Option<&DiscoveredDevice> {
        self.devices.get(id)
    }

    pub fn get_mut(&mut self, id: &DeviceId) -> Option<&mut DiscoveredDevice> {
        self.devices.get_mut(id)
    }

    pub fn len(&self) -> usize {
        self.devices.len()
    }

    pub fn is_empty(&self) -> bool {
        self.devices.is_empty()
    }

    /// 移除所有过期设备
    pub fn remove_expired(&mut self) {
        self.devices.retain(|_, d| !d.is_expired());
    }

    /// 遍历所有设备
    pub fn devices(&self) -> impl Iterator<Item = &DiscoveredDevice> {
        self.devices.values()
    }
}

/// 获取当前 Unix 毫秒时间戳
fn current_timestamp_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

// ===== 配对（T32）=====

/// 配对令牌
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PairingToken(String);

impl PairingToken {
    /// 生成随机配对令牌
    pub fn generate() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for PairingToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 配对请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingRequest {
    /// 请求方设备信息
    pub requester: DeviceInfo,
    /// 配对令牌（请求方生成）
    pub token: PairingToken,
    /// 请求时间戳
    pub timestamp: u128,
}

impl PairingRequest {
    pub fn new(requester: DeviceInfo) -> Self {
        Self {
            requester,
            token: PairingToken::generate(),
            timestamp: current_timestamp_ms(),
        }
    }
}

/// 配对响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingResponse {
    /// 响应方设备 ID
    pub responder_id: DeviceId,
    /// 配对令牌（与请求一致）
    pub token: PairingToken,
    /// 是否接受配对
    pub accepted: bool,
    /// 响应时间戳
    pub timestamp: u128,
}

impl PairingResponse {
    pub fn accept(responder_id: DeviceId, token: PairingToken) -> Self {
        Self {
            responder_id,
            token,
            accepted: true,
            timestamp: current_timestamp_ms(),
        }
    }

    pub fn reject(responder_id: DeviceId, token: PairingToken) -> Self {
        Self {
            responder_id,
            token,
            accepted: false,
            timestamp: current_timestamp_ms(),
        }
    }
}

/// 已配对设备
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairedDevice {
    pub info: DeviceInfo,
    /// 配对时间戳
    pub paired_at: u128,
}

/// 配对存储：维护已配对设备列表
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct PairingStore {
    paired: HashMap<String, PairedDevice>,
}

impl PairingStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 添加配对设备
    pub fn add(&mut self, device: DeviceInfo) {
        let key = device.id.to_string();
        self.paired.insert(
            key,
            PairedDevice {
                info: device,
                paired_at: current_timestamp_ms(),
            },
        );
    }

    /// 移除配对设备
    pub fn remove(&mut self, id: &DeviceId) {
        self.paired.remove(&id.to_string());
    }

    /// 是否已配对
    pub fn is_paired(&self, id: &DeviceId) -> bool {
        self.paired.contains_key(&id.to_string())
    }

    /// 获取配对设备
    pub fn get(&self, id: &DeviceId) -> Option<&PairedDevice> {
        self.paired.get(&id.to_string())
    }

    pub fn len(&self) -> usize {
        self.paired.len()
    }

    pub fn is_empty(&self) -> bool {
        self.paired.is_empty()
    }

    pub fn devices(&self) -> impl Iterator<Item = &PairedDevice> {
        self.paired.values()
    }
}

// ===== 设备状态机与确认策略（T33）=====

/// 设备配对状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceState {
    /// 已配对
    Paired,
    /// 已阻止（用户主动阻止）
    Blocked,
}

/// 接收确认策略（FR-05）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ConfirmationPolicy {
    /// 每次确认（默认）
    #[default]
    Always,
    /// 已配对设备免确认
    Trusted,
    /// 拒绝所有接收
    Never,
}

/// 单个设备的完整状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceStateEntry {
    pub info: DeviceInfo,
    pub state: DeviceState,
    /// 该设备的确认策略（覆盖全局策略）
    pub policy: Option<ConfirmationPolicy>,
}

impl DeviceStateEntry {
    pub fn paired(info: DeviceInfo) -> Self {
        Self {
            info,
            state: DeviceState::Paired,
            policy: None,
        }
    }

    pub fn is_blocked(&self) -> bool {
        self.state == DeviceState::Blocked
    }

    pub fn is_paired(&self) -> bool {
        self.state == DeviceState::Paired
    }
}

/// 设备管理器：统一管理发现、配对、状态、策略
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct DeviceManager {
    devices: HashMap<String, DeviceStateEntry>,
    /// 全局确认策略
    pub global_policy: ConfirmationPolicy,
}

impl DeviceManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册已配对设备
    pub fn add_paired(&mut self, device: DeviceInfo) {
        let key = device.id.to_string();
        self.devices.insert(key, DeviceStateEntry::paired(device));
    }

    /// 解除配对
    pub fn unpair(&mut self, id: &DeviceId) {
        self.devices.remove(&id.to_string());
    }

    /// 阻止设备
    pub fn block(&mut self, id: &DeviceId) -> bool {
        if let Some(entry) = self.devices.get_mut(&id.to_string()) {
            entry.state = DeviceState::Blocked;
            true
        } else {
            false
        }
    }

    /// 解除阻止
    pub fn unblock(&mut self, id: &DeviceId) -> bool {
        if let Some(entry) = self.devices.get_mut(&id.to_string()) {
            entry.state = DeviceState::Paired;
            true
        } else {
            false
        }
    }

    /// 获取设备状态
    pub fn get(&self, id: &DeviceId) -> Option<&DeviceStateEntry> {
        self.devices.get(&id.to_string())
    }

    /// 设备是否已配对且未阻止
    pub fn is_available(&self, id: &DeviceId) -> bool {
        self.get(id).map(|e| e.is_paired()).unwrap_or(false)
    }

    /// 设备是否被阻止
    pub fn is_blocked(&self, id: &DeviceId) -> bool {
        self.get(id).map(|e| e.is_blocked()).unwrap_or(false)
    }

    /// 设置某设备的确认策略
    pub fn set_policy(&mut self, id: &DeviceId, policy: ConfirmationPolicy) -> bool {
        if let Some(entry) = self.devices.get_mut(&id.to_string()) {
            entry.policy = Some(policy);
            true
        } else {
            false
        }
    }

    /// 判断是否需要确认接收
    ///
    /// 规则（FR-05）：
    /// - 被阻止设备：直接拒绝（无需确认）
    /// - 未配对设备：需要确认
    /// - 已配对设备：使用其专属策略，否则使用全局策略
    pub fn needs_confirmation(&self, id: &DeviceId) -> ConfirmationDecision {
        match self.get(id) {
            None => ConfirmationDecision::NeedsConfirmation, // 未配对
            Some(entry) => {
                if entry.is_blocked() {
                    return ConfirmationDecision::Reject;
                }
                let policy = entry.policy.unwrap_or(self.global_policy);
                match policy {
                    ConfirmationPolicy::Always => ConfirmationDecision::NeedsConfirmation,
                    ConfirmationPolicy::Trusted => ConfirmationDecision::Allow,
                    ConfirmationPolicy::Never => ConfirmationDecision::Reject,
                }
            }
        }
    }

    pub fn len(&self) -> usize {
        self.devices.len()
    }

    pub fn is_empty(&self) -> bool {
        self.devices.is_empty()
    }
}

/// 确认决策
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationDecision {
    /// 需要用户确认
    NeedsConfirmation,
    /// 自动允许
    Allow,
    /// 拒绝
    Reject,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;

    fn make_device_info() -> DeviceInfo {
        DeviceInfo {
            id: DeviceId::new(),
            name: "Test Device".to_string(),
            addr: SocketAddr::from(([192, 168, 1, 100], 9000)),
            capabilities: DeviceCapabilities {
                can_send: true,
                can_receive: true,
            },
        }
    }

    #[test]
    fn device_id_is_unique() {
        let id1 = DeviceId::new();
        let id2 = DeviceId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn beacon_serialization_roundtrip() {
        let device = make_device_info();
        let beacon = DiscoveryBeacon::new(device.clone());

        let json = serde_json::to_string(&beacon).unwrap();
        let parsed: DiscoveryBeacon = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.device.id, device.id);
        assert_eq!(parsed.device.name, device.name);
        assert_eq!(parsed.device.addr, device.addr);
    }

    #[test]
    fn beacon_has_protocol_version() {
        let beacon = DiscoveryBeacon::new(make_device_info());
        assert_eq!(beacon.protocol_version, PROTOCOL_VERSION);
    }

    #[test]
    fn beacon_expiry_works() {
        let beacon = DiscoveryBeacon::new(make_device_info());
        // 刚创建时未过期
        assert!(!beacon.is_expired());
    }

    #[test]
    fn device_capabilities_default_all_true() {
        let caps = DeviceCapabilities::default();
        assert!(caps.can_send);
        assert!(caps.can_receive);
    }

    #[test]
    fn discovered_device_tracks_last_seen() {
        let device = make_device_info();
        let mut discovered = DiscoveredDevice::from_beacon(DiscoveryBeacon::new(device.clone()));

        assert_eq!(discovered.device.id, device.id);
        assert!(!discovered.is_expired());

        discovered.touch();
        assert!(!discovered.is_expired());
    }

    #[test]
    fn discovery_table_adds_and_updates_device() {
        let mut table = DiscoveryTable::new();
        let device = make_device_info();
        let beacon = DiscoveryBeacon::new(device.clone());

        table.update(&beacon);
        assert_eq!(table.len(), 1);

        // 同一设备再次出现，不增加数量
        table.update(&beacon);
        assert_eq!(table.len(), 1);

        let found = table.get(&device.id).unwrap();
        assert_eq!(found.device.name, device.name);
    }

    #[test]
    fn discovery_table_removes_expired() {
        let mut table = DiscoveryTable::new();
        let device = make_device_info();
        let beacon = DiscoveryBeacon::new(device.clone());
        table.update(&beacon);

        // 手动标记为过期
        if let Some(d) = table.get_mut(&device.id) {
            d.last_seen = std::time::Instant::now()
                - std::time::Duration::from_secs(DEVICE_TIMEOUT_SECS + 10);
        }

        table.remove_expired();
        assert_eq!(table.len(), 0);
    }

    #[test]
    fn discovery_table_iterates_devices() {
        let mut table = DiscoveryTable::new();
        let d1 = DeviceInfo {
            id: DeviceId::new(),
            name: "Device 1".into(),
            addr: SocketAddr::from(([192, 168, 1, 1], 9000)),
            capabilities: DeviceCapabilities::default(),
        };
        let d2 = DeviceInfo {
            id: DeviceId::new(),
            name: "Device 2".into(),
            addr: SocketAddr::from(([192, 168, 1, 2], 9000)),
            capabilities: DeviceCapabilities::default(),
        };
        table.update(&DiscoveryBeacon::new(d1));
        table.update(&DiscoveryBeacon::new(d2));

        let devices: Vec<_> = table.devices().collect();
        assert_eq!(devices.len(), 2);
    }

    // ===== 配对测试 =====

    #[test]
    fn pairing_token_is_unique() {
        let t1 = PairingToken::generate();
        let t2 = PairingToken::generate();
        assert_ne!(t1, t2);
    }

    #[test]
    fn pairing_request_carries_requester_and_token() {
        let device = make_device_info();
        let req = PairingRequest::new(device.clone());

        assert_eq!(req.requester.id, device.id);
        assert!(!req.token.as_str().is_empty());
    }

    #[test]
    fn pairing_accept_response_marks_accepted() {
        let responder_id = DeviceId::new();
        let token = PairingToken::generate();
        let resp = PairingResponse::accept(responder_id, token.clone());

        assert!(resp.accepted);
        assert_eq!(resp.token, token);
        assert_eq!(resp.responder_id, responder_id);
    }

    #[test]
    fn pairing_reject_response_marks_rejected() {
        let responder_id = DeviceId::new();
        let token = PairingToken::generate();
        let resp = PairingResponse::reject(responder_id, token);

        assert!(!resp.accepted);
    }

    #[test]
    fn pairing_store_add_and_check() {
        let mut store = PairingStore::new();
        let device = make_device_info();

        assert!(!store.is_paired(&device.id));
        store.add(device.clone());
        assert!(store.is_paired(&device.id));
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn pairing_store_remove() {
        let mut store = PairingStore::new();
        let device = make_device_info();
        store.add(device.clone());

        store.remove(&device.id);
        assert!(!store.is_paired(&device.id));
        assert!(store.is_empty());
    }

    #[test]
    fn pairing_store_dedupes_by_device_id() {
        let mut store = PairingStore::new();
        let device = make_device_info();
        store.add(device.clone());
        store.add(device.clone()); // 同一设备再次添加

        assert_eq!(store.len(), 1);
    }

    #[test]
    fn pairing_request_serialization_roundtrip() {
        let device = make_device_info();
        let req = PairingRequest::new(device);
        let json = serde_json::to_string(&req).unwrap();
        let parsed: PairingRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.requester.id, req.requester.id);
        assert_eq!(parsed.token, req.token);
    }

    #[test]
    fn pairing_store_serialization_roundtrip() {
        let mut store = PairingStore::new();
        store.add(make_device_info());
        store.add(make_device_info());

        let json = serde_json::to_string(&store).unwrap();
        let parsed: PairingStore = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.len(), store.len());
    }

    // ===== 设备状态机与确认策略测试 =====

    #[test]
    fn default_confirmation_policy_is_always() {
        assert_eq!(ConfirmationPolicy::default(), ConfirmationPolicy::Always);
    }

    #[test]
    fn add_paired_device_is_available() {
        let mut mgr = DeviceManager::new();
        let device = make_device_info();
        mgr.add_paired(device.clone());

        assert!(mgr.is_available(&device.id));
        assert!(!mgr.is_blocked(&device.id));
    }

    #[test]
    fn block_device_makes_it_unavailable() {
        let mut mgr = DeviceManager::new();
        let device = make_device_info();
        mgr.add_paired(device.clone());

        assert!(mgr.block(&device.id));
        assert!(!mgr.is_available(&device.id));
        assert!(mgr.is_blocked(&device.id));
    }

    #[test]
    fn unblock_restores_availability() {
        let mut mgr = DeviceManager::new();
        let device = make_device_info();
        mgr.add_paired(device.clone());
        mgr.block(&device.id);
        mgr.unblock(&device.id);

        assert!(mgr.is_available(&device.id));
    }

    #[test]
    fn block_unknown_device_returns_false() {
        let mut mgr = DeviceManager::new();
        let id = DeviceId::new();
        assert!(!mgr.block(&id));
    }

    #[test]
    fn unpair_removes_device() {
        let mut mgr = DeviceManager::new();
        let device = make_device_info();
        mgr.add_paired(device.clone());

        mgr.unpair(&device.id);
        assert!(!mgr.is_available(&device.id));
        assert!(mgr.get(&device.id).is_none());
    }

    #[test]
    fn unpaired_device_needs_confirmation() {
        let mgr = DeviceManager::new();
        let id = DeviceId::new();

        assert_eq!(
            mgr.needs_confirmation(&id),
            ConfirmationDecision::NeedsConfirmation
        );
    }

    #[test]
    fn blocked_device_rejected() {
        let mut mgr = DeviceManager::new();
        let device = make_device_info();
        mgr.add_paired(device.clone());
        mgr.block(&device.id);

        assert_eq!(
            mgr.needs_confirmation(&device.id),
            ConfirmationDecision::Reject
        );
    }

    #[test]
    fn paired_device_with_default_policy_needs_confirmation() {
        let mut mgr = DeviceManager::new();
        let device = make_device_info();
        mgr.add_paired(device.clone());
        // 全局策略默认 Always

        assert_eq!(
            mgr.needs_confirmation(&device.id),
            ConfirmationDecision::NeedsConfirmation
        );
    }

    #[test]
    fn paired_device_with_trusted_policy_allowed() {
        let mut mgr = DeviceManager::new();
        let device = make_device_info();
        mgr.add_paired(device.clone());
        mgr.set_policy(&device.id, ConfirmationPolicy::Trusted);

        assert_eq!(
            mgr.needs_confirmation(&device.id),
            ConfirmationDecision::Allow
        );
    }

    #[test]
    fn never_policy_rejects() {
        let mut mgr = DeviceManager::new();
        let device = make_device_info();
        mgr.add_paired(device.clone());
        mgr.set_policy(&device.id, ConfirmationPolicy::Never);

        assert_eq!(
            mgr.needs_confirmation(&device.id),
            ConfirmationDecision::Reject
        );
    }

    #[test]
    fn global_trusted_policy_applies_to_all_paired() {
        let mut mgr = DeviceManager::new();
        mgr.global_policy = ConfirmationPolicy::Trusted;
        let device = make_device_info();
        mgr.add_paired(device.clone());

        assert_eq!(
            mgr.needs_confirmation(&device.id),
            ConfirmationDecision::Allow
        );
    }

    #[test]
    fn device_policy_overrides_global() {
        let mut mgr = DeviceManager::new();
        mgr.global_policy = ConfirmationPolicy::Always;
        let device = make_device_info();
        mgr.add_paired(device.clone());
        mgr.set_policy(&device.id, ConfirmationPolicy::Trusted);

        assert_eq!(
            mgr.needs_confirmation(&device.id),
            ConfirmationDecision::Allow
        );
    }

    #[test]
    fn device_manager_serialization_roundtrip() {
        let mut mgr = DeviceManager::new();
        let device = make_device_info();
        let id = device.id;
        mgr.add_paired(device);
        mgr.block(&id);

        let json = serde_json::to_string(&mgr).unwrap();
        let parsed: DeviceManager = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.len(), mgr.len());
    }
}
