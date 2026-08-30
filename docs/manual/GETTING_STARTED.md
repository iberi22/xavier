# Getting Started with Xavier

## 1. Installation

### From Pre-built Binaries (Recommended)
Download the official release for your operating system from GitHub Releases:

- **Linux x86_64 / ARM64**: `xavier-x86_64-unknown-linux-gnu.tar.gz`
- **macOS (Apple Silicon / Intel)**: `xavier-aarch64-apple-darwin.tar.gz`
- **Windows x86_64**: `xavier-x86_64-pc-windows-msvc.zip`

Extract and place the binary on your system `PATH` (e.g. `/usr/local/bin` or `~/.local/bin`).

### From Source
```bash
git clone https://github.com/iberi22/xavier.git
cd xavier
cargo build --release
cargo install --path . --locked
```

### Docker Container
```bash
docker pull ghcr.io/iberi22/xavier:0.0.1
docker run -d -p 8006:8006 -v $(pwd)/data:/app/data ghcr.io/iberi22/xavier:0.0.1
```

---

## 2. Configuration & Initialization

Initialize default configuration in `~/.config/xavier/xavier.toml` (or `config/xavier.config.json`):

```bash
xavier init
```

Set your master secret and token:
```bash
export XAVIER_TOKEN="your-secure-agent-token"
export XAVIER_DATA_DIR="./data"
```

---

## 3. Running Xavier

### Foreground Server
```bash
xavier http --port 8006
```

### Background Service (systemd)
```bash
sudo cp scripts/systemd/xavier.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now xavier
```

Verify service health:
```bash
curl http://localhost:8006/health
```
