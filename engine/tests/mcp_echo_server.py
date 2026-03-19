#!/usr/bin/env python3
"""Simple stdio MCP test server."""

import json
import sys


TOOLS = [
    {
        "name": "test_echo",
        "description": "Echo the provided arguments",
        "inputSchema": {
            "type": "object",
            "properties": {
                "message": {"type": "string"}
            }
        }
    },
    {
        "name": "test_multiple",
        "description": "Echo repeated test payloads",
        "inputSchema": {
            "type": "object",
            "properties": {
                "iteration": {"type": "integer"}
            }
        }
    },
    {
        "name": "test",
        "description": "Generic test tool",
        "inputSchema": {
            "type": "object",
            "properties": {}
        }
    },
]

def main():
    # Ensure line buffering for immediate I/O
    sys.stdout.reconfigure(line_buffering=True)
    sys.stderr.reconfigure(line_buffering=True)
    
    # Don't print to stderr - it might interfere with JSON-RPC
    
    for line in sys.stdin:
        try:
            # Parse JSON-RPC request
            request = json.loads(line.strip())
            
            if request.get("jsonrpc") != "2.0":
                response = {
                    "jsonrpc": "2.0",
                    "id": request.get("id"),
                    "error": {
                        "code": -32600,
                        "message": "Invalid Request: jsonrpc must be '2.0'"
                    }
                }
            elif request.get("method") == "initialize":
                response = {
                    "jsonrpc": "2.0",
                    "id": request.get("id"),
                    "result": {
                        "protocolVersion": request["params"].get("protocolVersion", "2024-11-05"),
                        "serverInfo": {
                            "name": "rove-test-echo",
                            "version": "0.1.0"
                        },
                        "capabilities": {
                            "tools": {}
                        }
                    }
                }
            elif request.get("method") == "notifications/initialized":
                continue
            elif request.get("method") == "tools/list":
                response = {
                    "jsonrpc": "2.0",
                    "id": request.get("id"),
                    "result": {
                        "tools": TOOLS
                    }
                }
            elif request.get("method") == "tools/call":
                tool_name = request.get("params", {}).get("name")
                arguments = request.get("params", {}).get("arguments", {})
                response = {
                    "jsonrpc": "2.0",
                    "id": request.get("id"),
                    "result": {
                        "content": [
                            {
                                "type": "text",
                                "text": "success"
                            }
                        ],
                        "structuredContent": {
                            "method": tool_name,
                            "params": arguments,
                            "echo": "success"
                        }
                    }
                }
            else:
                response = {
                    "jsonrpc": "2.0",
                    "id": request.get("id"),
                    "error": {
                        "code": -32601,
                        "message": f"Method not found: {request.get('method')}"
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
