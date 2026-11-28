# Titanium Options audit

Remaining differences from the installed Titanium Options window, checked against client main `b8813eb4`. Titanium default and Velious have the same seven stock tabs. The picker and layout findings below come from in-game checks. TAKP’s Colors layout still needs an attended check.

**Trivial** means a local UI or binding change. **New feature** means the underlying behavior or service still needs work. **Skip** records a deliberate difference. For controls already supported and their placement, see [Settings](settings.md) and [Skin extensions](skin-extensions.md).

## General

| Option | Remaining difference | Rating | Work or reason |
| --- | --- | --- | --- |
| Client-added option markers | No asterisks visible in the Titanium General tab during the live check | Trivial | The three added rows should show a blue asterisk beside Ask Before Deleting Spells, Show Times in Chat and Flash on Tells. Check visibility after scrolling to them. Red is reserved for unsafe additions. |
| Receive Guild Invites | Missing binding | New feature | Receiving an invitation is separate from saving and honoring an invitation policy. Verify who enforces the preference. |
| Allow Context Menus | Missing binding | New feature | Define which existing menus obey the switch and supply a common input gate across windows. |
| Combat Music | Preference only | New feature | The option and slash command already exist. Encounter track selection and playback are missing. |
| Load UI Skin | Command only | New feature | `/loadskin` already switches skins. The button needs a skin browser, selection and error feedback. |
| Allow SysInfo Send | Missing | Skip | The historical vendor reporting service has no destination in this client. |
| Target Click Shows Help | Missing binding | New feature | Implement the target-specific help and when it appears before exposing a switch. |
| Use Tell Windows | Missing binding | New feature | Detachable chat tabs exist. Automatically routing tells into individual conversations requires additional behavior. |
| Fast AA Purchase | Missing binding | New feature | Requires a working AA purchase/confirmation path for the active server dialect. |
| Join General Channels | Missing binding | New feature | Implement or verify automatic channel enrollment on login and how failures are shown. |
| Auto Consent Group | Missing binding | New feature | Establish the group-specific corpse consent setting and its server update. |
| Auto Consent Raid | Missing binding | New feature | Same consent work with raid membership and server support. |
| Auto Consent Guild | Missing binding | New feature | Same consent work with guild membership and server support. |
| Fast Advanced Merchant Purchase | Missing binding | New feature | Verify alternate-currency purchases and their confirmation rules. Ordinary coin sales are a different path. |
| Fast Leadership Purchase | Missing binding | New feature | Requires leadership ability purchase and confirmation support. |
| Sound Realism | Missing slider | New feature | Identify the native audible effect and map it to the mixer. A second volume slider would give the wrong behavior. |
| Font selector and Accept | Missing selection and apply | New feature | The client has font sizes. Changing the UI font family requires font discovery, loading and reflow. |
| Player Trade | Missing selector | New feature | Define the selectable trade policies and how incoming offers obey them. |

## Display

| Option | Remaining difference | Rating | Work or reason |
| --- | --- | --- | --- |
| Level of Detail | Missing binding | New feature | Implement or identify a model-detail policy with a visible effect. |
| LOD Bias | Missing slider | New feature | Needs the same working detail-selection policy. |
| Gamma | Missing slider | New feature | Add calibrated output adjustment. Zone lighting math alone does not implement a user gamma control. |
| Video Modes | No exclusive display-mode dialog | Skip | Resizing the window sets its size. Borderless fullscreen follows the desktop mode. The in-game Max FPS control sets the frame cap independently. |
| Advanced | Missing opener | New feature | Needs a useful set of supported renderer controls. |
| Sky | Missing selector | New feature | Define and render the supported sky detail levels. |
| Loading Screen | Missing selector | New feature | Implement the stock choices where corresponding loading presentations exist. |

### Display → Spell

| Option | Remaining difference | Rating | Work or reason |
| --- | --- | --- | --- |
| Opacity | Missing | New feature | Apply a saved multiplier to spell particle rendering. |
| Filter | Missing | New feature | Select effects by caster or target according to the native modes. |
| Density | Partial via command | New feature | The current particle-density command separates off from on. Implement the distinct active densities. |
| Near Clip | Missing | New feature | Apply camera-distance clipping to the correct emitters. |

### Display → Env

| Option | Remaining difference | Rating | Work or reason |
| --- | --- | --- | --- |
| Opacity | Missing | New feature | Apply opacity to environment emitters independently of spells. |
| Density | Missing | New feature | Implement the environment emission levels. |
| Near Clip | Missing | New feature | Identify and clip the environment effect group. |

### Display → Player

| Option | Remaining difference | Rating | Work or reason |
| --- | --- | --- | --- |
| Opacity | Missing | New feature | Apply opacity to actor effects. |
| Filter | Missing | New feature | Select actor effects according to the native filter. |
| Density | Missing | New feature | Implement actor-effect emission levels. |
| Near Clip | Missing | New feature | Clip actor effects independently of spell and environment effects. |

