import QtQuick
import QtTest
import Quickshell
import "../.." as Shell

Item {
    width: 500
    height: 700

    Component {
        id: networkComponent
        Shell.NetworkState {}
    }
    Component {
        id: marketComponent
        Shell.MarketState {}
    }
    Component {
        id: powerComponent
        Shell.PowerMenu {
            anchorItem: null
        }
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
            process(network, "networkStatusProcess").finish(0, statusResult());
            compare(network.available, true);
            compare(network.loading, false);
            compare(network.error, "");
            return network;
        }

        function test_initial_queries_report_start_failures() {
            const network = make(networkComponent);
            const market = make(marketComponent);
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
            tryCompare(query, "attempts", 2);
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
            tryCompare(query, "attempts", 2);
            query.finish(0, statusResult());
            wait(0);
            compare(query.attempts, 2);
            compare(network.error, data.error);
        }

        function test_market_start_failure_preserves_previous_quotes_and_recovers() {
            const market = make(marketComponent);
            const query = process(market, "marketQueryProcess");
            const result = JSON.stringify({ quotes: [{ symbol: "TEST", price: 10 }], updated: 123 });
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

        function test_market_exit_failure_is_not_replaced_by_start_error() {
            const market = make(marketComponent);
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
