use anyhow::Result;

pub fn show() -> Result<()> {
    println!("llama.cpp installation instructions");
    println!();
    println!("macOS:");
    println!("  brew install llama.cpp");
    println!();
    println!("Linux (Ubuntu/Debian):");
    println!("  git clone https://github.com/ggerganov/llama.cpp");
    println!("  cd llama.cpp");
    println!("  make");
    println!("  sudo cp llama-server /usr/local/bin/");
    println!();
    println!("Windows:");
    println!("  Download a release build from:");
    println!("  https://github.com/ggerganov/llama.cpp/releases");
    println!();
    println!("After installation, verify with:");
    println!("  rove brain check");
    Ok(())
}
