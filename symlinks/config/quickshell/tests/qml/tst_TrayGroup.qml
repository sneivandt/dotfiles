import QtQuick
import QtTest
import Quickshell
import "../.." as Shell

Item {
    id: scene
    width: 800
    height: 60
    readonly property Item contentItem: scene
    property int dismissals: 0

    function dismiss() {
        dismissals++;
    }

    Component {
        id: groupComponent
        Shell.TrayGroup {}
    }
    Component {
        id: buttonComponent
        Shell.TrayButton {}
    }
    Component {
        id: itemComponent
        QtObject {
            property string id: "ordinary-app"
            property string title: ""
            property string tooltipTitle: ""
            property string icon: ""
            property bool onlyMenu: false
            property int activations: 0
            property int secondaryActivations: 0
            property int menus: 0
            property var menuWindow: null
            property point menuPoint
            property int scrollDelta: 0
            property bool horizontalScroll: true

            function activate() { activations++; }
            function secondaryActivate() { secondaryActivations++; }
            function display(window, x, y) {
                menus++;
                menuWindow = window;
                menuPoint = Qt.point(x, y);
            }
            function scroll(delta, horizontal) {
                scrollDelta = delta;
                horizontalScroll = horizontal;
            }
        }
    }

    TestCase {
        name: "TrayGroup"
        when: windowShown

        function initTestCase() {
            verify(Quickshell.testMode === true, "Run with -import tests/qml/mocks; live processes are forbidden.");
        }

        function item(properties) {
            return createTemporaryObject(itemComponent, scene, properties || {});
        }

        function group(items) {
            return createTemporaryObject(groupComponent, scene, {
                items: items,
                anchorWindow: scene,
                menuController: scene
            });
        }

        function trayButton(tray, index) {
            return findChild(tray, "trayRow").children[index];
        }

        function test_single_tray_and_custom_icons() {
            const regular = item();
            const copilot = item({ title: "github" });
            const chatgpt = item({ id: "chrome_status_icon_1", tooltipTitle: "ChatGPT" });
            const tray = group([regular, chatgpt, copilot]);
            compare(tray.shownItems.length, 3);
            compare(tray.shownItems[0], regular);
            compare(tray.shownItems[1], chatgpt);
            compare(tray.shownItems[2], copilot);
            tryCompare(tray, "implicitWidth", 84);

            const regularButton = trayButton(tray, 0);
            const chatgptButton = trayButton(tray, 1);
            const copilotButton = trayButton(tray, 2);
            compare(regularButton.parent, chatgptButton.parent);
            compare(chatgptButton.parent, copilotButton.parent);
            compare(chatgptButton.x, regularButton.x + regularButton.width);
            compare(copilotButton.x, chatgptButton.x + chatgptButton.width);
            compare(regularButton.customIcon, "");
            compare(chatgptButton.appName, "ChatGPT");
            verify(chatgptButton.customIcon.endsWith("/icons/chatgpt.svg"));
            compare(copilotButton.appName, "GitHub Copilot");
            verify(copilotButton.customIcon.endsWith("/icons/copilot.svg"));
            tryCompare(chatgptButton.contentItem.children[0], "status", Image.Ready);
            tryCompare(copilotButton.contentItem.children[0], "status", Image.Ready);
        }

        function test_spotify_custom_icon_and_activation() {
            const spotify = item({ id: "spotify" });
            const tray = group([spotify]);
            const button = trayButton(tray, 0);
            compare(button.appName, "Spotify");
            verify(button.customIcon.endsWith("/icons/spotify.svg"));
            tryCompare(button.contentItem.children[0], "status", Image.Ready);
            mouseClick(button, 14, 14, Qt.LeftButton);
            compare(spotify.activations, 1);
        }

        function test_matching_data() {
            return [
                { tag: "copilot-id", fields: { id: "github copilot" }, expected: "GitHub Copilot" },
                { tag: "copilot-tooltip", fields: { tooltipTitle: " Copilot " }, expected: "GitHub Copilot" },
                { tag: "chatgpt-id", fields: { id: "chatgpt-desktop" }, expected: "ChatGPT" },
                { tag: "chatgpt-title", fields: { title: "CHATGPT" }, expected: "ChatGPT" },
                { tag: "spotify-title", fields: { title: " Spotify " }, expected: "Spotify" },
                { tag: "spotify-tooltip", fields: { tooltipTitle: "SPOTIFY" }, expected: "Spotify" },
                { tag: "unrelated-browser", fields: { id: "chrome_status_icon_1", tooltipTitle: "Chromium" }, expected: "" },
                { tag: "unrelated-substring", fields: { title: "GitHub notifications" }, expected: "" },
                { tag: "long-title", fields: { title: "ChatGPT ".repeat(100) }, expected: "" },
                { tag: "empty", fields: { id: "" }, expected: "" }
            ];
        }

        function test_matching(data) {
            const tray = group([]);
            compare(tray.appName(item(data.fields)), data.expected);
            compare(tray.appName(null), "");
        }

        function test_lifecycle_and_reactive_metadata() {
            const delayed = item({ id: "chrome_status_icon_1" });
            const tray = group([null, delayed]);
            compare(tray.shownItems.length, 1);
            compare(trayButton(tray, 0).appName, "");
            tryCompare(tray, "implicitWidth", 28);

            delayed.tooltipTitle = "ChatGPT";
            compare(tray.shownItems.length, 1);
            compare(trayButton(tray, 0).appName, "ChatGPT");
            tryCompare(tray, "implicitWidth", 28);

            const copilot = item({ title: "github" });
            tray.items = [copilot, delayed];
            tryCompare(tray, "implicitWidth", 56);
            tray.items = [delayed, copilot];
            compare(tray.shownItems[0], delayed);
            tray.items = [copilot];
            tryCompare(tray, "implicitWidth", 28);
            tray.items = [];
            tryCompare(tray, "implicitWidth", 0);
            verify(!tray.visible);
        }

        function test_monitor_instances_and_narrow_layout() {
            const app = item({ tooltipTitle: "ChatGPT" });
            const first = group([app]);
            const second = group([app]);
            second.x = 100;
            second.width = 28;
            compare(first.shownItems[0], second.shownItems[0]);
            tryCompare(first, "implicitWidth", 28);
            tryCompare(second, "implicitWidth", 28);
            app.tooltipTitle = "";
            compare(first.shownItems.length, 1);
            compare(second.shownItems.length, 1);
            compare(trayButton(first, 0).appName, "");
            compare(trayButton(second, 0).appName, "");
        }

        function test_button_actions() {
            const app = item({ title: "github" });
            const button = createTemporaryObject(buttonComponent, scene, {
                trayItem: app,
                anchorWindow: scene,
                menuController: scene,
                x: 40,
                y: 5
            });
            mouseClick(button, 14, 14, Qt.LeftButton);
            compare(app.activations, 1);
            mouseClick(button, 14, 14, Qt.MiddleButton);
            compare(app.secondaryActivations, 1);
            mouseClick(button, 14, 14, Qt.RightButton);
            compare(app.menus, 1);
            compare(app.menuWindow, scene);
            compare(app.menuPoint, Qt.point(40, 33));
            mouseWheel(button, 14, 14, 0, 120);
            compare(app.scrollDelta, 120);
            compare(app.horizontalScroll, false);
            app.onlyMenu = true;
            mouseClick(button, 14, 14, Qt.LeftButton);
            compare(app.menus, 2);
            compare(app.activations, 1);
            button.forceActiveFocus();
            keyClick(Qt.Key_Return);
            compare(app.menus, 3);
            keyClick(Qt.Key_Enter);
            compare(app.menus, 4);
            keyClick(Qt.Key_Menu);
            compare(app.menus, 5);
        }

        function test_copilot_opens_through_launcher_and_chatgpt_activates() {
            const copilot = item({ title: "github", onlyMenu: true });
            const chatgpt = item({ tooltipTitle: "ChatGPT" });
            const tray = group([copilot, chatgpt]);
            const copilotButton = trayButton(tray, 0);
            const chatgptButton = trayButton(tray, 1);
            const process = findChild(tray, "copilotTrayProcess");
            verify(process !== null);

            mouseClick(copilotButton, 14, 14, Qt.LeftButton);
            compare(process.command, ["sh", "/fixture-home/.config/hypr/scripts/launch-github.sh"]);
            compare(process.attempts, 1);
            compare(copilot.activations, 0);
            compare(copilot.menus, 0);
            mouseClick(copilotButton, 14, 14, Qt.LeftButton);
            compare(process.attempts, 1);
            process.finish(0);
            compare(tray.copilotError, "");

            copilotButton.forceActiveFocus();
            keyClick(Qt.Key_Return);
            compare(process.attempts, 2);
            process.finish(0);
            mouseClick(copilotButton, 14, 14, Qt.RightButton);
            compare(copilot.menus, 1);
            compare(process.attempts, 2);
            mouseClick(chatgptButton, 14, 14, Qt.LeftButton);
            compare(chatgpt.activations, 1);
            compare(process.attempts, 2);
        }

        function test_copilot_launcher_failure_is_visible_and_retryable() {
            const tray = group([item({ title: "github" })]);
            const button = trayButton(tray, 0);
            const process = findChild(tray, "copilotTrayProcess");
            tray.openCopilot();
            ignoreWarning("GitHub Copilot: Cannot start the Copilot launcher.");
            process.failStart();
            compare(button.activationError, "Cannot start the Copilot launcher.");

            tray.openCopilot();
            compare(button.activationError, "");
            compare(process.attempts, 2);
            ignoreWarning("GitHub Copilot: Could not find Copilot's Open GitHub Copilot tray action");
            process.finish(2, "", "Could not find Copilot's Open GitHub Copilot tray action\n");
            compare(button.activationError, "Could not find Copilot's Open GitHub Copilot tray action");
        }
    }
}
