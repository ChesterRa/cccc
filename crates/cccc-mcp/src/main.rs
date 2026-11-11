use anyhow::Result;
use cccc_core::HomeLayout;

#[tokio::main]
async fn main() -> Result<()> {
    cccc_core::build_info::initialize(env!("CCCC_SOURCE_ID"));
    cccc_mcp::run_stdio(HomeLayout::resolve()?).await
}
