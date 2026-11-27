#!/usr/bin/env python3
"""Fetch desktop market quotes with a shared, atomically replaced JSON cache."""

import argparse
from concurrent.futures import ThreadPoolExecutor
import contextlib
import json
import math
import os
from pathlib import Path
import re
import sys
import tempfile
import time
import urllib.error
import urllib.request

# The desktop is Linux-only; importing this module remains safe for parser tests.
if sys.platform == "linux":
    import fcntl


DEFAULT_QUOTES = (
    ("MSFT", "MSFT", "Microsoft", "$"),
    ("TSLA", "TSLA", "Tesla", "$"),
    ("GOOG", "GOOG", "Alphabet", "$"),
    ("BTC-USD", "BTC", "Bitcoin", "$"),
)
QUOTES = DEFAULT_QUOTES
_TICKER_RE = re.compile(r"^[A-Za-z0-9.^=-]{1,20}$")
CACHE_VERSION = 4
CACHE_TTL = 300
FETCH_WORKERS = 3
FETCH_TIMEOUT = 4
MAX_RESPONSE_BYTES = 1024 * 1024


def config_path():
    override = os.environ.get("DOTFILES_STOCKS_CONFIG")
    if override:
        return Path(override)
    base = os.environ.get("XDG_CONFIG_HOME")
    config_root = Path(base) if base and Path(base).is_absolute() else Path.home() / ".config"
    return config_root / "quickshell-stocks.json"


def _parse_configured_quotes(raw):
    items = raw.get("quotes") if isinstance(raw, dict) else raw
    if not isinstance(items, list) or not items:
        return None
    parsed = []
    seen = set()
    for entry in items:
        if not isinstance(entry, dict):
            return None
        query = entry.get("query")
        if not isinstance(query, str):
            return None
        query = query.strip()
        if not _TICKER_RE.match(query):
            return None
        symbol = entry.get("symbol", query)
        name = entry.get("name", symbol)
        prefix = entry.get("prefix", "$")
        if not all(isinstance(value, str) and value.strip() for value in (symbol, name, prefix)):
            return None
        symbol, name, prefix = symbol.strip(), name.strip(), prefix.strip()
        if symbol in seen:
            return None
        seen.add(symbol)
        parsed.append((query, symbol, name, prefix))
    return tuple(parsed)


def load_quotes(path=None):
    target = config_path() if path is None else Path(path)
    try:
        raw = json.loads(target.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return QUOTES
    except (OSError, ValueError) as error:
        print(f"Ignoring invalid market config {target}: {error}", file=sys.stderr)
        return QUOTES
    parsed = _parse_configured_quotes(raw)
    if parsed is None:
        print(
            f"Ignoring invalid market config {target}: expected a non-empty list of quote objects",
            file=sys.stderr,
        )
        return QUOTES
    return parsed


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
        (
            stamp
            for value, stamp in zip(closes, timestamps, strict=False)
            if number(value) and number(stamp)
        ),
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
        "symbol": symbol,
        "name": name.strip(),
        "prefix": prefix,
        "price": price,
        "change": change,
        "history": history,
        "historyStart": history_start,
        "yearChange": round(year_change, 4),
        "yearLow": low if number(low) else min(history),
        "yearHigh": high if number(high) else max(history),
    }


def fetch_quote(query, symbol, fallback_name, prefix, now):
    request = urllib.request.Request(
        f"https://query1.finance.yahoo.com/v8/finance/chart/{query}?interval=1wk&range=1y",
        headers={"User-Agent": "Mozilla/5.0"},
    )
    with urllib.request.urlopen(request, timeout=FETCH_TIMEOUT) as response:
        body = response.read(MAX_RESPONSE_BYTES + 1)
    if len(body) > MAX_RESPONSE_BYTES:
        raise ValueError("Upstream market response exceeded size limit")
    return parse_quote(
        json.loads(body.decode("utf-8", errors="replace")), symbol, fallback_name, prefix, now
    )


def read_cache(path):
    empty = {"quotes": [], "updated": 0}
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
        if (
            payload.get("version") != CACHE_VERSION
            or not isinstance(payload.get("quotes"), list)
            or not number(payload.get("updated"))
        ):
            return empty
        valid = [quote for quote in payload["quotes"] if valid_quote(quote)]
        return {"quotes": valid, "updated": payload["updated"]}
    except (OSError, ValueError, AttributeError):
        return empty


def valid_quote(quote):
    """Cached data crosses the same trust boundary as an upstream response."""
    return (
        isinstance(quote, dict)
        and all(
            isinstance(quote.get(key), str) and quote[key].strip()
            for key in ("symbol", "name", "prefix")
        )
        and all(
            number(quote.get(key))
            for key in (
                "price",
                "change",
                "historyStart",
                "yearChange",
                "yearLow",
                "yearHigh",
                "updated",
            )
        )
        and quote["updated"] > 0
        and isinstance(quote.get("history"), list)
        and bool(quote["history"])
        and all(number(value) for value in quote["history"])
    )


