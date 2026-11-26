import QtQuick
import QtTest
import Quickshell
import "../../../../symlinks/config/quickshell/state" as State
import "../../../../symlinks/config/quickshell/menus" as Menus

Item {
    width: 500
    height: 700

    Component {
        id: networkComponent
        State.NetworkState {}
    }
    Component {
        id: marketComponent
        State.MarketState {}
    }
    Component {
        id: stockCardComponent
        Menus.StockCard {}
    }
    Component {
        id: powerComponent
        Menus.PowerMenu {
            anchorItem: null
        }
    }
    Component {
        id: spyComponent
        SignalSpy {}
    }

    TestCase {
        name: "ServiceLifecycle"
        when: windowShown

        function initTestCase() {
            verify(Quickshell.testMode === true, "Run with -import tests/qml/mocks; live Quickshell services are forbidden.");
        }

        function make(component) {
            const object = createTemporaryObject(component, parent);
            verify(object !== null);
            return object;
        }

        function process(object, name) {
            const result = findChild(object, name);
            verify(result !== null);
            return result;
        }

        function statusResult() {
            return JSON.stringify({
                ok: true,
                state: {
                    connected: true,
                    connectionName: "Fixture Wi-Fi",
                    connectionType: "wifi",
                    deviceName: "wlan-test",
                    wifiEnabled: true,
                    wifiHardwareEnabled: true,
                    wifiAvailable: true,
                    adapters: [{ name: "wlan-test", managed: true }],
                    networks: [],
                    connections: [{ name: "Fixture Wi-Fi", type: "wifi", device: "wlan-test", uuid: "fixture" }],
                    connectivity: "full"
                }
            });
        }

        function readyNetwork() {
            const network = make(networkComponent);
            const query = process(network, "networkStatusProcess");
            query.finish(0, JSON.stringify({ ok: true, state: {connected: false, connectionName: "", connectionType: ""} }));
            network.lastScan = Date.now();
            network.menuOpen = true;
            query.finish(0, statusResult());
            compare(network.available, true);
            compare(network.detailsAvailable, true);
            compare(network.loading, false);
            compare(network.error, "");
            return network;
        }

        function test_open_during_summary_waits_for_details_then_scans_data() {
            return [{ tag: "summary-ok", failed: false }, { tag: "summary-failed-to-start", failed: true }];
        }

        function test_open_during_summary_waits_for_details_then_scans(data) {
            const network = make(networkComponent);
            const query = process(network, "networkStatusProcess");
            const action = process(network, "networkActionProcess");
            compare(query.command.slice(0, 2), ["python3", "-B"]);
            compare(action.command.slice(0, 2), ["python3", "-B"]);
            compare(query.command[3], "summary");
            network.menuOpen = true;
            network.refresh();
            if (data.failed) {
                query.failStart();
            } else {
                query.finish(0, JSON.stringify({ ok: true, state: {connected: true, connectionName: "Wired", connectionType: "ethernet"} }));
                compare(network.available, true);
                compare(network.connectionName, "Wired");
                compare(network.connectionType, "ethernet");
            }
            compare(network.detailsAvailable, false);
            compare(action.attempts, 0);
            tryCompare(query, "attempts", 2);
            compare(query.command[3], "status");
            query.finish(0, statusResult());
            compare(network.detailsAvailable, true);
            compare(network.scanning, true);
            network.refresh();
            compare(query.attempts, 2);
            // A close during the scan downgrades the coalesced post-action refresh.
            network.menuOpen = false;
            action.started();
            action.finish(0, JSON.stringify({ ok: true }));
            tryCompare(query, "attempts", 3);
            compare(query.command[3], "summary");
            compare(network.detailsAvailable, false);
        }

        function test_reopening_ignores_previous_visits_in_flight_details() {
            const network = readyNetwork();
            const query = process(network, "networkStatusProcess");
            network.refresh();
            compare(query.command[3], "status");
            network.menuOpen = false;
            network.menuOpen = true;
            query.finish(0, statusResult());
            compare(network.detailsAvailable, false);
            compare(network.connections[0].name, "Fixture Wi-Fi"); // last known, not actionable
            network.connectNetwork({ name: "Fixture", advanced: false }, "", "");
            compare(process(network, "networkActionProcess").attempts, 0);
            tryCompare(query, "attempts", 4);
            compare(query.command[3], "status");
            query.finish(0, statusResult());
            compare(network.detailsAvailable, true);
        }

        function test_initial_queries_report_start_failures() {
            const network = make(networkComponent);
            const market = make(marketComponent);
            market.menuOpen = true;
            process(network, "networkStatusProcess").failStart();
            process(market, "marketQueryProcess").failStart();
            compare(network.available, false);
            compare(network.loading, false);
            verify(network.error.includes("Cannot start"));
            compare(market.loading, false);
            compare(market.quotes, []);
            verify(market.error.includes("Cannot start"));
        }

        function test_network_status_start_failure_invalidates_snapshot_and_retries() {
            const network = readyNetwork();
            const query = process(network, "networkStatusProcess");
            network.scanOnRefresh = true;
            network.refresh();
            compare(network.loading, true);
            query.failStart();

            compare(network.available, false);
            compare(network.loading, false);
            compare(network.scanOnRefresh, false);
            compare(network.detailsAvailable, false);
            verify(network.statusError.includes("Cannot start"));
            compare(network.connections[0].name, "Fixture Wi-Fi");

            network.refresh();
            query.finish(0, statusResult());
            compare(network.available, true);
            compare(network.statusError, "");
            compare(network.loading, false);
        }

        function test_network_status_exit_error_is_not_replaced_by_start_error() {
            const network = make(networkComponent);
            process(network, "networkStatusProcess").finish(1, JSON.stringify({ ok: false, error: "Fixture status failure" }));
            compare(network.available, false);
            compare(network.loading, false);
            compare(network.statusError, "Fixture status failure");
        }

        function test_network_action_start_failure_clears_credentials_and_refreshes() {
            const network = readyNetwork();
            const query = process(network, "networkStatusProcess");
            const action = process(network, "networkActionProcess");
            network.connectNetwork({ name: "Fixture", advanced: false }, "synthetic-password", "");
            compare(network.working, true);
            verify(network.pendingRequest.includes("synthetic-password"));
            const request = network.pendingRequest;
            network.runAction({ operation: "scan" }, "Overlapping action");
            compare(network.pendingRequest, request);
            compare(action.attempts, 1);
            action.failStart();

            compare(network.working, false);
            compare(network.available, false);
            compare(network.pendingRequest, "");
            compare(network.pendingNetwork, null);
            compare(network.actionLabel, "");
            compare(network.operation, "");
            compare(action.stdinEnabled, false);
            compare(action.input, "");
            verify(network.actionError.includes("Cannot start"));
            tryCompare(query, "attempts", 3);
            query.finish(0, statusResult());

            network.runAction({ operation: "wifi", enabled: true }, "Retrying");
            action.started();
            action.finish(0, JSON.stringify({ ok: true }));
            compare(network.actionError, "");
            compare(network.working, false);
        }

        function test_network_action_exit_order_data() {
            return [
                { tag: "success", code: 0, result: { ok: true }, error: "" },
                { tag: "failure", code: 4, result: { ok: false, error: "Fixture action failure" }, error: "Fixture action failure" }
            ];
        }

        function test_network_action_exit_order(data) {
            const network = readyNetwork();
            const query = process(network, "networkStatusProcess");
            const action = process(network, "networkActionProcess");
            network.runAction({ operation: "wifi", enabled: false }, "Changing Wi-Fi");
            action.started();
            compare(network.pendingRequest, "");
            compare(action.stdinEnabled, false);
            action.finish(data.code, JSON.stringify(data.result));
            compare(network.actionError, data.error);
            compare(network.working, false);
            compare(network.actionLabel, "");
            compare(network.operation, "");
            tryCompare(query, "attempts", 3);
            query.finish(0, statusResult());
            wait(0);
            compare(query.attempts, 3);
            compare(network.error, data.error);
        }

        function test_network_monitor_debounces_events_and_controls_polling() {
            const network = make(networkComponent);
            const query = process(network, "networkStatusProcess");
            const monitor = process(network, "networkMonitorProcess");
            const debounce = findChild(network, "networkMonitorDebounce");
            const restart = findChild(network, "networkMonitorRestart");
            const poll = findChild(network, "networkPollTimer");
            verify(debounce !== null && restart !== null && poll !== null);

            compare(monitor.command, ["nmcli", "monitor"]);
            compare(monitor.running, true);
            compare(network.available, false);
            compare(poll.running, true);

            query.finish(0, JSON.stringify({ ok: true, state: { connected: true, connectionName: "Wired", connectionType: "ethernet" } }));
            compare(query.attempts, 1);
            compare(network.available, true);
            compare(poll.running, false);

            // Burst of monitor lines collapses into one debounced refresh.
            monitor.stdout.read("eth0: disconnected");
            monitor.stdout.read("Connectivity is now 'none'");
            compare(debounce.running, true);
            compare(query.attempts, 1);
            debounce.stop();
            debounce.triggered();
            compare(query.attempts, 2);
            query.finish(0, JSON.stringify({ ok: true, state: { connected: false, connectionName: "", connectionType: "" } }));

            // Opening the menu enables 10s polling; closing disables it while monitor runs.
            network.lastScan = Date.now();
            network.menuOpen = true;
            compare(poll.running, true);
            query.finish(0, statusResult());
            network.menuOpen = false;
            compare(poll.running, false);

            // Monitor exit enables fallback polling and schedules backoff restart.
            monitor.finish(1);
            compare(monitor.running, false);
            compare(poll.running, true);
            compare(restart.running, true);

            // Backoff restart keeps fallback polling running until a real event arrives.
            restart.stop();
            restart.triggered();
            compare(monitor.running, true);
            compare(monitor.attempts, 2);
            compare(poll.running, true);

            // An actual monitor event while running confirms recovery and stops fallback polling.
            monitor.stdout.read("eth0: connected");
            compare(poll.running, false);
            debounce.stop();
        }

        function test_network_flapping_monitor_does_not_reset_fallback_poll() {
            const network = make(networkComponent);
            const query = process(network, "networkStatusProcess");
            const monitor = process(network, "networkMonitorProcess");
            const restart = findChild(network, "networkMonitorRestart");
            const poll = findChild(network, "networkPollTimer");
            const debounce = findChild(network, "networkMonitorDebounce");
            verify(restart !== null && poll !== null && debounce !== null);

            query.finish(0, JSON.stringify({ ok: true, state: { connected: true, connectionName: "Wired", connectionType: "ethernet" } }));
            compare(network.available, true);
            compare(poll.running, false);

            // Monitor exits without emitting an event -> fallback polling starts.
            monitor.finish(1);
            compare(poll.running, true);

            const pollRunningSpy = createTemporaryObject(spyComponent, parent, {
                target: poll,
                signalName: "runningChanged"
            });
            verify(pollRunningSpy !== null);

            // Repeated 5s restarts and exits without events never toggle or reset the 10s poll timer.
            for (let i = 0; i < 3; i++) {
                restart.stop();
                restart.triggered();
                compare(monitor.running, true);
                compare(poll.running, true);
                monitor.finish(1);
                compare(monitor.running, false);
                compare(poll.running, true);
            }
            compare(pollRunningSpy.count, 0);

            // Post-exit stdout flush must not clear fallback polling while monitor is stopped.
            monitor.stdout.read("stale output after exit");
            debounce.stop();
            compare(poll.running, true);

            // Fallback poll fires and refreshes the closed bar even while monitor flaps.
            poll.triggered();
            compare(query.attempts, 2);
            query.finish(0, JSON.stringify({ ok: true, state: { connected: false, connectionName: "", connectionType: "" } }));
            compare(network.connected, false);
            compare(poll.running, true);

            // Once monitor restarts and emits an actual event, fallback polling stops.
            restart.stop();
            restart.triggered();
            compare(poll.running, true);
            monitor.stdout.read("wlan0: connected");
            compare(poll.running, false);
            debounce.stop();
        }

        function test_network_unavailable_polls_until_recovery_with_running_monitor() {
            const network = make(networkComponent);
            const query = process(network, "networkStatusProcess");
            const monitor = process(network, "networkMonitorProcess");
            const poll = findChild(network, "networkPollTimer");
            verify(poll !== null);

            query.finish(0, JSON.stringify({ ok: true, state: { connected: true, connectionName: "Wired", connectionType: "ethernet" } }));
            compare(monitor.running, true);
            compare(network.available, true);
            compare(poll.running, false);

            // A failed summary while monitor stays running enables recovery polling.
            network.refresh();
            query.finish(1, JSON.stringify({ ok: false, error: "Temporary failure" }));
            compare(monitor.running, true);
            compare(network.available, false);
            compare(poll.running, true);

            // Recovery poll retries and disables polling once available recovers.
            poll.triggered();
            compare(query.attempts, 3);
            query.finish(0, JSON.stringify({ ok: true, state: { connected: true, connectionName: "Wired", connectionType: "ethernet" } }));
            compare(network.available, true);
            compare(poll.running, false);
        }

        function test_market_stays_idle_until_menu_opens_and_stops_polling_when_closed() {
            const market = make(marketComponent);
            const query = process(market, "marketQueryProcess");
            const poll = findChild(market, "marketPollTimer");
            verify(poll !== null);
            compare(query.attempts, 0);
            compare(market.loading, false);
            compare(poll.running, false);

            market.menuOpen = true;
            compare(query.attempts, 1);
            compare(poll.running, true);
            const now = Math.floor(Date.now() / 1000);
            query.finish(0, JSON.stringify({ quotes: [{ symbol: "TEST", updated: now }], updated: now }));
            compare(market.loading, false);

            market.menuOpen = false;
            compare(poll.running, false);
        }

        function test_market_start_failure_preserves_previous_quotes_and_recovers() {
            const market = make(marketComponent);
            const query = process(market, "marketQueryProcess");
            const result = JSON.stringify({ quotes: [{ symbol: "TEST", price: 10, updated: Math.floor(Date.now() / 1000) }], updated: 123 });
            market.refresh();
            query.finish(0, result);
            compare(market.error, "");
            compare(market.loading, false);

            market.refresh();
            query.failStart();
            verify(market.error.includes("Cannot start"));
            compare(market.loading, false);
            compare(market.quotes[0].symbol, "TEST");
            compare(market.updated, 123);

            market.refresh();
            query.finish(0, result);
            compare(market.error, "");
            compare(market.loading, false);
        }

        function test_market_failure_uses_cached_payload_and_preserves_error() {
            const market = make(marketComponent);
            const query = process(market, "marketQueryProcess");
            market.refresh();
            query.finish(1, JSON.stringify({ quotes: [{ symbol: "CACHED", updated: 1 }], updated: 1 }), "Cache unavailable");
            compare(market.quotes[0].symbol, "CACHED");
            compare(market.updated, 1);
            compare(market.error, "Cache unavailable");
            market.refresh();
            query.finish(1, "invalid", "Still unavailable");
            compare(market.quotes[0].symbol, "CACHED");
            compare(market.error, "Still unavailable");
        }

        function test_market_partial_refresh_reports_stale_quotes() {
            const market = make(marketComponent);
            const query = process(market, "marketQueryProcess");
            market.refresh();
            compare(query.command, ["python3", "-B", market.helperPath]);
            verify(market.helperPath.endsWith("/symlinks/config/quickshell/helpers/stocks.py"));
            query.finish(0,
                JSON.stringify({ quotes: [{ symbol: "OLD", updated: 1 }], updated: 1 }));
            verify(market.error.includes("stale"));
            compare(market.quotes[0].symbol, "OLD");

            // Manual refresh passes --force; a 300s-old quote is below the 900s stale threshold.
            const now = Math.floor(Date.now() / 1000);
            market.refresh(true);
            compare(query.command, ["python3", "-B", market.helperPath, "--force"]);
            query.finish(0, JSON.stringify({ quotes: [{ symbol: "FRESH", updated: now - 300 }], updated: now - 300 }));
            compare(market.error, "");
        }

        function test_stock_card_stale_label_reacts_to_current_date() {
            const base = Date.UTC(2024, 0, 1) / 1000;
            const card = createTemporaryObject(stockCardComponent, parent, {
                quote: {
                    symbol: "TEST",
                    name: "Test Corp",
                    prefix: "$",
                    price: 100,
                    change: 1.5,
                    history: [90, 100],
                    historyStart: base - 86400,
                    yearChange: 11.1,
                    yearLow: 80,
                    yearHigh: 120,
                    updated: base
                },
                currentDate: new Date((base + 300) * 1000)
            });
            verify(card !== null);
            compare(card.stale, false);
            card.currentDate = new Date((base + 900) * 1000);
            compare(card.stale, true);
        }

        function test_market_partial_refresh_exposes_missing_symbols_until_recovery() {
            const market = make(marketComponent);
            const query = process(market, "marketQueryProcess");
            const now = Date.now() / 1000;
            market.refresh();
            query.finish(0, JSON.stringify({ quotes: [{ symbol: "FOUND", updated: now }],
                    updated: now, missing: ["MISSING"] }), "");
            compare(market.missingSymbols, ["MISSING"]);
            compare(market.quotes[0].symbol, "FOUND");
            compare(market.error, "");
            market.refresh();
            query.finish(0, JSON.stringify({ quotes: [{ symbol: "FOUND", updated: now },
                    { symbol: "MISSING", updated: now }], updated: now, missing: [] }), "");
            compare(market.missingSymbols, []);
        }

        function test_market_exit_failure_is_not_replaced_by_start_error() {
            const market = make(marketComponent);
            market.refresh();
            process(market, "marketQueryProcess").finish(1, "", "Fixture market failure");
            compare(market.error, "Fixture market failure");
            compare(market.loading, false);
        }

        function test_power_start_failure_stays_open_and_retry_can_succeed() {
            const menu = make(powerComponent);
            const action = process(menu, "powerActionProcess");
            menu.visible = true;
            menu.run(["fixture-command"]);
            menu.run(["overlapping-command"]);
            compare(action.command, ["fixture-command"]);
            action.failStart();
            compare(menu._actionActive, false);
            verify(menu.error.includes("Cannot start"));
            compare(menu.visible, true);
            compare(menu.closing, false);

            menu.run(["fixture-command"]);
            compare(menu.error, "");
            action.finish(0);
            compare(menu._actionActive, false);
            compare(menu.error, "");
            tryCompare(menu, "visible", false);
            compare(menu.error, "");
        }

        function test_power_exit_failure_is_not_replaced_by_start_error() {
            const menu = make(powerComponent);
            const action = process(menu, "powerActionProcess");
            menu.visible = true;
            menu.run(["fixture-command"]);
            action.finish(1, "", "Fixture power failure");
            compare(menu._actionActive, false);
            compare(menu.error, "Fixture power failure");
            compare(menu.visible, true);
            compare(menu.closing, false);
        }
    }
}
