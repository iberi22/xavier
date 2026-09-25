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
The only image this project's CI publishes is `ghcr.io/iberi22/xavier` (GHCR, not Docker Hub),
and only when a `v*` release tag is pushed — see
[`docs/DEPLOY/DOCKER_DEPLOY.md`](../DEPLOY/DOCKER_DEPLOY.md) for the real publish story and a
tested from-scratch walkthrough (build → first admin user → login) in section 4 below.
```bash
docker pull ghcr.io/iberi22/xavier:latest   # or a specific vX.Y.Z that was actually released
docker run -d -p 8006:8006 -e XAVIER_TOKEN=change-me -e XAVIER_STATE_DIR=/data \
  -v xavier-state:/data ghcr.io/iberi22/xavier:latest
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

---

## 4. Instalación desde cero (Docker + primer usuario admin)

Pasos probados de punta a punta: build de la imagen Docker desde el `Dockerfile` de la raíz
del repo, arranque del servidor, creación del primer usuario admin y su login desde el panel.
No hace falta ningún paso manual contra `/v1/auth/*` — esa API está deprecada, ver
[GH #2545](https://github.com/iberi22/xavier/issues/2545) y
[`docs/api/README.md`](../api/README.md#2-authentication-protocol).

```bash
# 1. Clonar y construir la imagen (build multi-stage, ~20 min en frío — ver
#    docs/DEPLOY/DOCKER_DEPLOY.md para el detalle y por qué tarda tanto).
git clone https://github.com/iberi22/xavier.git
cd xavier
docker build -t xavier:local .

# 2. Arrancar el contenedor. El runtime corre como usuario no-root `xavier`
#    (ver Dockerfile) y /data ya viene creado con sus permisos; XAVIER_STATE_DIR
#    ahí apunta es donde vive .xavier/auth.db (usuarios, sesiones) — persistirlo
#    en un volumen para no perder cuentas entre reinicios.
docker run -d --name xavier \
  -p 8006:8006 \
  -e XAVIER_TOKEN=your-secure-token \
  -e XAVIER_JWT_SECRET=$(openssl rand -hex 32) \
  -e XAVIER_STATE_DIR=/data \
  -v xavier-state:/data \
  xavier:local

# 3. Inicializar configuración (dentro o fuera del contenedor; ambos leen/escriben
#    el mismo XAVIER_STATE_DIR si el volumen está montado igual).
docker exec xavier xavier init --non-interactive

# 4. Crear el primer usuario admin. `--email` es un flag NOMBRADO en `create`
#    (a diferencia de `set-role`/`reset-password`, que toman el email como
#    argumento POSICIONAL — inconsistencia real de la CLI, ver nota abajo).
#    OJO: la contraseña SIEMPRE se pide interactivo (prompt oculto, dos veces;
#    dialoguer::Password, sin flag --password) — hace falta -it en docker exec
#    o el comando se queda esperando stdin sin dar ninguna pista.
docker exec -it xavier xavier users create \
  --email admin@example.com \
  --role admin
# Anota la recovery seed phrase (24 palabras) que imprime UNA sola vez: la pide
# POST /auth/recovery si alguna vez pierdes la contraseña o el dispositivo TOTP.

# 5. (Opcional pero recomendado) Enrolar 2FA/TOTP para esa cuenta.
docker exec -it xavier xavier users totp-enroll --email admin@example.com
# Escanea el QR (se imprime en Unicode en la terminal) con Google/Microsoft
# Authenticator, confirma con un código de 6 dígitos, y guarda los 10 backup
# codes que se muestran al final.

# 6. Login en el panel: abre http://localhost:8006/ (o /panel), o directamente:
curl -X POST http://localhost:8006/auth/login \
  -H "Content-Type: application/json" \
  -d '{"email":"admin@example.com","password":"<la que pusiste en el paso 4>","totp_code":"<6 digitos, si activaste TOTP>"}'
# => 200 con {access_token, refresh_token, user, requires_2fa}
```

**Inconsistencia conocida de la CLI (`xavier users`):** `create`, `totp-enroll` y
`totp-disable` toman `--email <email>` (flag), mientras que `set-role` y `reset-password`
toman el email como argumento posicional (`xavier users set-role <email> <role>`). Es un
problema real del código (`src/cli/commands/enums.rs`, `UsersCommand`), documentado aquí tal
cual es hoy — no se corrigió como parte de este cambio de documentación; ver issue de
seguimiento.
