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
    property bool _actionActive: false
    readonly property string helperPath: decodeURIComponent(Qt.resolvedUrl("network_helper.py").toString().replace(/^file:\/\//, ""))

    signal credentialsRequested(var accessPoint)

    function clearError() {
        actionError = "";
    }

    function refresh() {
        refreshQueue.request();
    }

    function refreshOnOpen() {
        scanOnRefresh = Date.now() - lastScan > 10000;
        refresh();
    }

    function rescan() {
        if (working)
            return;

        if (!available || !wifiAvailable || !wifiEnabled) {
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
        if (!available || !wifiAvailable || working)
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
        if (!available || !wifiEnabled || working)
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
            statusQuery.running = true;
        }
    }

    Process {
        id: statusQuery
        objectName: "networkStatusProcess"

        command: ["python3", root.helperPath, "status"]
        onRunningChanged: {
            // FailedToStart emits runningChanged without an exited signal.
            if (!running && root._statusActive) {
                root._statusActive = false;
                root.available = false;
                root.statusError = "Cannot start the network helper. Check that Python is installed and executable.";
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
                root.deviceName = state.deviceName;
                root.wifiEnabled = state.wifiEnabled;
                root.wifiHardwareEnabled = state.wifiHardwareEnabled;
                root.wifiAvailable = state.wifiAvailable;
                root.adapters = state.adapters;
                root.networks = state.networks;
                root.connections = state.connections;
                root.connectivity = state.connectivity;
                root.available = true;
                root.statusError = "";
            } else {
                root.available = false;
                root.statusError = result.error || "NetworkManager status is unavailable.";
            }
            const shouldScan = root.scanOnRefresh;
            root.scanOnRefresh = false;
            if (shouldScan && root.available && root.wifiAvailable && root.wifiEnabled && !root.working)
                root.rescan();
        }

        stdout: StdioCollector {
            id: statusOutput
        }

        stderr: StdioCollector {}
    }

    Process {
        id: actionProcess
        objectName: "networkActionProcess"

        command: ["python3", root.helperPath, "action"]
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

    Timer {
        id: scanSettled

        interval: 2500
        onTriggered: root.refresh()
    }

    Timer {
        interval: 10000
        repeat: true
        running: true
        onTriggered: root.refresh()
    }
}
