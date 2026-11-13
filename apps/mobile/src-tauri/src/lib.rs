use minerdesk_mobile_core::{ConnectionStatus, MobileClient};
use serde_json::Value;
use tauri::State;

#[tauri::command]
async fn mobile_connection_status(
    client: State<'_, MobileClient>,
) -> Result<ConnectionStatus, String> {
    Ok(client.status().await)
}

#[tauri::command]
async fn mobile_connect(
    address: String,
    key: String,
    allow_insecure_lan: bool,
    client: State<'_, MobileClient>,
) -> Result<ConnectionStatus, String> {
    client.connect(&address, &key, allow_insecure_lan).await
}

#[tauri::command]
async fn mobile_disconnect(client: State<'_, MobileClient>) -> Result<(), String> {
    client.disconnect().await;
    Ok(())
}

#[tauri::command]
async fn mobile_request(
    connection_id: String,
    path: String,
    method: String,
    body_json: Option<String>,
    timeout_ms: u64,
    client: State<'_, MobileClient>,
) -> Result<Value, String> {
    client
        .request(&connection_id, &path, &method, body_json, timeout_ms)
        .await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(MobileClient::default())
        .invoke_handler(tauri::generate_handler![
            mobile_connection_status,
            mobile_connect,
            mobile_disconnect,
            mobile_request
        ])
        .run(tauri::generate_context!())
        .expect("failed to run MinerDesk Mobile");
}
