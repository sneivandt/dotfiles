import QtQuick
import Quickshell
import Quickshell.Io

Scope {
    id: root
    property var quotes: []
    property var missingSymbols: []
    property int updated: 0
    property string error: ""
    property bool menuOpen: false
    property bool _queryActive: false
    readonly property int staleAfterSeconds: 15 * 60
    readonly property bool loading: _queryActive || query.running
    readonly property string helperPath: decodeURIComponent(Qt.resolvedUrl("../helpers/stocks.py").toString().replace(/^file:\/\//, ""))

    function refresh(force) {
        if (loading)
            return;
        _queryActive = true;
        const command = ["python3", "-B", helperPath];
        if (force === true)
            command.push("--force");
        query.exec(command);
    }

    onMenuOpenChanged: {
        if (menuOpen)
            refresh();
    }

    Process {
        id: query
        objectName: "marketQueryProcess"
        onRunningChanged: {
            if (!running && root._queryActive) {
                root._queryActive = false;
                root.error = "Cannot start the market helper. Check that Python and stocks.py are installed.";
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
            const refreshError = code !== 0 ? (errors.text.trim() || "Could not refresh market data.") : "";
            let payload;
            try {
                payload = JSON.parse(output.text);
            } catch (error) {
                root.error = refreshError || "The market service returned invalid data.";
                console.warn(root.error, error.message);
                return;
            }
            if (!payload || !Array.isArray(payload.quotes) || !Number.isFinite(payload.updated)) {
                root.error = refreshError || "The market service returned an unexpected response.";
                console.warn(root.error);
                return;
            }
            if (payload.quotes.length === 0) {
                root.error = refreshError || "No quotes available. The market service may be offline.";
                return;
            }
            root.quotes = payload.quotes;
            root.missingSymbols = Array.isArray(payload.missing) ? payload.missing : [];
            root.updated = payload.updated;
            root.error = refreshError || (payload.quotes.some(quote => !Number.isFinite(quote.updated) || Date.now() / 1000 - quote.updated >= root.staleAfterSeconds)
                ? "Some market quotes are stale. Showing the last available prices." : "");
        }
    }
    Timer {
        objectName: "marketPollTimer"
        interval: 300000
        repeat: true
        running: root.menuOpen
        onTriggered: root.refresh()
    }
}
