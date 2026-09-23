PYTHON ?= python3

.PHONY: test run check lint clean

test:
	$(PYTHON) -m tests.run_all -v

run:
	$(PYTHON) -m telemetry.cli summarize data/measurements.csv

check:
	$(PYTHON) -m telemetry.cli check data/measurements.csv --limit=temp=-20:70

lint:
	$(PYTHON) -m compileall -q telemetry tests

clean:
	rm -rf __pycache__ telemetry/__pycache__ tests/__pycache__
