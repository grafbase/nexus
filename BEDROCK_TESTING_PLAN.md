# Amazon Bedrock Integration Testing Plan

## Quick Start for AWS Beginners

### 🎯 What You'll Need
1. **AWS Account** (credit card required)
2. **About $5-10** for initial testing (Bedrock is not free)
3. **30-45 minutes** to set everything up
4. **Basic command line knowledge**

### ⚠️ Cost Warning Upfront
**Bedrock charges per token (piece of text).** For initial testing:
- **Budget needed**: $5-10 for comprehensive testing
- **Cheapest model**: Amazon Titan Express (~$0.0003 per 1K tokens)
- **Safe testing**: Claude 3 Haiku (~$0.00025 per 1K input tokens)
- **Avoid**: Claude 3 Opus, Llama 70B (10x more expensive)

### 💡 Pro Tips for Beginners
- Start with **Amazon Titan** models (cheapest, instant approval)
- Test in **us-east-1** region (most models available)
- Set up **AWS Budget Alert** at $10 (see below)
- Use **IAM user**, never root account
- Save credentials in a password manager immediately

## Prerequisites

### 1. AWS Account Setup

#### Step 1.1: Create AWS Account
1. Go to https://aws.amazon.com
2. Click "Create an AWS Account"
3. Follow the signup process (requires credit card)
4. ⚠️ **IMPORTANT**: AWS offers a free tier, but Bedrock is NOT included. You will be charged for usage.

#### Step 1.2: Secure Your Root Account
1. **Enable MFA immediately**:
   - Go to IAM → Security credentials
   - Set up Multi-Factor Authentication
