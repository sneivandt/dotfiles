import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import Quickshell.Hyprland
import Quickshell.Wayland
import "Theme.js" as Theme

PanelWindow {
    id: root

    property string errorMessage: ""
    readonly property bool hasError: errorMessage.length > 0
    readonly property real availableHeight: Math.max(1, (screen ? screen.height : 720) - margins.top - Theme.spacing)

    function showResult(errorString) {
        autoHide.stop();
        const focused = Hyprland.focusedMonitor;
        const output = Quickshell.screens.find(screen => focused && screen.name === focused.name);
        if (output)
            root.screen = output;
        else if (!root.screen && Quickshell.screens.length > 0)
            root.screen = Quickshell.screens[0];

        errorMessage = errorString || "";
        errorScroll.contentY = 0;
        errorText.deselect();
        visible = true;
        if (!hasError)
            autoHide.restart();
    }

    function dismiss() {
        autoHide.stop();
        visible = false;
    }

    visible: false
    color: "transparent"
    implicitWidth: Math.max(1, Math.min(hasError ? 480 : 368, (screen ? screen.width : 1280) - Theme.spacing * 2))
    implicitHeight: Math.min(content.implicitHeight + Theme.padding * 2, availableHeight)
    exclusionMode: ExclusionMode.Ignore
    exclusiveZone: 0
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "quickshell-reload-notice"
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.OnDemand
    anchors {
        top: true
        right: true
    }
    margins {
        top: Theme.barHeight + Theme.spacing
        right: Theme.spacing
    }

    Timer {
        id: autoHide

        interval: 3000
        onTriggered: root.visible = false
    }

    MenuPanel {
        anchors.fill: parent
        clip: true

        ColumnLayout {
            id: content

            x: Theme.padding
            y: Theme.padding
            width: Math.max(0, parent.width - Theme.padding * 2)
            spacing: Theme.spacing
            Keys.onEscapePressed: root.dismiss()

            MenuHeader {
                id: header

                Layout.fillWidth: true
                title: root.hasError ? "Reload failed" : "Quickshell reloaded"
                subtitle: root.hasError ? "" : "Configuration is up to date."
                icon: root.hasError ? "\uf071" : "\uf00c"
                accentColor: root.hasError ? Theme.red : Theme.green
                accentBackground: "transparent"
                trailingItem: MenuIconButton {
                    glyph: "\uf00d"
                    tooltip: "Close reload notice"
                    onTriggered: root.dismiss()
                }
            }

            Text {
                id: explanation

                Layout.fillWidth: true
                visible: root.hasError
                text: "Your last valid configuration remains active."
                color: Theme.mutedStrong
                font.family: Theme.font
                font.pixelSize: Theme.textBody
                wrapMode: Text.WordWrap
            }

            Flickable {
                id: errorScroll

                Layout.fillWidth: true
                visible: root.hasError
                implicitHeight: Math.min(contentHeight, 300, Math.max(0, root.availableHeight - Theme.padding * 2 - header.implicitHeight - explanation.implicitHeight - retry.implicitHeight - Theme.spacing * 3))
                contentWidth: width
                contentHeight: errorText.implicitHeight
                clip: true
                boundsBehavior: Flickable.StopAtBounds
                flickableDirection: Flickable.VerticalFlick
                Keys.onDownPressed: contentY = Math.min(Math.max(0, contentHeight - height), contentY + 40)
                Keys.onUpPressed: contentY = Math.max(0, contentY - 40)
                ScrollBar.vertical: ShellScrollBar {}

                TextEdit {
                    id: errorText

                    width: Math.max(0, errorScroll.width - Theme.spacing)
                    text: root.errorMessage
                    textFormat: TextEdit.PlainText
                    readOnly: true
                    selectByMouse: true
                    persistentSelection: true
                    activeFocusOnTab: true
                    wrapMode: TextEdit.Wrap
                    color: Theme.foreground
                    selectionColor: Theme.blueSoft
                    selectedTextColor: Theme.foreground
                    font.family: Theme.font
                    font.pixelSize: Theme.textSmall
                    Accessible.name: "Reload error details"
                    onCursorRectangleChanged: {
                        if (!activeFocus)
                            return;
                        if (cursorRectangle.y < errorScroll.contentY)
                            errorScroll.contentY = cursorRectangle.y;
                        else if (cursorRectangle.y + cursorRectangle.height > errorScroll.contentY + errorScroll.height)
                            errorScroll.contentY = cursorRectangle.y + cursorRectangle.height - errorScroll.height;
                    }
                }
            }

            MenuButton {
                id: retry

                Layout.fillWidth: true
                visible: root.hasError
                glyph: "\uf2f9"
                label: "Retry reload"
                showChevron: false
                onTriggered: Quickshell.reload(false)
            }
        }
    }
}
