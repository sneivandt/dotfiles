pragma Singleton
import QtQml

QtObject {
    readonly property bool testMode: true

    function env(name) {
        return name === "HOME" ? "/fixture-home" : "";
    }
}
