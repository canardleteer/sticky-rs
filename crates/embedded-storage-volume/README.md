# embedded-storage-volume

`no_std` adapters for partitioned, rewriteable 512-byte media. Applications own
the device, bus arbitration, and scheduling. Rust 1.88 is required.

The crate combines `hadris-storage` block traits with `littlefs2`,
`starry-fatfs` (imported as `fatfs`), and Google's GPT/MBR crates. The optional
`sd` feature wraps `embedded-sdmmc` single-sector writes. It waits for the
driver's busy/status handshake before reporting completion.

## Layout and configuration

`Layout::default_for` creates three volumes with 1 MiB alignment:

| Volume | Size | Filesystem | Intended use |
| --- | --- | --- | --- |
| State | 64 MiB | littlefs | Small durable records |
| Updates | 64 MiB | littlefs | Validated firmware packages |
| Bulk | Remaining aligned capacity | FAT32 | Replaceable bulk files |

Callers may provide their own `Layout` and `FilesystemKind` values. Check
`Layout::validate` before constructing partition views. MBR is the default;
`write_gpt` and `Layout::read_gpt` support a 128-entry GPT profile using caller
scratch space. Reserve the last 33 sectors for GPT metadata. Supply fresh,
distinct GUIDs when provisioning GPT. Reading checks CRCs and falls back to
the backup header/table when the primary is damaged.

`Littlefs<D, BLOCKS, CYCLES>` checks that the partition matches its compiled
geometry. Applications can choose among these types at runtime. The default
uses 4 KiB blocks, 512-byte caches, 128 bytes of lookahead, and 500 block cycles.
These are logical allocation settings; the SD controller's flash geometry is
unknown. `WritePolicy` tells an application when to commit its own buffer;
`LOG` defaults to 4 KiB or one second. It does not create a logger or cache.

## Durability contract

littlefs metadata and a pending-file/rename publication pattern protect file
replacement when the underlying media respects completed writes. Each
littlefs program callback flushes the lower device: littlefs2's C sync callback
does no I/O. Logical erase is a no-op on rewriteable media, matching upstream
littlefs's file-backed block device. A failed read, write, or flush poisons that
adapter until the caller releases it and constructs a fresh one for recovery.
Call `SdMedia::invalidate` when the caller interrupts power or the bus between
driver callbacks; a cached successful write cannot establish continued power.

FAT32 directory updates are not atomic. Use it for bulk data that can be
recreated. `FatStream` handles partial sectors without crossing the partition;
a later sector failure can leave an earlier part of a write completed. Call
file flush and filesystem unmount explicitly, then flush the media. The native
FAT filesystem also attempts unmount in its destructor; do not rely on that
attempt to report errors.

Adapter drop performs no device I/O. Explicit `shutdown` flushes and returns
the device borrow; `release` abandons it without I/O. `DiskIo` flushes each
write so the upstream GPT disk's drop-time flush has no pending work.

A consumer SD card has an internal flash translation layer. Partitioning and
littlefs allocation cannot promise physical wear leveling or survival of a
card-controller power failure. Host tests exercise 100 interrupted writes,
including torn sectors; physical power-loss behavior requires tests of the
actual card and power circuit.

## Update staging

`package::Manifest` accepts bounded, versioned JSON for an ESP32-S3 app0
payload. `Verifier` checks exact length, SHA-256, and the target image header.
Store the package as pending, close it, read it back, then rename it to ready.
`package::next_offset` checks this boot's acknowledged length and rejects
reordered, duplicated, missing, empty, or oversized chunks before I/O. Clear
that length after interruption rather than resuming stale pending data.
`jobs::Jobs` retains four explicit completion receipts so later work cannot
substitute for the requested operation's result.

Staging does not activate an image or change OTA metadata. SHA-256 detects
corruption; a future installer must verify the publisher and define rollback
and downgrade policy.

See the [Sticky storage guide](https://github.com/canardleteer/sticky-rs/blob/main/docs/storage.md)
for board integration and operator commands.

## Build prerequisites

littlefs2 builds C and generates bindings. Host builds need a C compiler,
libclang, and Clang's builtin headers. Bare-metal builds also need compatible
C compiler flags and string routines. The Sticky firmware enables the
upstream `littlefs2-sys` minimal runtime and uses ESP32-S3 GCC with `-mlongcalls`.
No C allocator is enabled by this crate.
