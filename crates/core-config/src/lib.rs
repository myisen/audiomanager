//! core-config: 配置模型
//!
//! 实现 FR-09：管理全产品配置，分为 8 个分组。
//! 当前实现 P0 所需的「音频库」分组，其余分组以默认值占位，后续迭代补充。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 音频库根目录配置
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryRoot {
    /// 根目录路径
    pub path: PathBuf,
    /// 是否递归扫描子目录（每个子目录生成独立清单）
    pub recursive: bool,
}

/// 音频库配置（FR-09 §1）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LibraryConfig {
    /// 库根目录列表
    pub roots: Vec<LibraryRoot>,
}

/// 全产品配置
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AppConfig {
    /// 音频库
    pub library: LibraryConfig,
    // 其余 7 个分组（播放器、播放与续播、设备与发现、传输、元数据、分享、日志）
    // 将在后续迭代中补充，当前以默认值占位。
}

/// 配置键名（存储在 core-store 中）
pub const CONFIG_KEY: &str = "config";

impl AppConfig {
    /// 从 core-store 加载配置；不存在则返回默认配置
    pub fn load(store: &core_store::Store) -> Result<Self, core_store::StoreError> {
        match store.read::<AppConfig>(CONFIG_KEY)? {
            Some(config) => Ok(config),
            None => Ok(Self::default()),
        }
    }

    /// 保存配置到 core-store
    pub fn save(&self, store: &core_store::Store) -> Result<(), core_store::StoreError> {
        store.write(CONFIG_KEY, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_store::Store;
    use std::fs;

    fn temp_dir() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("core-config-test-{}-{}", std::process::id(), id));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn default_config_has_empty_roots() {
        let config = AppConfig::default();
        assert!(config.library.roots.is_empty());
    }

    #[test]
    fn config_roundtrip_through_store() {
        let dir = temp_dir();
        let store = Store::open(&dir).unwrap();

        let config = AppConfig {
            library: LibraryConfig {
                roots: vec![
                    LibraryRoot {
                        path: PathBuf::from("/music"),
                        recursive: true,
                    },
                    LibraryRoot {
                        path: PathBuf::from("/podcasts"),
                        recursive: false,
                    },
                ],
            },
        };

        config.save(&store).unwrap();
        let loaded = AppConfig::load(&store).unwrap();
        assert_eq!(config, loaded);
    }

    #[test]
    fn load_returns_default_when_missing() {
        let dir = temp_dir();
        let store = Store::open(&dir).unwrap();

        let config = AppConfig::load(&store).unwrap();
        assert_eq!(config, AppConfig::default());
    }

    #[test]
    fn library_root_recursive_flag_persists() {
        let dir = temp_dir();
        let store = Store::open(&dir).unwrap();

        let mut config = AppConfig::default();
        config.library.roots.push(LibraryRoot {
            path: PathBuf::from("/music"),
            recursive: true,
        });
        config.save(&store).unwrap();

        let loaded = AppConfig::load(&store).unwrap();
        assert!(loaded.library.roots[0].recursive);
    }
}
