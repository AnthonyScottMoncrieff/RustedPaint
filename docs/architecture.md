# Initial architecture

The core owns a `Document`, ordered layers, and a document revision. A layer
has stable identity, a name, visibility, opacity, blend mode, and pixels.
Presentation, file codecs, tool interactions, and undo history will be separate
adapters around this core.

```
input/tool -> command + history -> document -> compositor -> canvas/exporter
```

The model implements the 14 documented Paint.NET layer blend modes with
deterministic source-over compositing from the bottom layer upward. Pixels use
straight RGBA8 values; transparent pixels retain their RGB channels. Per-mode
native conformance fixtures remain the next verification step.

`History` records individual pixel changes and atomic groups of distinct pixel
changes. Undo and redo check that every pixel still has the state expected by
the command before changing any of them; this prevents an out-of-band document
change from being overwritten. Pencil strokes will use grouped entries.

`Selection` is a separate tool-state value. Its first representation is either
the whole canvas or a half-open rectangular region. `PencilTool` uses integer
Bresenham coverage, clips to both canvas and selection, and submits its result
as a single grouped history operation.

Binary selections can also be composed using Replace, Add, Subtract,
Intersect, and XOR inside known document bounds. The result is a bounded mask;
antialiased selection coverage remains a later extension. PNG export is a
separate adapter that flattens the current composite to straight RGBA8.

Flat PNG import converts decoded RGBA, RGB, grayscale, or grayscale-alpha
pixels into one Background layer. Palette images are expanded by the decoder.
Metadata, colour profiles, bit-depth preservation, and multi-image formats are
not yet part of the document contract.

The first paint-bucket operation reads from and changes one layer. It matches
exact straight RGBA8 values with 4-connected flood fill, treats selection
coverage as a connectivity boundary, and records the resulting changes as one
history entry. Tolerance and composite sampling are separate future choices.

Layer-stack commands currently support rename, reorder, duplicate, delete, and
merge down. They update document revision state; full layer-operation undo/redo
will be added as a separate history entry type.
