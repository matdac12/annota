# Annota

A Windows tray app that turns a region of the screen into one annotated image for pasting into agent chats.

## Language

**Snap**:
One capture and everything drawn on it, from Caps+S until it is copied or discarded.
_Avoid_: Screenshot, capture (for the whole thing), session

**Region**:
The rectangle of one monitor that the user drags out; it becomes the Snap's image.
_Avoid_: Selection, crop, area

**Overlay**:
The frozen, dimmed view of every monitor shown while the user drags a Region.
_Avoid_: Selector, picker

**Editor**:
The window where the user annotates the Region before copying.
_Avoid_: Canvas, annotation screen

**Box**:
A numbered red rectangle drawn on the Region to point at something.
_Avoid_: Rectangle, highlight, marker, annotation

**Note**:
The text the user writes or dictates for one Box; it may be empty.
_Avoid_: Comment, caption, label

**Note field**:
The card in the Editor where a Note is typed or dictated.
_Avoid_: Input, popup, text box

**Callout**:
A Note burned into the copied image beside its Box, as a yellow sticky with the Box's number.
_Avoid_: Sticky, tooltip, legend

**Resident**:
The one running Annota process that owns the tray and receives Caps+S.
_Avoid_: Daemon, server, main instance
