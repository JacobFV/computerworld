# sensor-gateway

The service that sits between the Atlas sensor-node boards and the dashboard. It is
plain Node (no dependencies) typed with JSDoc and checked by `tsc --checkJs`, so it
runs straight from the checkout.

```
npm test                         # unit tests (assert)
node src/gateway.js ingest data/batch-2026-09-15.json
node src/gateway.js report
node src/gateway.js health
```

`ingest` validates a batch (`node`, `channel`, `value`, `unit`, `at`), appends the
good readings to `data/readings.json` (override with `GATEWAY_STORE`) and lists
the rejected ones with a reason. `report` prints per-node, per-channel
count/min/max/mean/last and any reading outside the limits from the LDO spec
(`v33` 3.234–3.366 V, `iload` ≤ 250 mA, `temp` −10…60 °C).

See `~/Notes/deploy-runbook.md` for how this runs on app-server.
