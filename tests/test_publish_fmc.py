"""scripts/publish-fmc: the per-model record lookup (no network)."""
import importlib.machinery
import importlib.util
import os
from pathlib import Path
import unittest
from unittest import mock

SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "publish-fmc"


def load(env):
    with mock.patch.dict(os.environ, env, clear=False):
        loader = importlib.machinery.SourceFileLoader("publish_fmc", str(SCRIPT))
        spec = importlib.util.spec_from_loader("publish_fmc", loader)
        mod = importlib.util.module_from_spec(spec)
        loader.exec_module(mod)
    return mod


class PublishFmcModels(unittest.TestCase):
    def test_wp2_records_unchanged(self):
        mod = load({})
        self.assertEqual(mod.live_id("wp2", "patched"), "kqbnulphohzw2wb")
        self.assertEqual(mod.live_id("wp2", "loader"), "bfe60t13x4bgbgc")
        self.assertEqual(mod.BOARD["wp2"], "jx402_01_3089c")

    def test_wp3_needs_its_own_records(self):
        mod = load({"FMC_LIVE_PATCHED_WP3": "", "FMC_LIVE_LOADER_WP3": ""})
        with self.assertRaises(SystemExit) as e:
            mod.live_id("wp3", "patched")
        self.assertIn("FMC_LIVE_PATCHED_WP3", str(e.exception))

    def test_wp3_records_from_env(self):
        mod = load({"FMC_LIVE_PATCHED_WP3": "p3", "FMC_LIVE_LOADER_WP3": "l3"})
        self.assertEqual(mod.live_id("wp3", "patched"), "p3")
        self.assertEqual(mod.live_id("wp3", "loader"), "l3")
        self.assertEqual(mod.BOARD["wp3"], "jx402_08")
        self.assertEqual(mod.LOADER_MODEL, {"wp2": 0, "wp3": 2})

    def test_unknown_model_refused(self):
        with self.assertRaises(SystemExit):
            load({}).live_id("wp9", "patched")


if __name__ == "__main__":
    unittest.main()
