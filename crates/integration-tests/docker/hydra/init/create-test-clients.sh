#!/bin/bash

# Determine which Hydra instance to use
if [ -n "$HYDRA_ADMIN_URL" ]; then
  ADMIN_URL=$HYDRA_ADMIN_URL
  PUBLIC_URL=$HYDRA_PUBLIC_URL
else
  ADMIN_URL="http://hydra:4445"
  PUBLIC_URL="http://hydra:4444"
fi

# Wait for Hydra to be ready
echo "Waiting for Hydra to be ready at $PUBLIC_URL..."
until curl -s -f $PUBLIC_URL/.well-known/jwks.json > /dev/null; do
  echo "Waiting for Hydra..."
  sleep 1
done

echo "Hydra is ready, creating universal client..."

# Create universal client that can handle all test scenarios
CLIENT_ID="shared-test-client-universal"

# For Hydra 2, use different client ID to avoid conflicts
if [ "$ADMIN_URL" = "http://hydra-2:4455" ]; then
  CLIENT_ID="shared-hydra2-client-universal"
fi

echo "Creating universal client: $CLIENT_ID"

REQUEST_BODY="{
  \"client_id\": \"$CLIENT_ID\",
  \"client_secret\": \"$CLIENT_ID-secret\",
  \"grant_types\": [\"client_credentials\"],
  \"token_endpoint_auth_method\": \"client_secret_basic\",
  \"access_token_strategy\": \"jwt\",
  \"skip_consent\": true,
  \"skip_logout_consent\": true,
  \"audience\": [
    \"test-audience-1\",
    \"test-audience-2\",
    \"service-a\",
    \"correct-audience\",
    \"wrong-audience\",
    \"CaseSensitiveAudience\",
    \"casesensitiveaudience\",
    \"test-api\",
    \"https://api.example.com\",
    \"combined-test-audience\",
    \"test-service-audience\",
    \"http://127.0.0.1:8080\"
  ]
}"

curl -s -X POST $ADMIN_URL/admin/clients \
  -H "Content-Type: application/json" \
  -d "$REQUEST_BODY" || echo "Client $CLIENT_ID might already exist"

echo "Universal client created successfully!"

# Keep the container alive if needed for debugging
if [ "$1" = "debug" ]; then
  tail -f /dev/null
fi
