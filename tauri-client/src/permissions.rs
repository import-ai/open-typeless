use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum Permission {
    Microphone,
    Accessibility,
    Bluetooth,
    InputMonitoring,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    NotDetermined,
    Restricted,
    Denied,
    Granted,
    NotGranted,
    Unsupported,
    Unknown,
}

#[cfg(any(target_os = "macos", test))]
fn status_from_native(value: i32) -> Status {
    match value {
        0 => Status::NotDetermined,
        1 => Status::Restricted,
        2 => Status::Denied,
        3 => Status::Granted,
        4 => Status::NotGranted,
        5 => Status::Unsupported,
        _ => Status::Unknown,
    }
}

#[derive(Serialize)]
pub struct PermissionState {
    id: Permission,
    status: Status,
}

#[cfg(target_os = "macos")]
extern "C" {
    fn ot_permission_status(permission: u32) -> i32;
    fn ot_request_permission(permission: u32) -> bool;
    fn ot_request_startup_permissions();
}

pub fn request_startup_permissions(app: &tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    app.run_on_main_thread(|| unsafe { ot_request_startup_permissions() }).map_err(|error| error.to_string())?;
    #[cfg(not(target_os = "macos"))]
    let _ = app;
    Ok(())
}

#[tauri::command]
pub fn get_permissions() -> Vec<PermissionState> {
    [Permission::Microphone, Permission::Accessibility, Permission::Bluetooth, Permission::InputMonitoring]
        .into_iter().map(|id| {
            #[cfg(target_os = "macos")]
            let status = status_from_native(unsafe { ot_permission_status(id as u32) });
            #[cfg(not(target_os = "macos"))]
            let status = if matches!(id, Permission::Microphone) { Status::Unknown } else { Status::Unsupported };
            PermissionState { id, status }
        }).collect()
}

#[tauri::command]
pub async fn request_permission(app: tauri::AppHandle, permission: Permission) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let (send, receive) = tokio::sync::oneshot::channel();
        app.run_on_main_thread(move || {
            let _ = send.send(unsafe { ot_request_permission(permission as u32) });
        }).map_err(|error| error.to_string())?;
        if receive.await.map_err(|error| error.to_string())? { Ok(()) }
        else { Err("无法打开系统授权设置，请在系统设置 → 隐私与安全性中授权".into()) }
    }
    #[cfg(target_os = "windows")]
    {
        let _ = app;
        if !matches!(permission, Permission::Microphone) { return Err("Windows 不需要此授权项".into()); }
        tauri_plugin_opener::open_url("ms-settings:privacy-microphone", None::<&str>).map_err(|error| error.to_string())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    { let _ = (app, permission); Err("当前平台暂不支持权限引导".into()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_statuses_preserve_denial_restrictions_and_unknown_values() {
        assert_eq!(status_from_native(0), Status::NotDetermined);
        assert_eq!(status_from_native(1), Status::Restricted);
        assert_eq!(status_from_native(2), Status::Denied);
        assert_eq!(status_from_native(3), Status::Granted);
        assert_eq!(status_from_native(4), Status::NotGranted);
        assert_eq!(status_from_native(99), Status::Unknown);
    }

    #[test]
    fn permission_requests_accept_only_known_permissions() {
        assert!(serde_json::from_str::<Permission>("\"input_monitoring\"").is_ok());
        assert!(serde_json::from_str::<Permission>("\"file:///tmp/arbitrary\"").is_err());
    }
}
