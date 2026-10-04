# Changelog

## [Unreleased]

## [0.2.0] - 2026-10-05

### Fixed

- Fixed displaced glyphs in Code fonts, including Regular U+BF29.
- Corrected corner rounding for repeated outline points and TrueType inputs, including Sans Bold U+02C7 and Sans Regular U+5F13.
- Updated font bounds, style information and internal versions for consistent font identification and layout.
- Invalid generation parameters are rejected, and failed generation preserves existing output files.

## [0.1.2] - 2026-10-04

### Changed

- Rounded Noto Code CJK JP Regular / Bold now use zero-, half- and full-em advances. Latin, Greek, Cyrillic, box-drawing and block characters use half-em cells; Japanese and Hangul syllables use full-em cells. Combining marks retain zero-width behavior, and width-changing typography features no longer alter the grid.
