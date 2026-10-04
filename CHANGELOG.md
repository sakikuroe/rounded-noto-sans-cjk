# Changelog

## [Unreleased]

### Fixed

- Generated fonts now have CFF2-compatible tables, updated outline metrics, consistent style flags and matching name/head versions.
- Duplicate outline points and degenerate curve tangents no longer cause invalid rounding. TrueType contour directions are normalized automatically, including previously reversed inputs.
- Invalid roundness and unrepresentable CharString numbers are rejected before output. Batch generation preserves existing output when a later processing step fails.
- Code fonts preserve point-only contours through CharString generation so compression no longer shifts later contours. Width verification now checks every glyph's left side bearing against its outline.

### Changed

- Rounded Noto Code CJK JP Regular / Bold now use zero-, half- and full-em advances. Latin, Greek, Cyrillic, box-drawing and block characters use half-em cells; Japanese and Hangul syllables use full-em cells. Combining marks retain zero-width behavior, and width-changing typography features no longer alter the grid.
