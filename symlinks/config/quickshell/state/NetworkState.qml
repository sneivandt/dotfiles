import QtQuick
import Quickshell
import Quickshell.Io

Scope {
    id: root

    property bool connected: false
    property string connectionName: ""
    property string connectionType: ""
    property string deviceName: ""
    readonly property string icon: !available ? "\uf071" : connected ? (connectionType === "wifi" ? "\uf1eb" : "\uf6ff") : "\uf127"
    property bool available: false
    property bool menuOpen: false
    readonly property bool detailsAvailable: _hasMenuSnapshot && available
    property bool wifiEnabled: false
    property bool wifiHardwareEnabled: false
    property bool wifiAvailable: false
    property var adapters: []
    property var networks: []
    property var connections: []
    property string connectivity: "unknown"
    readonly property bool loading: _statusActive || statusQuery.running
    readonly property bool busy: working && operation !== "scan"
    readonly property bool scanning: working && operation === "scan"
    readonly property bool working: _actionActive || actionProcess.running
    readonly property string error: actionError || statusError
    property string operation: ""
    property string actionLabel: ""
    property string actionError: ""
    property string statusError: ""
    property string pendingRequest: ""
    property var pendingNetwork: null
    property bool scanOnRefresh: false
    property double lastScan: 0
    property bool _statusActive: false
    property bool _detailQuery: false
    property bool _hasMenuSnapshot: false
    property bool _actionActive: false
    property bool _monitorFallbackPolling: false
    readonly property string helperPath: decodeURIComponent(Qt.resolvedUrl("../helpers/network_helper.py").toString().replace(/^file:\/\//, ""))

    signal credentialsRequested(var accessPoint)

    function clearError() {
        actionError = "";
    }

    function refresh() {
        refreshQueue.request();
    }

    onMenuOpenChanged: {
        scanOnRefresh = menuOpen && Date.now() - lastScan > 10000;
        if (menuOpen) {
            refresh();
        } else {
            // A late result from this menu visit must not enable choices on the next.
            _detailQuery = false;
            _hasMenuSnapshot = false;
        }
    }

    function rescan() {
        if (working)
            return;

        if (!detailsAvailable || !wifiAvailable || !wifiEnabled) {
            refresh();
            return;
        }
        scanOnRefresh = false;
        lastScan = Date.now();
        runAction({
            "operation": "scan"
        }, "Scanning for Wi-Fi networks…");
    }

    function setWifiEnabled(enabled) {
        if (!detailsAvailable || !wifiAvailable || working)
            return;

        if (enabled && !wifiHardwareEnabled) {
            actionError = "Wi-Fi is blocked by a hardware switch or airplane mode.";
            return;
        }
        runAction({
            "operation": "wifi",
            "enabled": enabled
        }, enabled ? "Enabling Wi-Fi…" : "Disabling Wi-Fi…");
    }

    function connectNetwork(accessPoint, password, ssid) {
        if (!detailsAvailable || !wifiEnabled || working)
            return;

        if (accessPoint.advanced) {
            actionError = "This network requires Advanced settings.";
            return;
        }
        pendingNetwork = accessPoint;
        runAction({
            "operation": "connect",
            "network": accessPoint,
            "password": password || "",
            "ssid": ssid || ""
        }, "Connecting to " + (ssid || accessPoint.name) + "…");
    }

    function runAction(request, label) {
        if (working)
            return;

        actionError = "";
        operation = request.operation;
        actionLabel = label;
        pendingRequest = JSON.stringify(request) + "\n";
        _actionActive = true;
        actionProcess.stdinEnabled = true;
        actionProcess.running = true;
    }

    function readResult(text) {
        try {
            return JSON.parse(text);
        } catch (_) {
            return {
                "ok": false,
                "error": "The network helper did not respond. Check Python and NetworkManager."
            };
        }
    }

    Component.onCompleted: refresh()

    RefreshQueue {
        id: refreshQueue

        blocked: root.loading || root.working
        onReady: {
            root._statusActive = true;
            root._detailQuery = root.menuOpen;
            statusQuery.exec(["python3", "-B", root.helperPath, root.menuOpen ? "status" : "summary"]);
        }
    }

    Process {
        id: statusQuery
        objectName: "networkStatusProcess"

        onRunningChanged: {
            // FailedToStart emits runningChanged without an exited signal.
            if (!running && root._statusActive) {
                root._statusActive = false;
                root.available = false;
                root.statusError = "Cannot start the network helper. Check that Python is installed and executable.";
                if (root._detailQuery)
                    root.scanOnRefresh = false;
            }
        }
        onExited: code => {
            if (!root._statusActive)
                return;
            root._statusActive = false;
            const result = root.readResult(statusOutput.text);
            if (code === 0 && result.ok) {
                const state = result.state;
                root.connected = state.connected;
                root.connectionName = state.connectionName;
                root.connectionType = state.connectionType;
                if (root._detailQuery) {
                    root.deviceName = state.deviceName;
                    root.wifiEnabled = state.wifiEnabled;
                    root.wifiHardwareEnabled = state.wifiHardwareEnabled;
                    root.wifiAvailable = state.wifiAvailable;
                    root.adapters = state.adapters;
                    root.networks = state.networks;
                    root.connections = state.connections;
                    root.connectivity = state.connectivity;
                    root._hasMenuSnapshot = true;
                }
                root.available = true;
                root.statusError = "";
            } else {
                root.available = false;
                root.statusError = result.error || "NetworkManager status is unavailable.";
            }
            if (root._detailQuery && root.scanOnRefresh) {
                root.scanOnRefresh = false;
                if (root.detailsAvailable && root.wifiAvailable && root.wifiEnabled && !root.working)
                    root.rescan();
            }
        }

        stdout: StdioCollector {
            id: statusOutput
        }

        stderr: StdioCollector {}
    }

    Process {
        id: actionProcess
        objectName: "networkActionProcess"

        command: ["python3", "-B", root.helperPath, "action"]
        onRunningChanged: {
            if (!running && root._actionActive) {
                root._actionActive = false;
                root.available = false;
                root.actionError = "Cannot start the network helper. Check that Python is installed and executable.";
                root.pendingRequest = "";
                root.pendingNetwork = null;
                root.actionLabel = "";
                root.operation = "";
                stdinEnabled = false;
                root.refresh();
            }
        }
        onStarted: {
            write(root.pendingRequest);
            root.pendingRequest = "";
            stdinEnabled = false;
        }
        onExited: code => {
            if (!root._actionActive)
                return;
            root._actionActive = false;
            root.pendingRequest = "";
            const result = root.readResult(actionOutput.text);
            if (code !== 0 || !result.ok) {
                root.actionError = result.error || "The network operation failed.";
                if (result.needsPassword && root.pendingNetwork)
                    root.credentialsRequested(root.pendingNetwork);
            }
            root.pendingNetwork = null;
            root.actionLabel = "";
            const scanned = root.operation === "scan";
            root.operation = "";
            root.refresh();
            if (scanned)
                scanSettled.restart();
        }

        stdout: StdioCollector {
            id: actionOutput
        }

        stderr: StdioCollector {}
    }

    Process {
        id: monitorProcess
        objectName: "networkMonitorProcess"

        command: ["nmcli", "monitor"]
        running: true
        onRunningChanged: {
            if (running) {
                monitorRestart.stop();
            } else {
                root._monitorFallbackPolling = true;
                monitorRestart.restart();
            }
        }

        stdout: SplitParser {
            id: monitorOutput
            onRead: {
                if (monitorProcess.running)
                    root._monitorFallbackPolling = false;
                monitorDebounce.restart();
            }
        }
    }

    Timer {
        id: monitorDebounce
        objectName: "networkMonitorDebounce"

        interval: 500
        repeat: false
        onTriggered: root.refresh()
    }

    Timer {
        id: monitorRestart
        objectName: "networkMonitorRestart"

        interval: 5000
        repeat: false
        onTriggered: monitorProcess.running = true
    }

    Timer {
        id: scanSettled

        interval: 2500
        onTriggered: root.refresh()
    }

    Timer {
        id: pollTimer
        objectName: "networkPollTimer"

        interval: 10000
        repeat: true
        running: root.menuOpen || !monitorProcess.running || !root.available || root._monitorFallbackPolling
        onTriggered: root.refresh()
    }
}
