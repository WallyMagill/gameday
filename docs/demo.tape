# gameday README GIF: board → the red-zone beat → the cut → zoom (the
# sitting's winning cut over board → the cut → tv).
# Renders with vhs (https://github.com/charmbracelet/vhs): `vhs docs/demo.tape`.
# 120x36 cells, the broadcast theme, ~20 s: the demo slate seeds at tick 0 and
# the scripted KC touchdown lands at sim tick 15 (one tick per second in --demo),
# so the sleeps below sit the viewer on the red zone, then the cut, then the view.
Output docs/demo.gif
Set Shell "bash"
Set FontSize 14
Set Width 1010
Set Height 640
Set Padding 8
Set TypingSpeed 40ms
Set Framerate 20
Type "target/release/gameday --demo"
Enter
Sleep 9s
Sleep 6s
Sleep 3s
Type "z"
Sleep 5s
Type "q"
Sleep 500ms
