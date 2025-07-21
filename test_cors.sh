#!/bin/bash

# Test CORS OPTIONS handling for Nexus MCP endpoint

echo "Starting Nexus server..."
cargo run -p nexus -- --config test.toml --listen-address 127.0.0.1:8000 &
SERVER_PID=$!

# Give the server time to start
echo "Waiting for server to start..."
sleep 5

# Test OPTIONS request with CORS headers
echo -e "\n=== Testing OPTIONS request to /mcp ==="
curl -v -X OPTIONS http://localhost:8000/mcp \
  -H "Origin: http://localhost:5173" \
  -H "Access-Control-Request-Method: POST" \
  -H "Access-Control-Request-Headers: content-type" \
  2>&1 | grep -E "(< HTTP|< Access-Control-|Method Not Allowed)"

# Test regular POST request
echo -e "\n=== Testing POST request to /mcp ==="
curl -v -X POST http://localhost:8000/mcp \
  -H "Origin: http://localhost:5173" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"1.0.0"},"id":1}' \
  2>&1 | grep -E "(< HTTP|< Access-Control-)"

# Clean up
echo -e "\nStopping server..."
kill $SERVER_PID
wait $SERVER_PID 2>/dev/null

echo "Test complete!"
