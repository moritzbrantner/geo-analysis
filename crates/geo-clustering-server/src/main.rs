use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "geo-clustering-server",
    version,
    about = "Thin HTTP API adapter for geo-clustering"
)]
struct Args {
    /// Address to bind, for example 127.0.0.1:3000.
    #[arg(long, default_value = "127.0.0.1:3000")]
    addr: String,
}

fn main() -> std::io::Result<()> {
    let args = Args::parse();
    eprintln!("geo-clustering-server listening on http://{}", args.addr);
    geo_clustering_server::serve(&args.addr)
}
