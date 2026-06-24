use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand, Debug, Clone)]
pub enum MaturityCommand {
    Scan {
        #[clap(long)]
        features_path: Option<String>,
    },
}

pub async fn handle_maturity_command(_cmd: MaturityCommand) -> Result<()> {
    println!("Maturity CLI is being migrated to MaturityEngine.");
    Ok(())
}
