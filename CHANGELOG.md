# Changelog

## Unreleased
### Changed
- clear/truncate_front now cleanup chunks not affected by readers immediately! Which, at least partially, solves "emergency cleanup" problem.
Now you don't have to have access to all readers!  
- Subscribe/unsubscribe now O(1).

## 0.4.1
### Added
- miri support
### Changed
- Using spin mutex everywhere. Write performance improved x2 in non-heavy concurrent cases.

## 0.4.0
### Added
- `spmc` version
### Changed
- `EventQueue::subscribe` -> `EventReader::new`

## 0.3.1
### Changed
- Improved read performance on long runs.

## 0.3.0
### Security
- `EventReader::iter` now return `LendingIterator`. This prevent references from outlive iterator. 
Since chunk with items, pointed by references returned by iterator, may die after iterator drop,
it is not safe to keep them outside iterator. 
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