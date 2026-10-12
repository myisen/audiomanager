//! core-store: 原子持久化层
//!
//! 提供键值对存储，所有写入操作保证原子性（写入过程中崩溃不会留下半写文件）。
//! 用于清单、播放位置、配置、元数据等核心数据的持久化。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;
use thiserror::Error;

/// core-store 的错误类型
#[derive(Error, Debug)]
pub enum StoreError {
    #[error("I/O 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("序列化/反序列化错误: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("键名非法: {0}")]
    InvalidKey(String),
}

/// 原子键值对存储
///
/// 每个键对应一个以键名为文件名的 JSON 文件。
/// 写入采用「写临时文件 → fsync → rename → fsync 目录」的模式，
/// 确保崩溃时要么保留旧值，要么保留新值，不会出现半写文件。
pub struct Store {
    path: PathBuf,
}

impl Store {
    /// 打开或创建存储目录
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, StoreError> {
        let path = path.as_ref().to_path_buf();
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    /// 读取键对应的值，不存在则返回 None
    pub fn read<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, StoreError> {
        self.validate_key(key)?;
        let file_path = self.path.join(key);
        if !file_path.exists() {
            return Ok(None);
        }
        let content = fs::read(&file_path)?;
        let value = serde_json::from_slice(&content)?;
        Ok(Some(value))
    }

    /// 原子写入键值对
    pub fn write<T: Serialize>(&self, key: &str, value: &T) -> Result<(), StoreError> {
        self.validate_key(key)?;
        let file_path = self.path.join(key);
        let tmp_path = self.path.join(format!("{}.tmp", key));

        // 1. 序列化
        let bytes = serde_json::to_vec_pretty(value)?;

        // 2. 写入临时文件并 fsync
        let mut f = fs::File::create(&tmp_path)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        drop(f);

        // 3. 重命名为目标文件（原子操作）
        fs::rename(&tmp_path, &file_path)?;

        // 4. fsync 目录，确保 rename 持久化
        if let Ok(dir) = fs::File::open(&self.path) {
            let _ = dir.sync_all();
        }

        Ok(())
    }

    /// 删除键
    pub fn remove(&self, key: &str) -> Result<(), StoreError> {
        self.validate_key(key)?;
        let file_path = self.path.join(key);
        if file_path.exists() {
            fs::remove_file(&file_path)?;
        }
        // 清理可能遗留的临时文件
        let tmp_path = self.path.join(format!("{}.tmp", key));
        if tmp_path.exists() {
            let _ = fs::remove_file(&tmp_path);
        }
        Ok(())
    }

    /// 列出所有键
    pub fn keys(&self) -> Result<Vec<String>, StoreError> {
        let mut keys = Vec::new();
        for entry in fs::read_dir(&self.path)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy().into_owned();
            // 跳过临时文件
            if !name.ends_with(".tmp") {
                keys.push(name);
            }
        }
        Ok(keys)
    }

    /// 校验键名：不允许包含路径分隔符或为空
    fn validate_key(&self, key: &str) -> Result<(), StoreError> {
        if key.is_empty() {
            return Err(StoreError::InvalidKey("键名不能为空".to_string()));
        }
        if key.contains('/') || key.contains('\\') || key.contains('\0') {
            return Err(StoreError::InvalidKey(format!("键名包含非法字符: {}", key)));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::fs;

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct TestValue {
        name: String,
        count: u32,
    }

    fn temp_dir() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("core-store-test-{}-{}", std::process::id(), id));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn write_and_read_roundtrip() {
        let dir = temp_dir();
        let store = Store::open(&dir).unwrap();

        let value = TestValue {
            name: "hello".to_string(),
            count: 42,
        };
        store.write("key1", &value).unwrap();

        let read: Option<TestValue> = store.read("key1").unwrap();
        assert_eq!(read, Some(value));
    }

    #[test]
    fn read_missing_key_returns_none() {
        let dir = temp_dir();
        let store = Store::open(&dir).unwrap();

        let read: Option<TestValue> = store.read("nonexistent").unwrap();
        assert_eq!(read, None);
    }

    #[test]
    fn overwrite_replaces_value() {
        let dir = temp_dir();
        let store = Store::open(&dir).unwrap();

        let v1 = TestValue {
            name: "a".into(),
            count: 1,
        };
        let v2 = TestValue {
            name: "b".into(),
            count: 2,
        };

        store.write("k", &v1).unwrap();
        store.write("k", &v2).unwrap();

        let read: TestValue = store.read("k").unwrap().unwrap();
        assert_eq!(read, v2);
    }

    #[test]
    fn remove_deletes_value() {
        let dir = temp_dir();
        let store = Store::open(&dir).unwrap();

        store
            .write(
                "k",
                &TestValue {
                    name: "x".into(),
                    count: 1,
                },
            )
            .unwrap();
        assert!(store.read::<TestValue>("k").unwrap().is_some());

        store.remove("k").unwrap();
        assert!(store.read::<TestValue>("k").unwrap().is_none());
    }

    #[test]
    fn keys_lists_all_entries() {
        let dir = temp_dir();
        let store = Store::open(&dir).unwrap();

        store.write("a", &1u32).unwrap();
        store.write("b", &2u32).unwrap();
        store.write("c", &3u32).unwrap();

        let mut keys = store.keys().unwrap();
        keys.sort();
        assert_eq!(keys, vec!["a", "b", "c"]);
    }

    #[test]
    fn write_leaves_no_temp_files() {
        let dir = temp_dir();
        let store = Store::open(&dir).unwrap();

        store
            .write(
                "k",
                &TestValue {
                    name: "v".into(),
                    count: 1,
                },
            )
            .unwrap();

        // 原子写入不应留下 .tmp 文件
        let entries: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(!entries.iter().any(|n| n.ends_with(".tmp")));
    }

    #[test]
    fn atomic_write_survives_incomplete_temp() {
        let dir = temp_dir();
        let store = Store::open(&dir).unwrap();

        let v1 = TestValue {
            name: "original".into(),
            count: 100,
        };
        store.write("k", &v1).unwrap();

        // 模拟写入过程中留下一个临时文件（崩溃场景）
        let tmp_path = dir.join("k.tmp");
        fs::write(&tmp_path, b"incomplete garbage").unwrap();

        // 读取应返回已提交的值，而非临时文件内容
        let read: TestValue = store.read("k").unwrap().unwrap();
        assert_eq!(read, v1);

        // 下次写入应能清理临时文件并成功
        let v2 = TestValue {
            name: "updated".into(),
            count: 200,
        };
        store.write("k", &v2).unwrap();
        let read: TestValue = store.read("k").unwrap().unwrap();
        assert_eq!(read, v2);
        assert!(!tmp_path.exists());
    }

    #[test]
    fn store_creates_directory_if_missing() {
        let dir = temp_dir().join("nested").join("store");
        assert!(!dir.exists());

        let store = Store::open(&dir).unwrap();
        store.write("k", &1u32).unwrap();

        assert!(dir.exists());
    }
}
