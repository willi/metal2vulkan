# What this tree adds to the source repository

This tree follows the source repository's `development` branch. One thing it adds departs from
the source's rules on purpose: `DevicePolicy::half_rounding`, with
`HalfRoundingPolicy { Native, Exact }`. Read this file before removing or reworking it, and when
merging the source in. It also adds `DevicePolicy::watchdog`, which keeps the source's rules
([The watchdog's bound](#the-watchdogs-bound)), and `DevicePolicy::surface_write_back`, whose
default keeps the source's behaviour ([A plane's write back](#a-planes-write-back)).

## The switch

`Native` (the default) is the device's own `half` rounding, as in the source repository: every
default module, fingerprint and precision note is the source's. `Exact` rounds every `float`
result to `half`, and every `half` texel, by integer arithmetic on the bits, which is Metal's
`half(float)` on every device. Some callers need Metal's `half` answers on drivers whose own
conversion differs: one that drops the conversion pair, so a `half` keeps `float` precision until
it is stored, or one that flushes half denormals.

## The exception to AGENTS.md rule 1

Rule 1 forbids a switch that emulates Metal's bits where the device has a native form. `Exact` is
such a switch. It is the one exception: keep it, opt-in and off by default, and do not add others
on its strength.

`DevicePolicy::exact_answers()` does not select it, and `DevicePolicy::UNTRUSTED` keeps `Native`.
The validation runs (`crates/metal2vulkan-validation/src/candidate.rs`) and conformance runs
(`crates/metal2vulkan-conformance/src/device.rs`) take `exact_answers()` and expect the device's
rounding notes, which `Exact` removes. A caller sets it by name.

## Where it lives

Paths are below `crates/metal2vulkan/src/` unless they name their crate.

- `device/policy.rs`: the field, `HalfRoundingPolicy` and its doc (what `Exact` covers and what it
  leaves to the device).
- `device/profile.rs`: `DeviceProfile` holds the value beside `ProfileFields`, not in them, and
  `fingerprint_of` hashes it only when it is not `Native`. Default fingerprints, and so the golden
  table, stay the source's.
- `lower/arith/narrow.rs`: `exact_rounding` (the decision, run once in `arith::finish`), `integer`
  and `scaled` (the rounding), and the `exact_half` checks in `round`, `round_measured` and
  `round_texel`. `ModuleBuilder::exact_half` carries the decision to emission.
- `lower/arith/half_chain.rs`: under `Exact` no fast-math chain is held at `float`.
- `lower/texture/half.rs`: `ROUTINE_WORDS_EXACT`, the half texel routine's size under `Exact`.
- Tests: `lower/arith/tests/narrow.rs` (`integer_half_rounding_is_metals_conversion`),
  `lower/arith/tests/half_texel.rs`, `tests/engine_native_half.rs` (the `exact_half` tests),
  `device/tests/profile.rs`, `tests/fast_paths.rs`, and the runtime's device test
  `crates/metal2vulkan-runtime/src/tests/half_rounding.rs`.
- `docs/FAST-PATHS.md`: the section "Metal's `half` rounding, by name".

## Docs left as the source wrote them

These describe the default and say nothing of `Exact`. They are left alone so merges stay clean:

- AGENTS.md rule 1 and `docs/FAST-PATHS.md`'s opening ("no path emulates Metal's bits"), each
  followed by one added sentence.
- `docs/API.md`'s note on the device's own `half` rounding and its list of what `exact_answers()`
  selects, and the catalogue entries for `native-half-conversion` and `native-half-quantize` with
  their rustdoc in `pipeline/artifact.rs`: under `Exact` neither note is recorded.
- `docs/HOWTO.md` section 2's paragraph on the native `half` notes.
- `crates/metal2vulkan-conformance/src/device.rs`'s comment that `half_chains` is the one
  arithmetic switch.

## Known limits

- A `half` read from or written to its bits is the device's widening or narrowing, so NaN payloads,
  and half denormals on a device that flushes them, stay the device's.
- A texel class whose Metal `half` is not the rounding of its `f32` value
  (`PrecisionNote::HalfTexelConversion`) reads as under `Native`.
- The word forecasts (`narrow::round_words`) count the form's rounding, so they undercount an
  `Exact` module; `check_emitted` still caps the emitted words.
- No release-mode timing of `Exact` has been taken.

## The watchdog's bound

`DevicePolicy::watchdog: Option<Duration>` is the runtime's bound on a queue's front batch
(`crates/metal2vulkan-runtime/src/command_buffer/stall.rs`), where the source has only the
constant `WATCHDOG`. The default, `Some(20.2 s)`, is that constant, and a `const` assert beside it
fails the build if the two part, so a default device acts as the source's. `None` fails no batch
for its time. A caller whose device runs far slower than any Mac's GPU (a CPU rasterizer under
load) sets it, because past the bound the device recovers, and a recovered device refuses what was
made before the recovery (`MadeBeforeRecovery`).

The cost of `None`: only a probe that goes unanswered still finds a stop, and a device without a
staging queue (lavapipe) sends none. There a command buffer that waits on what is never signalled
holds its queue, and every later batch on it, until the process ends, where Metal fails it from
about 5 s. A long finite bound keeps the detector; the field takes either.

The runtime alone reads the field: no profile carries it, and no fingerprint moves. A serialized
policy without it reads as the default.

### Where it lives

- `crates/metal2vulkan/src/device/policy.rs`: the field and its `serde` default, its value in
  `UNTRUSTED`, and its name in the opening list of the runtime's fields.
- `crates/metal2vulkan/src/tests/fast_paths.rs`: its class in `choices()`, after `isolation`, and
  two tests at the file's end (no profile moves; a policy serialized without it).
- `crates/metal2vulkan-runtime/src/command_buffer/stall.rs`: `watchdog_after` reads the policy, and
  the `const` assert ties the default to `WATCHDOG`.
- `crates/metal2vulkan-runtime/src/command_buffer/completion.rs`: the one read, which takes `None`
  as no bound.
- `crates/metal2vulkan-runtime/src/tests/stall.rs`'s last line, `mod watchdog_policy;`, and
  `tests/stall/watchdog_policy.rs`: the default fails a front at its bound; `None` fails none.
- `docs/FAST-PATHS.md`'s "The watchdog's bound, by policy".
- One row of `crates/metal2vulkan/tests/api_signatures.tsv`, and
  `crates/metal2vulkan-validation/runtime-sources.sha256`.

### Docs left as the source wrote them

These state the default's 20.2 s, or call the bound Metal's, and are left alone so merges stay
clean:

- `stall.rs`'s module docs and `WATCHDOG`'s, the timeout message (`stall::timed_out`) and the
  recovery reason (`device/line.rs`).
- `docs/API.md`'s paragraph on a `VkDevice` whose work stopped, and `docs/VALIDATION.md`'s on work
  past a CPU device's reach.
- `crates/metal2vulkan-runtime/README.md`'s note that Metal's own watchdog is kept.
- `WorkCounts::watchdog_timeouts`'s rustdoc (`work_counts.rs`), and the validation crate's
  `runtime_lane/indirect_dispatches.rs` note on dispatches past 2^32 threads.

### Retiring it

If the source makes the bound a setting of its own, or derives it per device, drop this field for
the source's.

## A plane's write back

`DevicePolicy::surface_write_back: SurfaceWriteBackPolicy { Merge, CopyBack }` decides how a write
through a shadowed texture over an IOSurface plane the CPU may write goes back into the plane: host
pages, the caller's own `VkBuffer`'s memory, or a `Shared` or `Managed` buffer's
(`shadow::Backing::Surface`). The source merges it back, as it does every shadow over bytes another
writer may write (`shadow.rs`, "Merged back"). `Merge`, the default and `exact_answers()`'s value,
is that. `CopyBack` copies it back, as a tracked `Private` buffer's plane is. It reaches nothing
else: a plane in a `Private` buffer, and a texture that is no plane's (over host pages, a mapped
buffer or a caller's `VkBuffer`), keep the source's rule.

AGENTS.md rule 1 puts a choice's fast form first. Here the default is the accurate form, as
`render_lowering`'s is, so that a default device acts as the source's: the ledger lists it under
"Accuracy switches whose default is Metal's", and a caller sets `CopyBack` by name.

Each merge is a keep (a copy of the texels after the copy in) and a compute dispatch that compares
the image with it. On lavapipe those dispatches run on llvmpipe's compute threads. An iOS display
server drawing into three 1179x2556 framebuffers over host pages spent 31 to 77 CPU-seconds a scene
there, and showed a tenth or less of the new frames it shows under `CopyBack` (the numbers are in
`docs/FAST-PATHS.md`'s row). Earlier source revisions copied these planes back after each write, so
a caller pinned to one of them ran as `CopyBack` does.

### When `CopyBack` is not safe

A copy back writes every texel it moves: those inside the rectangle the commands bound their writes
to (a render pass's scissors over an attachment it loads), else all of the texture's. The texels
no command wrote get what the copy in read. So it differs from a merge, and from Metal, in one
case: another writer writes the bytes of a texel it moves after the copy in and before the copy
back, which both lie inside the run of a command buffer that writes through the plane. The copy
back then overwrites that write with the older value. A write before the copy in, or after the
command buffer completed, is kept; a write to a texel the commands also write races them under
Metal too; and no copy writes the bytes between rows or past the last texel. For a render pass that
loads and stores the plane, Metal itself rewrites every texel of each 32x32 tile its draws reach
(`shadow.rs`, "Stored attachments", measured on an M3 Max), so there the difference narrows to
texels inside the copied rectangle and outside those tiles.

Set `CopyBack` only where nothing writes a plane's bytes beside the device's own commands while a
command buffer that writes through it runs.

### Why the caller it is for meets that

This tree is pinned for a host that runs an iOS display server's compositor on lavapipe. Its
planes are the display server's framebuffers, and their writers are:

1. The compositor's render passes: the device's own commands, which the tracker orders, not
   another writer.
2. The host's frame reader, in the same process: it copies the newest frame out under the
   surface's read-only lock, and never writes.
3. Any other CPU access in the process: IOSurface's contract is that it takes `IOSurfaceLock`, and
   the host interposes that lock so that a write lock waits for the surface's command buffers in
   flight, from their commit to their completion, and so lands after the copy back.
4. Other processes: none writes the display server's framebuffers.

In traced boots of that host, every lock that had to wait for the framebuffers' command buffers was
a read lock (271 of them, all the frame reader's), no write lock waited in any process, and only
the display server's command buffers wrote the three framebuffers.

That leaves three ways a write could still be lost: a CPU write without the lock, which IOSurface's
contract forbids and which is unordered against the GPU on a Mac too; a write lock that gives up
after the host's 1 s bound while a pass over the same plane runs; and a write from another
process, since the host's locks are per process. The damage is bounded: only texels inside the one
copy back's rectangle of that one plane, never the bytes between rows or another resource, and
they hold the frame the copy in read, not garbage, until the next write covers them.

### Where it lives

- `crates/metal2vulkan/src/device/policy.rs`: `SurfaceWriteBackPolicy` (after
  `HostPageTexturePolicy`), the field and its `serde` default, its value in `UNTRUSTED` and in
  `exact_answers()`, and its name at the end of the opening list of the runtime's fields.
- `crates/metal2vulkan/src/device/mod.rs`: its own `pub use` line.
- `crates/metal2vulkan/src/tests/fast_paths.rs`: its class in `choices()` (after
  `indirect_records`), its `choice!` row, and three tests at the file's end (no profile moves;
  `exact_answers()` from `CopyBack`; a policy serialized without it).
- `crates/metal2vulkan-runtime/src/shadow.rs`: `Backing::Surface` carries the policy, which
  `Backing::of` reads from the buffer's device; `merges_back` reads it; the module's "Merged back"
  rule names it. `work_counts.rs` and `large_surface` match `Backing::Surface(_)`.
- Tests: `crates/metal2vulkan-runtime/src/tests/shadow.rs`
  (`a_texture_over_a_buffer_s_bytes_is_merged_where_it_would_be_copied_back`, and the two
  `Backing::Surface(SurfaceWriteBackPolicy::Merge)` of the large-surface test), and the device
  test `tests/surface_write_back.rs`, behind one `mod` line in `tests/mod.rs`.
- `docs/FAST-PATHS.md`: its row under "Accuracy switches whose default is Metal's".
- `crates/metal2vulkan/tests/api_surface.rs`: the enum's line, after `HostPageTexturePolicy`; four
  rows of `crates/metal2vulkan/tests/api_signatures.tsv` (the enum, its two variants and the
  field); and `crates/metal2vulkan-validation/runtime-sources.sha256`.

### Docs left as the source wrote them

These say a plane the CPU may write is merged back, or list what `exact_answers()` selects without
this field. They describe the default and are left alone so merges stay clean:

- The `shadowed-texture` notice's cost (`crates/metal2vulkan-runtime/src/degradation.rs`) and its
  catalogue entry in `docs/API.md`: under `CopyBack` a plane's shadow is copied back, with no keep.
- `docs/API.md`'s list of what `exact_answers()` selects, and the one in
  `crates/metal2vulkan-conformance/src/device.rs` (`Policy::Exact`).
- `docs/HOWTO.md`'s and `crates/metal2vulkan-runtime/README.md`'s notes on shadows over host pages
  merged back.

### Retiring it

If the source copies such planes back by a setting of its own, or puts them in place (an image over
the pages, no shadow at all), drop this field for the source's.

## Each merge of the source

1. Resolve conflicts in the files above. Each new `DevicePolicy` constructor needs all three
   fields.
2. Check that every `float`-to-`half` rounding in `lower/` still goes through `narrow::round`,
   `round_measured` or `round_texel`: search for new `QuantizeToF16` and 32-to-16 `FConvert`
   emission.
3. Write the runtime-sources hash the guard test prints to
   `crates/metal2vulkan-validation/runtime-sources.sha256`: `device/derive.rs`, `device/mod.rs`,
   `device/policy.rs` and `device/profile.rs` are among its inputs.
4. Run `python3 scripts/api-signatures/signatures.py --bless` and check that only the
   `HalfRoundingPolicy`, `SurfaceWriteBackPolicy`, `DevicePolicy::watchdog` and
   `DevicePolicy::surface_write_back` rows are new.
5. Run the `half` tests with spirv-val and llvm-as on `PATH`, and the runtime's `half_rounding`,
   `stall` and `surface_write_back` device tests on a Vulkan device.
6. Keep `watchdog` and `surface_write_back` in `device/policy.rs`'s opening list of the runtime's
   fields, at the end of the list and of its parenthetical, with the count there two more than the
   source's. Taking the source's side of that paragraph drops them, and no test notices.
7. Where the source changes `WATCHDOG`, the `const` assert beside it stops the build: carry the new
   value into `UNTRUSTED.watchdog` and this file. Then
   `rg -n 'watchdog_after|WATCHDOG' crates/metal2vulkan-runtime/src` should name only `stall.rs`,
   the one call in `completion.rs`, the test hook in `device/line.rs` and the tests. A new bound
   beside `Stop::Watchdog` elsewhere needs the policy too.
8. Where the source changes how a shadow's write goes back (`ShadowCopies::merges_back`,
   `only_commands_write`, a new `Backing` that holds an IOSurface plane's bytes the CPU may write),
   carry `CopyBack` to it: a plane's shadow under `CopyBack` takes the path of a tracked `Private`
   buffer's, and every other shadow keeps the source's.