2. **Create an IAM user** (don't use root account):
   - Go to IAM → Users → Create user
   - Username: `bedrock-testing`
   - Check "Provide user access to the AWS Management Console"
   - Create an access key for CLI access (save these!)

### 2. Enable Bedrock Model Access (Step-by-Step)

#### Step 2.1: Navigate to Bedrock
1. **Sign in to AWS Console**: https://console.aws.amazon.com
2. **Select Region** (CRITICAL - top right corner):
   - Choose `US East (N. Virginia) us-east-1` for most models
   - Or `US West (Oregon) us-west-2` as alternative
   - ⚠️ **Not all regions have Bedrock!**
3. **Search for "Bedrock"** in the search bar
4. Click on **"Amazon Bedrock"**

#### Step 2.2: Request Model Access
1. In Bedrock console, click **"Model access"** in the left sidebar
2. You'll see a list of available models with their access status
3. Click the **"Manage model access"** button (orange button, top right)

#### Step 2.3: Select Models to Enable

**For testing, request these models** (check the boxes):

📝 **Free to Enable** (No charges until you use them):

**Anthropic Claude Models:**
- ✅ Claude 3 Haiku (cheapest Claude model - good for testing)
- ✅ Claude 3 Sonnet (balanced price/performance)
- ✅ Claude Instant (older but cheaper)
- ⚠️ Skip Claude 3 Opus (most expensive)

**Amazon Titan Models:**
- ✅ Titan Text Express (cheap, good for testing)
- ✅ Titan Text Lite (even cheaper)

**Meta Llama Models:**
- ✅ Llama 3 8B Instruct (cheaper)
- ⚠️ Llama 3 70B Instruct (more expensive, skip initially)

**Mistral Models:**
- ✅ Mistral 7B Instruct (cheaper)
- ⚠️ Mixtral 8x7B (more expensive, skip initially)

**Cohere Models:**
- ✅ Command Light (cheaper)
- ⚠️ Command (more expensive, skip initially)

**AI21 Models** (optional - no streaming):
- ⚠️ Skip these initially (limited features)

4. After selecting, click **"Request model access"** at the bottom
5. **Access is usually granted instantly** for most models
6. Some models may show **"Submit use case"** - these require manual approval (can take 1-2 days)

#### Step 2.4: Verify Access
1. After submission, the page will refresh
2. Models should show **"Access granted"** status
3. If a model shows **"Available to request"**, you need to click it again
4. Titan models are usually **auto-approved immediately**
5. Claude and Llama models typically approve within minutes

### 3. Get Your AWS Credentials

#### Step 3.1: Create Access Keys (For Nexus Testing)

1. Go to **IAM** service (search for IAM in AWS Console)
2. Click **Users** → Click your username
3. Click **Security credentials** tab
4. Under **Access keys**, click **"Create access key"**
5. Select **"Command Line Interface (CLI)"**
6. Check the confirmation box and click **"Next"**
7. Add a description: "Nexus Bedrock Testing"
8. Click **"Create access key"**
9. **CRITICAL**: Save both keys immediately!
   ```
   Access key ID: AKIA...
   Secret access key: [long string]
   ```
10. **⚠️ You won't see the secret key again!**

#### Step 3.2: Set Permissions for Your IAM User

1. Still in IAM → Users → Your user
2. Click **"Add permissions"** → **"Attach policies directly"**
3. Search for and select:
   - `AmazonBedrockFullAccess` (for Bedrock)
   - Optional: `CloudWatchLogsReadOnlyAccess` (for debugging)
4. Click **"Next"** → **"Add permissions"**

### 4. Set Up Cost Controls (IMPORTANT!)

#### Step 4.1: Create a Budget Alert
Protect yourself from unexpected charges:

1. Go to **AWS Budgets** (search in console)
2. Click **"Create budget"**
3. Choose **"Cost budget - Recommended"**
4. Set your budget:
   - Budget name: `Bedrock-Testing-Limit`
   - Period: **Monthly**
   - Budget amount: **$10.00** (adjust as needed)
5. Set alert thresholds:
   - Alert at **50%** of budget ($5)
   - Alert at **80%** of budget ($8)
   - Alert at **100%** of budget ($10)
6. Enter your email for notifications
7. Click **"Create budget"**

#### Step 4.2: Monitor Your Spending
- Check AWS Cost Explorer daily during testing
- View Bedrock-specific costs: Cost Explorer → Filter by Service → Bedrock
- Stop testing immediately if you hit your budget

### 5. Install AWS CLI (For Testing)

#### On macOS:
```bash
# Using Homebrew
brew install awscli

# Or download from AWS
curl "https://awscli.amazonaws.com/AWSCLIV2.pkg" -o "AWSCLIV2.pkg"
sudo installer -pkg AWSCLIV2.pkg -target /
```

#### On Linux:
```bash
curl "https://awscli.amazonaws.com/awscli-exe-linux-x86_64.zip" -o "awscliv2.zip"
unzip awscliv2.zip
sudo ./aws/install
```

#### On Windows:
Download and run: https://awscli.amazonaws.com/AWSCLIV2.msi

#### Configure AWS CLI:
```bash
aws configure
# Enter your credentials:
AWS Access Key ID [None]: AKIA...your-key...
AWS Secret Access Key [None]: your-secret-key
Default region name [None]: us-east-1
Default output format [None]: json
```

#### Test AWS CLI Access:
```bash
# List available Bedrock models (confirms credentials work)
aws bedrock list-foundation-models --region us-east-1 | grep modelId

# Test a model directly (costs ~$0.001)
echo '{"messages":[{"role":"user","content":"Say hello"}],"max_tokens":10}' > test.json
aws bedrock-runtime invoke-model \
  --model-id amazon.titan-text-express-v1 \
  --body file://test.json \
  --region us-east-1 \
  response.json
cat response.json | jq
```

### 5. AWS Credentials Configuration for Nexus

Choose ONE of these methods:

#### Option A: Environment Variables
```bash
export AWS_ACCESS_KEY_ID="your-access-key"
export AWS_SECRET_ACCESS_KEY="your-secret-key"
export AWS_REGION="us-east-1"
```

#### Option B: AWS Profile
```bash
# ~/.aws/config
[profile bedrock-test]
region = us-east-1

# ~/.aws/credentials
[bedrock-test]
aws_access_key_id = your-access-key
aws_secret_access_key = your-secret-key
```

#### Option C: IAM Role (for EC2/ECS)
Ensure the role has the `AmazonBedrockFullAccess` policy attached.

## Minimum Viable Test (Start Here!)

If you're overwhelmed, just do this minimal test first:

### Step 1: Simple Config File
Create `config/bedrock-minimal.toml`:

```toml
[server]
host = "127.0.0.1"
port = 8080

[llm]
enabled = true

# Just one provider with one model
[llm.providers.bedrock]
type = "bedrock"
region = "us-east-1"

# Start with Titan - it's cheap and auto-approves
[llm.providers.bedrock.models.titan]
rename = "amazon.titan-text-express-v1"
```

### Step 2: Set Your Credentials
```bash
export AWS_ACCESS_KEY_ID="your-access-key-here"
export AWS_SECRET_ACCESS_KEY="your-secret-key-here"
export AWS_REGION="us-east-1"
```

### Step 3: Start Nexus
```bash
cargo run --release -- --config config/bedrock-minimal.toml
```

### Step 4: Test One Request
```bash
curl -X POST http://localhost:8080/llm/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "bedrock/titan",
    "messages": [{"role": "user", "content": "Say hello"}],
    "max_tokens": 10
  }'
```

**Expected cost**: Less than $0.001

If this works, you're ready for more comprehensive testing!

## Test Configuration Files

### 1. Basic Configuration Test
Create `config/bedrock-basic.toml`:

```toml
[server]
host = "127.0.0.1"
port = 8080

[llm]
enabled = true

# Test with environment variables (Option A)
[llm.providers.bedrock-claude]
type = "bedrock"
region = "us-east-1"

[llm.providers.bedrock-claude.models.claude-3-sonnet]
rename = "anthropic.claude-3-sonnet-20240229-v1:0"

[llm.providers.bedrock-claude.models.claude-3-haiku]
rename = "anthropic.claude-3-haiku-20240307-v1:0"
```

### 2. Multi-Region Configuration
Create `config/bedrock-multi-region.toml`:

```toml
[server]
host = "127.0.0.1"
port = 8080

[llm]
enabled = true

# US East region - Claude models
[llm.providers.bedrock-us-east]
type = "bedrock"
region = "us-east-1"

[llm.providers.bedrock-us-east.models.claude]
rename = "anthropic.claude-3-sonnet-20240229-v1:0"

[llm.providers.bedrock-us-east.models.titan]
rename = "amazon.titan-text-express-v1"

# US West region - Llama models
[llm.providers.bedrock-us-west]
type = "bedrock"
region = "us-west-2"

[llm.providers.bedrock-us-west.models.llama-70b]
rename = "meta.llama3-70b-instruct-v1:0"

[llm.providers.bedrock-us-west.models.mistral]
rename = "mistral.mistral-7b-instruct-v0:2"
```

### 3. Full Model Test Configuration
Create `config/bedrock-all-models.toml`:

```toml
[server]
host = "127.0.0.1"
port = 8080

[llm]
enabled = true

[llm.providers.bedrock]
type = "bedrock"
region = "us-east-1"

# Anthropic models
[llm.providers.bedrock.models.claude-sonnet]
rename = "anthropic.claude-3-sonnet-20240229-v1:0"

[llm.providers.bedrock.models.claude-haiku]
rename = "anthropic.claude-3-haiku-20240307-v1:0"

[llm.providers.bedrock.models.claude-instant]
rename = "anthropic.claude-instant-v1"

# Amazon Titan models
[llm.providers.bedrock.models.titan-express]
rename = "amazon.titan-text-express-v1"

[llm.providers.bedrock.models.titan-lite]
rename = "amazon.titan-text-lite-v1"

# Meta Llama models
[llm.providers.bedrock.models.llama-70b]
rename = "meta.llama3-70b-instruct-v1:0"

[llm.providers.bedrock.models.llama-8b]
rename = "meta.llama3-8b-instruct-v1:0"

# Mistral models
[llm.providers.bedrock.models.mistral-7b]
rename = "mistral.mistral-7b-instruct-v0:2"

[llm.providers.bedrock.models.mixtral]
rename = "mistral.mixtral-8x7b-instruct-v0:1"

# Cohere models
[llm.providers.bedrock.models.command]
rename = "cohere.command-text-v14"

[llm.providers.bedrock.models.command-light]
rename = "cohere.command-light-text-v14"

# AI21 models (no streaming support)
[llm.providers.bedrock.models.j2-ultra]
rename = "ai21.j2-ultra-v1"
```

## Testing Steps

### Step 1: Start Nexus Server
```bash
# Build in release mode for performance testing
cargo build --release

# Start with basic configuration
./target/release/nexus --config config/bedrock-basic.toml
```

### Step 2: Model Listing Test
```bash
# Test model listing endpoint
curl http://localhost:8080/llm/models | jq

# Expected: List of all configured models with correct naming
```

### Step 3: Basic Completion Tests

#### Test Each Model Family
```bash
# Claude (Anthropic)
curl -X POST http://localhost:8080/llm/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "bedrock/claude-sonnet",
    "messages": [
      {"role": "user", "content": "Say hello in one sentence."}
    ],
    "max_tokens": 50
  }' | jq

# Titan (Amazon)
curl -X POST http://localhost:8080/llm/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "bedrock/titan-express",
    "messages": [
      {"role": "user", "content": "What is 2+2?"}
    ],
    "max_tokens": 50
  }' | jq

# Llama (Meta)
curl -X POST http://localhost:8080/llm/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "bedrock/llama-70b",
    "messages": [
      {"role": "user", "content": "Explain quantum computing in one sentence."}
    ],
    "max_tokens": 100
  }' | jq

# Mistral
curl -X POST http://localhost:8080/llm/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "bedrock/mistral-7b",
    "messages": [
      {"role": "user", "content": "Write a haiku about coding."}
    ],
    "max_tokens": 50
  }' | jq

# Cohere
curl -X POST http://localhost:8080/llm/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "bedrock/command",
    "messages": [
      {"role": "user", "content": "List three colors."}
    ],
    "max_tokens": 50
  }' | jq
```

### Step 4: Streaming Tests

Create `test-streaming.sh`:
```bash
#!/bin/bash

echo "Testing streaming for Claude..."
curl -X POST http://localhost:8080/llm/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "bedrock/claude-sonnet",
    "messages": [{"role": "user", "content": "Count from 1 to 5 slowly."}],
    "stream": true,
    "max_tokens": 100
  }' --no-buffer

echo -e "\n\nTesting streaming for Titan..."
curl -X POST http://localhost:8080/llm/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "bedrock/titan-express",
    "messages": [{"role": "user", "content": "Tell me a short story."}],
    "stream": true,
    "max_tokens": 150
  }' --no-buffer

echo -e "\n\nTesting streaming for Llama..."
curl -X POST http://localhost:8080/llm/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "bedrock/llama-70b",
    "messages": [{"role": "user", "content": "Explain the water cycle."}],
    "stream": true,
    "max_tokens": 150
  }' --no-buffer
```

### Step 5: System Message Tests

```bash
# Test system messages with Claude
curl -X POST http://localhost:8080/llm/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "bedrock/claude-sonnet",
    "messages": [
      {"role": "system", "content": "You are a pirate. Always speak like a pirate."},
      {"role": "user", "content": "How is the weather today?"}
    ],
    "max_tokens": 100
  }' | jq

# Test with Titan (system messages handled differently)
curl -X POST http://localhost:8080/llm/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "bedrock/titan-express",
    "messages": [
      {"role": "system", "content": "You are a helpful assistant."},
      {"role": "user", "content": "What is the capital of France?"}
    ],
    "max_tokens": 50
  }' | jq
```

### Step 6: Parameter Tests

```bash
# Test temperature parameter
for temp in 0.0 0.5 1.0; do
  echo "Testing temperature=$temp"
  curl -X POST http://localhost:8080/llm/chat/completions \
    -H "Content-Type: application/json" \
    -d "{
      \"model\": \"bedrock/claude-sonnet\",
      \"messages\": [{\"role\": \"user\", \"content\": \"Generate a random word.\"}],
      \"temperature\": $temp,
      \"max_tokens\": 20
    }" | jq -r '.choices[0].message.content'
done

# Test max_tokens
curl -X POST http://localhost:8080/llm/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "bedrock/titan-express",
    "messages": [{"role": "user", "content": "Count from 1 to 100."}],
    "max_tokens": 10
  }' | jq

# Test top_p parameter
curl -X POST http://localhost:8080/llm/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "bedrock/llama-70b",
    "messages": [{"role": "user", "content": "Write a creative sentence."}],
    "top_p": 0.9,
    "temperature": 0.8,
    "max_tokens": 50
  }' | jq
```

### Step 7: Error Handling Tests

```bash
# Test invalid model
curl -X POST http://localhost:8080/llm/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "bedrock/invalid-model",
    "messages": [{"role": "user", "content": "Test"}]
  }' | jq

# Test empty messages
curl -X POST http://localhost:8080/llm/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "bedrock/claude-sonnet",
    "messages": []
  }' | jq

# Test exceeding token limit (if quota allows)
curl -X POST http://localhost:8080/llm/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "bedrock/claude-sonnet",
    "messages": [{"role": "user", "content": "Write a very long essay."}],
    "max_tokens": 4000
  }' | jq
```

### Step 8: Concurrent Request Test

Create `test-concurrent.sh`:
```bash
#!/bin/bash

# Test concurrent requests to same model
for i in {1..5}; do
  curl -X POST http://localhost:8080/llm/chat/completions \
    -H "Content-Type: application/json" \
    -d "{
      \"model\": \"bedrock/claude-sonnet\",
      \"messages\": [{\"role\": \"user\", \"content\": \"Say number $i\"}],
      \"max_tokens\": 10
    }" &
done
wait
```

### Step 9: Multi-Region Test

```bash
# Start with multi-region config
./target/release/nexus --config config/bedrock-multi-region.toml

# Test US East model
curl -X POST http://localhost:8080/llm/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "bedrock-us-east/claude",
    "messages": [{"role": "user", "content": "Which region are you in?"}],
    "max_tokens": 50
  }' | jq

# Test US West model
curl -X POST http://localhost:8080/llm/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "bedrock-us-west/llama-70b",
    "messages": [{"role": "user", "content": "Hello from the west!"}],
    "max_tokens": 50
  }' | jq
```

## Performance Testing

### Load Testing Script
Create `load-test.sh`:
```bash
#!/bin/bash

MODEL="bedrock/claude-haiku"  # Use cheaper/faster model for load testing
ENDPOINT="http://localhost:8080/llm/chat/completions"
CONCURRENT=10
REQUESTS=100

echo "Load testing with $CONCURRENT concurrent requests, $REQUESTS total"

seq 1 $REQUESTS | xargs -P $CONCURRENT -I {} curl -s -X POST $ENDPOINT \
  -H "Content-Type: application/json" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"Test request {}\"}],
    \"max_tokens\": 20
  }" -w "\n%{http_code} %{time_total}s\n" -o /dev/null

echo "Load test complete"
```

## Validation Checklist

### Basic Functionality
- [ ] All configured models appear in `/llm/models` endpoint
- [ ] Each model family returns valid responses
- [ ] Response format matches OpenAI API structure
- [ ] Usage statistics are included in responses

### Streaming
- [ ] Streaming works for Claude models
- [ ] Streaming works for Titan models
- [ ] Streaming works for Llama models
- [ ] Streaming works for Mistral models
- [ ] Streaming works for Cohere models
- [ ] AI21 models correctly return streaming not supported error
- [ ] Stream chunks arrive incrementally (not all at once)
- [ ] Final chunk includes usage statistics (where available)

### Parameters
- [ ] `temperature` affects response randomness
- [ ] `max_tokens` limits response length
- [ ] `top_p` parameter is respected
- [ ] `stop` sequences work (where supported)

### Error Handling
- [ ] Invalid model returns 404
- [ ] Empty messages return 400
- [ ] Rate limiting returns 429 (when hit)
- [ ] Invalid credentials return 401
- [ ] Network errors are handled gracefully

### Authentication Methods
- [ ] Environment variables work
- [ ] AWS profile works
- [ ] IAM role works (if testing on EC2)
- [ ] Explicit credentials in config work

### Multi-Region
- [ ] Different regions can be configured
- [ ] Models route to correct regions
- [ ] Region-specific models work correctly

## Cost Monitoring

⚠️ **IMPORTANT**: Bedrock charges per token. Monitor your AWS billing dashboard.

Approximate costs (as of 2024):
- Claude 3 Sonnet: ~$3 per 1M input tokens, ~$15 per 1M output tokens
- Claude 3 Haiku: ~$0.25 per 1M input tokens, ~$1.25 per 1M output tokens
- Titan Express: ~$0.30 per 1M input tokens, ~$0.40 per 1M output tokens
- Llama 3 70B: ~$2.65 per 1M input tokens, ~$3.50 per 1M output tokens

**Testing tip**: Use Claude 3 Haiku or Titan Express for load testing to minimize costs.

## Troubleshooting

### Common Beginner Issues

1. **"Invalid security token" or "Security token expired"**
   ```
   Error: The security token included in the request is invalid
   ```
   - **Cause**: Wrong credentials or wrong region
   - **Fix**: 
     - Double-check your Access Key ID and Secret Access Key
     - Ensure you're using the same region everywhere (us-east-1)
     - Run `aws configure` again to reset

2. **"Model not found" or "no such model"**
   ```
   Error: Model not found: amazon.titan-text-express-v1
   ```
   - **Cause**: Model not enabled or wrong region
   - **Fix**:
     - Go back to Bedrock Console → Model access
     - Ensure the model shows "Access granted"
     - Check you're in the correct region (top-right corner of AWS Console)

3. **"AccessDeniedException"**
   ```
   Error: User is not authorized to perform bedrock:InvokeModel
   ```
   - **Cause**: IAM user lacks permissions
   - **Fix**:
     - Go to IAM → Users → Your user → Add permissions
     - Attach `AmazonBedrockFullAccess` policy
     - Wait 1-2 minutes for permissions to propagate

4. **Nothing happens when requesting model access**
   - **Cause**: Wrong region selected
   - **Fix**: 
     - Ensure you're in `us-east-1` (N. Virginia) in AWS Console
     - Bedrock is not available in all regions!

5. **"Throttling" or "Rate exceeded"**
   ```
   Error: Rate exceeded
   ```
   - **Cause**: Default Bedrock quotas
   - **Fix**:
     - Wait a few seconds between requests
     - Or request quota increase in AWS Service Quotas

6. **Can't find Bedrock in AWS Console**
   - **Cause**: Using an old AWS account or unsupported region
   - **Fix**:
     - Search for "Bedrock" not "Amazon Bedrock"
     - Switch to us-east-1 region
     - Bedrock launched in 2023, ensure your account is recent

### Quick Diagnostic Commands

```bash
# Test AWS credentials are working
aws sts get-caller-identity

# List your Bedrock model access
aws bedrock list-foundation-models --region us-east-1 | grep modelId

# Check specific model access
aws bedrock get-foundation-model \
  --model-identifier amazon.titan-text-express-v1 \
  --region us-east-1

# Test invoke directly (simplest test)
echo '{"inputText":"Hello"}' > test.json
aws bedrock-runtime invoke-model \
  --model-id amazon.titan-text-express-v1 \
  --body file://test.json \
  --region us-east-1 \
  output.json
```

### Common Issues

1. **"Model not found" error**
   - Ensure model access is granted in AWS Console
   - Check the exact model ID in the configuration
   - Verify the region supports the model

2. **Authentication errors**
   - Run `aws bedrock list-foundation-models` to test credentials
   - Check IAM permissions include `bedrock:InvokeModel`
   - Ensure region is correctly set

3. **Streaming not working**
   - Check if the model supports streaming
   - Verify `stream: true` is in the request
   - Check for proxy/firewall blocking SSE connections

4. **Rate limiting**
   - Bedrock has default quotas (varies by model)
   - Request quota increases through AWS Console
   - Implement exponential backoff for production

## Production Readiness

Before deploying to production:

1. **Set up monitoring**:
   - CloudWatch metrics for Bedrock API calls
   - Application logs for request/response tracking
   - Error rate monitoring

2. **Implement caching** (optional):
   - Cache frequent identical requests
   - Use Redis/Memcached for distributed caching

3. **Set up rate limiting**:
   - Configure Nexus rate limiting
   - Implement per-user quotas if needed

4. **Security**:
   - Use IAM roles in production (not access keys)
   - Enable CloudTrail for audit logging
   - Set up VPC endpoints for private connectivity

5. **Cost controls**:
   - Set up AWS Budget alerts
   - Monitor token usage per model
   - Implement request throttling if needed

## Automated Test Suite

Create `run-all-tests.sh`:
```bash
#!/bin/bash

set -e

echo "Starting Bedrock integration tests..."

# Start server
./target/release/nexus --config config/bedrock-all-models.toml &
SERVER_PID=$!
sleep 5

# Run tests
echo "Testing model listing..."
curl -s http://localhost:8080/llm/models | jq -e '.data | length > 0'

echo "Testing each model family..."
for model in claude-sonnet titan-express llama-70b mistral-7b command; do
  echo "Testing $model..."
  curl -s -X POST http://localhost:8080/llm/chat/completions \
    -H "Content-Type: application/json" \
    -d "{
      \"model\": \"bedrock/$model\",
      \"messages\": [{\"role\": \"user\", \"content\": \"Say OK\"}],
      \"max_tokens\": 10
    }" | jq -e '.choices[0].message.content'
done

echo "All tests passed!"

# Cleanup
kill $SERVER_PID
```

## Reporting Issues

If you encounter issues:

1. Enable debug logging:
   ```bash
   RUST_LOG=debug ./target/release/nexus --config config/bedrock-basic.toml
   ```

2. Capture:
   - Full error messages
   - Request/response bodies
   - AWS region and model IDs
   - Nexus logs

3. Check AWS Bedrock logs in CloudWatch

4. Verify with AWS CLI:
   ```bash
   aws bedrock-runtime invoke-model \
     --model-id anthropic.claude-3-sonnet-20240229-v1:0 \
     --body '{"messages":[{"role":"user","content":"Hi"}],"max_tokens":10}' \
     --region us-east-1 \
     output.json
   ```