#!/usr/bin/env python3
"""
Simple MCP server for testing STDIO functionality.
This server implements a basic MCP protocol with a few test tools.
"""

import json
import sys
import asyncio
from typing import Dict, Any, List, Optional

class SimpleMcpServer:
    def __init__(self):
        # Log startup to stderr for testing file redirection
        print("SimpleMcpServer: Starting server initialization", file=sys.stderr, flush=True)
        self.tools = {
            "echo": {
                "name": "echo",
                "description": "Echoes back the input text",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "text": {
                            "type": "string",
                            "description": "Text to echo back"
                        }
                    },
                    "required": ["text"]
                }
            },
            "add": {
                "name": "add",
                "description": "Adds two numbers together",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "a": {
                            "type": "number",
                            "description": "First number"
                        },
                        "b": {
                            "type": "number",
                            "description": "Second number"
                        }
                    },
                    "required": ["a", "b"]
                }
            },
            "environment": {
                "name": "environment",
                "description": "Returns environment variable value",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "var": {
                            "type": "string",
                            "description": "Environment variable name"
                        }
                    },
                    "required": ["var"]
                }
            },
            "fail": {
                "name": "fail",
                "description": "Always fails for testing error handling",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            }
        }

        # Log completion of initialization to stderr
        print("SimpleMcpServer: Server initialization complete", file=sys.stderr, flush=True)

    async def handle_message(self, message: Dict[str, Any]) -> Dict[str, Any]:
        """Handle incoming MCP message"""
        method = message.get("method")
        params = message.get("params", {})
        msg_id = message.get("id")

        try:
            if method == "initialize":
                print(f"SimpleMcpServer: Handling initialize request", file=sys.stderr, flush=True)
                return {
                    "jsonrpc": "2.0",
                    "id": msg_id,
                    "result": {
                        "protocolVersion": "2025-03-26",
                        "capabilities": {
                            "tools": {}
                        },
                        "serverInfo": {
                            "name": "simple-test-server",
                            "version": "1.0.0"
                        }
                    }
                }

            elif method == "notifications/initialized":
                # No response needed for notifications
                return None

            elif method == "tools/list":
                return {
                    "jsonrpc": "2.0",
                    "id": msg_id,
                    "result": {
                        "tools": list(self.tools.values())
                    }
                }

            elif method == "tools/call":
                tool_name = params.get("name")
                arguments = params.get("arguments", {})

                if tool_name not in self.tools:
                    return {
                        "jsonrpc": "2.0",
                        "id": msg_id,
                        "error": {
                            "code": -32602,
                            "message": f"Unknown tool: {tool_name}"
                        }
                    }

                result = await self.execute_tool(tool_name, arguments)
                return {
                    "jsonrpc": "2.0",
                    "id": msg_id,
                    "result": result
                }

            else:
                return {
                    "jsonrpc": "2.0",
                    "id": msg_id,
                    "error": {
                        "code": -32601,
                        "message": f"Method not found: {method}"
                    }
                }

        except Exception as e:
            return {
                "jsonrpc": "2.0",
                "id": msg_id,
                "error": {
                    "code": -32603,
                    "message": f"Internal error: {str(e)}"
                }
            }

    async def execute_tool(self, tool_name: str, arguments: Dict[str, Any]) -> Dict[str, Any]:
        """Execute a tool and return the result"""
        import os

        if tool_name == "echo":
            text = arguments.get("text", "")
            return {
                "content": [
                    {
                        "type": "text",
                        "text": f"Echo: {text}"
                    }
                ]
            }

        elif tool_name == "add":
            a = arguments.get("a", 0)
            b = arguments.get("b", 0)
            result = a + b
            return {
                "content": [
                    {
                        "type": "text",
                        "text": f"{a} + {b} = {result}"
                    }
                ]
            }

        elif tool_name == "environment":
            var_name = arguments.get("var", "")
            value = os.environ.get(var_name, f"Environment variable '{var_name}' not found")
            return {
                "content": [
                    {
                        "type": "text",
                        "text": f"{var_name}={value}"
                    }
                ]
            }

        elif tool_name == "fail":
            raise Exception("This tool always fails")

        else:
            raise Exception(f"Unknown tool: {tool_name}")

    async def run(self):
        """Main server loop"""
        print("SimpleMcpServer: Starting main server loop", file=sys.stderr, flush=True)
        while True:
            try:
                # Read line from stdin
                line = await asyncio.get_event_loop().run_in_executor(None, sys.stdin.readline)
                if not line:
                    break

                line = line.strip()
                if not line:
                    continue

                # Parse JSON message
                try:
                    message = json.loads(line)
                except json.JSONDecodeError as e:
                    # Send error response for invalid JSON
                    error_response = {
                        "jsonrpc": "2.0",
                        "id": None,
                        "error": {
                            "code": -32700,
                            "message": f"Parse error: {str(e)}"
                        }
                    }
                    print(json.dumps(error_response), flush=True)
                    continue

                # Handle message
                response = await self.handle_message(message)

                # Send response if needed
                if response is not None:
                    print(json.dumps(response), flush=True)

            except KeyboardInterrupt:
                break
            except Exception as e:
                # Log error to stderr but continue running
                print(f"Error: {e}", file=sys.stderr, flush=True)

async def main():
    """Entry point"""
    server = SimpleMcpServer()
    await server.run()

if __name__ == "__main__":
    asyncio.run(main())
