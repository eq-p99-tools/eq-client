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
Font, color, alignment, wrapping and placement follow the authored label.
The binding takes priority if that label also declares an EQType.

The existing server-reported field has 330 steps per level, so each step is
about 0.303 percentage points. Two decimal places do not recover finer XP or
estimate it from kills. For example, reported value 123 displays 37.27%.
Values at or beyond the full scale display 100.00%.

Stock skins continue using their authored labels. No separate client option
is needed. Other clients may ignore the attribute, so an empty Text provides
a suitable fallback.
