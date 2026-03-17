//! Example demonstrating ToolInput and ToolOutput usage

use sdk::{ToolInput, ToolOutput};
use serde_json::json;

fn main() {
    // Example 1: Creating a ToolInput with parameters
    let input = ToolInput::new("read_file")
        .with_param("path", json!("/home/user/file.txt"))
        .with_param("encoding", json!("utf-8"));

    println!("Created ToolInput: {:?}", input);

    // Example 2: Extracting parameters
    match input.param_str("path") {
        Ok(path) => println!("Path parameter: {}", path),
        Err(e) => println!("Error: {}", e),
    }

    // Example 3: Optional parameters
    if let Some(encoding) = input.param_str_opt("encoding") {
        println!("Encoding: {}", encoding);
    }

    // Example 4: Creating successful output
    let success_output = ToolOutput::text("File contents here");
    println!("Success output: {}", success_output.to_json());

    // Example 5: Creating JSON output
    let json_output = ToolOutput::json(json!({
        "files": ["file1.txt", "file2.txt"],
        "count": 2
    }));
    println!("JSON output: {}", json_output.to_json());

    // Example 6: Creating error output
    let error_output = ToolOutput::error("File not found");
    println!("Error output: {}", error_output.to_json());

    // Example 7: Serialization round-trip
    let original = ToolInput::new("test_method").with_param("key", json!("value"));

    let serialized = serde_json::to_string(&original).unwrap();
    let deserialized: ToolInput = serde_json::from_str(&serialized).unwrap();

    println!("\nSerialization test:");
    println!("Original method: {}", original.method);
    println!("Deserialized method: {}", deserialized.method);
    println!("Match: {}", original.method == deserialized.method);
}
