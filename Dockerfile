# Dockerfile for reproducible Patina development and testing
FROM rust:1.75-slim

# Install system dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    git \
    chibi-scheme \
    && rm -rf /var/lib/apt/lists/*

# Create app directory
WORKDIR /app

# Copy project files
COPY . .

# Build the project
RUN cargo build --release

# Run tests to verify everything works
RUN cargo test --release

# Default command runs the REPL
CMD ["./target/release/patina"]
