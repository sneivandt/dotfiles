#!/usr/bin/env python3
"""Fetch desktop market quotes with a shared, atomically replaced JSON cache."""

import json
from concurrent.futures import ThreadPoolExecutor
import math
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

# The desktop is Linux-only; importing this module remains safe for parser tests.
if sys.platform == "linux":
    import fcntl


QUOTES = (
    ("MSFT", "MSFT", "Microsoft", "$"),
    ("TSLA", "TSLA", "Tesla", "$"),
    ("GOOG", "GOOG", "Alphabet", "$"),
    ("SPCX", "SPCX", "SpaceX", "$"),
    ("BTC-USD", "BTC", "Bitcoin", "$"),
)
CACHE_VERSION = 4
CACHE_TTL = 300
FETCH_WORKERS = 3


def number(value):
    try:
        return type(value) in (int, float) and math.isfinite(value)
    except OverflowError:
        return False


def parse_quote(payload, symbol, fallback_name, prefix, now):
    """Validate upstream data before it reaches QML or the persistent cache."""
    result = payload["chart"]["result"][0]
    meta = result["meta"]
    price = meta.get("regularMarketPrice")
    change = meta.get("regularMarketChangePercent")
    if not number(price) or not number(change):
        raise ValueError("Missing or invalid price/change")
    closes = result.get("indicators", {}).get("quote", [{}])[0].get("close", [])
    timestamps = result.get("timestamp", [])
    history = [value for value in closes if number(value)] or [price]
    history_start = next(
        (stamp for value, stamp in zip(closes, timestamps) if number(value) and number(stamp)),
        meta.get("firstTradeDate", now),
    )
    if not number(history_start):
        history_start = now
    name = meta.get("shortName") or meta.get("longName") or fallback_name
    if not isinstance(name, str) or not name.strip():
        name = fallback_name
    low, high = meta.get("fiftyTwoWeekLow"), meta.get("fiftyTwoWeekHigh")
    year_change = (price - history[0]) / history[0] * 100 if history[0] else 0
    if not number(year_change):
        raise ValueError("Invalid history change")
    return {
        "symbol": symbol, "name": name.strip(), "prefix": prefix,
        "price": price, "change": change, "history": history,
        "historyStart": history_start, "yearChange": round(year_change, 4),
        "yearLow": low if number(low) else min(history),
        "yearHigh": high if number(high) else max(history),
    }


def fetch_quote(query, symbol, fallback_name, prefix, now):
    # Keep curl's bounded HTTP behavior and use Python for JSON and arithmetic.
    result = subprocess.run(
        ["curl", "-fsS", "--max-time", "4", "-H", "User-Agent: Mozilla/5.0",
         f"https://query1.finance.yahoo.com/v8/finance/chart/{query}?interval=1wk&range=1y"],
        stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
        text=True, encoding="utf-8", errors="replace", timeout=6, check=True,
    )
    return parse_quote(json.loads(result.stdout), symbol, fallback_name, prefix, now)


def read_cache(path):
    empty = {"quotes": [], "updated": 0}
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
        if (payload.get("version") != CACHE_VERSION
                or not isinstance(payload.get("quotes"), list)
                or not number(payload.get("updated"))):
            return empty
        valid = [quote for quote in payload["quotes"] if valid_quote(quote)]
        return {"quotes": valid, "updated": payload["updated"]}
    except (OSError, ValueError, AttributeError):
        return empty


def valid_quote(quote):
    """Cached data crosses the same trust boundary as an upstream response."""
    return (isinstance(quote, dict)
            and all(isinstance(quote.get(key), str) and quote[key].strip()
                    for key in ("symbol", "name", "prefix"))
            and all(number(quote.get(key)) for key in
                    ("price", "change", "historyStart", "yearChange", "yearLow", "yearHigh", "updated"))
            and quote["updated"] > 0
            and isinstance(quote.get("history"), list) and bool(quote["history"])
            and all(number(value) for value in quote["history"]))


def fresh_quote(quote, now):
    return 0 <= now - quote["updated"] < CACHE_TTL


def fresh_cache(path, now):
    cached = read_cache(path)
    try:
        if (len(cached["quotes"]) == len(QUOTES)
                and {quote["symbol"] for quote in cached["quotes"]} == {item[1] for item in QUOTES}
                and all(fresh_quote(quote, now) for quote in cached["quotes"])
                and 0 <= now - path.stat().st_mtime < CACHE_TTL):
            return cached
    except OSError:
        pass
    return None


def write_cache(path, payload):
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", dir=path.parent, prefix=".quotes-prices.", delete=False,
        ) as output:
            temporary = Path(output.name)
            json.dump(payload, output, separators=(",", ":"), allow_nan=False)
        os.replace(temporary, path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def quotes(cache_dir, fetch=fetch_quote):
    cache_dir.mkdir(parents=True, exist_ok=True)
    path = cache_dir / "quotes.json"
    cached = fresh_cache(path, time.time())
    if cached is not None:
        return cached
    # Keep the inode: unlinking the lock file would allow simultaneous owners.
    with (cache_dir / "quotes-prices.flock").open("a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return read_cache(path)
        cached = fresh_cache(path, time.time())
        if cached is not None:
            return cached
        now = int(time.time())
        previous = {quote["symbol"]: quote for quote in read_cache(path)["quotes"]}

        def refresh(item):
            query, symbol, name, prefix = item
            cached_quote = previous.get(symbol)
            if cached_quote is not None and fresh_quote(cached_quote, now):
                return cached_quote, False
            try:
                quote = {**fetch(query, symbol, name, prefix, now), "updated": now}
                if valid_quote(quote):
                    return quote, True
            except (OSError, subprocess.SubprocessError, ValueError, KeyError, IndexError, TypeError, AttributeError):
                pass
            return cached_quote, False

        # map preserves configured display order even when requests finish out of order.
        with ThreadPoolExecutor(max_workers=FETCH_WORKERS) as pool:
            refreshed = list(pool.map(refresh, QUOTES))
        if not any(success for _, success in refreshed):
            return read_cache(path)
        results = [quote for quote, _ in refreshed if quote is not None]
        payload = {"version": CACHE_VERSION, "quotes": results,
                   "updated": min(quote["updated"] for quote in results)}
        try:
            write_cache(path, payload)
        except OSError as error:
            print(f"Cannot save market cache: {error.strerror or error}", file=sys.stderr)
        return payload


def main():
    base = os.environ.get("XDG_CACHE_HOME")
    cache_dir = (Path(base) if base and Path(base).is_absolute() else Path.home() / ".cache") / "quickshell-stocks"
    try:
        payload = quotes(cache_dir)
        code = 0
    except OSError as error:
        print(f"Cannot refresh market cache: {error.strerror or error}", file=sys.stderr)
        payload = read_cache(cache_dir / "quotes.json")
        code = 1
    print(json.dumps(payload, separators=(",", ":"), allow_nan=False))
    return code


if __name__ == "__main__":
    sys.exit(main())
