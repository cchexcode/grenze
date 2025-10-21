# Grenze

> A little HTTP rate limiting for everyone.

**Grenze** (German for "border" or "boundary") is a lightweight, high-performance HTTP proxy server that implements rate limiting using the leaky bucket algorithm. Built with Rust and Redis, it provides a simple yet powerful way to control request rates for any HTTP endpoint.

## Features

- **🚦 Rate Limiting**: Implements a leaky bucket algorithm for smooth, predictable rate limiting
- **🔑 Custom Keys**: Clients provide their own rate limit keys for flexible multi-tenant usage
- **🔄 HTTP Proxy**: Forward any HTTP request (GET, POST, etc.) through the rate limiter
- **⚡ Redis-Backed**: Uses Redis with Lua scripts for atomic, distributed rate limiting
- **🐳 Docker Ready**: Includes Docker Compose setup for easy deployment
- **🛡️ Graceful Shutdown**: Handles SIGINT/SIGTERM signals properly
- **📊 Health Checks**: Built-in health endpoint for monitoring

## Architecture

The server acts as a rate-limiting proxy that:
1. Accepts HTTP requests with a rate limit key
2. Checks the rate limit using Redis
3. If allowed, forwards the request to the target URL
4. Returns the response (or 429 Too Many Requests if rate limited)

## Quick Start

### Using Docker Compose (Recommended)

```bash
docker compose -f docker/compose.yml up
```

This starts both Redis and the grenze-server on port 8080.

### Manual Setup

1. **Start Redis:**
   ```bash
   docker run -d -p 6379:6379 redis:7-alpine
   ```

2. **Set environment variables:**
   ```bash
   export REDIS_URL=redis://localhost:6379/
   ```

3. **Build and run:**
   ```bash
   cargo build --release
   cargo run --release -p grenze-server
   ```

The server will start on `0.0.0.0:8080`.

## API Reference

### Health Check

**Endpoint:** `GET /health`

Returns the server's health status and version.

**Response:**
```json
{
  "status": "ok",
  "version": "0.0.0"
}
```

### Proxy Request

**Endpoint:** `POST /proxy`

Forwards an HTTP request through the rate limiter.

**Request Body:**
```json
{
  "key": "user-123",           // Required: Rate limit key
  "url": "https://api.example.com/data",
  "method": "POST",            // GET, POST, PUT, DELETE, etc.
  "headers": {                 // Optional: Custom headers
    "Authorization": "Bearer token",
    "Content-Type": "application/json"
  },
  "query": {                   // Optional: Query parameters
    "page": "1",
    "limit": "10"
  },
  "body": {                    // Optional: Request body (JSON)
    "name": "value"
  },
  "timeout_ms": 5000          // Optional: Request timeout in milliseconds
}
```

**Success Response:**
- Returns the downstream API's response with status code and body
- Passes through `Content-Type`, `Content-Length`, and `Cache-Control` headers

**Error Responses:**

**400 Bad Request** - Missing or empty rate limit key:
```json
{
  "error": "missing_key",
  "message": "Request must include non-empty 'key'"
}
```

**429 Too Many Requests** - Rate limit exceeded:
```json
{
  "error": "rate_limited",
  "message": "Too many requests"
}
```

**502 Bad Gateway** - Downstream request failed:
```json
{
  "error": "downstream_error",
  "message": "Error details..."
}
```

## Rate Limiting

### Algorithm: Leaky Bucket

Grenze uses the **leaky bucket** algorithm, which:
- Has a fixed capacity (bucket size)
- "Leaks" at a constant rate over time
- Allows bursts up to the bucket capacity
- Rejects requests when the bucket is full

### Configuration

Currently configured in the code (see `main.rs`):
- **Capacity**: 1 request per second (RPS)
- **Leak Rate**: 1 request per second

Each unique `key` gets its own independent bucket stored in Redis with automatic TTL expiration.

### Rate Limit Keys

