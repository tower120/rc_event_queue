# Changelog

## [Unreleased]
### Changed
- AUTO_CLEANUP -> CLEANUP{ ON_CHUNK_READ / ON_NEW_CHUNK / NEVER }

## 0.2.0
### Changed
- Chunks now have dynamic size. Chunks grow in size, 
in order to find optimal chunk size.
In ideal, we will work with just 2 same-sized chunks.
### Added
- `double_buffering` feature.

## 0.1.0
### Added
- Initial working implementation with fix-size chunks.