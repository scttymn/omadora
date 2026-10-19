import QtQuick
import qs.Commons

// Sign in with a Pandora account. The engine checks the email and password
// with Pandora, then keeps the password in the system keyring.
Rectangle {
  id: root

  required property var engine
  required property var theme
  property string fontFamily: Style.font.family

  property bool working: false
  property string error: ""

  color: theme.bg

  function submit() {
    if (working)
      return
    var user = emailField.input.text.trim()
    if (user === "" || passwordField.input.text === "") {
      error = "Enter your Pandora email and password."
      return
    }
    working = true
    error = ""
    engine.signIn(user, passwordField.input.text, function (ok, result) {
      root.working = false
      if (ok) {
        passwordField.input.text = ""
        return
      }
      root.error = String(result || "Pandora did not answer. Try again.")
      passwordField.input.forceActiveFocus()
      passwordField.input.selectAll()
    })
  }

  onVisibleChanged: {
    if (visible)
      Qt.callLater(function () { emailField.input.text === "" ? emailField.input.forceActiveFocus() : passwordField.input.forceActiveFocus() })
  }

  component Field: Rectangle {
    id: field
    property alias input: textInput
    property string placeholder: ""
    property bool secret: false
    signal accepted()
    width: parent.width
    height: Style.space(40)
    radius: Style.cornerRadius
    color: root.theme.surface
    border.color: textInput.activeFocus ? root.theme.accent : root.theme.border
    border.width: 1

    TextInput {
      id: textInput
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.leftMargin: Style.space(12)
      anchors.rightMargin: Style.space(12)
      anchors.verticalCenter: parent.verticalCenter
      clip: true
      color: root.theme.fg
      selectionColor: root.theme.accent
      selectedTextColor: root.theme.accentText
      font.family: root.fontFamily
      font.pixelSize: Style.font.body
      selectByMouse: true
      echoMode: field.secret ? TextInput.Password : TextInput.Normal
      enabled: !root.working
      onAccepted: field.accepted()

      Text {
        anchors.verticalCenter: parent.verticalCenter
        visible: textInput.text.length === 0
        textFormat: Text.PlainText
        text: field.placeholder
        color: root.theme.faint
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
      }
    }

    MouseArea {
      anchors.fill: parent
      enabled: !textInput.activeFocus
      cursorShape: Qt.IBeamCursor
      onClicked: textInput.forceActiveFocus()
    }
  }

  Column {
    anchors.centerIn: parent
    width: Math.min(parent.width - Style.space(48), Style.space(360))
    spacing: Style.space(14)

    OmadoraLogo {
      anchors.horizontalCenter: parent.horizontalCenter
      xHeight: Style.space(16)
      markColor: root.theme.accent
      wordColor: root.theme.strong
    }

    Item { width: 1; height: Style.space(6) }

    Text {
      width: parent.width
      textFormat: Text.PlainText
      text: "Sign in with your Pandora account."
      color: root.theme.fg
      font.family: root.fontFamily
      font.pixelSize: Style.font.body
      horizontalAlignment: Text.AlignHCenter
      wrapMode: Text.WordWrap
    }

    Field {
      id: emailField
      placeholder: "Email"
      onAccepted: passwordField.input.forceActiveFocus()
    }

    Field {
      id: passwordField
      placeholder: "Password"
      secret: true
      onAccepted: root.submit()
    }

    Text {
      visible: root.error !== ""
      width: parent.width
      textFormat: Text.PlainText
      text: root.error
      color: root.theme.accent
      font.family: root.fontFamily
      font.pixelSize: Style.font.bodySmall
      wrapMode: Text.WordWrap
    }

    Rectangle {
      width: parent.width
      height: Style.space(40)
      radius: Style.cornerRadius
      color: submitArea.containsMouse && !root.working ? Qt.lighter(root.theme.accent, 1.08) : root.theme.accent
      opacity: root.working ? 0.7 : 1

      Text {
        anchors.centerIn: parent
        textFormat: Text.PlainText
        text: root.working ? "Signing in…" : "Sign in"
        color: root.theme.accentText
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
        font.bold: true
      }

      MouseArea {
        id: submitArea
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: root.submit()
      }
    }

    Text {
      width: parent.width
      textFormat: Text.PlainText
      text: "Your password is kept in your system keyring, and is only ever sent to Pandora."
      color: root.theme.faint
      font.family: root.fontFamily
      font.pixelSize: Style.font.bodySmall
      horizontalAlignment: Text.AlignHCenter
      wrapMode: Text.WordWrap
    }
  }
}
