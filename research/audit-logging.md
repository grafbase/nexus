# Audit Logging Strategy for Nexus

## Overview

Audit logs are fundamentally different from operational logs. They serve compliance, security, and forensic purposes, requiring special handling for integrity, retention, and access control.

## Key Differences: Audit vs Operational Logs

| Aspect | Operational Logs | Audit Logs |
|--------|-----------------|------------|
| Purpose | Debugging, monitoring | Compliance, security, forensics |
| Content | Technical details | Who, what, when, where, why |
| Sensitivity | Low to medium | High (PII, access patterns) |
| Retention | Days to months | Years (regulatory requirements) |
| Mutability | Can be sampled/filtered | Must be immutable |
| Access | DevOps teams | Security/compliance teams |

## Recommended Approach: Separate Audit Pipeline

### 1. Dedicated Audit Logger

```rust
// Example audit event structure
#[derive(Serialize)]
pub struct AuditEvent {
    // Immutable event ID
    event_id: Uuid,
    
    // Timestamp with timezone
    timestamp: DateTime<Utc>,
    
    // Event metadata
    event_type: AuditEventType,
    severity: AuditSeverity,
    
    // Actor information
    actor: Actor,
    
    // Action details
    action: String,
    resource: String,
    resource_id: String,
    
    // Outcome
    outcome: AuditOutcome,
    reason: Option<String>,
    
    // Request context
    request_id: String,
    source_ip: IpAddr,
    user_agent: Option<String>,
    
    // Additional context
    metadata: HashMap<String, Value>,
}

#[derive(Serialize)]
pub enum AuditEventType {
    Authentication,
    Authorization,
    DataAccess,
    DataModification,
    Configuration,
    ToolExecution,
}

#[derive(Serialize)]
pub enum AuditSeverity {
    Info,      // Routine operations
    Notice,    // Significant but expected
    Warning,   // Potential issues
    Alert,     // Immediate attention needed
}
```

### 2. Separate Storage Options

#### Option A: Dedicated Audit Service (Recommended)
```toml
[server.audit]
enabled = true

# Separate endpoint for audit events
[server.audit.sink]
type = "dedicated"
endpoint = "{{ env.AUDIT_SERVICE_ENDPOINT }}"
api_key = "{{ env.AUDIT_SERVICE_API_KEY }}"

# Ensure delivery
[server.audit.delivery]
# At-least-once delivery guarantee
retry_count = 3
retry_delay = "1s"
# Local buffer for failures
buffer_path = "/var/lib/nexus/audit-buffer"
buffer_size_mb = 100

# Encryption at rest for buffer
[server.audit.encryption]
enabled = true
key_id = "{{ env.AUDIT_ENCRYPTION_KEY_ID }}"
```

#### Option B: Separate OTLP Stream
```toml
[server.audit]
enabled = true

# Use OTLP but with separate endpoint and configuration
[server.audit.otlp]
endpoint = "{{ env.AUDIT_OTLP_ENDPOINT }}"
# Different from operational logs endpoint!

# Strict batching - no sampling
[server.audit.otlp.batch]
max_queue_size = 10000
max_export_batch_size = 100
scheduled_delay = "500ms"

# No sampling for audit logs!
[server.audit.sampling]
enabled = false

# Different resource attributes
[server.audit.otlp.resource.attributes]
"telemetry.sdk.name" = "nexus-audit"
"audit.version" = "1.0"
"compliance.standard" = "SOC2"
```

#### Option C: Direct Database/S3 Write
```toml
[server.audit]
enabled = true

[server.audit.storage]
type = "s3"  # or "postgresql", "elasticsearch"

# S3 Configuration
[server.audit.storage.s3]
bucket = "{{ env.AUDIT_BUCKET }}"
region = "{{ env.AWS_REGION }}"
prefix = "nexus/audit/{year}/{month}/{day}/"
# Use IAM role or explicit credentials
use_iam_role = true

# Compression and encryption
compression = "gzip"
server_side_encryption = "aws:kms"
kms_key_id = "{{ env.AUDIT_KMS_KEY_ID }}"

# Write settings
[server.audit.storage.write]
# Write immediately for high-value events
immediate_write_events = ["Authentication", "Authorization", "DataModification"]
# Buffer others
buffer_timeout = "5s"
buffer_size = 1000
```

