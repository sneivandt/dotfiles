import QtQuick
import "Theme.js" as Theme

Rectangle {
    radius: Theme.menuRadius
    color: Theme.menuBackground
    border.width: 1
    border.color: Theme.panelBorder

    gradient: Gradient {
        GradientStop {
            position: 0
            color: Theme.menuBackgroundTop
        }

        GradientStop {
            position: 0.42
            color: Theme.menuBackground
        }

        GradientStop {
            position: 1
            color: Theme.menuBackground
        }

    }

}
