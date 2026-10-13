//! 音频输出抽象（T91）
//!
//! 提供跨平台的音频输出 trait。
//! Windows 上使用 WASAPI（通过 windows crate）。
//! Linux 上使用 PulseAudio/PipeWire（后续集成）。
//! 非 Windows 平台提供 Null 输出实现用于测试。

use crate::PlatformError;

/// 音频输出 trait
pub trait AudioOutput: Send + Sync {
    /// 初始化音频输出
    fn init(&mut self, sample_rate: u32, channels: u16) -> Result<(), PlatformError>;

    /// 写入 PCM 数据（交错格式，16-bit signed）
    fn write(&mut self, data: &[i16]) -> Result<(), PlatformError>;

    /// 暂停输出
    fn pause(&mut self) -> Result<(), PlatformError>;

    /// 恢复输出
    fn resume(&mut self) -> Result<(), PlatformError>;

    /// 设置音量（0.0 ~ 1.0）
    fn set_volume(&mut self, volume: f32) -> Result<(), PlatformError>;

    /// 刷新缓冲区
    fn flush(&mut self) -> Result<(), PlatformError>;

    /// 关闭输出
    fn close(&mut self) -> Result<(), PlatformError>;

    /// 获取延迟（秒）
    fn latency(&self) -> f64;

    /// 音频设备是否可用
    fn is_available(&self) -> bool;
}

/// WASAPI 输出（Windows）
#[cfg(target_os = "windows")]
pub struct WasapiOutput {
    initialized: bool,
    volume: f32,
    available: bool,
}

#[cfg(target_os = "windows")]
impl Default for WasapiOutput {
    fn default() -> Self {
        Self {
            initialized: false,
            volume: 1.0,
            available: true,
        }
    }
}

#[cfg(target_os = "windows")]
impl WasapiOutput {
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(target_os = "windows")]
impl AudioOutput for WasapiOutput {
    fn init(&mut self, sample_rate: u32, channels: u16) -> Result<(), PlatformError> {
        // WASAPI 初始化：
        // 1. CoInitializeEx(NULL, COINIT_MULTITHREADED)
        // 2. IMMDeviceEnumerator::GetDefaultAudioEndpoint(eRender, eConsole, &device)
        // 3. device->Activate(IID_IAudioClient, ...)
        // 4. audio_client->Initialize(AUDCLNT_SHAREMODE_SHARED, 0, ...)
        // 5. audio_client->GetService(IID_IAudioRenderClient, &render_client)
        //
        // 完整 WASAPI 实现在 T91 集成测试阶段补充
        self.initialized = true;
        Ok(())
    }

    fn write(&mut self, data: &[i16]) -> Result<(), PlatformError> {
        if !self.initialized {
            return Err(PlatformError::System("音频未初始化".into()));
        }
        // WASAPI buffer write: render_client->GetBuffer(frames) -> memcpy -> ReleaseBuffer
        Ok(())
    }

    fn pause(&mut self) -> Result<(), PlatformError> {
        Ok(())
    }

    fn resume(&mut self) -> Result<(), PlatformError> {
        Ok(())
    }

    fn set_volume(&mut self, volume: f32) -> Result<(), PlatformError> {
        self.volume = volume.clamp(0.0, 1.0);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), PlatformError> {
        Ok(())
    }

    fn close(&mut self) -> Result<(), PlatformError> {
        self.initialized = false;
        Ok(())
    }

    fn latency(&self) -> f64 {
        0.01 // WASAPI 共享模式典型延迟 ~10ms
    }

    fn is_available(&self) -> bool {
        self.available
    }
}

/// Null 输出（非 Windows 平台 / 无音频设备）
pub struct NullOutput {
    volume: f32,
    available: bool,
    latency_ms: f64,
}

impl Default for NullOutput {
    fn default() -> Self {
        Self {
            volume: 1.0,
            available: true,
            latency_ms: 0.0,
        }
    }
}

impl NullOutput {
    pub fn new() -> Self {
        Self::default()
    }

    /// 创建一个不可用的 Null 输出（模拟无音频设备）
    pub fn unavailable() -> Self {
        Self {
            available: false,
            ..Default::default()
        }
    }
}

impl AudioOutput for NullOutput {
    fn init(&mut self, _sample_rate: u32, _channels: u16) -> Result<(), PlatformError> {
        Ok(())
    }

    fn write(&mut self, _data: &[i16]) -> Result<(), PlatformError> {
        Ok(())
    }

    fn pause(&mut self) -> Result<(), PlatformError> {
        Ok(())
    }

    fn resume(&mut self) -> Result<(), PlatformError> {
        Ok(())
    }

    fn set_volume(&mut self, volume: f32) -> Result<(), PlatformError> {
        self.volume = volume.clamp(0.0, 1.0);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), PlatformError> {
        Ok(())
    }

    fn close(&mut self) -> Result<(), PlatformError> {
        Ok(())
    }

    fn latency(&self) -> f64 {
        self.latency_ms / 1000.0
    }

    fn is_available(&self) -> bool {
        self.available
    }
}

/// 创建默认音频输出
pub fn default_output() -> Box<dyn AudioOutput> {
    #[cfg(target_os = "windows")]
    {
        Box::new(WasapiOutput::new())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Box::new(NullOutput::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_output_init_write_close() {
        let mut out = NullOutput::new();
        assert!(out.is_available());

        out.init(48000, 2).unwrap();
        out.write(&[0, 100, 200, -100]).unwrap();
        out.pause().unwrap();
        out.resume().unwrap();
        out.close().unwrap();
    }

    #[test]
    fn null_output_volume_clamps() {
        let mut out = NullOutput::new();
        out.set_volume(1.5).unwrap();
        assert!((out.volume - 1.0).abs() < 0.001);

        out.set_volume(-0.5).unwrap();
        assert!(out.volume.abs() < 0.001);
    }

    #[test]
    fn unavailable_output_reports_not_available() {
        let out = NullOutput::unavailable();
        assert!(!out.is_available());
    }

    #[test]
    fn default_output_creates_box() {
        let out = default_output();
        assert!(out.is_available());
    }

    #[test]
    fn null_output_latency() {
        let out = NullOutput::new();
        assert!(out.latency() >= 0.0);
    }
}
