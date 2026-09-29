import QtQml

QtObject {
    property var command: []
    property bool running: false
    property bool stdinEnabled: false
    property QtObject stdout
    property QtObject stderr
    property string input: ""
    property int attempts: 0

    signal started
    signal exited(int code)

    onRunningChanged: {
        if (running)
            attempts++;
    }

    function exec(arguments) {
        command = arguments;
        running = true;
    }

    function write(text) {
        input += text;
    }

    function failStart() {
        // Quickshell 0.3.1 does not emit exited for FailedToStart.
        running = false;
    }

    function finish(code, output, errors) {
        if (stdout)
            stdout.text = output || "";
        if (stderr)
            stderr.text = errors || "";
        // Match the native signal order; no executable is ever launched.
        exited(code);
        running = false;
    }
}
