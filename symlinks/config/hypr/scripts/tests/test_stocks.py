"""Market parsing and isolated cache lifecycle; never query the live market."""

import copy
import importlib.util
import json
import os
import select
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / "stocks.py"
spec = importlib.util.spec_from_file_location("stocks", SCRIPT)
stocks = importlib.util.module_from_spec(spec)
spec.loader.exec_module(stocks)

PAYLOAD = {
    "chart": {"result": [{
        "meta": {"regularMarketPrice": 110, "regularMarketChangePercent": 5.5,
                 "shortName": "Test Instrument ", "fiftyTwoWeekLow": 70, "fiftyTwoWeekHigh": 120},
        "timestamp": [1000, 2000, 3000, 4000],
        "indicators": {"quote": [{"close": [80, None, 100, 110]}]},
    }]},
}


class QuoteTests(unittest.TestCase):
    def parse(self, payload):
        return stocks.parse_quote(payload, "TEST", "Fallback", "$", 5000)

    def test_quote_history_and_metadata(self):
        quote = self.parse(PAYLOAD)
        self.assertEqual(quote, {
            "symbol": "TEST", "name": "Test Instrument", "prefix": "$",
            "price": 110, "change": 5.5, "history": [80, 100, 110],
            "historyStart": 1000, "yearChange": 37.5, "yearLow": 70, "yearHigh": 120,
        })

    def test_missing_history_and_metadata_fall_back(self):
        payload = {"chart": {"result": [{"meta": {
            "regularMarketPrice": 10, "regularMarketChangePercent": 0,
        }}]}}
        quote = self.parse(payload)
        self.assertEqual(quote["name"], "Fallback")
        self.assertEqual(quote["history"], [10])
        self.assertEqual(quote["historyStart"], 5000)
        self.assertEqual(quote["yearLow"], 10)
        self.assertEqual(quote["yearHigh"], 10)

    def test_invalid_price_never_reaches_ui(self):
        for price in (None, "110", True, float("nan"), float("inf")):
            payload = copy.deepcopy(PAYLOAD)
            payload["chart"]["result"][0]["meta"]["regularMarketPrice"] = price
            with self.subTest(price=price), self.assertRaises(ValueError):
                self.parse(payload)

    def test_fetch_uses_argv_and_bounded_process(self):
        with patch.object(stocks.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, json.dumps(PAYLOAD))) as run:
            quote = stocks.fetch_quote("TEST", "TEST", "Fallback", "$", 5000)
        self.assertEqual(quote["price"], 110)
        self.assertEqual(run.call_args.args[0][0], "curl")
        self.assertEqual(run.call_args.kwargs["timeout"], 6)
        self.assertTrue(run.call_args.kwargs["check"])


