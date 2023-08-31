


default:
    just --list

@check:
    cargo check

@server:
    cargo run -- server 

@client1:
    cargo run -- client --name Ig

@client1-with-log-file:
    cargo run -- --log './log/clientIg.log' client --name Ks
    
@client2:
    cargo run -- client --name Ks

@client2-with-log-file:
    cargo run -- --log './log/clientKs.log' client --name Ks


@server-tokio-console:
    COBULETI_LOG=trace RUST_BACKTRACE=1 RUSTFLAGS="--cfg tokio_unstable" cargo run --features tokio-console -- server

# use for avoid recompile dependencies
@client1-tokio-console:
    COBULETI_LOG=trace RUST_BACKTRACE=1 RUSTFLAGS="--cfg tokio_unstable" cargo run  -- client --name Ig

@client2-tokio-console:
    COBULETI_LOG=trace RUST_BACKTRACE=1 RUSTFLAGS="--cfg tokio_unstable" cargo run  -- client --name Ks


# run unit tests
@test:
    cargo test --workspace --  --skip  show_game_layout show_select_role_layout

@test_with_output:
    RUST_BACKTRACE=1 cargo test --workspace -- --nocapture --skip  show_game_layout show_select_role_layout

# run unit tests (in release mode)
@test-release:
    cargo test --workspace --release --verbose  -- --skip show_game_layout


test-game-ui:
    cargo test show_game_layout 
test-select-role-ui:
    cargo test show_select_role_layout

@update-deps:
    cargo update
    command -v cargo-outdated >/dev/null || (echo "cargo-outdated not installed" && exit 1)
    cargo outdated

# list unused dependencies
@unused-deps:
    command -v cargo-udeps >/dev/null || (echo "cargo-udeps not installed" && exit 1)
    cargo +nightly udeps


