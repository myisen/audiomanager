//! core-jni: Android JNI 绑定层（T101）
//!
//! 编译为 cdylib `libaudiosync_core.so`，放入 app-android 的 jniLibs。
//! Kotlin 侧通过 `com.audiosync.JniBridge` 调用。
//!
//! 设计原则：JNI 函数只做字符串转换，业务逻辑全部在
//! 平台无关的 [ffi] 模块中（可单元测试）。

pub mod ffi;

use jni::objects::{JClass, JString};
use jni::sys::{jstring, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;

/// 处理一条 JSON 请求，返回 JSON 响应
///
/// Kotlin: `external fun handleRequest(requestJson: String): String`
#[no_mangle]
pub extern "system" fn Java_com_audiosync_JniBridge_handleRequest(
    mut env: JNIEnv,
    _class: JClass,
    request: JString,
) -> jstring {
    let request: String = match env.get_string(&request) {
        Ok(s) => s.into(),
        Err(_) => {
            return new_jstring(&mut env, &error_json("无法读取请求字符串"));
        }
    };

    let response = ffi::handle_request(&request);
    new_jstring(&mut env, &response)
}

/// 写入 SAF 授权 URI（T102）
///
/// Kotlin: `external fun setSafUri(uri: String)`
#[no_mangle]
pub extern "system" fn Java_com_audiosync_JniBridge_setSafUri(
    mut env: JNIEnv,
    _class: JClass,
    uri: JString,
) {
    let uri: String = match env.get_string(&uri) {
        Ok(s) => s.into(),
        Err(_) => return,
    };
    ffi::set_saf_uri(uri);
}

/// 写入扫描结果（T102），playlists_json 为 `PlaylistInfo` 数组的 JSON
///
/// Kotlin: `external fun setPlaylists(playlistsJson: String)`
#[no_mangle]
pub extern "system" fn Java_com_audiosync_JniBridge_setPlaylists(
    mut env: JNIEnv,
    _class: JClass,
    playlists_json: JString,
) {
    let json: String = match env.get_string(&playlists_json) {
        Ok(s) => s.into(),
        Err(_) => return,
    };
    if let Ok(list) = serde_json::from_str::<Vec<core_ipc::PlaylistInfo>>(&json) {
        ffi::set_playlists(list);
    }
}

/// 查询前台服务是否应保持活跃（T103 配套查询）
///
/// Kotlin: `external fun isForegroundServiceActive(): Boolean`
#[no_mangle]
pub extern "system" fn Java_com_audiosync_JniBridge_isForegroundServiceActive(
    _env: JNIEnv,
    _class: JClass,
) -> jni::sys::jboolean {
    let active = ffi::with_state(|s| s.foreground_service_active);
    if active {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

fn new_jstring(env: &mut JNIEnv, s: &str) -> jstring {
    match env.new_string(s) {
        Ok(j) => j.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

fn error_json(message: &str) -> String {
    serde_json::json!({ "type": "error", "data": { "message": message } }).to_string()
}
