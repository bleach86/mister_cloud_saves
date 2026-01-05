# ---------- Build stage ----------
FROM rust:latest AS builder

WORKDIR /usr/src/mister_cloud_saves

# Copy the source code
COPY . .

# Build the server binary in release mode
RUN cargo build --release --bin=mister_save_server --features=server


# ---------- Runtime stage ----------
FROM debian:stable-slim

# Install necessary runtime dependencies

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app

# Copy the built binary from the builder stage
COPY --from=builder /usr/src/mister_cloud_saves/target/release/mister_save_server .

RUN mkdir -p user_saves user_saves_sled

VOLUME ["/app/user_saves", "/app/user_saves_sled"]

# Expose the port the server will run on
EXPOSE 8000

# Set the entrypoint to run the server

ENV ROCKET_ADDRESS=0.0.0.0
ENV ROCKET_LIMITS='{ json = "25 MiB" }'

ENTRYPOINT ["./mister_save_server"]