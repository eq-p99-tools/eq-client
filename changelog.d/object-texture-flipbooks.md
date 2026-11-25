- Animate complete WLD texture sequences on placed objects using each material's
  authored interval. Shared placements and reflected material copies advance
  together, and zone changes discard their controllers. Doors using the zone's
  object materials share the animation. Boat actor metadata is retained, while
  its separate renderer still uses the initial image. Shared object phase is
  an inferred presentation choice.
