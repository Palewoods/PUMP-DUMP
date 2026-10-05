@echo off
rem Runs the movement sandbox. Works from any folder:
rem   .\run-sandbox
rem Extra arguments are passed to cargo, e.g. .\run-sandbox --release
cargo run -p pf-sandbox --manifest-path "%~dp0Cargo.toml" %*
