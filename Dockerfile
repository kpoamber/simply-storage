# ─── Stage 1: Build frontend ────────────────────────────────────────────────
FROM node:20-alpine AS frontend-builder

WORKDIR /app/frontend
COPY frontend/package.json frontend/package-lock.json* ./
RUN npm ci --ignore-scripts
COPY frontend/ .
RUN npm run build

# ─── Stage 2: Build Rust backend ───────────────────────────────────────────
FROM rust:1.94-bookworm AS backend-builder

WORKDIR /app

# Cache dependencies by building a dummy project first
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && echo '' > src/lib.rs
RUN cargo build --release 2>/dev/null || true
# Remove dummy binary and fingerprints so Cargo rebuilds with real source
RUN rm -rf src target/release/simply-storage target/release/deps/simply_storage* target/release/.fingerprint/simply-storage-*

# Copy actual source and migrations
COPY src/ src/
COPY migrations/ migrations/

# Build the real binary
RUN cargo build --release

# ─── Stage 3: Minimal runtime image ───────────────────────────────────────
FROM debian:bookworm-slim AS runtime

# Install postgresql-client-16 from the PGDG apt repo. The default Debian
# bookworm package is v15, which refuses to dump a v16 server (Citus 12.1).
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl gnupg libssl3 \
    && install -d /usr/share/postgresql-common/pgdg \
    && curl -fsSL https://www.postgresql.org/media/keys/ACCC4CF8.asc \
        | gpg --dearmor -o /usr/share/postgresql-common/pgdg/apt.postgresql.org.gpg \
    && echo "deb [signed-by=/usr/share/postgresql-common/pgdg/apt.postgresql.org.gpg] https://apt.postgresql.org/pub/repos/apt bookworm-pgdg main" \
        > /etc/apt/sources.list.d/pgdg.list \
    && apt-get update && apt-get install -y --no-install-recommends postgresql-client-16 \
    && apt-get purge -y --auto-remove gnupg \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN groupadd -r innovare && useradd -r -g innovare -m innovare

WORKDIR /app

# Copy binary from backend builder
COPY --from=backend-builder /app/target/release/simply-storage /app/simply-storage

# Copy frontend build output
COPY --from=frontend-builder /app/frontend/dist /app/frontend/dist

# Copy migrations for runtime migration execution
COPY migrations/ /app/migrations/

# Create data directories
RUN mkdir -p /app/data/temp /app/config && chown -R innovare:innovare /app

USER innovare

EXPOSE 8080

ENV RUST_LOG=info

CMD ["/app/simply-storage"]
