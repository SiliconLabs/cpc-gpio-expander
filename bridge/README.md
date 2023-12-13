# CPC GPIO Bridge

The CPC GPIO Bridge is a component part of the [CPC GPIO Expander](../README.md) and acts as a router for the [CPC GPIO Driver](../driver/README.md) and [CPC GPIO Secondary](../secondary/README.md).

## Table of Contents
- [Installation](#installation)
  - [Dependencies](#dependencies)
  - [Building](#building)
- [Usage](#usage)
  - [Command Line Options](#command-line-options)

## Installation

### Dependencies
* [Rust](https://www.rust-lang.org/tools/install)
* [Libcpc-rs](https://github.com/SiliconLabs/cpc-daemon/tree/main/lib/bindings/rust)

### Building
Now the Bridge can be built using [cargo build](https://doc.rust-lang.org/cargo/commands/cargo-build.html) and installed with [cargo install]((https://doc.rust-lang.org/cargo/commands/cargo-install.html)):

```
cargo build --release
cargo install
```

## Usage
`cargo run -- [OPTIONS]` or if installed: `cpc-gpio-bridge [OPTIONS]`

### Command Line Options

* `-t`, `--trace <TRACE>` — Enable tracing [default: none]
  - `none`:
    No tracing
  - `bridge`:
    Bridge tracing
  - `libcpc`:
    Libcpc tracing
  - `all`:
    Bridge and libcpc tracing
*  `-i`, `--instance <INSTANCE>`  — Name of the cpcd instance [default: cpcd_0]
*  `-l`, `--lock-dir <LOCK_DIR>`  — Bridge lock directory [default: /tmp]
*  `-d`, `--deinit`               — Deinit gpio chip and exit process
*  `-h`, `--help`                 — Print help
*  `-V`, `--version`              — Print version
