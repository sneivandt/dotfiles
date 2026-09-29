import QtQuick
import QtTest
import Quickshell
import "../.." as Shell

Item {
    width: 500
    height: 700

    Component {
        id: menuComponent

        Shell.NetworkMenu {
            anchorItem: null
            network: QtObject {
                property bool available: true
                property bool loading: false
                property bool connected: true
                property string connectivity: "full"
                property string icon: ""
                property bool wifiAvailable: true
                property bool wifiEnabled: true
                property bool wifiHardwareEnabled: true
                property var adapters: []
                property var connections: []
                property var networks: []
                property bool working: false
                property bool scanning: false
                property string actionLabel: ""
                property string error: ""

                signal credentialsRequested(var accessPoint)
                function refreshOnOpen() {}
            }
        }
    }

    TestCase {
        name: "NetworkMenu"
        when: windowShown

        function initTestCase() {
            verify(Quickshell.testMode === true, "Run with -import tests/qml/mocks; live Quickshell services are forbidden.");
        }

        function test_external_text_is_literal_data() {
            return [
                { tag: "markup", name: "<b>Home</b>" },
                { tag: "unclosed-markup", name: "<b>Home" },
                { tag: "angle-brackets", name: "Home & <Office>" },
                { tag: "unicode-newline", name: "Café:東京\\\n<b>" }
            ];
        }

        function test_external_text_is_literal(data) {
            const menu = createTemporaryObject(menuComponent, parent);
            verify(menu !== null);
            menu.selectedNetwork = { name: data.name, requiresSsid: false, protected: true };
            menu.network.actionLabel = "Connecting to " + data.name + "…";
            menu.network.error = "Could not connect to " + data.name;

            const expectations = [
                ["networkCredentialsTitle", "Connect to " + data.name],
                ["networkActionLabel", menu.network.actionLabel],
                ["networkError", menu.network.error]
            ];
            for (const expectation of expectations) {
                const label = findChild(menu, expectation[0]);
                verify(label !== null);
                compare(label.text, expectation[1]);
                compare(label.textFormat, Text.PlainText);
            }
        }
    }
}
