# Production Deployment

Guide for deploying Prism in production environments.

## Pre-Flight Checklist

- [ ] TLS configured (or behind TLS-terminating proxy)
- [ ] Security enabled with API keys
- [ ] Metrics enabled for monitoring
- [ ] Log aggregation configured
- [ ] Storage backend chosen and tested
- [ ] Resource limits set (memory, disk)
- [ ] Backup strategy defined

## Docker Deployment

### Dockerfile

```dockerfile
FROM rust:1.75-slim as builder
WORKDIR /app
COPY . .
RUN cargo build --release -p prism-server --features full

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/prism-server /usr/local/bin/
EXPOSE 3080
CMD ["prism-server"]
```

### Docker Compose

```yaml
version: '3.8'
services:
  prism:
    build: .
    ports:
      - "3080:3080"
    volumes:
      - prism-data:/data
      - ./prism.toml:/etc/prism/prism.toml:ro
      - ./schemas:/data/schemas:ro
    environment:
      - RUST_LOG=info,prism=debug
      - LOG_FORMAT=json
    command: ["prism-server", "-c", "/etc/prism/prism.toml", "--host", "0.0.0.0"]
    restart: unless-stopped
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:3080/health"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 10s

volumes:
  prism-data:
```

### Production Configuration

```toml
# prism.toml for production
[server]
bind_addr = "0.0.0.0:3080"

[server.cors]
enabled = true
origins = ["https://app.example.com"]

[storage]
data_dir = "/data"

[embedding]
enabled = true
model = "all-MiniLM-L6-v2"

[observability]
log_format = "json"
log_level = "info"
metrics_enabled = true

[security]
enabled = true

[[security.api_keys]]
key = "${PRISM_ADMIN_KEY}"
name = "admin"
roles = ["admin"]

[[security.api_keys]]
key = "${PRISM_APP_KEY}"
name = "application"
roles = ["app"]

[security.roles.admin.collections]
"*" = ["read", "write", "delete", "admin"]

[security.roles.app.collections]
"*" = ["read", "write"]
"_*" = []  # deny internal collections

[security.audit]
enabled = true
```

## Kubernetes Deployment

### Deployment

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: prism
spec:
  replicas: 1
  selector:
    matchLabels:
      app: prism
  template:
    metadata:
      labels:
        app: prism
      annotations:
        prometheus.io/scrape: "true"
        prometheus.io/port: "3080"
        prometheus.io/path: "/metrics"
    spec:
      containers:
        - name: prism
          image: prism:latest
          ports:
            - containerPort: 3080
          env:
            - name: RUST_LOG
              value: "info,prism=debug"
            - name: LOG_FORMAT
              value: "json"
            - name: PRISM_ADMIN_KEY
              valueFrom:
                secretKeyRef:
                  name: prism-secrets
                  key: admin-key
          volumeMounts:
            - name: config
              mountPath: /etc/prism
            - name: data
              mountPath: /data
          resources:
            requests:
              memory: "512Mi"
              cpu: "250m"
            limits:
              memory: "2Gi"
              cpu: "2000m"
          livenessProbe:
            httpGet:
              path: /health
              port: 3080
            initialDelaySeconds: 10
            periodSeconds: 10
          readinessProbe:
            httpGet:
              path: /health
              port: 3080
            initialDelaySeconds: 5
            periodSeconds: 5
      volumes:
        - name: config
          configMap:
            name: prism-config
        - name: data
          persistentVolumeClaim:
            claimName: prism-data
```

### Service

```yaml
apiVersion: v1
kind: Service
metadata:
  name: prism
spec:
  selector:
    app: prism
  ports:
    - port: 3080
      targetPort: 3080
```

### Ingress with TLS

```yaml
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: prism
  annotations:
    cert-manager.io/cluster-issuer: letsencrypt-prod
spec:
  tls:
    - hosts:
        - search.example.com
      secretName: prism-tls
  rules:
    - host: search.example.com
      http:
        paths:
          - path: /
            pathType: Prefix
            backend:
              service:
                name: prism
                port:
                  number: 3080
```

## TLS Configuration

### Direct TLS (Built-in)

```toml
[server.tls]
enabled = true
bind_addr = "0.0.0.0:3443"
cert_path = "/etc/prism/tls/cert.pem"
key_path = "/etc/prism/tls/key.pem"
```

### Behind Reverse Proxy (Recommended)

Use nginx, Caddy, or cloud load balancer for TLS termination:

```nginx
# nginx.conf
upstream prism {
    server 127.0.0.1:3080;
}

server {
    listen 443 ssl http2;
    server_name search.example.com;

    ssl_certificate /etc/letsencrypt/live/search.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/search.example.com/privkey.pem;

    location / {
        proxy_pass http://prism;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

## Resource Sizing

### Memory Guidelines

| Documents | RAM (minimum) | RAM (recommended) |
|-----------|---------------|-------------------|
| < 100K | 512 MB | 1 GB |
| 100K - 1M | 1 GB | 2 GB |
| 1M - 10M | 2 GB | 4 GB |
| > 10M | 4 GB+ | 8 GB+ |

Vector search requires additional memory for HNSW index.

### Disk Guidelines

| Content Type | Estimate |
|--------------|----------|
| Text index | ~1-2x raw text size |
| Vector index (384d) | ~2KB per document |
| Vector index (1536d) | ~8KB per document |

## Backup and Recovery

### Backup

```bash
# Stop writes during backup (optional for consistency)
# Export all collections
for collection in $(curl -s http://localhost:3080/admin/collections | jq -r '.[].name'); do
  prism-cli document export -c "$collection" -o "backup-$collection.jsonl"
done

# Or backup data directory directly
tar -czf prism-backup-$(date +%Y%m%d).tar.gz /data/
```

### Restore

```bash
# Restore from JSONL exports
for file in backup-*.jsonl; do
  collection=$(echo "$file" | sed 's/backup-\(.*\)\.jsonl/\1/')
  prism-cli document import -c "$collection" -f "$file"
done
```

## Maintenance

### Index Optimization

Merge segments periodically for better performance:

```bash
# Optimize all collections
for collection in $(curl -s http://localhost:3080/admin/collections | jq -r '.[].name'); do
  prism-cli index optimize -c "$collection"
done
```

### Log Rotation

With JSON logging, use logrotate:

```
/var/log/prism/*.log {
    daily
    rotate 14
    compress
    delaycompress
    missingok
    notifempty
    create 0640 prism prism
    postrotate
        systemctl reload prism
    endscript
}
```

## See Also

- [Configuration](configuration.md) — Full config reference
- [Security](security.md) — API keys and RBAC
- [Monitoring](monitoring.md) — Prometheus metrics
- [Storage Backends](storage-backends.md) — S3 and caching
