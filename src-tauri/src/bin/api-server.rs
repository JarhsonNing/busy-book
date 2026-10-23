use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let data_dir = std::env::var("TAOSHU_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data"));
    let port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8899);
    let state = taoshu_lib::state::AppState::new(data_dir, port)?;
    taoshu_lib::api::serve(state).await
}
