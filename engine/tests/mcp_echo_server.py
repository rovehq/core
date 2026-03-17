#!/usr/bin/env python3
"""
Simple echo MCP server for testing JSON-RPC communication.

This server implements the MCP protocol by:
1. Reading JSON-RPC requests from stdin (one per line)
2. Processing the request (echo back the params)
3. Writing JSON-RPC responses to stdout (one per line)

Usage:
    python3 mcp_echo_server.py
"""

import sys
import json

def main():
    # Ensure line buffering for immediate I/O
    sys.stdout.reconfigure(line_buffering=True)
    sys.stderr.reconfigure(line_buffering=True)
    
    # Don't print to stderr - it might interfere with JSON-RPC
    
    for line in sys.stdin:
        try:
            # Parse JSON-RPC request
            request = json.loads(line.strip())
            
            # Validate JSON-RPC 2.0 format
            if request.get("jsonrpc") != "2.0":
                response = {
                    "jsonrpc": "2.0",
                    "id": request.get("id"),
                    "error": {
                        "code": -32600,
                        "message": "Invalid Request: jsonrpc must be '2.0'"
                    }
                }
            else:
                # Echo back the params as the result
                response = {
                    "jsonrpc": "2.0",
                    "id": request.get("id"),
                    "result": {
                        "method": request.get("method"),
                        "params": request.get("params"),
                        "echo": "success"
                    }
                }
            
            # Write response to stdout
            print(json.dumps(response), flush=True)
            
        except json.JSONDecodeError as e:
            # Invalid JSON
            error_response = {
                "jsonrpc": "2.0",
                "id": None,
                "error": {
                    "code": -32700,
                    "message": f"Parse error: {str(e)}"
                }
            }
            print(json.dumps(error_response), flush=True)
        except Exception as e:
            # Internal error
            error_response = {
                "jsonrpc": "2.0",
                "id": request.get("id") if 'request' in locals() else None,
                "error": {
                    "code": -32603,
                    "message": f"Internal error: {str(e)}"
                }
            }
            print(json.dumps(error_response), flush=True)

if __name__ == "__main__":
    main()
