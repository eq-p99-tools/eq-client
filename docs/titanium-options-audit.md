# Titanium Options audit

The tables record the differences found during the audit. The follow-up implements the **Trivial** rows. Display hosts the three lighting and water controls where the skin has no Advanced page. Map owns the tracked-name outline control. **New feature** and **Skip** retain their original classifications. See [Settings](settings.md) for the resulting behavior and [Skin extensions](skin-extensions.md) for placement rules.


**Start with General and Keys.** Autosplit, Anonymous, Roleplaying, Environment Sounds, Item Dropping and Reload defaults can all reuse behavior the client already has. The largest structural difference is under Display, where Titanium has separate Spell, Env and Player particle pages. Our single Particles checkbox covers only part of the first page.

This compares client main `f9091d1e` (#811) with the installed Titanium default skin. Velious declares the same seven top-level tabs in the same order. The rows below cover **differences**, including controls the skin draws but the client does not bind. Colors also shows what this PR changes. Ratings are implementation estimates from source inspection. The active server, graphics hardware and skin can affect availability.

| Rating | What it means here |
| --- | --- |
| **Trivial** | Bind or move an existing control, or add a small local preference around an existing operation. Still test saving, restoring and the real action. |
| **New feature** | A checkbox alone would be misleading. A subsystem, interaction, renderer behavior, or server-backed preference still needs implementation or verification. |
| **Skip** | Reproducing this control as presented would have no useful effect on this client. For client-only options this means keep the existing feature without adding a duplicate native control. |

## General

Pet Window Popup, Allow Auto-stand, Sound Volume and Music Volume already have stock-page bindings.

| Option | Difference | Rating | Work or reason |
| --- | --- | --- | --- |
| Fast Item Destroy | Missing binding | Trivial | Add a saved bypass for the existing destroy confirmation. Keep the existing item eligibility and submission checks. |
| Receive Guild Invites | Missing binding | New feature | Receiving an invitation is separate from saving and honoring an invitation policy. Verify who enforces the preference. |
| Autosplit | Missing binding | Trivial | Read the current sharing state and invoke the same set-on/set-off request as `/autosplit`. |
| Anonymous | Missing binding | Trivial | Use the existing `/anonymous` request and display the received presence state. |
| Roleplaying | Missing binding | Trivial | Use the existing `/roleplay` request and keep the two presence checkboxes in sync. |
| Allow Context Menus | Missing binding | New feature | Define which existing menus obey the switch and supply a common input gate across windows. |
| Environment Sounds | Missing binding | Trivial | Bind the existing saved option. The audio system already uses it for ambient emitters. |
| Combat Music | Preference only | New feature | The option and slash command already exist. Encounter track selection and playback are missing. |
| Load UI Skin | Command only | New feature | `/loadskin` already switches skins. The button needs a skin browser, selection and error feedback. |
| Allow SysInfo Send | Missing | Skip | The historical vendor reporting service has no destination in this client. |
| Target Click Shows Help | Missing binding | New feature | Implement the target-specific help and when it appears before exposing a switch. |
| Use Tell Windows | Missing binding | New feature | Detachable chat tabs exist. Automatically routing tells into individual conversations requires additional behavior. |
| Allow Auto-duck | Missing binding | Trivial | Add a saved guard to automatic crouching. Check manual Duck, standing recovery and low-clearance routes. |
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
| Item Dropping | Command only | Trivial | Bind the three existing `/fastdrop` choices and saved `NoSafeDrop` value. Keep destroy separate. |

## Display

PC names, NPC names, helm, 3D target ring, far clip and Max FPS already have bindings. Showing the frame-rate meter is a separate client addition.

| Option | Difference | Rating | Work or reason |
| --- | --- | --- | --- |
| Level of Detail | Missing binding | New feature | Implement or identify a model-detail policy with a visible effect. |
| LOD Bias | Missing slider | New feature | Needs the same working detail-selection policy. |
| Gamma | Missing slider | New feature | Add calibrated output adjustment. Zone lighting math alone does not implement a user gamma control. |
| Fade Delay | Imported behavior, missing slider | Trivial | Set the delay already consumed by the skinned-window fade. Verify how a global edit interacts with per-window settings. |
| Fade Duration | Imported behavior, missing slider | Trivial | Expose the existing fade duration with the same scope check. |
| Window Alpha | Imported behavior, missing slider | Trivial | Expose the normal window background opacity. |
| Fade-to Alpha | Imported behavior, missing slider | Trivial | Expose the existing faded opacity. |
| Switch to Fullscreen | Keyboard only | Trivial | Reuse the window-mode action and show this button only when windowed. |
| Switch to Windowed | Stock button omitted | Trivial | Share the same handler and show the inverse button when fullscreen. The stock buttons overlap. |
| Video Modes | Missing opener | New feature | Needs the actual selection and apply flow listed below. |
| Advanced | Missing opener | New feature | Needs a useful set of supported renderer controls. |
| Sky | Missing selector | New feature | Define and render the supported sky detail levels. |
| Loading Screen | Missing selector | New feature | Implement the stock choices where corresponding loading presentations exist. |
| Particles tab | New top-level page in our client | Trivial | Move its existing Spell Effects switch under Display → Spell. The other particle controls need separate work. |

### Display → Spell

| Option | Difference | Rating | Work or reason |
| --- | --- | --- | --- |
| Opacity | Missing | New feature | Apply a saved multiplier to spell particle rendering. |
| Filter | Missing | New feature | Select effects by caster or target according to the native modes. |
| Density | Partial via command | New feature | The current particle-density command separates off from on. Implement the distinct active densities. |
| Near Clip | Missing | New feature | Apply camera-distance clipping to the correct emitters. |

### Display → Env

| Option | Difference | Rating | Work or reason |
| --- | --- | --- | --- |
| Opacity | Missing | New feature | Apply opacity to environment emitters independently of spells. |
| Density | Missing | New feature | Implement the environment emission levels. |
| Near Clip | Missing | New feature | Identify and clip the environment effect group. |

### Display → Player

| Option | Difference | Rating | Work or reason |
| --- | --- | --- | --- |
| Opacity | Missing | New feature | Apply opacity to actor effects. |
| Filter | Missing | New feature | Select actor effects according to the native filter. |
| Density | Missing | New feature | Implement actor-effect emission levels. |
| Near Clip | Missing | New feature | Clip actor effects independently of spell and environment effects. |

### Display → Video Modes dialog

| Option | Difference | Rating | Work or reason |
| --- | --- | --- | --- |
| Video resolution | Missing in Options | New feature | Enumerate modes on the current monitor and apply a supported selection. |
| Refresh rate | Missing in Options | New feature | Enumerate rates for the selected mode and monitor. |
| Bit depth | Missing | Skip | No user-selectable legacy display-depth path is exposed by the renderer. |
| Accept | Missing | New feature | Apply the selected mode and recover from a failed switch. |

### Display → Advanced dialog

| Option | Difference | Rating | Work or reason |
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

Invert Y, Look Spring, wheel zoom and sensitivity are bound in the default skin.

| Option | Difference | Rating | Work or reason |
| --- | --- | --- | --- |
| Mouse Look | Missing checkbox | New feature | Held RMB look exists. Verify the stock toggle and implement persistent look mode and its release rules. |
| Right-handed mouse | Missing checkbox | New feature | Apply one consistent mapping across world input, UI clicks, dragging and mouselook without double-swapping OS input. |
| Look Spring fallback | May appear on QoL with a custom skin | Trivial | Prefer Mouse if the skin has that page. Stock default already places it correctly. |

## Keys

| Option | Difference | Rating | Work or reason |
| --- | --- | --- | --- |
| Reload defaults | Missing button action | Trivial | Restore the active generation's default primary and alternate assignments through the existing key map and profile save path. |
| Assignment list | Partial | New feature | Existing supported actions can be edited and filtered. Missing original commands need working input actions before adding their rows. |

## Filters

| Option | Difference | Rating | Work or reason |
| --- | --- | --- | --- |
| Filter list | Replaced by generated category rows | Trivial | Present the existing categories in the skin's selectable list if exact widget parity is desired. |
| Choice for selected filter | Replaced by per-row cycling | Trivial | Bind the stock selector to the same category modes. Keep dialect-specific availability. |
| Applying filter changes | Different timing and storage | Skip | Keep per-character profiles and the supported server update rules. A second toggle would not improve this. |

## Colors

| Option | Difference | Rating | Work or reason |
| --- | --- | --- | --- |
| Chat color list and samples | Empty on #811 | Trivial | Existing installed colors and per-character overrides supplied the data. This PR populates the list. |
| Change Selected Color | Missing on #811 | Trivial | This PR makes each square open the shared hue, shade and hex picker directly. |
| Reset Default Colors | Missing on #811 | Trivial | This PR clears the current profile's chat overrides and restores both combat-text defaults. |
| Other damage | Client addition, formerly on QoL | Trivial | Moved to Colors in this PR. The previous percentage RGB values carry over. |
| Damage to you | Client addition, formerly on QoL | Trivial | Same move and saved-value compatibility. |

The new list has **84 chat swatches and two combat-text swatches**. It omits two unused slots. Chat color 86 retains a numeric label pending identification. The late chat labels use the server's message types, including damage over time, raid chat, item speech and stuns. [EQEmu chat type definitions](https://github.com/EQEmu/EQEmu/blob/master/common/eq_constants.h)

## Mail

| Option | Difference | Rating | Work or reason |
| --- | --- | --- | --- |
| Address-book path and Browse | Missing | Skip | A legacy local path chooser adds little on its own. Revisit storage if an address book is implemented. |
| Mail forwarding | Missing | New feature | Needs an account/service implementation that can actually configure forwarding. |
| New mail notification | Missing | New feature | Needs a received new-mail event and UI notification. |
| New mail audio notification | Missing | New feature | Needs the same event, sound choice and playback. |
| Confirm address-book deletion | Missing | New feature | Needs editable contacts and an actual deletion operation. |

## Particles (client-added tab)

| Option | Difference | Rating | Placement |
| --- | --- | --- | --- |
| Show Spell Effects | Client-added binary switch on a separate tab | Trivial | Move it under Display → Spell. Titanium offers density and other controls there. Preserve the saved on/off preference. |

## QoL (client-added tab)

All entries below are additions relative to the stock Options tabs. **Trivial** here rates moving an existing control. **Skip** means keep it on QoL and skip adding a second native-tab control. Always-on client behavior without a checkbox is outside this inventory.

| Option | Difference | Rating | Placement |
| --- | --- | --- | --- |
| Skip Food With Modifiers | Client addition | Skip | Keep the food-selection preference on QoL. |
| Draw Windows the Skin Hides | Client addition | Skip | Keep the explicit skin override on QoL. |
| Ask Before Deleting Spells | Client addition | Trivial | Fits General alongside other confirmation preferences. |
| Show Times in Chat | Client addition | Trivial | Fits General alongside tell-window behavior. |
| Flash on Tells | Client addition | Trivial | Fits General beside the other tell preference. |
| Corpses Lie Along the Ground | Client addition | Trivial | Fits Display as an actor presentation preference. |
| Long Chase Camera | Client addition | Trivial | Fits Display as a camera setting. |
| Free Camera | Client addition | Skip | Keep the independent camera override on QoL. |
| See the World Clearly | Client addition | Skip | Keep the visibility override on QoL. |
| Rich Lighting | Client addition | Trivial | Could move to Display → Advanced under its existing name. |
| Torch Shadows | Client addition | Trivial | Could move to Display → Advanced alongside lighting. |
| Better Water | Client addition | Trivial | Could move to Display → Advanced as a distinct water option. |
| Walk While Winded | Client addition | Skip | Keep the movement override on QoL. |
| Move While Feigned | Client addition | Skip | Keep the movement override on QoL. |
| Stand When Hit | Client addition | Skip | Keep separate from General’s Allow Auto-stand, which has a different trigger. |
| Loot Corpses Automatically | Client addition | Skip | Keep the automated loot preference on QoL. |
| Stay in Melee Range | Client addition | Skip | Keep the automatic positioning preference on QoL. |
| Floating Combat Text | Client addition | Skip | Keep the enable switch on QoL. Its two color choices belong in Colors. |
| Read the Spellbook Standing | Client addition | Skip | Keep the posture override on QoL. |
| Memorize Without Sitting | Client addition | Skip | Keep the memorization override on QoL. |
| Show Invisible Entities | Client addition | Skip | Keep the visibility override on QoL. |
| Answer List Presence Polls | Client addition | Skip | Keep the server-specific behavior on QoL. |
| Take Environmental Damage | Client addition | Skip | Keep the environmental damage preference on QoL. |
| Gills for Days | Client addition | Skip | Keep the breathing override on QoL. |
| Fly | Client addition | Skip | Keep the movement override on QoL. |
| Jesus Mode | Client addition | Skip | Keep the water traversal override on QoL. |
| Exit at Once | Client addition | Skip | Keep the exit behavior on QoL. |
| Zone Anywhere | Client addition | Skip | No native-tab counterpart. Moving the setting would not implement zoning. |
| Duel Answer Card | Client addition | Skip | Keep the client’s duel prompt preference on QoL. |
| Use Picks Nearest | Client addition | Skip | Keep the target-selection preference on QoL. |
| Show FPS | Client addition | Trivial | Fits Display beside Max FPS. One shows a meter and the other sets a limit. |
| Interface scale | Client addition | Trivial | Fits Display. Verify layout and persistence at the existing scale limits. |
| Run a Little Faster | Client addition | Skip | Keep the movement-rate override on QoL. |
| Outline tracked names | Client addition | Trivial | Expose through the existing Map display control. Both controls already edit the same value. |

## Evidence and scope

The stock inventory comes from the installed Titanium default and Velious `EQUI_OptionsWindow.xml` files, plus the default Advanced Display Options and Video Modes dialog definitions. A skin declaring a widget establishes its placement. The rating uses client bindings and the backing operation to distinguish an editable preference from an inert widget. Exact native behavior and per-server availability still need verification where the row calls for it.

- [Stock option button bindings](../crates/eq-client-render/src/skinned/bindings.rs) and [option controls](../crates/eq-client-render/src/skinned/controls.rs)
- [Window opacity and fading](../crates/eq-client-render/src/skinned/looks.rs)
- [Chat and combat color rows](../crates/eq-client-render/src/skinned/colors.rs)
- [Client QoL setting registry](../crates/eq-client-core/src/qol.rs)
