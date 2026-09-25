# Run through docs/demo/record.sh, which sets LAZYJIRA_DEMO_HOME and LAZYJIRA_DEMO_PATH.

Output docs/images/demo.gif

Set Shell "bash"
Set FontSize 14
Set Width 1400
Set Height 760
Set Padding 12
Set Framerate 24
Set PlaybackSpeed 1.0

Hide
Type 'export HOME="$LAZYJIRA_DEMO_HOME" TMPDIR="$LAZYJIRA_DEMO_HOME/tmp" PATH="$LAZYJIRA_DEMO_PATH"'
Enter
# Warm the caches first so the recording shows a normal cache-first start.
Type "lazyjira"
Enter
Sleep 4s
Type "q"
Type "clear && lazyjira"
Enter
Sleep 3s
Show

# My Work: open a ticket
Sleep 2s
Down
Sleep 800ms
Enter
Sleep 3.5s
Escape
Sleep 800ms

# Team
Tab
Sleep 2.5s

# Epics
Tab
Sleep 3s

# Unassigned
Tab
Sleep 2s

# Filters: run "My bugs"
Tab
Sleep 1s
Enter
Sleep 2.5s

Hide
Type "q"
