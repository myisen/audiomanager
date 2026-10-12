//! core-position: 播放位置记录
//!
//! 实现 FR-04：以「清单 + 文件」为键记录播放暂停位置，
//! 打开清单自动从上次暂停位置开始播放。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// 已播完阈值（Q3）：剩余时长小于此值视为已播完
pub const FINISHED_THRESHOLD_SECS: f64 = 5.0;

/// core-store 中的存储键
const POSITIONS_KEY: &str = "positions";

/// 播放位置存储
///
/// 以文件路径为键记录播放位置（秒）。由于每个文件仅属于一个目录（清单），
/// 文件路径本身即可唯一标识「清单 + 文件」。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PositionStore {
    positions: HashMap<String, f64>,
}

impl PositionStore {
    /// 从 core-store 加载
    pub fn load(store: &core_store::Store) -> Result<Self, core_store::StoreError> {
        match store.read::<PositionStore>(POSITIONS_KEY)? {
            Some(ps) => Ok(ps),
            None => Ok(Self::default()),
        }
    }

    /// 保存到 core-store（原子写入）
    pub fn save(&self, store: &core_store::Store) -> Result<(), core_store::StoreError> {
        store.write(POSITIONS_KEY, self)
    }

    /// 获取文件的播放位置
    pub fn get(&self, file: &Path) -> Option<f64> {
        self.positions
            .get(&file.to_string_lossy().to_string())
            .copied()
    }

    /// 设置文件的播放位置
    pub fn set(&mut self, file: &Path, position_secs: f64) {
        self.positions
            .insert(file.to_string_lossy().to_string(), position_secs);
    }

    /// 移除（重置）文件的播放位置
    pub fn remove(&mut self, file: &Path) {
        self.positions.remove(&file.to_string_lossy().to_string());
    }

    /// 存储的条目数
    pub fn len(&self) -> usize {
        self.positions.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }
}

/// 判断是否已播完（Q3：剩余 < 5 秒）
pub fn is_finished(position: f64, duration: f64) -> bool {
    if duration <= 0.0 {
        return false;
    }
    (duration - position) < FINISHED_THRESHOLD_SECS
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_store::Store;
    use std::fs;
    use std::path::PathBuf;

    fn temp_dir() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("core-position-test-{}-{}", std::process::id(), id));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn save_and_load_position_for_file() {
        let dir = temp_dir();
        let store = Store::open(&dir).unwrap();
        let mut pos = PositionStore::load(&store).unwrap();

        let file = PathBuf::from("/music/song.mp3");
        pos.set(&file, 42.5);
        pos.save(&store).unwrap();

        let loaded = PositionStore::load(&store).unwrap();
        assert_eq!(loaded.get(&file), Some(42.5));
    }

    #[test]
    fn get_returns_none_for_unknown_file() {
        let dir = temp_dir();
        let store = Store::open(&dir).unwrap();
        let pos = PositionStore::load(&store).unwrap();

        assert_eq!(pos.get(&PathBuf::from("/music/never.mp3")), None);
    }

    #[test]
    fn remove_resets_position() {
        let dir = temp_dir();
        let store = Store::open(&dir).unwrap();
        let mut pos = PositionStore::load(&store).unwrap();

        let file = PathBuf::from("/music/song.mp3");
        pos.set(&file, 100.0);
        pos.remove(&file);
        pos.save(&store).unwrap();

        let loaded = PositionStore::load(&store).unwrap();
        assert_eq!(loaded.get(&file), None);
    }

    #[test]
    fn update_overwrites_previous_position() {
        let dir = temp_dir();
        let store = Store::open(&dir).unwrap();
        let mut pos = PositionStore::load(&store).unwrap();

        let file = PathBuf::from("/music/song.mp3");
        pos.set(&file, 10.0);
        pos.set(&file, 25.0);
        pos.save(&store).unwrap();

        let loaded = PositionStore::load(&store).unwrap();
        assert_eq!(loaded.get(&file), Some(25.0));
    }

    #[test]
    fn is_finished_when_remaining_under_5_seconds() {
        // Q3: 已播完阈值 = 剩余 < 5 秒
        assert!(!is_finished(95.0, 100.0)); // 剩余 5 秒 → 不算播完
        assert!(is_finished(96.0, 100.0)); // 剩余 4 秒 → 播完
        assert!(!is_finished(50.0, 100.0)); // 剩余 50 秒 → 未播完
    }

    #[test]
    fn zero_duration_not_finished() {
        assert!(!is_finished(0.0, 0.0));
    }

    #[test]
    fn multiple_files_independent() {
        let dir = temp_dir();
        let store = Store::open(&dir).unwrap();
        let mut pos = PositionStore::load(&store).unwrap();

        let f1 = PathBuf::from("/music/a.mp3");
        let f2 = PathBuf::from("/music/b.mp3");
        pos.set(&f1, 10.0);
        pos.set(&f2, 20.0);
        pos.save(&store).unwrap();

        let loaded = PositionStore::load(&store).unwrap();
        assert_eq!(loaded.get(&f1), Some(10.0));
        assert_eq!(loaded.get(&f2), Some(20.0));
    }
}