### 3. Audit Events in Nexus Context

```toml
[server.audit.events]
# Define which events to audit
[server.audit.events.authentication]
enabled = true
include_jwt_claims = ["sub", "aud", "iss"]  # Not the full token!

[server.audit.events.tool_execution]
enabled = true
# Be careful with parameters - they might contain sensitive data
include_parameters = false
include_parameter_hash = true  # Hash for correlation without exposure

[server.audit.events.authorization]
enabled = true
include_denied_reason = true

[server.audit.events.configuration]
enabled = true
include_diff = true  # What changed
```

## Implementation Recommendations

### 1. Separate Concern in Code

```rust
// Separate audit logger from operational logger
pub struct AuditLogger {
    sink: Arc<dyn AuditSink>,
    buffer: Arc<AuditBuffer>,
    encryptor: Option<Arc<dyn Encryptor>>,
}

impl AuditLogger {
    pub async fn log_authentication(&self, event: AuthEvent) -> Result<()> {
        let audit_event = AuditEvent {
            event_type: AuditEventType::Authentication,
            actor: Actor {
                user_id: event.user_id,
                ip_address: event.ip_address,
                // Don't log tokens or passwords!
            },
            // ...
        };
        
        self.emit(audit_event).await
    }
}
```

### 2. Integration with Logforth

```rust
// Custom logforth appender for audit events
pub struct AuditAppender {
    audit_logger: Arc<AuditLogger>,
}

impl Append for AuditAppender {
    fn try_append(&self, record: &Record) -> anyhow::Result<()> {
        // Check if this is an audit log via target
        if record.target().starts_with("audit::") {
            // Parse and forward to audit logger
            // This keeps the same logging API
        }
        Ok(())
    }
}
```

## Security Considerations

1. **Separation of Duties**: Audit logs should be written to a system that the application itself cannot modify or delete
2. **Encryption**: Always encrypt audit logs in transit and at rest
3. **Access Control**: Strict RBAC for audit log access
4. **Integrity**: Consider adding cryptographic signatures to prevent tampering
5. **Retention**: Implement automated retention policies based on compliance requirements

## Why Not Mix with Operational Logs?

1. **Different Retention**: Audit logs often need 7+ years retention
2. **Access Patterns**: Security teams vs DevOps teams
3. **Compliance Scope**: Mixing increases compliance burden on operational systems
4. **Performance**: Audit logs can't be sampled or dropped
5. **Cost**: Different storage tiers appropriate for each

## Recommended Architecture

```
┌─────────────┐     ┌─────────────────┐     ┌──────────────────┐
│   Nexus     │────▶│ Operational     │────▶│ Observability    │
│             │     │ Logs (OTLP)     │     │ Platform         │
│             │     └─────────────────┘     └──────────────────┘
│             │
│             │     ┌─────────────────┐     ┌──────────────────┐
│             │────▶│ Audit Logs      │────▶│ Audit Service    │
│             │     │ (Dedicated)     │     │ or SIEM          │
└─────────────┘     └─────────────────┘     └──────────────────┘
```

## Example Audit Service Choices

1. **Cloud Native**:
   - AWS CloudTrail + S3
   - Google Cloud Audit Logs
   - Azure Monitor Logs

2. **SIEM Solutions**:
   - Splunk (with dedicated index)
   - Elastic Security
   - Datadog Security Monitoring

3. **Specialized Audit Services**:
   - Teleport Audit Log
   - Auth0 Log Streaming
   - Custom microservice with PostgreSQL + S3

## Summary

For Nexus, I recommend:

1. **Separate audit logging from operational telemetry**
2. **Use a dedicated audit sink** (not mixed with OTLP logs)
3. **Design audit events specifically** for compliance/security needs
4. **Implement proper retention and access controls**
5. **Consider using structured events** rather than text logs
6. **Encrypt and potentially sign** audit records

This separation ensures you can evolve operational and audit logging independently, maintain compliance without impacting performance monitoring, and provide appropriate access controls for different stakeholders.