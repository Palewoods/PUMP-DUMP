@echo off
rem Builds and runs PUMP&DUMP from source. Works from any folder:
rem   .\run
rem Extra arguments are passed to cargo, e.g. .\run --release
cargo run -p pumpdump --manifest-path "%~dp0Cargo.toml" %*
