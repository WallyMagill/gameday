# gameday README GIF: board → the red-zone beat → the cut → zoom (the
# sitting's winning cut over board → the cut → tv).
# Renders with vhs (https://github.com/charmbracelet/vhs). Run
# `cargo build --release` first, then `vhs docs/demo.tape` from the repo
# root.
# 120x36 cells, the broadcast theme, ~24 s (23.5 s of sleeps plus typing):
# the demo slate seeds at tick 0 and the scripted KC touchdown lands at sim
# tick 15 (one tick per second in --demo), so the three consecutive sleeps
# below sit the viewer on the red zone, then the cut, then the view — they
# are the beat this header narrates, and the checked-in GIF was recorded
# from them. Do not re-record.
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
