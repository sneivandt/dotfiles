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
                property bool detailsAvailable: true
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
                property string requestedUuid: ""

                signal credentialsRequested(var accessPoint)
                function connectNetwork(accessPoint, password, ssid) { requestedUuid = accessPoint.uuid; }
            }
        }
    }

    TestCase {
        name: "NetworkMenu"
        when: windowShown

        function initTestCase() {
            verify(Quickshell.testMode === true, "Run with -import tests/qml/mocks; live Quickshell services are forbidden.");
        }

        function test_profile_detail_identifies_the_clicked_choice() {
            const menu = createTemporaryObject(menuComponent, parent);
            const ap = {name: "Home", available: true, signal: 74, protected: true, saved: true, active: false, advanced: false};
            menu.network.networks = [
                Object.assign({}, ap, {uuid: "work-id", profileLabel: "Desk"}),
                Object.assign({}, ap, {uuid: "guest-id", profileLabel: "Guest"})
            ];
            menu.visible = true;
            tryCompare(menu, "reveal", 1);
            const choices = findChild(menu, "networkChoices");
            compare(choices.count, 2);
            for (const [i, label, uuid] of [[0, "Desk", "work-id"], [1, "Guest", "guest-id"]]) {
                const button = choices.itemAt(i);
                compare(button.label, "Home");
                compare(button.detail, label + " · Saved");
                mouseClick(button);
                compare(menu.network.requestedUuid, uuid);
            }
        }

        function test_connected_profile_can_be_inspected_without_reconnecting() {
            const menu = createTemporaryObject(menuComponent, parent);
            const name = "Household wireless profile for the upstairs office";
            const active = {name: "Home", uuid: "work-id", profileLabel: name, active: true,
                            available: true, signal: 74, protected: true, saved: true, advanced: false};
            menu.network.networks = [active, Object.assign({}, active, {active: false, uuid: "guest-id", profileLabel: "Guest"})];
            menu.visible = true;
            tryCompare(menu, "reveal", 1);
            const choices = findChild(menu, "networkChoices");
            const connected = choices.itemAt(0);
            const guest = choices.itemAt(1);
            const tip = connected.data.find(item => item instanceof Shell.ShellToolTip);
            verify(tip !== undefined);
            const detail = findText(connected.contentItem, name);
            verify(detail !== null && detail.truncated);
            let activations = 0;
            connected.triggered.connect(() => activations++);

            mouseMove(connected, connected.width / 2, connected.height / 2);
            tryCompare(tip, "opened", true);
            compare(tip.contentItem.text, name);
            verify(!tip.contentItem.truncated);
            mouseClick(connected);
            mouseMove(guest, guest.width / 2, guest.height / 2);
            guest.forceActiveFocus(Qt.TabFocusReason);
            tryCompare(tip, "visible", false);
            keyClick(Qt.Key_Backtab);
            tryCompare(connected, "visualFocus", true);
            tryCompare(tip, "opened", true);
            keyClick(Qt.Key_Return);
            keyClick(Qt.Key_Enter);
            keyClick(Qt.Key_Space);
            compare(activations, 0);
            compare(menu.network.requestedUuid, "");
            keyClick(Qt.Key_Tab);
            keyClick(Qt.Key_Return);
            compare(menu.network.requestedUuid, "guest-id");
        }

        function findText(item, text) {
            if (item instanceof Text && item.text === text)
                return item;
            for (const child of item.children) {
                const match = findText(child, text);
                if (match)
                    return match;
            }
            return null;
        }

        function test_reopen_keeps_last_known_choices_disabled_until_detail_arrives() {
            const menu = createTemporaryObject(menuComponent, parent);
            menu.network.networks = [{name: "Home", uuid: "old-id", available: true, signal: 54,
                                      protected: true, saved: true, active: false, advanced: false}];
            menu.network.detailsAvailable = false;
            menu.network.loading = true;
            menu.visible = true;
            tryCompare(menu, "reveal", 1);
            const choice = findChild(menu, "networkChoices").itemAt(0);
            verify(findChild(menu, "networkActionLabel").visible);
            verify(!choice.enabled);
            mouseClick(choice);
            compare(menu.network.requestedUuid, "");
            menu.network.detailsAvailable = true;
            menu.network.loading = false;
            verify(choice.enabled);
            mouseClick(choice);
            compare(menu.network.requestedUuid, "old-id");
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
