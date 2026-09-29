import QtQuick
import Quickshell
import Quickshell.Io

Scope {
    id: root
    property var quotes: []
    property int updated: 0
    property string error: ""
    property bool _queryActive: false
    readonly property bool loading: _queryActive || query.running

    function refresh() {
        if (loading)
            return;
        _queryActive = true;
        query.running = true;
    }

    Component.onCompleted: refresh()

    Process {
        id: query
        objectName: "marketQueryProcess"
        command: [Quickshell.env("HOME") + "/.config/hypr/scripts/stocks.sh"]
        onRunningChanged: {
            if (!running && root._queryActive) {
                root._queryActive = false;
                root.error = "Cannot start the market helper. Check that stocks.sh is installed and executable.";
            }
        }
        stdout: StdioCollector {
            id: output
        }
        stderr: StdioCollector {
            id: errors
        }
        onExited: code => {
            if (!root._queryActive)
                return;
            root._queryActive = false;
            if (code !== 0) {
                root.error = errors.text.trim() || "Could not refresh market data.";
                return;
            }
            let payload;
            try {
                payload = JSON.parse(output.text);
            } catch (error) {
                root.error = "The market service returned invalid data.";
                console.warn(root.error, error.message);
                return;
            }
            if (!payload || !Array.isArray(payload.quotes) || !Number.isFinite(payload.updated)) {
                root.error = "The market service returned an unexpected response.";
                console.warn(root.error);
                return;
            }
            if (payload.quotes.length === 0) {
                root.error = "No quotes available. The market service may be offline.";
                return;
            }
            root.quotes = payload.quotes;
            root.updated = payload.updated;
            root.error = "";
        }
    }
    Timer {
        interval: 300000
        repeat: true
        running: true
        onTriggered: root.refresh()
    }
}
