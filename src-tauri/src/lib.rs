mod agents;
mod api;

#[tauri::command]
fn get_launcher_session() -> Result<Option<api::LauncherSession>, String> {
    api::get_launcher_session()
}

#[tauri::command]
async fn clear_launcher_session() -> Result<(), String> {
    api::clear_launcher_session().await
}

#[tauri::command]
async fn begin_device_authorization() -> Result<api::DeviceCode, String> {
    api::begin_device_authorization().await
}

#[tauri::command]
async fn poll_device_authorization(
    device_code: api::DeviceCode,
) -> Result<api::DeviceAuthorizationPoll, String> {
    api::poll_device_authorization(device_code).await
}

#[tauri::command]
fn detect_agents() -> Result<Vec<agents::AgentDetection>, String> {
    agents::detect_agents()
}

#[tauri::command]
fn configure_agent(config: agents::AgentConfiguration) -> Result<agents::ConfigureResult, String> {
    agents::configure_agent(config)
}

#[tauri::command]
fn validate_agent(agent_id: String) -> Result<agents::ValidationResult, String> {
    agents::validate_agent(agent_id)
}

#[tauri::command]
async fn get_account_snapshot() -> Result<api::AccountSnapshot, String> {
    api::get_account_snapshot().await
}

#[tauri::command]
async fn create_api_key(group_type: String) -> Result<api::CreatedApiKey, String> {
    api::create_api_key(group_type).await
}

#[tauri::command]
async fn delete_api_key(prefix: String) -> Result<(), String> {
    api::delete_api_key(prefix).await
}

#[tauri::command]
async fn regenerate_api_key(prefix: String) -> Result<api::CreatedApiKey, String> {
    api::regenerate_api_key(prefix).await
}

#[tauri::command]
async fn check_for_update() -> Result<api::UpdateStatus, String> {
    api::check_for_update().await
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_launcher_session,
            clear_launcher_session,
            begin_device_authorization,
            poll_device_authorization,
            detect_agents,
            configure_agent,
            validate_agent,
            get_account_snapshot,
            create_api_key,
            delete_api_key,
            regenerate_api_key,
            check_for_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Accly Launcher");
}
