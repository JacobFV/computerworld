"""The isolation gate follows packages through grouping and module extraction."""
from pathlib import Path
import subprocess
import tempfile
import unittest

CHECKER = Path(__file__).with_name('check-boundaries.py')


class BoundaryTests(unittest.TestCase):
    def check_tree(self, files):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            tool = root / 'scripts/checks/check-boundaries.py'
            tool.parent.mkdir(parents=True)
            tool.write_bytes(CHECKER.read_bytes())
            for name, content in files.items():
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(content)
            return subprocess.run(['python3', str(tool)], capture_output=True, text=True)

    def test_new_nested_simulation_package_is_checked(self):
        result = self.check_tree({
            'crates/core/new-engine/Cargo.toml': '[package]\nname = "cw-new-engine"\n',
            'crates/core/new-engine/src/lib.rs': 'fn read() { std::fs::read("x"); }',
        })
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('crates/core/new-engine/src/lib.rs', result.stderr)

    def test_nested_dependency_alias_cannot_hide_host_io(self):
        result = self.check_tree({
            'crates/web/engine/Cargo.toml': '[package]\nname = "cw-web"\n[dependencies]\nhttp = { package = "reqwest", version = "1" }\n',
            'crates/web/engine/src/lib.rs': '',
        })
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('forbidden dependency reqwest', result.stderr)

    def test_host_adapter_is_an_explicit_exception(self):
        result = self.check_tree({
            'crates/machines/host-adapters/Cargo.toml': '[package]\nname = "cw-host-adapters"\n',
            'crates/machines/host-adapters/src/lib.rs': 'fn read() { std::fs::read("x"); }',
        })
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_module_named_tests_without_cfg_is_not_exempt(self):
        result = self.check_tree({
            'crates/core/example/Cargo.toml': '[package]\nname = "cw-example"\n',
            'crates/core/example/src/lib.rs': 'mod tests;\n',
            'crates/core/example/src/tests.rs': 'fn read() { std::fs::read("x"); }',
        })
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('tests.rs', result.stderr)

    def test_external_test_module_is_exempt_but_runtime_sibling_is_not(self):
        files = {
            'crates/core/example/Cargo.toml': '[package]\nname = "cw-example"\n',
            'crates/core/example/src/lib.rs': '#[cfg(test)]\nmod tests;\n',
            'crates/core/example/src/tests.rs': 'fn fixture() { std::fs::read("x"); }',
        }
        result = self.check_tree(files)
        self.assertEqual(result.returncode, 0, result.stderr)
        files['crates/core/example/src/runtime.rs'] = 'fn read() { std::fs::read("x"); }'
        result = self.check_tree(files)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('runtime.rs', result.stderr)


if __name__ == '__main__':
    unittest.main()
