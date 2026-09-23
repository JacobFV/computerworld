# Design review — Atlas sensor node, rev B

**Date:** 2026-09-12 · **Room:** Northstar lab bench 2 · **Attending:** Alice (mechanical), Bob (electrical), Carol (firmware/backend)

## Scope

Rev B of the sensor-node board (`~/Documents/KiCad/sensor-node`) and the mechanical
parts it mounts in (`~/Documents/Parts`): the motor mount bracket, the bearing housing
and the enclosure lid.

## Electrical (Bob)

- USB-C VBUS → 3.3 V LDO. Dropout measured at 0.31 V @ 150 mA, within the spec sheet's
  0.35 V max (see `~/Documents/datasheets/LDO-3v3-spec.txt`).
- Decoupling: 10 µF bulk at the LDO output plus 100 nF at every MCU supply pin. Bob to
  move C4 within 2 mm of pin 48 — currently 5.5 mm, ringing visible on the scope.
- Rev A LED resistor was 330 Ω (too bright at 3.3 V); rev B uses 1 kΩ.
- ERC and DRC clean as of 2026-09-11. Ground zone on the bottom copper; keep-out under
  the USB connector shield tabs.
- Open: add a TVS on VBUS before the pilot batch.

## Mechanical (Alice)

- Motor mount bracket: 6 mm plate, two Ø6.4 slots for M6, 4× fillet R3 on the outer
  corners, linear pattern of the sensor screw holes at 20 mm pitch. Expression binds the
  pattern length to the sketch width so a wider plate keeps the pitch.
- Bearing housing: revolution of the profile, groove for the circlip, 1 mm chamfers on
  both bores. Check press fit: bore Ø22.00 H7 for a 608-size race.
- Enclosure lid: 2 mm wall pocket, 4× M3 hole pattern matching the bracket. Lid clears
  the USB-C connector by 1.8 mm — Alice to confirm after the connector moves.
- Exported STEP of the bracket handed to the machine shop (`motor-mount-bracket.step`).

## Firmware / backend (Carol)

- Board posts a batch of readings every 10 s; `sensor-gateway` validates and stores
  them (`~/code/sensor-gateway`).
- `telemetry` package (`~/project`) summarises bench logs; limits shared with the
  gateway. Both flagged the v33 excursion to 3.41 V on node-02b3 — traced to a loose
  probe ground, not the regulator.

## Actions

| # | Owner | Action | Due |
|---|-------|--------|-----|
| 1 | Bob | Move C4 next to pin 48, re-run DRC | 2026-09-16 |
| 2 | Bob | TVS on VBUS (SMAJ5.0A footprint) | 2026-09-19 |
| 3 | Alice | Confirm lid clearance after connector move | 2026-09-17 |
| 4 | Alice | Re-export bracket STEP for the shop | 2026-09-17 |
| 5 | Carol | Add `rh` channel limits to telemetry | 2026-09-18 |

Next review: 2026-09-26, after the pilot batch of five boards.
