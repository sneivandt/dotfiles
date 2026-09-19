import QtQuick
import "Theme.js" as Theme

Rectangle {
    radius: Theme.controlRadius + 1
    color: Theme.barSurfaceBottom
    implicitHeight: Theme.barControlHeight

    gradient: Gradient {
        GradientStop {
            position: 0
            color: Theme.barSurfaceTop
        }

        GradientStop {
            position: 1
            color: Theme.barSurfaceBottom
        }

    }

}
