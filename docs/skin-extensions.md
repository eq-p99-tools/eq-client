# Custom skin extensions

[Documentation index](README.md)

This client accepts optional label bindings in the XML namespace
https://eq-p99-tools.github.io/eq-client/skin. The namespace is an identifier
and requires no network request. A namespaced attribute avoids allocating
numeric EQType values. Only recognized bindings activate client values.
Unknown values or other namespaces keep normal label behavior.

## Experience percentage

Declare the namespace on the XML root and place a label in the desired layout.

~~~xml
<XML xmlns:eqc="https://eq-p99-tools.github.io/eq-client/skin">
  <Label item="MyExperiencePercent" eqc:value="experience-percent">
    <Location><X>120</X><Y>60</Y></Location>
    <Size><CX>80</CX><CY>14</CY></Size>
    <Font>2</Font>
    <Text></Text>
  </Label>
  <!-- Include MyExperiencePercent among the desired Screen's Pieces. -->
</XML>
~~~

The label displays progress through the current level with two decimal places
and a percent sign, such as 37.27%. It stays empty until progress is known.
Font, color, alignment and placement follow the authored label.
The binding takes priority if that label also declares an EQType.

The existing server-reported field has 330 steps per level, so each step is
about 0.303 percentage points. Two decimal places do not recover finer XP or
estimate it from kills. For example, reported value 123 displays 37.27%.
Values at or beyond the full scale display 100.00%.

Stock skins continue using their authored labels. No separate client option
is needed. Other clients may ignore the attribute, so an empty Text provides
a suitable fallback.

## Options controls

Use an original Titanium or TAKP skin as installed. The Options window reads native controls by their ScreenID. Moving a supported checkbox or changing its caption keeps its binding. A Titanium Colors list and TAKP's numbered color buttons both open the same picker. Unsupported native actions keep their authored placement.

Client-added options appear after the skin's controls in the relevant tab. A blue asterisk marks an ordinary addition. A red asterisk marks a setting classified as unsafe in QoL. A skin can place an extension by ID to choose its location. Options without a matching native destination are available under QoL. The client builds these controls in memory each time it loads the skin.

| Control ID | Purpose |
| --- | --- |
| `EQC_OutlineTrackedNames` | Checkbox for outlining tracked names on the map. |
| `EQC_CombatOutgoingPicker` | “Other damage” color picker for combat text. |
| `EQC_CombatIncomingPicker` | “Damage to you” color picker for combat text. |

Combat text colors save each channel in one-percent increments. Chat swatches use the same picker and retain full byte precision.
