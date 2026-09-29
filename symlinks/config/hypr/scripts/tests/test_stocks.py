"""Exercise real flock contention with mocked, always-offline quote requests."""

import json
import os
import shutil
import subprocess
import sys
import unittest

from script_test_support import BASH, ScriptFixture

if sys.platform == "linux":
    import fcntl
    import select


MOCKS = r"""
curl() {
    printf 'request\n' >> "$HOME/requests"
    return 22
}
"""


@unittest.skipUnless(
    sys.platform == "linux" and BASH and shutil.which("flock") and shutil.which("jq"),
    "Requires Linux, Bash, flock and jq",
)
class StocksLockTests(ScriptFixture):
    def setUp(self):
        super().setUp()
        self.cache = self.work / "cache" / "quickshell-stocks"
        self.cache.mkdir(parents=True)
        self.lock = self.cache / "quotes-prices.flock"
        self.cached = {"version": 3, "quotes": [{"symbol": "TEST"}], "updated": 1}

    def write_cache(self, stale=True):
        path = self.cache / "quotes.json"
        path.write_text(json.dumps(self.cached))
        if stale:
            os.utime(path, (1, 1))

    def run_stocks(self):
        result = self.run_script("stocks.sh", MOCKS)
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)

    def test_busy_lock_returns_stale_cache_without_fetching(self):
        self.write_cache()
        with self.lock.open("w") as owner:
            fcntl.flock(owner, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.assertEqual(
                self.run_stocks(), {"quotes": self.cached["quotes"], "updated": 1}
            )
        self.assertFalse((self.work / "requests").exists())

    def test_busy_lock_without_cache_returns_empty(self):
        with self.lock.open("w") as owner:
            fcntl.flock(owner, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.assertEqual(self.run_stocks(), {"quotes": [], "updated": 0})
        self.assertFalse((self.work / "requests").exists())

    def test_flock_errors_are_not_treated_as_contention(self):
        self.write_cache()
        mocks = MOCKS + '\nflock() { return "$FLOCK_STATUS"; }\n'
        for status in (1, 69, 71):
            with self.subTest(status=status):
                result = self.run_script(
                    "stocks.sh", mocks, env={"FLOCK_STATUS": str(status)}
                )
                self.assertEqual(result.returncode, status)
                self.assertIn(f"flock exited {status}", result.stderr)
                self.assertEqual(
                    json.loads(result.stdout),
                    {"quotes": self.cached["quotes"], "updated": 1},
                )
        self.assertFalse((self.work / "requests").exists())

    def test_flock_error_without_cache_still_returns_visible_empty_data(self):
        result = self.run_script("stocks.sh", MOCKS + "\nflock() { return 71; }\n")
        self.assertEqual(result.returncode, 71)
        self.assertIn("flock exited 71", result.stderr)
        self.assertEqual(json.loads(result.stdout), {"quotes": [], "updated": 0})
        self.assertFalse((self.work / "requests").exists())

    def test_missing_flock_is_an_explicit_failure_even_with_fresh_cache(self):
        self.write_cache(stale=False)
        mocks = MOCKS + r"""
command() {
    if [ "$1" = "-v" ] && [ "$2" = "flock" ]; then
        return 1
    fi
    builtin command "$@"
}
"""
        result = self.run_script("stocks.sh", mocks)
        self.assertEqual(result.returncode, 127)
        self.assertIn("flock is required", result.stderr)
        self.assertEqual(
            json.loads(result.stdout), {"quotes": self.cached["quotes"], "updated": 1}
        )
        self.assertFalse(self.lock.exists())
        self.assertFalse((self.work / "requests").exists())

    def test_legacy_orphan_directories_do_not_block_refresh(self):
        for name in ("quotes-prices.lock", "quotes-prices.lock.reap"):
            (self.cache / name).mkdir()
        (self.cache / "quotes-prices.lock" / "pid").write_text("99999999\n")
        self.assertEqual(self.run_stocks(), {"quotes": [], "updated": 0})
        self.assertEqual((self.work / "requests").read_text().splitlines(), ["request"] * 5)
        self.assertTrue(self.lock.is_file())
        with self.lock.open() as owner:
            fcntl.flock(owner, fcntl.LOCK_EX | fcntl.LOCK_NB)

    def test_killed_lock_owner_does_not_leave_stale_lock(self):
        code = (
            "import fcntl, sys; "
            "f = open(sys.argv[1], 'w'); "
            "fcntl.flock(f, fcntl.LOCK_EX); "
            "print('locked', flush=True); sys.stdin.read()"
        )
        owner = subprocess.Popen(
            [sys.executable, "-B", "-c", code, str(self.lock)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True,
        )
        try:
            ready, _, _ = select.select([owner.stdout], [], [], 5)
            self.assertTrue(ready, "Lock owner did not start")
            self.assertEqual(owner.stdout.readline(), "locked\n")
            self.assertEqual(self.run_stocks(), {"quotes": [], "updated": 0})
            self.assertFalse((self.work / "requests").exists())
            owner.kill()
            owner.communicate(timeout=5)
            self.assertEqual(self.run_stocks(), {"quotes": [], "updated": 0})
            self.assertTrue((self.work / "requests").exists())
        finally:
            if owner.poll() is None:
                owner.kill()
            owner.communicate(timeout=5)

    def test_fresh_cache_needs_no_lock_or_fetch(self):
        self.write_cache(stale=False)
        self.assertEqual(
            self.run_stocks(), {"quotes": self.cached["quotes"], "updated": 1}
        )
        self.assertFalse(self.lock.exists())
        self.assertFalse((self.work / "requests").exists())

    def test_cache_refreshed_during_lock_acquisition_is_reused(self):
        mocks = MOCKS + r"""
flock() {
    command flock "$@" || return
    printf '%s' "$NEW_CACHE" > "$XDG_CACHE_HOME/quickshell-stocks/quotes.json"
}
"""
        result = self.run_script(
            "stocks.sh", mocks, env={"NEW_CACHE": json.dumps(self.cached)}
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            json.loads(result.stdout), {"quotes": self.cached["quotes"], "updated": 1}
        )
        self.assertFalse((self.work / "requests").exists())

    def test_failed_refresh_preserves_stale_cache(self):
        self.write_cache()
        self.assertEqual(
            self.run_stocks(), {"quotes": self.cached["quotes"], "updated": 1}
        )
        self.assertEqual(json.loads((self.cache / "quotes.json").read_text()), self.cached)


if __name__ == "__main__":
    unittest.main()