### Display → Video Modes dialog

| Option | Remaining difference | Rating | Work or reason |
| --- | --- | --- | --- |
| Video resolution | Uses the window or desktop size | Skip | The player resizes the window or uses borderless fullscreen. |
| Refresh rate | Set by Windows | Skip | Borderless fullscreen uses the monitor’s current desktop refresh rate. |
| Bit depth | Missing | Skip | No user-selectable legacy display-depth path is exposed by the renderer. |
| Accept | No exclusive mode to apply | Skip | Window resizing and the existing fullscreen switch take effect directly. |

### Display → Advanced dialog

| Option | Remaining difference | Rating | Work or reason |
| --- | --- | --- | --- |
| Shadows | Missing stock control | New feature | Torch Shadows handles one player light. Stock scene shadow scope needs a separate renderer policy. |
| Water swap | Missing stock control | Skip | No equivalent legacy rendering path has been identified. Keep Better Water as its own option. |
| Vertex shaders | Missing | Skip | The modern renderer requires shaders. |
| Pixel shaders 1.1 | Missing | Skip | No selectable DirectX 8 shader-model path. |
| Pixel shaders 1.4 | Missing | Skip | Same obsolete hardware path. |
| Pixel shaders 2.0 | Missing | Skip | Same obsolete hardware path. |
| Actor clip plane | Missing slider | New feature | Entity loading distance already exists. Verify drawing, culling and reload behavior before binding it as actor clipping. |
| Shadow clip plane | Missing slider | New feature | Requires a supported shadow-distance policy. |
| Sky reflection size | Missing selector | New feature | Requires configurable reflection resolution in a supported water/render path. |
| Sky update interval | Missing selector | New feature | Requires configurable reflection/sky update scheduling. |
| Card features list | Missing | Skip | A historical GPU capability checklist would describe the wrong graphics API. |
| Advanced lighting | Missing stock control | New feature | Establish which native lighting changes it controls. Rich Lighting is a separate visual departure. |

## Mouse

| Option | Remaining difference | Rating | Work or reason |
| --- | --- | --- | --- |
| Mouse Look | Missing checkbox | New feature | Held RMB look exists. Verify the stock toggle and implement persistent look mode and its release rules. |
| Right-handed mouse | Missing checkbox | New feature | Apply one consistent mapping across world input, UI clicks, dragging and mouselook without double-swapping OS input. |

## Keys

| Option | Remaining difference | Rating | Work or reason |
| --- | --- | --- | --- |
| Assignment list | Partial | New feature | Existing supported actions can be edited and filtered. Missing original commands need working input actions before adding their rows. |

## Filters

| Option | Remaining difference | Rating | Work or reason |
| --- | --- | --- | --- |
| Applying filter changes | Different timing and storage | Skip | Keep per-character profiles and the supported server update rules. A second toggle would not improve this. |

## Colors

| Option | Remaining difference | Rating | Work or reason |
| --- | --- | --- | --- |
| Click the swatch | Clicking the colored square does nothing | Trivial | Open the same picker that already opens from the label or Change Selected Color. |
| Escape | Closes the entire Options window | Trivial | Dismiss the picker first and keep Options open. |
| Click elsewhere in Options | The picker stays open | Trivial | Dismiss it when clicking another part of Options, as already happens when clicking the world. |
| Swatch alignment | Four samples sit one line below their labels in default and Velious | Trivial | Align OOC, You hit others, Others hit you and You miss others with their samples. |
| Chat color 86 | The row still has a numeric label | Trivial | Identify its message category before assigning a name. |

## Mail

| Option | Remaining difference | Rating | Work or reason |
| --- | --- | --- | --- |
| Address-book path and Browse | Missing | Skip | A legacy local path chooser adds little on its own. Revisit storage if an address book is implemented. |
| Mail forwarding | Missing | New feature | Needs an account/service implementation that can actually configure forwarding. |
| New mail notification | Missing | New feature | Needs a received new-mail event and UI notification. |
| New mail audio notification | Missing | New feature | Needs the same event, sound choice and playback. |
| Confirm address-book deletion | Missing | New feature | Needs editable contacts and an actual deletion operation. |

## QoL (client-added tab)

| Option | Remaining difference | Rating | Work or reason |
| --- | --- | --- | --- |
| Button row spacing | Large empty gaps remain between rows on Titanium | Trivial | Lay out the controls that remain on this page after moving additions to their destination tabs. |
| Dedicated QoL tab | Titanium has no equivalent tab | Skip | Keep client-only settings here when they have no native destination. The Ordinary and Dangerous headings distinguish these settings. Individual risk markers are intended for additions placed on stock tabs. |
