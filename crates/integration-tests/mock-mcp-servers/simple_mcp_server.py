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
        """Handle incoming MCP message or TransportMessage"""
        # Check if this is a TransportMessage wrapper from pmcp
        if "request" in message and "id" in message and not "jsonrpc" in message:
            # Extract the actual JSON-RPC message from the TransportMessage
            msg_id = message["id"]
            inner_request = message["request"]
            method = inner_request.get("method")
            params = inner_request.get("params", {})
        else:
            # Standard JSON-RPC message
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

    async def read_message(self):
        """Read an LSP-style message with Content-Length header"""
        # Read headers
        content_length = None
        while True:
            line = await asyncio.get_event_loop().run_in_executor(None, sys.stdin.readline)
            if not line:
                return None
            
            line = line.strip()
            if not line:
                # Empty line marks end of headers
                break
            
            # Parse header
            if line.startswith("Content-Length:"):
                content_length = int(line.split(":", 1)[1].strip())
        
        if content_length is None:
            print("Missing Content-Length header", file=sys.stderr, flush=True)
            return None
        
        # Read the message body
        body = await asyncio.get_event_loop().run_in_executor(
            None, sys.stdin.read, content_length
        )
        
        return json.loads(body)
    
    def send_message(self, message):
        """Send an LSP-style message with Content-Length header"""
        # Wrap response in TransportMessage format for pmcp compatibility
        # The message should already be a proper JSON-RPC response
        if "jsonrpc" in message and "id" in message:
            # This is a response - wrap it as TransportMessage::Response
            wrapped_message = message  # pmcp expects the response directly
        else:
            wrapped_message = message
            
        body = json.dumps(wrapped_message)
        content_length = len(body.encode('utf-8'))
        sys.stdout.write(f"Content-Length: {content_length}\r\n\r\n")
        sys.stdout.write(body)
        sys.stdout.flush()
    
    async def run(self):
        """Main server loop"""
        print("SimpleMcpServer: Starting main server loop", file=sys.stderr, flush=True)
        while True:
            try:
                # Read LSP-style message
                message = await self.read_message()
                if message is None:
                    break

                # Handle message
                response = await self.handle_message(message)

                # Send response if needed
                if response is not None:
                    self.send_message(response)

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
