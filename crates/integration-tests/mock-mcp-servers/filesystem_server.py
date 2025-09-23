#!/usr/bin/env python3
"""
Filesystem MCP server - manages files and directories with operations.
"""

import json
import sys
import asyncio
from typing import Dict, Any

class FileSystemMcpServer:
    def __init__(self):
        print("FileSystemMcpServer: Starting server initialization", file=sys.stderr, flush=True)
        self.tools = {
            "filesystem": {
                "name": "filesystem",
                "description": "Manages files and directories with operations like listing, creating, and deleting",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "File or directory path"
                        },
                        "operation": {
                            "type": "string",
                            "enum": ["list", "create", "delete", "exists"],
                            "description": "Filesystem operation to perform"
                        }
                    },
                    "required": ["path", "operation"]
                }
            }
        }
        print("FileSystemMcpServer: Server initialization complete", file=sys.stderr, flush=True)

    async def handle_message(self, message: Dict[str, Any]) -> Dict[str, Any]:
        method = message.get("method")
        params = message.get("params", {})
        msg_id = message.get("id")

        try:
            if method == "initialize":
                return {
                    "jsonrpc": "2.0",
                    "id": msg_id,
                    "result": {
                        "protocolVersion": "2025-03-26",
                        "capabilities": {"tools": {}},
                        "serverInfo": {"name": "filesystem-server", "version": "1.0.0"}
                    }
                }
            elif method == "notifications/initialized":
                return None
            elif method == "tools/list":
                return {
                    "jsonrpc": "2.0",
                    "id": msg_id,
                    "result": {"tools": list(self.tools.values())}
                }
            elif method == "tools/call":
                tool_name = params.get("name")
                arguments = params.get("arguments", {})

                if tool_name not in self.tools:
                    return {
                        "jsonrpc": "2.0",
                        "id": msg_id,
                        "error": {"code": -32602, "message": f"Unknown tool: {tool_name}"}
                    }

                result = await self.execute_tool(tool_name, arguments)
                return {"jsonrpc": "2.0", "id": msg_id, "result": result}
            else:
                return {
                    "jsonrpc": "2.0",
                    "id": msg_id,
                    "error": {"code": -32601, "message": f"Method not found: {method}"}
                }
        except Exception as e:
            return {
                "jsonrpc": "2.0",
                "id": msg_id,
                "error": {"code": -32603, "message": f"Internal error: {str(e)}"}
            }

    async def execute_tool(self, tool_name: str, arguments: Dict[str, Any]) -> Dict[str, Any]:
        if tool_name == "filesystem":
            path = arguments.get("path", "/tmp")
            operation = arguments.get("operation", "list")

            # Mock implementation for testing
            if operation == "list":
                result = f"Contents of {path}: file1.txt, file2.txt, directory1/"
            elif operation == "create":
                result = f"Created: {path}"
            elif operation == "delete":
                result = f"Deleted: {path}"
            elif operation == "exists":
                result = f"Path {path} exists: true"
            else:
                raise Exception(f"Unknown operation: {operation}")

            return {
                "content": [{
                    "type": "text",
                    "text": f"FileSystem: {result}"
                }]
            }
        else:
            raise Exception(f"Unknown tool: {tool_name}")

    async def run(self):
        print("FileSystemMcpServer: Starting main server loop", file=sys.stderr, flush=True)
        while True:
            try:
                line = await asyncio.get_event_loop().run_in_executor(None, sys.stdin.readline)
                if not line:
                    break

                line = line.strip()
                if not line:
                    continue

                try:
                    message = json.loads(line)
                except json.JSONDecodeError as e:
                    error_response = {
                        "jsonrpc": "2.0",
                        "id": None,
                        "error": {"code": -32700, "message": f"Parse error: {str(e)}"}
                    }
                    print(json.dumps(error_response), flush=True)
                    continue

                response = await self.handle_message(message)
                if response is not None:
                    print(json.dumps(response), flush=True)

            except KeyboardInterrupt:
                break
            except Exception as e:
                print(f"Error: {e}", file=sys.stderr, flush=True)

async def main():
    server = FileSystemMcpServer()
    await server.run()

if __name__ == "__main__":
    asyncio.run(main())