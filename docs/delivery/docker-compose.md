# Run with Docker Compose

Jush's Compose file runs Jush from its container image, with Caddy in front
of it to get the certificate and serve HTTPS. It runs on any machine with
Docker, such as a home server, or one of Oracle Cloud's Always Free Arm
machines, which cost nothing.

## Before you start

- Docker with Compose on the machine.
- A domain whose DNS points at the machine, with ports 80 and 443 free.

## Run

Replace `jush.example.com` with your domain:

```sh
curl -fsSLO https://raw.githubusercontent.com/jush-ai/jush/main/deploy/compose/compose.yaml
JUSH_DOMAIN=jush.example.com docker compose up -d
```

Open your domain, and create the admin account right away: until it exists,
anyone who reaches the address can.

Then add a model in **Settings › Providers**, and update Jush from
**Settings › Updates**. Pulling a newer image and starting it,
`docker compose pull && docker compose up -d`, also updates Jush.

## Options

Each changes `compose.yaml`:

- **Your own proxy:** remove the `caddy` service and publish port 3271 of
  `jush`. The proxy passes `Host` and `Origin` unchanged and allows WebSocket
  upgrades.
- **A bucket for attachments:** add `JUSH_STORAGE: s3`, the bucket's
  `JUSH_S3_BUCKET`, `JUSH_S3_ENDPOINT` and `JUSH_S3_REGION`, and its
  `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` to the `jush` service's
  `environment`.
- **gVisor:** uncomment the two lines marked `gVisor`. Without them, Clouds
  run on Simple Isolation.
