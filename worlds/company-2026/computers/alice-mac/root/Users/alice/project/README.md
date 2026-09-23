# telemetry

Tools for the Atlas sensor-node bring-up: parse the board's CSV measurement logs
and summarise them per channel, then check the numbers against the limits in
`~/Documents/datasheets/LDO-3v3-spec.txt` and the design review notes.

```
python3 -m telemetry.cli summarize data/measurements.csv
python3 -m telemetry.cli check data/measurements.csv --limit=temp=-20:70
python3 -m telemetry.cli limits
```

`main.py` runs the summary of the sample log (the editor's Run button).

## Layout

- `telemetry/parse.py` reads a log into `Sample`s (timestamp, channel, value, unit)
- `telemetry/stats.py` computes `Summary` rows and `check_limits`
- `telemetry/cli.py` is the argparse front end
- `tests/` are `unittest` cases: `make test` or `python3 -m tests.run_all -v`

A limit override is written `--limit=temp=-20:70` (the `=` form).

## Log format

```
timestamp,channel,value,unit
2026-09-15T14:02:10,v33,3.298,V
```

Channels seen on the bench so far: `vin` (USB VBUS), `v33` (LDO output),
`iload` (board current), `temp` (surface thermocouple).
