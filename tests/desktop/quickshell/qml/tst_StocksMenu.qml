import QtQuick
import QtTest
import Quickshell
import "../../../../symlinks/config/quickshell/menus" as Menus
import "../../../../symlinks/config/quickshell/lib/Theme.js" as Theme

Item {
    width: 480
    height: 700

    Component {
        id: stocksMenuComponent

        Menus.StocksMenu {
            anchorItem: null
            currentDate: new Date(Date.UTC(2026, 8, 19, 14, 0, 0))
        }
    }

    Component {
        id: stockDetailsComponent

        Menus.StockDetails {
            width: 360
        }
    }

    TestCase {
        name: "StocksMenu"
        when: windowShown

        function initTestCase() {
            verify(Quickshell.testMode === true, "Run with -import tests/qml/mocks; live Quickshell services are forbidden.");
        }

        function sampleQuotes(updatedSeconds) {
            return [
                {
                    symbol: "MSFT",
                    name: "Microsoft",
                    prefix: "$",
                    price: 425.5,
                    change: 1.25,
                    yearChange: 18.4,
                    yearLow: 360.0,
                    yearHigh: 468.0,
                    updated: updatedSeconds,
                    historyStart: updatedSeconds - 365 * 24 * 60 * 60,
                    history: [370, 395, 425.5]
                },
                {
                    symbol: "NVDA",
                    name: "NVIDIA",
                    prefix: "$",
                    price: 120.0,
                    change: -2.5,
                    yearChange: -5.0,
                    yearLow: 90.0,
                    yearHigh: 150.0,
                    updated: updatedSeconds,
                    historyStart: updatedSeconds - 60 * 24 * 60 * 60,
                    history: [128, 124, 120]
                }
            ];
        }

        function test_status_messages_for_loading_stale_error_and_missing_symbols() {
            const menu = createTemporaryObject(stocksMenuComponent, parent);
            verify(menu !== null);
            const nowSec = Math.floor(menu.currentDate.getTime() / 1000);

            compare(menu.statusText, "No quote data available");

            menu.loading = true;
            compare(menu.statusText, "Loading quotes...");

            menu.quotes = sampleQuotes(nowSec - 60);
            menu.updated = nowSec - 60;
            compare(menu.statusText, "Refreshing quotes...");

            menu.loading = false;
            compare(menu.stale, false);
            compare(menu.statusText, "");

            menu.updated = nowSec - 20 * 60;
            compare(menu.stale, true);
            compare(menu.statusText, "Quotes may be out of date.");

            menu.missingSymbols = ["AAPL", "GOOG"];
            compare(menu.statusText, "Quotes may be out of date.\nQuotes unavailable for AAPL, GOOG.");

            menu.error = "Quote feed unreachable";
            compare(menu.statusText, "Quote feed unreachable\nQuotes unavailable for AAPL, GOOG.");
        }

        function test_expand_collapse_and_closing_resets_expanded_symbol() {
            const nowSec = Math.floor(Date.UTC(2026, 8, 19, 14, 0, 0) / 1000);
            const menu = createTemporaryObject(stocksMenuComponent, parent, {
                quotes: sampleQuotes(nowSec - 30),
                updated: nowSec - 30
            });
            verify(menu !== null);
            menu.visible = true;
            tryCompare(menu, "reveal", 1);

            menu.expandedSymbol = "MSFT";
            compare(menu.expandedSymbol, "MSFT");

            menu.visible = false;
            compare(menu.expandedSymbol, "");
        }

        function test_stock_details_range_position_and_formatting() {
            const details = createTemporaryObject(stockDetailsComponent, parent, {
                quote: {
                    symbol: "MSFT",
                    prefix: "$",
                    price: 414.0,
                    yearChange: 12.345,
                    yearLow: 360.0,
                    yearHigh: 468.0,
                    historyStart: 0,
                    history: [360, 400, 414]
                }
            });
            verify(details !== null);

            compare(details.hasHistory, true);
            compare(details.hasRange, true);
            compare(details.rangePosition, 0.5);
            compare(details.accentColor, Qt.color(Theme.green));
            compare(details.periodLabel(), "1 year");
            compare(details.price(1234.5), "1,234.50");
            compare(details.percent(12.345), "+12.35%");
            compare(details.percent(-3.2), "-3.20%");
            verify(findChild(details, "stockRangeMarker") !== null);

            details.quote = {
                symbol: "FLAT",
                prefix: "$",
                price: 500.0,
                yearChange: 0,
                yearLow: 100.0,
                yearHigh: 100.0,
                historyStart: 0,
                history: [100]
            };
            compare(details.hasHistory, false);
            compare(details.hasRange, false);
            compare(details.rangePosition, 0);
            compare(details.accentColor, Qt.color(Theme.mutedStrong));
        }
    }
}
