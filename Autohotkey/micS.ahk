; 请替换为你实际的麦克风Device#
micName := 8

; 创建 GUI
myGui := Gui("+AlwaysOnTop +ToolWindow")
myGui.BackColor := "202020"
myGui.SetFont("s10 cWhite", "Segoe UI")

; 状态显示文字
statusText := myGui.AddText("x10 y10 w180 h25 Center", "检测中...")

; 添加两个按钮
btnMute := myGui.AddButton("x10 y45 w85 h30", "静音")
btnUnmute := myGui.AddButton("x105 y45 w85 h30", "解除静音")

; 绑定按钮事件
btnMute.OnEvent("Click", (*) => SetMicMute(true))
btnUnmute.OnEvent("Click", (*) => SetMicMute(false))

; 显示在右下角，不抢焦点
myGui.Show("x" A_ScreenWidth-320 " y" A_ScreenHeight-210 " NoActivate")

lastState := ""
SetTimer CheckMic, 500

; 设置静音状态的函数
SetMicMute(mute) {
    global micName
    try {
        SoundSetMute(mute, , micName)  ; 参数：设置值, 组件(留空), 设备名
    } catch as e {
        TrayTip "操作失败", e.Message
    }
}

; 轮询检测状态的函数
CheckMic() {
    global lastState, statusText, micName
    try {
        muteState := SoundGetMute( , micName)
        state := (muteState = 1) ? "静音" : "未静音"
        color := (muteState = 1) ? "cRed" : "cLime"
    } catch {
        state := "无法读取"
        color := "cGray"
    }
    if (state != lastState) {
        lastState := state
        statusText.Text := "麦克风: " state
        statusText.SetFont(color)
    }
}