use anyhow::Result;
use brain::reasoning::LocalBrain;

pub async fn run() -> Result<()> {
    println!("Checking for llama-server...");
    println!();

    match which::which("llama-server") {
        Ok(path) => println!("llama-server found: {}", path.display()),
        Err(_) => {
            println!("llama-server not found in PATH");
            println!();
            println!("Run `rove brain setup` for installation instructions.");
            return Ok(());
        }
    }

    let brain = LocalBrain::new("http://localhost:8080", "unknown");
    if brain.check_available().await {
        println!("llama-server is running at http://localhost:8080");
    } else {
        println!("llama-server is not running");
        println!();
        println!("Start it with: rove brain start");
    }

    Ok(())
}
