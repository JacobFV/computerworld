# Runbook — sensor-gateway on app-server

Carol, 2026-09-14. The gateway is plain Node; no build step.

## Layout on app-server (10.0.0.13)

```
/srv/sensor-gateway/            checkout of ~/code/sensor-gateway
/srv/sensor-gateway/data/       readings.json store (GATEWAY_STORE)
/var/log/sensor-gateway.log
```

## Deploy

1. On the workstation: `cd ~/code/sensor-gateway && npm test` — all tests pass.
2. `git push origin main`, then on app-server `git pull` in `/srv/sensor-gateway`.
3. `node src/gateway.js health` — expect `{"status":"ok", ...}`.
4. Ingest the pending batches: `node src/gateway.js ingest data/batch-*.json`
   (a non-zero exit means rejected readings — read the reasons, do not retry blindly).
5. `node src/gateway.js report` and check the alert list is empty.

## Rollback

`git checkout <previous tag>` in `/srv/sensor-gateway`; the store is append-only JSON
and stays compatible across the 0.x versions.

## Board side

`~/bin/flash-board.sh <firmware.bin>` writes the firmware over SWD and checks the
version string on the debug console. Boards with an unknown `node-xxxx` id are rejected
by the gateway until added to the allow list in `src/readings.js`.
