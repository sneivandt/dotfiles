import QtQuick

Item {
    property color color: "transparent"
    property bool grabFocus: false
    property QtObject mask
    property Anchor anchor: Anchor {}

    component Anchor: QtObject {
        property var window: null
        property int adjustment: 0
        property int gravity: 0
        property rect rect
        signal anchoring
    }
}
