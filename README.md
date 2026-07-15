# s3dl

S3 file downloader with multi-environment, multi-service credential management and automatic file type detection.

## Features

- **Multi-environment configs** — prod, staging, dev, etc. with separate credentials
- **Multi-service support** — different buckets and keys per service within each environment, with env-level defaults as fallback
- **Auto file extension** — detects content type from S3 metadata and sets the correct file extension
- **Interactive setup** — guided wizard to configure environments, services, and credentials
- **Flexible auth** — static keys, AWS CLI profiles, or default credential chain per env/service
- **Streaming download** — progress bar with streaming I/O, no full-file buffering
- **Shell completions** — tab completion for bash, zsh, fish, powershell
- **Inline mode** — skip config entirely with `--ak`, `--sk`, `-b` flags

## Installation

### Recommended: Build using the provided script

The repository includes build scripts in the `dist/` directory that handle building the project for you, so you don't have to run Cargo commands manually.

```bash
git clone https://github.com/YOUR_USERNAME/s3dl.git
cd s3dl

# Linux/macOS/Windows
./dist/build.sh
```

Everything will be completed automatically

### Install with Cargo

If you already have Rust installed and simply want to install the binary locally:

```bash
cargo install --path .
```

### Manual build

If you prefer not to use the build script:

```bash
git clone https://github.com/YOUR_USERNAME/s3dl.git
cd s3dl
cargo build --release
```

The binary will be located at:

```text
./target/release/s3dl
```

## Quick Start

```bash
# 1. Run the interactive setup wizard
s3dl setup

# 2. Download a file
s3dl -e prod -s kyc -f "documents/report"

# 3. Enable tab completions (pick your shell)
s3dl completions zsh >> ~/.zshrc
s3dl completions bash >> ~/.bashrc
s3dl completions fish > ~/.config/fish/completions/s3dl.fish
```

## Usage

### Download

Download arguments work both directly and via the `get` subcommand.

```bash
# These are equivalent
s3dl -e prod -s kyc -f "path/to/key"
s3dl get -e prod -s kyc -f "path/to/key"

# Service is optional (uses environment defaults)
s3dl -e prod -f "path/to/key"

# Override output path
s3dl -e prod -s kyc -f "path/to/key" -o ~/Desktop
s3dl -e prod -s kyc -f "path/to/key" -o ~/Desktop/custom-name.pdf

# Skip config entirely with inline credentials
s3dl -f "path/to/key" \
  --ak AKIAIOSFODNN7EXAMPLE \
  --sk wJalrXUtn/... \
  -b my-bucket

# Skip automatic extension detection
s3dl -e prod -s kyc -f "data/export" --no-auto-ext

# Quiet mode (no progress bar, only errors)
s3dl -e prod -s kyc -f "data/export" -q
```

## Configuration Management

```bash
s3dl setup                    # Interactive first-time wizard
s3dl config list              # Show all environments and services
s3dl config edit              # Open config in $EDITOR
s3dl config path              # Print config file location
s3dl config add-env           # Add a new environment
s3dl config add-service       # Add a service to an environment
s3dl config remove-env        # Remove an environment
s3dl config remove-service    # Remove a service
```

## Configuration

Configuration is stored at:

```text
~/.config/s3dl/config.toml
```

The configuration file is created with **600** permissions.

### Example

```toml
[defaults]
region = "us-east-1"
output_dir = "~/Downloads"

[env.prod]
bucket = "my-prod-default-bucket"
access_key = "AKIA..."
secret_key = "..."
region = "ap-south-1"
output_dir = "~/work/prod-downloads"

# Services inherit from their environment.
# Only specify values you want to override.

[env.prod.services.kyc]
bucket = "kyc-specific-bucket"

[env.prod.services.esign]
bucket = "esign-bucket"
access_key = "AKIA_ESIGN..."
secret_key = "..."

[env.prod.services.uploads]
bucket = "uploads-bucket"
profile = "uploads-sso"
output_dir = "~/work/uploads"

[env.staging]
profile = "staging-sso"
bucket = "staging-bucket"

[env.staging.services.kyc]
bucket = "staging-kyc-bucket"
```

## Configuration Resolution

For each configurable field (`bucket`, `credentials`, `region`, `output_dir`), values are resolved in the following order:

1. CLI flags (`--ak`, `--sk`, `-b`, `-o`, `-r`)
2. Service configuration (`[env.NAME.services.SVC]`)
3. Environment configuration (`[env.NAME]`)
4. Global defaults (`[defaults]`)
5. Built-in defaults (`us-east-1`, `~/Downloads`)

Authentication resolution order:

1. `--access-key` + `--secret-key`
2. Service `access_key` / `secret_key` or `profile`
3. Environment `access_key` / `secret_key` or `profile`
4. AWS default credential chain (environment variables, `~/.aws/credentials`, IAM roles, etc.)

## Automatic Extension Detection

Before downloading, `s3dl` performs a `HEAD` request to determine the object's `Content-Type` and automatically appends the correct file extension.

| Content-Type | Extension |
|--------------|-----------|
| application/pdf | .pdf |
| application/json | .json |
| application/xml | .xml |
| text/xml | .xml |
| image/jpeg | .jpg |
| image/png | .png |
| text/csv | .csv |
| text/plain | .txt |
| application/zip | .zip |

If the object key already ends with the correct extension, nothing is changed.

Disable this behavior with:

```bash
s3dl --no-auto-ext
```

## Shell Completions

### Bash

```bash
s3dl completions bash >> ~/.bashrc
```

### Zsh

```bash
s3dl completions zsh >> ~/.zshrc
```

### Fish

```bash
mkdir -p ~/.config/fish/completions
s3dl completions fish > ~/.config/fish/completions/s3dl.fish
```

### PowerShell

```powershell
s3dl completions powershell >> $PROFILE
```

## License

MIT
