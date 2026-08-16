use clap::Parser;

#[derive(Parser)]
#[command(
    name = "hk-steward",
    about = "Read-only local Brain Steward proposal service"
)]
struct Args {
    #[arg(long, default_value_t = 7071)]
    port: u16,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    hk_steward::serve(Args::parse().port).await
}
