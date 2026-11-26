import QtQml

QtObject {
    id: root

    property bool blocked: false
    property bool pending: false
    signal ready

    function request() {
        pending = true;
        drain();
    }

    function drain() {
        if (blocked || !pending)
            return;
        pending = false;
        ready();
    }

    // Let process exit handlers finish before starting the queued query.
    onBlockedChanged: {
        if (!blocked && pending)
            Qt.callLater(root.drain);
    }
}
