# FROM rust:slim
# RUN cargo install cargo-watch
# WORKDIR /app
# COPY . .
# RUN cargo build --bin stream
# CMD ["cargo", "watch", "-x", "run --bin stream"]

FROM rust:slim
RUN apt-get update && apt-get install -y libssl-dev pkg-config
RUN apt-get update && apt-get install -y curl
RUN cargo install cargo-watch
WORKDIR /app
COPY . .
RUN cargo build --bin stream
# CMD ["cargo", "watch", "-w", "/app", "-x", "run --bin stream"]
CMD ["cargo", "run", "--bin", "stream"]