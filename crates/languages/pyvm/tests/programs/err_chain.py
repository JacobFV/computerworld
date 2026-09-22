class ConfigError(Exception):
    """Raised for bad configuration."""


def load(cfg):
    try:
        return int(cfg["port"])
    except KeyError as e:
        raise ConfigError(f"missing key {e}") from e
    except ValueError:
        raise ConfigError("port is not a number")


print(load({"port": "80"}))
try:
    load({})
except ConfigError as e:
    print("handled:", e, "| cause:", repr(e.__cause__))
load({"port": "eighty"})
