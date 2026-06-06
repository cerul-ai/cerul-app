#[tokio::main]
async fn main() -> anyhow::Result<()> {
    cerul_api::serve().await
}
