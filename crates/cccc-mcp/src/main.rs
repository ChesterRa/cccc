use anyhow::Result;
use cccc_core::HomeLayout;

#[tokio::main]
async fn main() -> Result<()> {
    cccc_mcp::run_stdio(HomeLayout::resolve()?).await
}
