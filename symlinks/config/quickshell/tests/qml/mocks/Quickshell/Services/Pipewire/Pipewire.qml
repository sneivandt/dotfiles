pragma Singleton
import QtQml

QtObject {
    property bool ready: false
    property var defaultAudioSink: null
    property var preferredDefaultAudioSink: null
    property QtObject nodes: QtObject {
        property var values: []
    }
}
