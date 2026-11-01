import QtQuick
import Quickshell
import Quickshell.Io

Scope {
    id: root

    property bool active: false
    property var notifications: []
    property var history: []
    property bool available: false
    property string error: ""
    property bool querying: false
    readonly property bool loading: querying || query.running
    readonly property string helperPath: decodeURIComponent(Qt.resolvedUrl("notification_helper.py").toString().replace(/^file:\/\//, ""))

    function refresh() {
        queue.request();
    }

    onActiveChanged: {
        if (active)
            refresh();
        else {
            available = false;
            notifications = [];
            history = [];
        }
    }

    RefreshQueue {
        id: queue
        blocked: root.loading || !root.active
        onReady: {
            root.querying = true;
            query.running = true;
        }
    }

    Process {
        id: query
        objectName: "notificationQueryProcess"
        command: ["python3", root.helperPath]
        onRunningChanged: {
            if (!running && root.querying) {
                root.querying = false;
                root.available = false;
                root.error = "Cannot start the notification helper. Check that Python is installed.";
            }
        }
        onExited: code => {
            if (!root.querying)
                return;
            root.querying = false;
            try {
                const result = JSON.parse(output.text);
                if (code !== 0 || !result.ok || !Array.isArray(result.active) || !Array.isArray(result.history))
                    throw new Error("Invalid snapshot");
                root.notifications = root.active ? result.active : [];
                root.history = root.active ? result.history : [];
                root.available = true;
                root.error = "";
            } catch (_) {
                root.available = false;
                root.error = "Notifications unavailable. Check that Mako 1.11 or newer is running.";
            }
        }
        stdout: StdioCollector { id: output }
        stderr: StdioCollector {}
    }

    Timer {
        interval: 3000
        repeat: true
        running: root.active
        onTriggered: root.refresh()
    }
}
