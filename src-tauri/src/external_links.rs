use tauri_plugin_opener::OpenerExt;

const RELEASES: &str = "https://github.com/jontechlabs/MinerDesk/releases/";

fn allowed_download_page(url: &str) -> bool {
    let Some(path) = url.strip_prefix(RELEASES) else { return false; };
    path == "latest" || regex::Regex::new(r"\Atag/v[0-9]+\.[0-9]+\.[0-9]+\z").unwrap().is_match(path)
}

#[tauri::command]
pub async fn open_github_downloads(app: tauri::AppHandle, url: String) -> Result<(), String> {
    if !allowed_download_page(&url) { return Err("Only MinerDesk GitHub release pages can be opened".into()); }
    tauri::async_runtime::spawn_blocking(move || app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string()))
        .await.map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_pages_are_allowed_but_foreign_urls_and_other_protocols_are_not() {
        for url in [format!("{RELEASES}latest"), format!("{RELEASES}tag/v0.7.29")] { assert!(allowed_download_page(&url)); }
        for url in [
            "javascript:alert(1)", "file:///etc/passwd", "https://example.com",
            "https://github.com/evil/MinerDesk/releases/latest",
            "https://github.com/jontechlabs/MinerDesk/releases/latest?redirect=evil",
            "https://github.com/jontechlabs/MinerDesk/releases/tag/v0.7.29\n",
            "https://github.com/jontechlabs/MinerDesk/releases/tag/v0.7.29#x",
            "https://github.com/jontechlabs/MinerDesk/releases/tag/v0.7.29/../../evil",
            "https://github.com@evil.example/jontechlabs/MinerDesk/releases/latest",
        ] { assert!(!allowed_download_page(url), "{url}"); }
    }
}
