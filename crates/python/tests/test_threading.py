"""The Python handles are safe to use from another thread.

Run against an installed wheel:

    python -m pip install .
    python crates/python/tests/test_threading.py

Earlier alpha builds marked the classes `#[pyclass(unsendable)]`, so touching an
`Environment` from a second thread aborted the interpreter with a pyo3 panic instead of
raising. This asserts the two halves of the replacement: a call from another thread
succeeds, and a world can never take the host process down with it.
"""
import json
import sys
import threading
from pathlib import Path

from computerworld import World

ROOT = Path(__file__).resolve().parents[3]
DEFINITION = json.loads(
    (ROOT / "examples/worlds/agent-desktop.json").read_text(encoding="utf-8")
)
MACHINE = "workstation"


def session():
    world = World(DEFINITION, 7)
    env = world.environment(DEFINITION["metadata"]["actor_session"])
    return world, env


def call_on_thread(work):
    """Run `work()` on a fresh thread, returning its value or re-raising its error."""
    box = {}

    def run():
        try:
            box["value"] = work()
        except BaseException as error:  # noqa: BLE001 - re-raised on the caller's thread
            box["error"] = error

    thread = threading.Thread(target=run)
    thread.start()
    thread.join()
    if "error" in box:
        raise box["error"]
    assert "value" in box, "the worker thread died without an exception"
    return box["value"]


def test_render_from_another_thread():
    _world, env = session()
    frame = call_on_thread(lambda: env.render(320, 240))
    assert frame["width"] == 320 and frame["height"] == 240, frame
    assert len(frame["rgba"]) == 320 * 240 * 4


def test_every_environment_method_from_another_thread():
    _world, env = session()
    action = [
        dict(
            family="terminal.v1",
            op="execute",
            machine=MACHINE,
            payload={"command": "echo threaded"},
        )
    ]
    result = call_on_thread(lambda: env.step(action))
    assert result["outcomes"][0]["success"], result
    assert "threaded" in json.dumps(result), result
    assert call_on_thread(env.observe) is not None
    assert isinstance(call_on_thread(lambda: env.scene(800, 600)), dict)


def test_owner_methods_from_another_thread():
    world, env = session()
    before = call_on_thread(world.state_hash)
    call_on_thread(
        lambda: env.step(
            [
                dict(
                    family="terminal.v1",
                    op="execute",
                    machine=MACHINE,
                    payload={"command": "echo one > /tmp/one"},
                )
            ]
        )
    )
    assert call_on_thread(world.state_hash) != before
    checkpoint = call_on_thread(world.snapshot)
    assert call_on_thread(lambda: world.fork(checkpoint)) is not None


def test_a_refused_call_raises_rather_than_aborting():
    """A bad call from a worker thread is an exception, not a dead interpreter."""
    _world, env = session()
    try:
        call_on_thread(lambda: env.step("not a list of actions"))
    except Exception as error:  # noqa: BLE001 - any Python exception is the point
        assert not isinstance(error, SystemExit), error
    else:
        raise AssertionError("an invalid batch should have raised")


def test_two_threads_share_one_world_without_losing_actions():
    """Calls are serialized, not concurrent: every action lands exactly once."""
    _world, env = session()

    def hammer(tag):
        for i in range(10):
            result = env.step(
                [
                    dict(
                        family="terminal.v1",
                        op="execute",
                        machine=MACHINE,
                        payload={"command": f"echo {tag}{i} >> /tmp/log"},
                    )
                ]
            )
            assert result["outcomes"][0]["success"], result

    threads = [threading.Thread(target=hammer, args=(tag,)) for tag in ("a", "b")]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
    written = env.step(
        [
            dict(
                family="terminal.v1",
                op="execute",
                machine=MACHINE,
                payload={"command": "wc -l < /tmp/log"},
            )
        ]
    )
    assert written["outcomes"][0]["value"]["stdout"].strip() == "20", written


if __name__ == "__main__":
    tests = [value for name, value in sorted(globals().items()) if name.startswith("test_")]
    for test in tests:
        test()
        print(f"ok {test.__name__}")
    print(f"{len(tests)} threading checks passed")
    sys.exit(0)