The `key` field in the proxy request determines which rate limit bucket to use. This design allows for:
- **Per-user rate limiting**: `user-{user_id}`
- **Per-tenant rate limiting**: `tenant-{tenant_id}`
- **Per-endpoint rate limiting**: `api-{endpoint_name}`
- **Per-IP rate limiting**: `ip-{ip_address}`
- **Combined keys**: `user-{user_id}-api-{endpoint}`

## Configuration

### Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `REDIS_URL` | Yes | - | Redis connection URL (e.g., `redis://localhost:6379/`) |
| `RUST_LOG` | No | `info` | Log level (`error`, `warn`, `info`, `debug`, `trace`) |
| `RUST_BACKTRACE` | No | `1` | Enable backtraces on panic |

## Development

### Prerequisites

- Rust 1.90+ (using 2024 edition)
- Docker (for Redis)
- cargo-chef (optional, for Docker builds)

### Building

```bash
# Debug build
cargo build -p grenze-server

# Release build
cargo build -p grenze-server --release

# Run tests
cargo test

# Generate documentation
cargo doc --open
```

### Docker Build

```bash
# Build the image
docker build -f docker/rust.Dockerfile \
  --build-arg PACKAGE=grenze-server \
  --build-arg CRATE_DIR=crates/grenze-server \
  -t grenze-server .

# Run the container
docker run -p 8080:8080 \
  -e REDIS_URL=redis://host.docker.internal:6379/ \
  grenze-server
```

## Example Usage

### cURL

```bash
# Health check
curl http://localhost:8080/health

# Proxy a GET request
curl -X POST http://localhost:8080/proxy \
  -H "Content-Type: application/json" \
  -d '{
    "key": "demo-user",
    "url": "https://api.github.com/users/octocat",
    "method": "GET",
    "headers": {
      "User-Agent": "my-app"
    }
  }'

# Proxy a POST request with body
curl -X POST http://localhost:8080/proxy \
  -H "Content-Type: application/json" \
  -d '{
    "key": "demo-user",
    "url": "https://httpbin.org/post",
    "method": "POST",
    "headers": {
      "Content-Type": "application/json"
    },
    "body": {
      "message": "Hello, World!"
    }
  }'
```

### Python

```python
import requests

response = requests.post(
    "http://localhost:8080/proxy",
    json={
        "key": "user-42",
        "url": "https://api.example.com/data",
        "method": "GET",
        "headers": {
            "Authorization": "Bearer your-token"
        },
        "timeout_ms": 5000
    }
)

print(response.status_code)
print(response.json())
```

### JavaScript/Node.js

```javascript
const response = await fetch('http://localhost:8080/proxy', {
  method: 'POST',
  headers: {
    'Content-Type': 'application/json',
  },
  body: JSON.stringify({
    key: 'user-42',
    url: 'https://api.example.com/data',
    method: 'GET',
    headers: {
      Authorization: 'Bearer your-token'
    },
    timeout_ms: 5000
  })
});

const data = await response.json();
console.log(data);
```

## Security Considerations

⚠️ **Important**: This is a basic implementation suitable for internal services or development. For production use, consider:

1. **URL Allowlisting**: Add validation to restrict which URLs can be proxied
2. **Authentication**: Implement API key or OAuth authentication
3. **Key Validation**: Validate and sanitize rate limit keys
4. **Request Size Limits**: Add limits on request body size
5. **TLS/HTTPS**: Use HTTPS in production
6. **Network Policies**: Restrict which networks the server can access
7. **Rate Limit Configuration**: Make capacity and leak rate configurable per key
8. **Monitoring**: Add metrics and alerting for production deployments

## Technology Stack

- **[Rust](https://www.rust-lang.org/)**: Systems programming language
- **[Axum](https://github.com/tokio-rs/axum)**: Web framework
- **[Tokio](https://tokio.rs/)**: Async runtime
- **[Redis](https://redis.io/)**: In-memory data store for rate limit state
- **[Reqwest](https://github.com/seanmonstar/reqwest)**: HTTP client
- **[Docker](https://www.docker.com/)**: Containerization

## License

MIT - See [LICENSE](LICENSE) file for details.

## Contributing

Contributions are welcome! Please feel free to submit issues or pull requests.
