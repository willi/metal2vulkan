# What this tree adds to the source repository

This tree follows the source repository's `development` branch. One thing it adds departs from
the source's rules on purpose: `DevicePolicy::half_rounding`, with
`HalfRoundingPolicy { Native, Exact }`. Read this file before removing or reworking it, and when
merging the source in. It also adds `DevicePolicy::watchdog`, which keeps the source's rules
([The watchdog's bound](#the-watchdogs-bound)).

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

## Each merge of the source

1. Resolve conflicts in the files above. Each new `DevicePolicy` constructor needs both fields.
2. Check that every `float`-to-`half` rounding in `lower/` still goes through `narrow::round`,
   `round_measured` or `round_texel`: search for new `QuantizeToF16` and 32-to-16 `FConvert`
   emission.
3. Write the runtime-sources hash the guard test prints to
   `crates/metal2vulkan-validation/runtime-sources.sha256`: `device/derive.rs`, `device/mod.rs`,
   `device/policy.rs` and `device/profile.rs` are among its inputs.
4. Run `python3 scripts/api-signatures/signatures.py --bless` and check that only the
   `HalfRoundingPolicy` and `DevicePolicy::watchdog` rows are new.
5. Run the `half` tests with spirv-val and llvm-as on `PATH`, and the runtime's `half_rounding`
   and `stall` device tests on a Vulkan device.
6. Keep `watchdog` in `device/policy.rs`'s opening list of the runtime's fields, at the end of the
   list and of its parenthetical, with the count there one more than the source's. Taking the
   source's side of that paragraph drops it, and no test notices.
7. Where the source changes `WATCHDOG`, the `const` assert beside it stops the build: carry the new
   value into `UNTRUSTED.watchdog` and this file. Then
   `rg -n 'watchdog_after|WATCHDOG' crates/metal2vulkan-runtime/src` should name only `stall.rs`,
   the one call in `completion.rs`, the test hook in `device/line.rs` and the tests. A new bound
   beside `Stop::Watchdog` elsewhere needs the policy too.
