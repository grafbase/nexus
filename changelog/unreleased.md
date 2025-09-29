# Nexus XXX

## Breaking Changes

- OpenTelemetry metrics are now reported in seconds with the `s` unit, instead of milliseconds.

## Features

- Configurable client ip extraction:

```toml
[server.client_ip]
# Defaults to `false`
# Use `X-Real-Ip` header to retrieve the client IP address.
x_real_ip = true
# Absent by default, deactivating it.
# Use `X-Forwarded-For` header to retrieve the client IP address.
x_forwarded_for_trusted_hops = 1
```

Nexus will first try `X-Real-Ip` if enabled, the `X-Forwarded-For` if enabled and finally fallback to the client IP of the connection.
