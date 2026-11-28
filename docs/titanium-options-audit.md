# Titanium Options parity audit

**Baseline:** eq-client `f9091d1e` (#811), before the Colors redesign. The installed Titanium default skin declares **seven top-level tabs** in the order below. Display contains three more tabs. Velious declares the same hierarchy. The inventory below follows controls referenced by the default skin, excluding captions and value labels. “Bound” means the renderer connects the control to a setting or action. This is a source audit, not an in-game acceptance result.

| Titanium tab | Bound or available another way | Missing from the tab, partial, or misplaced |
| --- | --- | --- |
| General | Pet Window Popup, Allow Auto-stand, music volume and sound volume have bindings. Item Dropping has a separate saved preference and `/fastdrop`. | No Options binding found for fast item destroy, guild invites, autosplit, anonymous/roleplaying, context menus, environment sounds, combat music, load skin, system-info consent, target-click help, tell windows, Allow Auto-duck, fast AA/advanced merchant/leadership purchases, join general channels, group/raid/guild auto-consent, sound realism, font selector/Accept, player trade or item dropping. An existing movement feature or slash command does not make its checkbox work. |
| Display | PC names, NPC names, helm, target ring, far clip and max FPS are bound. Fullscreen switching has a keyboard route. | Level of detail and bias, gamma, window fade delay/duration/opacity/fade-to, sky, loading screen, Video Modes, Advanced and the window-mode buttons lack corresponding Options actions. The overlapping Windowed button is intentionally omitted. |
| Mouse | Invert Y, Look Spring, wheel zoom and sensitivity are bound. A skin missing Look Spring can receive a generated Mouse control. | Mouse Look and right-handed mouse lack bindings. The Look Spring fallback can land on QoL if the skin has no recognizable Mouse control. |
| Keys | Assignment list and category filter work for supported actions, including primary/alternate bindings. | Reload defaults has no Options button handler. The list follows the client's supported actions, so a working list does not establish complete Titanium command coverage. |
| Filters | The client replaces the native list and choice box in place with working per-category controls. `/filter` opens this page. | The widget and storage differ. Available rows and when changes reach the server depend on dialect. See [server filters](settings.md#server-filters). |
| Colors | Chat already supports per-character RGB overrides through `/usercolor`. | The authored color list, change-selected and reset-default controls have no Options binding at baseline. Two always-expanded floating combat text pickers occupy QoL. Put chat and combat-text swatches together here and retain their existing saved values. |
| Mail | The stock page is present in the skin. | No Options binding found for address-book path/browse, forwarding, new-mail visual/audio notifications or address-book deletion confirmation. Mail service support needs independent verification. |

### Display detail

| Titanium surface | Controls | Current placement or gap |
| --- | --- | --- |
| Display → Spell | Opacity, filter, density, near clip | The client adds a top-level Particles page with one Show Spell Effects switch. It cannot express these settings. The separate density command distinguishes off from on, without four rendering densities. |
| Display → Env | Opacity, density, near clip | No per-environment bindings found. |
| Display → Player | Opacity, filter, density, near clip | No per-actor bindings found. |
| Video Modes dialog | Resolution list, refresh rate, bit depth, Accept | No opener on Display. A fullscreen shortcut covers only the mode switch. |
| Advanced Display dialog | Shadows, water swap, shader controls, actor/shadow clipping, sky reflection/update, card features and advanced lighting | No opener on Display. Renderer capability and legacy hardware-specific controls require individual decisions. |

### Reorganization priorities

1. **Colors first.** Keep the floating-text enable switch with the feature. Put color editing, preview and reset on Colors. A small swatch can open the same picker for chat and damage colors.
2. **Wire existing behavior in its native location.** Item Dropping on General and the window-mode buttons on Display already have related behavior to reuse. Check consent, auto-duck and role flags against their actual owners before enabling their controls.
3. **Restore the Display hierarchy.** Group Spell, Env and Player particle settings under Display. Mark unsupported effects explicitly. Keep client-specific visual settings such as interface scale and the FPS overlay distinguishable from Titanium's Max FPS cap.
4. **Keep service and hardware gaps separate.** Mail forwarding, system-info transmission and historical shader switches need their own capability decisions. A placeholder should never imply a successful operation.

**Evidence.** The local installation supplies `EQUI_OptionsWindow.xml`, `EQUI_AdvancedDisplayOptionsWnd.xml` and `EQUI_VideoModesWnd.xml`. The client-side checks are the [button bindings](../crates/eq-client-render/src/skinned/bindings.rs), [slider, list and combo bindings](../crates/eq-client-render/src/skinned/controls.rs), [Filters](../crates/eq-client-render/src/skinned/filters.rs), [Particles](../crates/eq-client-render/src/skinned/particles.rs), [QoL page](../crates/eq-client-render/src/skinned/qol.rs) and [slash settings](../crates/eq-client-render/src/local_settings.rs). This comparison does not establish that every control in the stock skin was enabled by every server or graphics card.

