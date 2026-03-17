use anyhow::Result;
use brain::reasoning::LocalBrain;

pub async fn run() -> Result<()> {
    println!("Verifying llama-server...");
    println!();

    let brain = LocalBrain::new("http://localhost:8080", "test");
    if brain.check_available().await {
        println!("llama-server is running and responding");
        println!("URL: http://localhost:8080");
    } else {
        println!("llama-server is not responding");
        println!();
        println!("Start it with: rove brain start");
    }

    Ok(())
}
