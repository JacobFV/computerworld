# Lab notebook — Atlas sensor node

Bench 2, Northstar workshop. Instruments: Keysight 34465A DMM (cal 2026-03), Rigol
DHO924S scope, Siglent SPD3303X supply, K-type thermocouple on the DMM.

## 2026-09-08 — LDO dropout, rev A board #3

Supply into VBUS pad, electronic load on the 3.3 V rail. Dropout = Vin at which Vout
falls 2 % (3.234 V).

| I_load (mA) | V_in at dropout (V) | Dropout (V) |
|------------:|--------------------:|------------:|
|          50 |                3.44 |        0.21 |
|         100 |                3.50 |        0.27 |
|         150 |                3.55 |        0.31 |
|         200 |                3.62 |        0.38 |
|         250 |                3.70 |        0.46 |

Spec says 0.35 V max at 150 mA: OK. At 250 mA we exceed the sheet's 0.45 V typical,
but the board budget is 250 mA worst case and VBUS never sits below 4.75 V, so margin
is fine (≥ 1 V).

## 2026-09-09 — Current draw by mode

Board #3, 5.00 V in, DMM in series on VBUS.

| Mode                    | I (mA) | Note                       |
|-------------------------|-------:|----------------------------|
| Reset held              |   4.2  | LDO quiescent + LED off    |
| Idle, radio off         |  18.7  |                            |
| Sampling, 10 s period   |  27.9  | average over 60 s          |
| Sampling + radio burst  | 143.0  | peak, 40 ms                |
| Firmware update         |  96.4  | flash writes               |

## 2026-09-10 — Thermal

Board #3 in the rev A enclosure lid, ambient 23.1 °C, thermocouple on the LDO tab.

| t (min) | T_LDO (°C) | I_load (mA) |
|--------:|-----------:|------------:|
|       0 |       23.1 |         150 |
|       5 |       38.4 |         150 |
|      10 |       44.9 |         150 |
|      15 |       47.6 |         150 |
|      20 |       48.3 |         150 |
|      30 |       48.5 |         150 |

Settles at ~48.5 °C, ΔT ≈ 25 K at 0.26 W → about 96 K/W, close to the sheet's 100 K/W
for the SOT-223 pad we have. Comfortable under the 60 °C surface limit; the lid's vent
slots (Alice) should bring this down further.

## 2026-09-15 — Bench log for the telemetry tool

Logged one minute of `vin`, `v33`, `iload`, `temp` at 10 s → `~/project/data/measurements.csv`.
`python3 -m telemetry.cli check data/measurements.csv` passes. The 3.41 V spike seen
on node-02b3 the day before did not reproduce with a proper ground spring on the probe.

## 2026-09-16 — Bracket fit

Machined bracket from the STEP export: slots and hole pattern land on the housing
within 0.1 mm. Fillets look right; the shop asked for the chamfer on the bore to be
called out on a drawing next time.
