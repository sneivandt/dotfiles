pragma ComponentBehavior: Bound

import QtQuick
import "Theme.js" as Theme

// Keep every control reachable when a bar section runs out of horizontal room.
Item {
    id: root

    property real naturalWidth: 0
    property real maximumWidth: naturalWidth
    property string sectionName: "Controls"
    readonly property bool overflowing: naturalWidth > width
    readonly property real viewportWidth: viewport.width
    readonly property real scrollPosition: viewport.contentX
    default property alias contentData: stripContent.data

    implicitWidth: Math.min(naturalWidth, Math.max(68, maximumWidth))
    implicitHeight: Theme.barControlHeight

    function reveal(x, itemWidth) {
        if (x < viewport.contentX)
            viewport.contentX = x;
        else if (x + itemWidth > viewport.contentX + viewport.width)
            viewport.contentX = x + itemWidth - viewport.width;
        clampPosition();
    }

    function clampPosition() {
        viewport.contentX = Math.max(0, Math.min(viewport.contentX, naturalWidth - viewport.width));
    }

    function scrollBy(amount) {
        viewport.contentX += amount;
        clampPosition();
    }

    onNaturalWidthChanged: clampPosition()

    BarBlock {
        objectName: "previousControls"
        width: 20
        height: parent.height
        visible: root.overflowing
        enabled: viewport.contentX > 0
        horizontalPadding: 0
        glyph: "\uf104"
        tooltip: root.sectionName + ": scroll left"
        onActivated: root.scrollBy(-viewport.width)
    }

    Flickable {
        id: viewport
        x: root.overflowing ? 20 : 0
        width: Math.max(0, root.width - (root.overflowing ? 40 : 0))
        height: parent.height
        contentWidth: root.naturalWidth
        contentHeight: height
        flickableDirection: Flickable.HorizontalFlick
        boundsBehavior: Flickable.StopAtBounds
        interactive: root.overflowing
        clip: true
        onWidthChanged: root.clampPosition()

        Item {
            id: stripContent
            width: root.naturalWidth
            height: viewport.height
        }
    }

    BarBlock {
        objectName: "nextControls"
        x: parent.width - width
        width: 20
        height: parent.height
        visible: root.overflowing
        enabled: viewport.contentX < Math.max(0, root.naturalWidth - viewport.width)
        horizontalPadding: 0
        glyph: "\uf105"
        tooltip: root.sectionName + ": scroll right"
        onActivated: root.scrollBy(viewport.width)
    }
}