@unittest.skipUnless(sys.platform == "linux", "Linux desktop cache uses flock")
class StocksCacheTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.cache = Path(temporary.name)
        self.path = self.cache / "quotes.json"
        self.lock = self.cache / "quotes-prices.flock"
        self.cached = {"version": stocks.CACHE_VERSION, "quotes": [
            {**stocks.parse_quote(PAYLOAD, symbol, name, prefix, 5000), "updated": 1}
            for _, symbol, name, prefix in stocks.QUOTES], "updated": 1}
        self.fetch = unittest.mock.Mock(side_effect=OSError("offline"))

    def write_cache(self, stale=True):
        if not stale:
            self.cached["updated"] = int(time.time())
            for quote in self.cached["quotes"]:
                quote["updated"] = self.cached["updated"]
        self.path.write_text(json.dumps(self.cached))
        if stale:
            os.utime(self.path, (1, 1))

    def test_busy_lock_returns_stale_cache_without_fetching(self):
        self.write_cache()
        with self.lock.open("a") as owner:
            stocks.fcntl.flock(owner, stocks.fcntl.LOCK_EX | stocks.fcntl.LOCK_NB)
            result = stocks.quotes(self.cache, self.fetch)
        self.assertEqual(result, {"quotes": self.cached["quotes"], "updated": 1})
        self.fetch.assert_not_called()

    def test_busy_lock_without_cache_returns_empty(self):
        with self.lock.open("a") as owner:
            stocks.fcntl.flock(owner, stocks.fcntl.LOCK_EX | stocks.fcntl.LOCK_NB)
            self.assertEqual(stocks.quotes(self.cache, self.fetch), {"quotes": [], "updated": 0})
        self.fetch.assert_not_called()

    def test_lock_errors_are_not_treated_as_contention(self):
        with patch.object(stocks.fcntl, "flock", side_effect=OSError("lock failed")):
            with self.assertRaises(OSError):
                stocks.quotes(self.cache, self.fetch)
        self.fetch.assert_not_called()

    def test_killed_owner_releases_lock_without_cleanup(self):
        code = (
            "import fcntl, sys; f = open(sys.argv[1], 'a'); "
            "fcntl.flock(f, fcntl.LOCK_EX); "
            "print('locked', flush=True); sys.stdin.read()"
        )
        owner = subprocess.Popen(
            [sys.executable, "-B", "-c", code, str(self.lock)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        )
        try:
            readable, _, _ = select.select([owner.stdout], [], [], 5)
            self.assertTrue(readable, "Lock owner did not start")
            self.assertEqual(owner.stdout.readline(), "locked\n")
            stocks.quotes(self.cache, self.fetch)
            self.fetch.assert_not_called()
            owner.kill()
            owner.communicate(timeout=5)
            stocks.quotes(self.cache, self.fetch)
            self.assertEqual(self.fetch.call_count, 5)
        finally:
            if owner.poll() is None:
                owner.kill()
            owner.communicate(timeout=5)

    def test_fresh_cache_needs_no_lock_or_fetch(self):
        self.write_cache(stale=False)
        self.assertEqual(stocks.quotes(self.cache, self.fetch)["updated"], self.cached["updated"])
        self.fetch.assert_not_called()
        self.assertFalse(self.lock.exists())

    def test_cache_refreshed_during_lock_acquisition_is_reused(self):
        lock = stocks.fcntl.flock

        def acquire(*args):
            lock(*args)
            self.write_cache(stale=False)

        with patch.object(stocks.fcntl, "flock", side_effect=acquire):
            self.assertEqual(stocks.quotes(self.cache, self.fetch)["updated"], self.cached["updated"])
        self.fetch.assert_not_called()

    def test_failed_refresh_preserves_stale_cache(self):
        self.write_cache()
        self.assertEqual(stocks.quotes(self.cache, self.fetch)["updated"], self.cached["updated"])
        self.assertEqual(self.fetch.call_count, 5)
        self.assertEqual(json.loads(self.path.read_text()), self.cached)

    def test_partial_failure_retains_previous_quotes_and_their_timestamps(self):
        self.write_cache()
        def fetch(query, symbol, name, prefix, now):
            if symbol == "MSFT":
                raise OSError("offline")
            return stocks.parse_quote(PAYLOAD, symbol, name, prefix, now)
        result = stocks.quotes(self.cache, fetch)
        self.assertEqual([q["symbol"] for q in result["quotes"]], [q[1] for q in stocks.QUOTES])
        self.assertEqual(result["quotes"][0], self.cached["quotes"][0])
        self.assertTrue(all(q["updated"] > 1 for q in result["quotes"][1:]))
        self.assertEqual(result["updated"], 1)
        self.assertIsNone(stocks.fresh_cache(self.path, time.time()))
        self.assertEqual(result, json.loads(self.path.read_text()))
        self.assertEqual(list(self.cache.glob(".quotes-prices.*")), [])

    def test_parallel_fetches_are_bounded_and_preserve_order(self):
        barrier = threading.Barrier(stocks.FETCH_WORKERS)
        lock = threading.Lock()
        active = peak = 0
        def fetch(query, symbol, name, prefix, now):
            nonlocal active, peak
            with lock:
                active += 1
                peak = max(peak, active)
            try:
                if symbol in [q[1] for q in stocks.QUOTES[:stocks.FETCH_WORKERS]]:
                    barrier.wait(timeout=5)
                return stocks.parse_quote(PAYLOAD, symbol, name, prefix, now)
            finally:
                with lock:
                    active -= 1
        result = stocks.quotes(self.cache, fetch)
        self.assertEqual(peak, stocks.FETCH_WORKERS)
        self.assertEqual([q["symbol"] for q in result["quotes"]], [q[1] for q in stocks.QUOTES])

    def test_invalid_cached_quotes_are_filtered_and_trigger_refresh(self):
        self.write_cache(stale=False)
        good = self.cached["quotes"][0]
        for bad in (None, {}, {**good, "price": float("nan")},
                    {**good, "history": [float("inf")]}, {**good, "updated": True},
                    {**good, "name": 42}):
            with self.subTest(bad=bad):
                self.path.write_text(json.dumps({**self.cached, "quotes": [good, bad]}))
                self.assertEqual(stocks.read_cache(self.path)["quotes"], [good])
                self.assertIsNone(stocks.fresh_cache(self.path, time.time()))
                json.dumps(stocks.read_cache(self.path), allow_nan=False)

    def test_failed_atomic_replace_preserves_previous_cache_and_cleans_temp(self):
        self.write_cache()
        with patch.object(stocks.os, "replace", side_effect=OSError("write failed")):
            with self.assertRaises(OSError):
                stocks.write_cache(self.path, {"quotes": []})
        self.assertEqual(json.loads(self.path.read_text()), self.cached)
        self.assertEqual(list(self.cache.glob(".quotes-prices.*")), [])

    def test_corrupt_or_incompatible_cache_is_ignored(self):
        for text in ("not json", "[]", '{"version":2}', '{"version":3,"quotes":[],"updated":"bad"}'):
            self.path.write_text(text)
            self.assertEqual(stocks.quotes(self.cache, self.fetch), {"quotes": [], "updated": 0})

    def test_main_reports_cache_failure_with_stale_payload(self):
        self.write_cache()
        with patch.dict(os.environ, {"XDG_CACHE_HOME": str(self.cache.parent)}), \
             patch.object(stocks, "quotes", side_effect=OSError("lock failed")), \
             patch.object(stocks, "read_cache", return_value=self.cached), \
             patch("builtins.print") as output:
            self.assertEqual(stocks.main(), 1)
        self.assertIn("Cannot refresh market cache", output.call_args_list[0].args[0])


if __name__ == "__main__":
    unittest.main()