def fresh_quote(quote, now):
    return 0 <= now - quote["updated"] < CACHE_TTL


def fresh_cache(path, now, configured=None):
    quote_specs = QUOTES if configured is None else configured
    cached = read_cache(path)
    try:
        if (
            len(cached["quotes"]) == len(quote_specs)
            and {quote["symbol"] for quote in cached["quotes"]} == {item[1] for item in quote_specs}
            and all(fresh_quote(quote, now) for quote in cached["quotes"])
            and 0 <= now - path.stat().st_mtime < CACHE_TTL
        ):
            return cached
    except OSError:
        pass
    return None


def write_cache(path, payload):
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            dir=path.parent,
            prefix=".quotes-prices.",
            delete=False,
        ) as output:
            temporary = Path(output.name)
            json.dump(payload, output, separators=(",", ":"), allow_nan=False)
        os.replace(temporary, path)
        temporary = None
    finally:
        if temporary is not None:
            with contextlib.suppress(OSError):
                temporary.unlink(missing_ok=True)


def quotes(cache_dir, fetch=None, force=False, configured=None):
    quote_specs = QUOTES if configured is None else configured
    if fetch is None:
        fetch = fetch_quote
    cache_dir.mkdir(parents=True, exist_ok=True)
    path = cache_dir / "quotes.json"
    if not force:
        cached = fresh_cache(path, time.time(), configured=quote_specs)
        if cached is not None:
            return cached
    # Keep the inode: unlinking the lock file would allow simultaneous owners.
    with (cache_dir / "quotes-prices.flock").open("a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            cached = read_cache(path)
            if force:
                error = (
                    "Market data refresh is already in progress. Showing cached prices."
                    if cached["quotes"]
                    else "Market data refresh is already in progress."
                )
                return {**cached, "error": error}
            return cached
        if not force:
            cached = fresh_cache(path, time.time(), configured=quote_specs)
            if cached is not None:
                return cached
        now = int(time.time())
        previous = {quote["symbol"]: quote for quote in read_cache(path)["quotes"]}

        def refresh(item):
            query, symbol, name, prefix = item
            cached_quote = previous.get(symbol)
            if not force and cached_quote is not None and fresh_quote(cached_quote, now):
                return cached_quote, False, None
            try:
                quote = {**fetch(query, symbol, name, prefix, now), "updated": now}
                if valid_quote(quote):
                    return quote, True, None
                reason = "invalid quote payload"
            except (
                OSError,
                urllib.error.URLError,
                ValueError,
                KeyError,
                IndexError,
                TypeError,
                AttributeError,
            ) as error:
                reason = str(error) or type(error).__name__
            return cached_quote, False, f"Failed to fetch quote for {symbol}: {reason}"

        # map preserves configured display order even when requests finish out of order.
        with ThreadPoolExecutor(max_workers=FETCH_WORKERS) as pool:
            refreshed = list(pool.map(refresh, quote_specs))
        failures = [failure for _, _, failure in refreshed if failure]
        if not any(success for _, success, _ in refreshed):
            cached = read_cache(path)
            if force:
                error = (
                    "Could not refresh market data. Showing cached prices."
                    if cached["quotes"]
                    else "Could not refresh market data."
                )
                return {**cached, "error": error}
            for failure in failures:
                print(failure, file=sys.stderr)
            return cached
        for failure in failures:
            print(failure, file=sys.stderr)
        results = [quote for quote, _, _ in refreshed if quote is not None]
        payload = {
            "version": CACHE_VERSION,
            "quotes": results,
            "updated": min(quote["updated"] for quote in results),
        }
        try:
            write_cache(path, payload)
        except OSError as error:
            print(f"Cannot save market cache: {error.strerror or error}", file=sys.stderr)
        return payload


def main(argv=()):
    parser = argparse.ArgumentParser(description="Fetch desktop market quotes.")
    parser.add_argument(
        "--force", action="store_true", help="Bypass fresh cache and refetch quotes."
    )
    args = parser.parse_args(list(argv))
    configured = load_quotes()
    base = os.environ.get("XDG_CACHE_HOME")
    cache_dir = (
        Path(base) if base and Path(base).is_absolute() else Path.home() / ".cache"
    ) / "quickshell-stocks"
    try:
        kwargs = {}
        if args.force:
            kwargs["force"] = True
        if configured != QUOTES:
            kwargs["configured"] = configured
        payload = quotes(cache_dir, **kwargs)
        if payload.get("error"):
            print(payload["error"], file=sys.stderr)
            code = 1
        else:
            code = 0
    except OSError as error:
        print(f"Cannot refresh market cache: {error.strerror or error}", file=sys.stderr)
        payload = read_cache(cache_dir / "quotes.json")
        code = 1
    available = {quote["symbol"] for quote in payload["quotes"]}
    missing = [symbol for _, symbol, _, _ in configured if symbol not in available]
    print(json.dumps({**payload, "missing": missing}, separators=(",", ":"), allow_nan=False))
    return code


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
