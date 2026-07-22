# Deploying pebblestrength.app (VPS + reverse proxy)

Multi-user Google sign-in is already implemented in the app — this is the
configuration and hosting to turn it on in production. Target setup: the app
runs on a VPS behind Caddy (automatic HTTPS), open registration (any Google
account gets its own workspace).

> The `.app` TLD is HSTS-preloaded — browsers will **only** load it over HTTPS.
> TLS in front of the app is mandatory; there is no http fallback.

## 1. Google OAuth client

In the [Google Cloud console](https://console.cloud.google.com/):

1. **OAuth consent screen** → User type **External**. App name "Pebble Strength",
   your support email, developer email. Scopes are just `openid`, `email`,
   `profile` (non-sensitive, so **no verification needed**). **Publish** the app
   ("In production") — while it's in "Testing" only listed test users can sign
   in, which breaks open registration.
2. **Credentials** → **Create credentials** → **OAuth client ID** →
   **Web application**:
   - Authorized JavaScript origins: `https://pebblestrength.app`
   - Authorized redirect URIs: `https://pebblestrength.app/auth/google/callback`
3. Copy the **Client ID** and **Client secret**.

## 2. DNS

Point an `A` record for `pebblestrength.app` (and optionally `www`) at the VPS's
public IP. Wait for it to resolve before starting Caddy (it needs DNS to issue
the certificate).

## 3. Build and run the app on the VPS

Only Rust + a C compiler are needed — SQLite is bundled and TLS uses rustls, so
no `libsqlite3-dev`/OpenSSL.

```bash
# deps (Debian/Ubuntu)
sudo apt update && sudo apt install -y git build-essential
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

# build
git clone https://github.com/azw413/pebble-strength.git
cd pebble-strength/server
cargo build --release           # -> target/release/strength-server
```

Create `server/.env` (see `.env.example` for the annotated version):

```ini
DATABASE_URL=/var/lib/strength/strength.db
BASE_URL=https://pebblestrength.app
GOOGLE_CLIENT_ID=<your-web-client-id>.apps.googleusercontent.com
GOOGLE_CLIENT_SECRET=<your-secret>
DEV_LOGIN=0
BIND_ADDR=127.0.0.1
PORT=8090
```

```bash
sudo mkdir -p /var/lib/strength      # DB lives outside the repo
```

### systemd service

`/etc/systemd/system/strength.service`:

```ini
[Unit]
Description=Pebble Strength server
After=network.target

[Service]
WorkingDirectory=/home/<user>/pebble-strength/server
EnvironmentFile=/home/<user>/pebble-strength/server/.env
ExecStart=/home/<user>/pebble-strength/server/target/release/strength-server
Restart=on-failure
User=<user>

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now strength
```

The app migrates and seeds its own DB on first start. It binds `127.0.0.1:8090`
— only Caddy talks to it.

## 4. Caddy (automatic HTTPS)

```bash
sudo apt install -y caddy    # or per caddyserver.com/docs/install
```

`/etc/caddy/Caddyfile`:

```
pebblestrength.app {
    reverse_proxy 127.0.0.1:8090
}
```

```bash
sudo systemctl reload caddy
```

Caddy provisions a Let's Encrypt certificate automatically. Visit
`https://pebblestrength.app` — you should see the landing page with
**Sign in with Google**.

## 5. Verify

- Sign in with Google → a fresh account is created and you land on the dashboard.
- A second Google account sees an empty, independent workspace (multi-user).
- `DEV_LOGIN=0` means there is no dev-login button and `/auth/dev` returns 404.

## Known follow-up: watch recordings need a device token

With `DEV_LOGIN=0`, the device API (`/api/device/workouts`,
`/api/device/recordings`) requires a **Bearer device token** — the dev fallback
that currently lets the phone relay post tokenlessly is gone in production.

Before real watch sync works against the live server, we need to:

1. Have each user create a device on `/devices` (it issues a token).
2. Send that token from the phone relay — `src/pkjs/index.js` posts **without**
   an `Authorization` header today, and `SERVER` points at the LAN IP. Both must
   change (token + `https://pebblestrength.app`), then rebuild/reinstall the app.

Until then, production sign-in, the workout builder, sessions, and the dashboard
all work; only new uploads from a watch would be unauthorised.
