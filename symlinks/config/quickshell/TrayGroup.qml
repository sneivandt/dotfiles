pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Io
import "Theme.js" as Theme

BarGroup {
    id: root

    required property var items
    required property var anchorWindow
    required property var menuController
    readonly property var shownItems: items.filter(item => item)
    property string copilotError: ""
    property bool _copilotActive: false

    implicitWidth: shownItems.length * Theme.barControlHeight
    visible: shownItems.length > 0

    function openCopilot() {
        if (_copilotActive || copilotProcess.running)
            return;
        menuController.dismiss();
        copilotError = "";
        _copilotActive = true;
        copilotProcess.exec(["sh", Quickshell.env("HOME") + "/.config/hypr/scripts/launch-github.sh"]);
    }

    function reportCopilotError(message) {
        copilotError = message;
        console.warn("GitHub Copilot: " + message);
    }

    Process {
        id: copilotProcess
        objectName: "copilotTrayProcess"
        stderr: StdioCollector {
            id: copilotErrors
        }
        onRunningChanged: {
            if (!running && root._copilotActive) {
                root._copilotActive = false;
                root.reportCopilotError("Cannot start the Copilot launcher.");
            }
        }
        onExited: code => {
            if (!root._copilotActive)
                return;
            root._copilotActive = false;
            if (code !== 0)
                root.reportCopilotError(copilotErrors.text.trim() || "Could not open GitHub Copilot.");
        }
    }

    function appName(item) {
        if (!item)
            return "";
        const names = [item.id, item.title, item.tooltipTitle];
        for (const name of names) {
            const normalized = (name || "").trim().toLowerCase();
            if (normalized === "github" || normalized === "github copilot" || normalized === "copilot")
                return "GitHub Copilot";
            if (normalized === "chatgpt" || normalized === "chatgpt-desktop")
                return "ChatGPT";
            if (normalized === "spotify")
                return "Spotify";
        }
        return "";
    }

    Row {
        objectName: "trayRow"
        Repeater {
            model: root.shownItems
            TrayButton {
                required property var modelData
                trayItem: modelData
                anchorWindow: root.anchorWindow
                menuController: root.menuController
                appName: root.appName(modelData)
                customIcon: appName ? Qt.resolvedUrl(appName === "GitHub Copilot" ? "icons/copilot.svg" : appName === "Spotify" ? "icons/spotify.svg" : "icons/chatgpt.svg") : ""
                customActivation: appName === "GitHub Copilot"
                activationError: customActivation ? root.copilotError : ""
                onActivationRequested: root.openCopilot()
            }
        }
    }
}
