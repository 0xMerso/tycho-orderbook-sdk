cargo clippy --fix --allow-dirty --allow-staged --workspace --all-targets --all-features
cargo check --all
# cargo test --all --all-features
rustup run nightly cargo fmt -- --check
# rustup run nightly cargo clippy --workspace --all-features --all-targets -- -D warnings
# rustup run nightly cargo clippy --fix --allow-dirty --allow-staged --workspace --all-features --all-targets
cargo clippy --fix --allow-dirty --allow-staged --workspace --all-targets --all-features
