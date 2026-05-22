use clap::Parser;
use koochi::cli::Cli;
use koochi::cli::RunExit;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    match koochi::cli::run(cli).await {
        Ok(RunExit::Success) => std::process::exit(0),
        Ok(RunExit::TestFailures) => std::process::exit(1),
        Err(error) => {
            eprintln!("koochi: {error}");
            std::process::exit(2);
        }
    }
}
