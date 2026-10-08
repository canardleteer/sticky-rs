# microSD storage

The default Embassy image includes persistent SD storage alongside Wi-Fi and
BLE. Add `remote-debug` for encrypted storage controls. The separate `sd`
feature remains a read-only diagnostic and drops default features.

## Filesystems and partitions

| Region | Placement | Format | Purpose |
| --- | --- | --- | --- |
| Partition table | Sector 0; first volume starts at 1 MiB | MBR | Explicit provisioning |
| State | 64 MiB | littlefs | Digest-checked durable test record |
| Updates | 64 MiB | littlefs | Pending and ready app0 packages |
| Bulk | Remaining whole MiB | FAT32 | Replaceable bulk files |

The card must have room for at least 64 MiB of bulk storage. Provisioning clears
the MBR, formats each volume, then publishes the table last. Mount failures
never format automatically. These partitions affect logical allocation; the
card's controller decides where physical writes land.

The reusable [`embedded-storage-volume`](../crates/embedded-storage-volume)
crate allows caller-selected volume sizes and filesystem kinds. Its littlefs
profiles use const parameters; select a compiled profile at runtime. The board
image uses the default profile: 4 KiB blocks, 512-byte read/program units and
caches, 128-byte lookahead, and 500 block cycles. GPT is available through the
library with CRC validation and backup-table recovery; this image provisions
and mounts the three-volume MBR layout.

