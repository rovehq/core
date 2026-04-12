#!/bin/bash
# Test LocalBrain integration

echo "Testing LocalBrain with actual llama-server..."
echo ""

# Test with a simple prompt
echo "write a hello world function in rust" | ./target/debug/rove

echo ""
echo "Test complete!"
