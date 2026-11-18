# Detachable chat tabs

Click a tab to select its channel filter. Drag it to the left or right half of
another tab to insert it on that side. Drop it on the open part of a tab strip
to append it. The blue insertion mark shows where the tab will land.

To open a separate chat window, drag a tab off the chat and release it over the
game view. To dock it again, drop it onto another window's tab strip. Drag a
window by its title to move it. Resize from its edges or corners where the
window supports resizing.

Escape cancels a tab drag. Releasing over other controls, or moving the pointer
out of the client window, also cancels the drag. Close a detached window with its close box or with
Escape when it is the foremost closable window. Its tabs return to the main
chat. The main chat also stays available as a dock when all its tabs are detached.
Detached windows keep their stack order when zoning rebuilds the HUD.

Each window shows its selected tab. A moved tab keeps its channel filter,
scrollback, unread tracking and scroll position for the current session. Tabs share
the same retained chat history, and item and web links work in each window.
Click an input line or select a tab to choose where Enter opens the shared
draft. Plain text uses that window's selected channel. Slash commands still
choose channels explicitly.

Enter sends the current line and returns keyboard input to the game. Keys
pressed after Enter start from that new state, including a slash opening a
new command. Holding Enter does not repeatedly send or reopen the line.
Escape leaves composition while keeping the draft for the next Enter.
While typing, Shift+Up and Shift+Down step through the last 40 lines sent from
that window, as the Titanium program does; Shift+Down past the newest clears
the line. The lines stay in memory only, for that window and character.

Tab membership, order, selection and window placement are saved with the
character's window layout.