littlefs uses copy-on-write metadata and CRCs. Updating a pending file, closing
it, then renaming it over the published record gives a recoverable publication
boundary. The adapters flush each physical program callback because
littlefs2's C sync callback is a no-op. Logical erase is also a no-op on this
rewriteable media. Those choices follow the upstream
[file-backed block
device](https://github.com/littlefs-project/littlefs/blob/master/bd/lfs_filebd.c)
and [littlefs
design](https://github.com/littlefs-project/littlefs/blob/master/DESIGN.md).

FAT32 has no atomic directory transaction or equivalent power-loss guarantee.
Bulk files must be replaceable. For accumulated logs, applications can choose
`WritePolicy::LOG` (4 KiB or one second), immediate commits, or explicit
synchronization. The test image commits every state replacement and bulk
round; it does not persist the UART event stream as an SD log.

Neither filesystem controls the SD card's internal flash translation layer.
Class 10/U1 labels describe speed. Endurance and power-loss behavior need
separate evidence.
Repeated hot metadata writes and small synchronous writes increase write
amplification. Leave free space, batch replaceable data, and select industrial
media with documented endurance/power-loss behavior when that becomes a
product requirement. SD rail voltage decay and back-powering remain unmeasured.

## Shared SPI and radio scheduling

Core 1 owns SPI2, the panel, and the SD driver. Both CS lines stay high between
transactions. Clock selection happens before CS assertion: at most 400 kHz
during SD initialization, then 10 MHz for normal card traffic; the panel uses
its configured clock. `RefCellDevice` arbitrates synchronous transactions on
that core. There is no cross-core SPI lock or interrupt mask across card I/O.

Core 0 runs the radios and UART. Touch and product-key tasks also stay there.
Cross-core channels
carry bounded requests and replies. Stress rounds and package-validation
chunks yield on Core 1. A native format callback remains synchronous and may
delay panel work; cached status and radio processing stay on Core 0. Measured
coexistence results are recorded separately below; synchronous
formatting still has no hard latency bound.

`storage quiesce` drains storage and parks its rail; `storage power-cycle`
resumes it. Normal reboot, deep sleep, and latch release stop new writes and
wait for a
storage barrier. A failed or timed-out barrier preserves MCU power and BLE
recovery. `reboot --force` deliberately bypasses that barrier. Software reset
does not guarantee that the SD controller loses power. `power-cycle` abandons
a failed driver, parks the bus, cuts GPIO10's rail, then reinitializes at the
low clock. Its name describes firmware sequencing; zero rail voltage has not
been measured.

## Prepare and provision the card

1. Build the storage image. Source the toolchain script printed by espup,
   then run:

   ```sh
   cargo xtask build-fw embassy-debug --features remote-debug
   ```

   Expect an ELF and `embassy-debug.bin` under the printed target directory.
   littlefs requires libclang with builtin headers and the ESP32-S3 C compiler.
   Xtask locates headers from `LIBCLANG_PATH`; target-specific bindgen arguments
   can override that discovery. `.cargo/config.toml` supplies the C compiler
   and `-mlongcalls`. Host checks need a native C compiler and matching Clang
   headers. If using an ESP Clang distribution that defaults to a 32-bit ABI,
   set native bindgen arguments in your shell before running host checks:

   ```sh
   export BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_linux_gnu="--target=x86_64-unknown-linux-gnu -ffreestanding -isystem $(clang -print-resource-dir)/include"
   ```

   Use the `clang` belonging to `LIBCLANG_PATH`. Keep machine paths in local
   environment files rather than tracked Cargo configuration.

2. Install app0. Stop UART listeners, then run:

   ```sh
   cargo xtask flash-app --image target/xtensa-esp32s3-none-elf/release-fw/embassy-debug.bin --yes
   ```

   If this unit's backup is stored elsewhere, add `--force`. That option reads
   and checksum-checks the live factory table and validates the ESP32-S3 image;
   it still writes app0 only. Expect the device to reboot to Ferris. No erase
   or OTA metadata change is part of this command.

3. Connect and replace the card contents. Leave Ferris showing and run:

   ```sh
   cargo xtask remote-debug connect --wait
   cargo xtask remote-debug storage status
   cargo xtask remote-debug storage provision --yes
   cargo xtask remote-debug storage status
   ```

   Provisioning requires a card whose contents may be replaced. The command
   waits for its own completion receipt; success reports `jobComplete=true`,
   `jobOk=true` and `mounted=true`. A failed job makes the command fail. Use
   `storage power-cycle` to retry initialization;
   formatting is always a separate confirmed command.

4. Check persistence. Run:

   ```sh
   cargo xtask remote-debug storage stress --rounds 100
   cargo xtask remote-debug storage status
   cargo xtask remote-debug reboot
   cargo xtask remote-debug storage verify
   cargo xtask remote-debug storage status
   ```

   Each command waits for its own job result. `verified` is the persisted
   sequence, so it
   survives a successful reboot/remount. Every stress round replaces a
   digest-checked littlefs record and writes/reads a FAT bulk file. Watch the
   display while the jobs run. A successful reboot reconnects with a fresh PIN.

## Stage an update

1. Build the app image using the first preparation step above. Keep the BLE
   session connected, then run:

   ```sh
   cargo xtask remote-debug storage stage \
     --image target/xtensa-esp32s3-none-elf/release-fw/embassy-debug.bin \
     --version test-1
   ```

   Expect completion with `jobComplete=true`, `jobOk=true`, and `ready=true`.
   An image or transfer failure exits with an error.

2. Check publication after reboot:

   ```sh
   cargo xtask remote-debug reboot
   cargo xtask remote-debug storage verify
   cargo xtask remote-debug storage status
   ```

   Expect `ready=true` after boot readback. A failed readback reports failure;
   staging never activates or selects the application.

The host builds a package containing a four-byte little-endian JSON length,
bounded JSON metadata, and the app binary. Metadata names the board, chip,
payload kind, version, exact length, and SHA-256. Chunks are ordered and at
most 512 bytes; each acknowledgement follows file close and completed media
writes. Finish reads the pending package back and publishes `ready.pkg` only
after validation. The command polls its correlated completion receipt for up to
five minutes. Stress allows an additional second per requested round.
Four recent receipts are retained; eviction or reboot is an error. A global
`busy=false` or an older ready package never substitutes for that receipt.
An existing ready package survives a failed transfer. Startup revalidates it.

The host validates the complete ESP32-S3 application image before transfer
(segments, XOR checksum, padding and optional appended digest). Firmware
readback validates the manifest, exact payload length, SHA-256 and basic image
header. It does not repeat the full host flash validator or authenticate a
publisher. A future SD installer must validate the full image itself.

This is staging only. A future installer needs publisher verification, app1
validation, an OTA selection/rollback policy compatible with the factory
bootloader, and explicit protection of NVS, PHY data, and the partition table.
The current firmware never writes application flash or `otadata` from SD.
USB-host access to the filesystem is deferred.

## Exercise interruptions without removing the card

1. Install the test image. Build with `--features storage-test`, stop UART
   listeners, flash the resulting app0 image, then reconnect. Use the provisioned
   card already in the unit. Normal images reject the test controls.

2. Interrupt the MCU during writes:

   ```sh
   cargo xtask remote-debug storage fault --kind reset --after-ms 50 --yes
   cargo xtask remote-debug connect --wait
   cargo xtask remote-debug storage verify
   cargo xtask remote-debug storage status
   ```

   Expect a disconnect followed by a fresh pairing. Verification must succeed
   with an old or complete newer sequence; any digest failure fails the trial.

3. Interrupt the SD rail and recover it:

   ```sh
   cargo xtask remote-debug storage fault --kind sd --after-ms 50 --yes
   cargo xtask remote-debug storage status
   cargo xtask remote-debug storage power-cycle
   cargo xtask remote-debug storage verify
   ```

   Wait for `busy=false` after the cut. The write job must report failure;
   normal reboot should refuse while the transport is poisoned. `power-cycle`
   must remount and verify a complete state record. Keep the card inserted.

4. Exercise timer wake:

   ```sh
   cargo xtask remote-debug storage sleep --seconds 2 --yes
   cargo xtask remote-debug connect --wait
   cargo xtask remote-debug storage verify
   ```

   Expect the BLE connection to drop during sleep, then fresh pairing after
   timer wake. Leave the product keys released; no manual wake press is needed.

Reset/power faults start background writes before the requested delay. The SD
fault cuts power after a pending file closes and before rename, then stops the
job; it tests that publication boundary. Use `power-cycle` to restore it. The
sleep command drains SD, parks the panel, and arms RTC timer wake; timer wake
does not require a Page Up hold. Reconnect after wake.

For `--kind power`, first remove USB VBUS from the exact known Sticky hub port
while the device is idle. BLE then runs on battery. Schedule the latch fault
and restore that port's USB power to boot again. A USB cycle alone cannot cut
MCU battery power. `resetReason` reports the HAL's read-only RTC reset cause;
combine it with continued BLE on battery before the fault and link loss after
latch release. `ChipPowerOn` alone is insufficient: the HAL also maps some
analog reset causes to that value. No reply establishes a measured voltage.
The optional `externalPower` field reports GPIO9's digital divider indication;
it is absent before the first sample. A latch fault refuses a high or unknown
indication. Check this field after the hub reports off: USB disappearance alone
does not establish that the hub removed VBUS. A low GPIO9 indication also does
not measure zero volts.
Never cycle a hub or an unidentified port. If USB is still present when the
latch is released, the intended power cut may not occur.

Acceptance requires recovery of an old or complete new state record across
repeated cuts, and sustained SD/Wi-Fi/BLE/display operation on the actual card.
Host tests cover 50 failed-write positions twice, including torn sectors;
they cannot establish physical rail behavior or a consumer card's controller
guarantees.

## Physical validation

On one installed card on 2026-10-08, firmware provisioning completed all three
volumes. Stress and subsequent verification recovered state sequence 1500,
including a normal reboot, with a valid record digest. The normal image rejected
confirmed fault and timed-sleep requests as unavailable.

A locally built 1,170,672-byte application image completed BLE staging in
ordered chunks of at most 512 bytes, followed by package readback and ready
publication. Canceling a second upload after 12 seconds preserved that ready
package; another full readback passed. Installing the rebuilt test image then
revalidated the ready package on boot and recovered the same state sequence.
The card was explicitly reprovisioned afterward for the recovery trials.

Fifty physical recovery trials passed with delays of 15, 25, 50, 100, 200,
350, 600, 1000, 2000 and 4000 ms: 30 MCU resets and 20 commanded SD rail
interruptions. Every recovered record had a valid digest and an old or complete
new sequence: 16 recovered the prior sequence and 34 recovered a newer one.
Explicit quiescence rejected new writes; normal reboot drained
an active job and refused a poisoned driver while preserving BLE. Explicit
forced reboot recovered it. A two-second timer sleep woke with the same valid
record, UART sleep/wake evidence and `CoreDeepSleep` status.

Two latch-release attempts left BLE and writes running, including one with
GPIO9 low and both latch outputs commanded low with pad holds. They provide
no confirmed MCU power-loss evidence.

After restoring the normal image, survey ran alongside SD stress. A subsequent
30-minute 38-second interval completed 3,800 stress rounds with BLE controls,
active SoftAP, UART monitoring, injected touch/key input, 366 HTTP replies,
and 35 completed display snapshot cycles. HTTP ran independently of display
controls, with a maximum reply gap of 5.2 seconds. Every command in that
interval succeeded. Explicit SD rail recovery midway through the run remounted
with the same digest-valid state sequence. Final digest verification passed at
sequence 12,920, including after a normal reboot. The temporary host Wi-Fi
profile and BLE session were removed afterward; Ethernet default routes stayed
unchanged and the normal reboot stopped SoftAP.

True MCU power-loss acceptance remains open.
These observations describe this unit and card; rail decay, back-powering,
and the card controller's undocumented durability behavior remain unmeasured.

## Build and memory validation

Local release builds on 2026-10-08 produced a 1,173,488-byte normal
`remote-debug` image and a 1,177,344-byte `storage-test` image. Both fit the
factory's 6,291,456-byte app0 partition. The save-image command's generic
16,384,000-byte partition estimate is unrelated to this factory layout.
Host default/all-feature tests and Clippy, the firmware feature matrix,
protocol lint, Markdown lint, dependency usage, and advisory checks passed
through `cargo xtask ci`. The host/no_std graph also passed all-feature tests
on Rust 1.88. Firmware uses the separate ESP compiler required by its HAL.
The preexisting `paste` maintenance advisory remains an allowed warning.

The linked DRAM span is 341,760 bytes plus a separate 65,536-byte reclaimed
region. Those spans include 128 KiB of allocator regions, the 16 KiB Core 1
call stack, the main call stack, task pools, and display buffers. The storage
task pool is 1,360 bytes; request/reply channel structures occupy 152/328
bytes, with payload allocation bounded separately to 512 bytes per request.
Four receipt slots occupy 80 bytes. LAST display planes use 96 KiB of PSRAM.
The Core 0 external-power observer task pool occupies 64 bytes.
These figures come from ELF sections and symbols; runtime heap and call-stack
high-water marks have not been measured.

GNU ld reports an RWX flash LOAD segment. Section inspection shows that the
upstream ESP linker groups its unloaded writable-tagged `.rotext_dummy`
padding with read/execute `.text`, `.init`, and `.fini`. The padding reserves
the flash address space shared by DROM and IROM. The image contains no writable
executable text section. Raw size-tool BSS totals also include that flash
padding and aliased IRAM reservations, so they are not physical RAM usage.
