
FROM rust:1.72-slim-buster AS builder

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    build-essential \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app

COPY Cargo.toml Cargo.lock ./

RUN mkdir src
RUN echo "fn main() {}" > src/main.rs

RUN cargo build --release || true
RUN rm -rf src

COPY . .

RUN cargo build --release

FROM debian:buster-slim AS runtime

RUN apt-get update && apt-get install -y \
    libssl1.1 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/get_a_bug /app/get_a_bug

COPY src/templates/ /app/templates/
COPY static/ /app/static/

EXPOSE 8080
ENV PORT 8080

ENV RUST_LOG=info

CMD ["./get_a_bug"]
