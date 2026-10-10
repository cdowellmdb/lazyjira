# Run through docs/demo/record.sh, which sets LAZYJIRA_DEMO_HOME and LAZYJIRA_DEMO_PATH.

Output docs/images/demo.gif

Set Shell "bash"
Set FontSize 16
Set Width 1200
Set Height 760
Set Padding 16
Set Framerate 15
Set PlaybackSpeed 1.0
Set Theme "Catppuccin Mocha"

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

# My Work: detail shows the top-left close control and Jira markup.
Sleep 2s
Down
Sleep 800ms
Enter
Sleep 2.5s
Screenshot docs/images/ticket-detail.png
Sleep 500ms
Type@150ms "jjjjjj"
Sleep 1.5s
Type "G"
Sleep 1.5s
Right
Sleep 1.5s
Type "z"
Sleep 1.5s
Type "z"
Sleep 800ms
Escape
Sleep 800ms

# Live settings, including roster, pinned epics, and starting tab.
Type "S"
Sleep 2.5s
Tab 2
Sleep 800ms
Down
Sleep 1s
Enter
Sleep 1.5s

# Write a ticket with searchable pickers, labels, and a multiline description.
Type "c"
Sleep 800ms
Tab
Type@55ms "Add mobile receipt downloads"
Tab
Type@100ms "Priya"
Sleep 1s
Tab
Type@100ms "Checkout"
Sleep 1s
Tab
Type@60ms "mobile, checkout"
Tab
Type@40ms "Let customers save a receipt after checkout."
Ctrl+J
Type@40ms "Include totals and a tax breakdown."
Sleep 2.5s
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

# Return to My Work with the previous selection restored.
Tab 2
Sleep 2s

Hide
Type "q"
