use clap::Parser;

#[derive(Parser)]
#[command(name = "prism-import")]
#[command(about = "Import data from external search engines into Prism")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Import from Elasticsearch
    Es {
        #[arg(long)]
        source: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Es { source } => {
            println!("Would import from: {}", source);
        }
    }

    Ok(())
}
