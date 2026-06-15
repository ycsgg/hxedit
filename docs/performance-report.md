# hxedit Performance Report

Date: 2026-06-15

## Environment

| Item | Value |
|---|---|
| Source | commit containing this report |
| Commands | `HXEDIT_BENCH_LARGE=1 cargo bench --bench perf_bench`; `HXEDIT_BENCH_SUITE=public cargo bench --bench perf_bench`; `HXEDIT_BENCH_SUITE=public HXEDIT_BENCH_REPEAT=3 cargo bench --bench perf_bench`; `cargo test --all-targets` |
| Rust | `rustc 1.94.1 (e408947bf 2026-03-25)`, `cargo 1.94.1` |
| OS | macOS Darwin 25.4.0 arm64 |
| CPU | Apple M5 Pro, 18 logical CPUs |
| Memory | 48 GiB |
| Bench config | 16 KiB page size, 128 cache pages |

## Public Summary

| Scenario | Bench | Min ms | Median ms | Max ms | Peak RSS |
|---|---|---:|---:|---:|---:|
| Open 1GiB file | `open_1gib_sparse` | 0.137 | 0.157 | 0.168 | 1.9 MiB |
| Open 1GiB file and read first view | `open_1gib_then_first_view` | 0.153 | 0.159 | 0.163 | 2.0 MiB |
| Random 1GiB viewport reads | `viewport_1gib_random_10k_rows` | 25.121 | 27.435 | 32.540 | 4.2 MiB |
| Save 256MiB patterned file | `save_256mb_patterned_clean_rewrite` | 73.135 | 74.581 | 75.138 | 4.3 MiB |
| Export 256MiB patterned file | `export_stream_256mb_patterned` | 69.092 | 74.155 | 96.528 | 4.2 MiB |
| Save 1GiB file after middle insert | `save_1gib_with_middle_insert` | 315.468 | 319.412 | 340.510 | 4.4 MiB |
| Save 1GiB file after tombstones and insert | `save_1gib_with_tombstone_and_insert` | 1203.816 | 1219.822 | 1234.788 | 6.7 MiB |
| Save 1GiB file with sparse replacement islands | `save_1gib_with_sparse_replacement_islands` | 336.417 | 340.914 | 343.036 | 7.0 MiB |
| Search 1GiB clean file | `search_1gib_clean_memmem` | 181.286 | 186.089 | 187.693 | 4.4 MiB |
| Search 1GiB dirty file | `search_1gib_dirty_many_islands` | 176.688 | 179.505 | 180.480 | 5.1 MiB |
| Diff next on 1GiB files | `diff_next_tail_mismatch_1gib_stepper` | 309.014 | 309.269 | 315.340 | 6.3 MiB |
| Diff next on dirty 1GiB files | `diff_next_tail_mismatch_1gib_dirty_stepper` | 305.778 | 310.013 | 314.484 | 7.1 MiB |
| Mixed 256MiB editing session | `session_256mb_mixed_10k_ops` | 13.614 | 13.792 | 13.997 | 5.1 MiB |

## Save, Piece Lookup, Format Parse

| Bench | Workload | Time ms | Peak RSS |
|---|---:|---:|---:|
| `resolve_piece_heavy` | 200k random lookups, 4 MiB file, ~5k pieces | 1.475 | 4.5 MiB |
| `save_16mb_with_insert` | 16 MiB save, one insert | 5.199 | 4.3 MiB |
| `save_16mb_with_tombstones` | 16 MiB save, 4096 tombstones | 6.108 | 4.4 MiB |
| `save_64mb_clean_rewrite` | 64 MiB clean rewrite | 18.677 | 4.2 MiB |
| `save_256mb_patterned_clean_rewrite` | 256MiB patterned file clean rewrite, opt-in large bench | 100.823 | 4.3 MiB |
| `save_64mb_with_middle_insert` | 64 MiB save, middle insert | 18.448 | 4.3 MiB |
| `save_64mb_with_tombstone_and_insert` | 64 MiB save, tombstones + insert | 21.363 | 4.6 MiB |
| `save_64mb_overwrite_replacements` | 64 MiB save, sparse replacements | 24.210 | 4.5 MiB |
| `save_1gib_with_middle_insert` | 1GiB sparse file save, middle 4-byte insert, opt-in large bench | 338.549 | 4.2 MiB |
| `save_1gib_with_tombstone_and_insert` | 1GiB sparse file save, 65536 tombstones + middle 4-byte insert, opt-in large bench | 1232.515 | 6.7 MiB |
| `save_1gib_with_64mb_range_overlay` | 1GiB sparse file save, 64MiB pattern range overlay, opt-in large bench | 569.029 | 4.3 MiB |
| `save_1gib_with_sparse_replacement_islands` | 1GiB sparse file save, 65536 sparse replacement islands, opt-in large bench | 342.622 | 6.9 MiB |
| `parse_elf_format` | 200 detect+parse iterations | 2.480 | 2.1 MiB |

## Edit Primitives

| Bench | Workload | Time ms | Per op ns | Peak RSS |
|---|---:|---:|---:|---:|
| `edit_mode_replace_nibbles` | 400k nibble writes, 8 MiB | 46.439 | 116.1 | 11.8 MiB |
| `edit_mode_insert_nibbles` | 200k nibble insert/compose ops, 8 MiB | 10.561 | 52.8 | 5.7 MiB |
| `edit_mode_pending_insert_backspace` | 200k insert+real-delete cycles, 8 MiB | 16.969 | 84.8 | 2.3 MiB |
| `edit_mode_backspace_real_delete` | 200k real deletes from inserted run, 8 MiB | 12.018 | 60.1 | 2.4 MiB |
| `edit_mode_normal_tombstone_delete` | 200k tombstones, 8 MiB | 8.974 | 44.9 | 8.7 MiB |
| `edit_mode_visual_tombstone_range` | 200k range tombstones, 8 MiB | 16.229 | 81.1 | 11.8 MiB |
| `edit_256mb_replace_nibbles` | 2M nibble writes, 256 MiB | 271.671 | 135.8 | 42.2 MiB |
| `edit_256mb_insert_nibbles` | 2M nibble insert/compose ops, 256 MiB | 113.455 | 56.7 | 38.6 MiB |
| `edit_256mb_pending_insert_backspace` | 1M insert+real-delete cycles, 256 MiB | 81.344 | 81.3 | 3.1 MiB |
| `edit_256mb_backspace_real_delete` | 1M real deletes from inserted run, 256 MiB | 59.905 | 59.9 | 4.0 MiB |
| `edit_256mb_normal_tombstone_delete` | 1M tombstones, 256 MiB | 45.983 | 46.0 | 35.6 MiB |
| `edit_256mb_visual_tombstone_range` | 1M range tombstones, 256 MiB | 87.727 | 87.7 | 50.9 MiB |

## Paste, Fill, XOR

| Bench | Workload | Time ms | Per op ns | Peak RSS |
|---|---:|---:|---:|---:|
| `edit_mode_paste_overwrite_1mb` | 1 MiB compact bytes overlay | 1.050 | 1.0 | 6.1 MiB |
| `edit_mode_paste_overwrite_16mb` | 16 MiB compact bytes overlay | 17.126 | 1.0 | 54.4 MiB |
| `edit_mode_paste_insert_1mb` | 1 MiB real insert | 0.093 | 0.1 | 3.9 MiB |
| `edit_mode_fill_overlay_1mb` | 1 MiB pattern overlay | 0.003 | 0.0 | 2.0 MiB |
| `edit_256mb_paste_overwrite_overlay_64mb` | 64 MiB compact bytes overlay | 67.902 | 1.0 | 198.3 MiB |
| `edit_256mb_paste_insert_16mb` | 16 MiB real insert | 1.651 | 0.1 | 33.9 MiB |
| `edit_256mb_fill_overlay_64mb` | 64 MiB pattern overlay | 0.002 | 0.0 | 1.9 MiB |
| `edit_256mb_mixed_paste_overwrite_64mb` | 64 MiB paste over existing overlay | 318.347 | 4.7 | 198.4 MiB |
| `edit_256mb_mixed_fill_overlay_64mb` | 64 MiB fill over existing overlay | 0.001 | 0.0 | 1.9 MiB |
| `edit_256mb_mixed_xor_overlay_64mb` | 64 MiB xor over existing overlay | 0.002 | 0.0 | 1.9 MiB |
| `dirty_islands_paste_64mb` | 64 MiB paste over 4096 replacement islands | 61.368 | 0.9 | 139.3 MiB |
| `dirty_islands_xor_64mb` | 64 MiB xor over replacement/tombstone islands | 16.693 | 0.2 | 6.0 MiB |

## Continuous Operations And Undo/Redo

| Bench | Workload | Time ms | Per op ns | Peak RSS |
|---|---:|---:|---:|---:|
| `session_256mb_mixed_10k_ops` | deterministic 10k mixed core ops, 256 MiB sparse file | 13.606 | 1360.6 | 5.1 MiB |
| `undo_redo_64mb_compact_paste` | 64 MiB compact paste apply | 67.150 | 1.0 | 198.4 MiB |
| `undo_redo_64mb_compact_paste` | compact paste undo | 0.072 | 0.0 | 198.4 MiB |
| `undo_redo_64mb_compact_paste` | compact paste redo | 0.000 | 0.0 | 198.4 MiB |

## Logical Bytes, Export, Hash

| Bench | Workload | Time ms | Peak RSS |
|---|---:|---:|---:|
| `open_1gib_sparse` | open 1GiB sparse file, opt-in large bench | 0.220 | 1.8 MiB |
| `open_1gib_then_first_view` | open 1GiB sparse file and read first 64 rows, opt-in large bench | 0.224 | 1.9 MiB |
| `viewport_1gib_random_10k_rows` | 1GiB sparse file random 10k viewport row reads, opt-in large bench | 38.682 | 4.1 MiB |
| `logical_bytes_large_copy` | materialize 8 MiB logical bytes | 1.860 | 12.2 MiB |
| `export_stream_64mb` | stream 64 MiB to file | 19.886 | 4.3 MiB |
| `export_stream_256mb_patterned` | stream 256MiB patterned file to output, opt-in large bench | 84.793 | 4.3 MiB |
| `hash_sha256_16mb` | SHA256 16 MiB | 29.113 | 4.2 MiB |
| `hash_crc32_16mb` | CRC32 16 MiB | 3.395 | 4.2 MiB |
| `hash_16mb_with_tombstones` | SHA256 16 MiB with 2 tombstones | 29.275 | 4.3 MiB |
| `hash_16mb_with_insert` | MD5 16 MiB with 2-byte insert | 20.572 | 4.2 MiB |
| `hash_crc32_1gib_clean` | CRC32 1GiB sparse file, opt-in large bench | 232.528 | 4.2 MiB |

## Search And Diff

| Bench | Workload | Time ms | Peak RSS |
|---|---:|---:|---:|
| `search_16mb_file` | forward miss-until-tail | 2.599 | 22.3 MiB |
| `search_16mb_file` | backward miss-until-head | 28.032 | 22.3 MiB |
| `search_256mb_clean_memmem` | clean forward, needle at tail | 46.116 | 4.3 MiB |
| `search_256mb_dirty_one_tombstone` | dirty forward, one tombstone | 46.183 | 4.3 MiB |
| `search_256mb_dirty_many_islands` | dirty forward, 4096 replacement/tombstone islands | 44.904 | 4.5 MiB |
| `search_1gib_clean_memmem` | clean forward 1GiB sparse file, needle at tail, opt-in large bench | 189.801 | 4.3 MiB |
| `search_1gib_dirty_many_islands` | dirty forward 1GiB sparse file, 16384 replacement/tombstone islands, opt-in large bench | 178.280 | 5.0 MiB |
| `diff_next_tail_mismatch_64mb` | tail mismatch scan | 19.824 | 6.3 MiB |
| `diff_next_tail_mismatch_256mb` | tail mismatch scan | 79.423 | 6.3 MiB |
| `diff_next_tail_mismatch_256mb_stepper` | same via 128 MiB stepper | 79.937 | 6.3 MiB |
| `diff_next_tail_mismatch_1gib_stepper` | tail mismatch 1GiB sparse file via 128 MiB stepper, opt-in large bench | 320.680 | 6.4 MiB |
| `diff_next_tail_mismatch_1gib_dirty_stepper` | tail mismatch 1GiB sparse file, 16384 equal sparse replacement islands, 128 MiB stepper, opt-in large bench | 311.009 | 7.0 MiB |

## Benchmark Behavior

| Variable | Value |
|---|---|
| `HXEDIT_BENCH_SUITE=public` | Runs the public-report subset. |
| `HXEDIT_BENCH_REPEAT=<N>` | Runs each selected bench in isolated child processes N times and prints min, median, and max process-total timings. |
| `HXEDIT_BENCH_LARGE=1` | Includes opt-in large-file benches in the default suite. |
| `HXEDIT_BENCH_LEGACY=1` | Includes legacy comparison benches. Not used for this report. |

| Bench | Behavior |
|---|---|
| `resolve_piece_heavy` | 4 MiB patterned file; insert about 5000 small pieces; perform 200k random `cell_id_at` lookups. |
| `save_16mb_with_insert` | 16 MiB patterned file; insert 2 bytes in the middle; save in place. |
| `save_16mb_with_tombstones` | 16 MiB patterned file; create 4096 tombstones; save in place. |
| `save_64mb_clean_rewrite` | 64 MiB patterned file; save in place without edits. |
| `save_256mb_patterned_clean_rewrite` | 256 MiB patterned file; save in place without edits. |
| `save_64mb_with_middle_insert` | 64 MiB patterned file; insert 4 bytes in the middle; save in place. |
| `save_64mb_with_tombstone_and_insert` | 64 MiB patterned file; create 8192 tombstones and a middle 4-byte insert; save in place. |
| `save_64mb_overwrite_replacements` | 64 MiB patterned file; add one sparse replacement every 4096 bytes; save in place. |
| `save_1gib_with_middle_insert` | 1 GiB sparse zero file; insert 4 bytes in the middle; save in place. |
| `save_1gib_with_tombstone_and_insert` | 1 GiB sparse zero file; create 65536 tombstones and a middle 4-byte insert; save in place. |
| `save_1gib_with_64mb_range_overlay` | 1 GiB sparse zero file; apply a 64 MiB repeating pattern range overlay; save in place. |
| `save_1gib_with_sparse_replacement_islands` | 1 GiB sparse zero file; add 65536 sparse replacement islands at 16 KiB spacing; save in place. |
| `parse_elf_format` | Repeatedly detect and parse `tests/fixtures/elf_header.bin` 200 times. |
| `edit_mode_replace_nibbles` | 8 MiB patterned file; perform 200k high-nibble and 200k low-nibble replacements. |
| `edit_mode_insert_nibbles` | 8 MiB patterned file; append bytes through high-nibble insert plus low-nibble compose. |
| `edit_mode_pending_insert_backspace` | 8 MiB patterned file; repeat pending EOF high-nibble insert followed by backspace real delete. |
| `edit_mode_backspace_real_delete` | 8 MiB patterned file; insert 200k bytes, then remove them by real-delete backspace. |
| `edit_mode_normal_tombstone_delete` | 8 MiB patterned file; create 200k single-byte tombstones. |
| `edit_mode_visual_tombstone_range` | 8 MiB patterned file; create tombstones through repeated visual-range deletes totaling 200k slots. |
| `edit_mode_paste_overwrite_1mb` | 8 MiB patterned file; apply 1 MiB compact bytes overwrite overlay. |
| `edit_mode_paste_overwrite_16mb` | 32 MiB patterned file; apply 16 MiB compact bytes overwrite overlay. |
| `edit_mode_paste_insert_1mb` | 8 MiB patterned file; insert 1 MiB at the middle as real insert. |
| `edit_mode_fill_overlay_1mb` | 8 MiB patterned file; apply 1 MiB repeating pattern range overlay. |
| `edit_256mb_replace_nibbles` | 256 MiB sparse zero file; perform 1M high-nibble and 1M low-nibble replacements. |
| `edit_256mb_insert_nibbles` | 256 MiB sparse zero file; append 1M bytes through high-nibble insert plus low-nibble compose. |
| `edit_256mb_pending_insert_backspace` | 256 MiB sparse zero file; repeat 1M pending insert plus backspace cycles. |
| `edit_256mb_backspace_real_delete` | 256 MiB sparse zero file; insert 1M bytes, then remove them by real-delete backspace. |
| `edit_256mb_normal_tombstone_delete` | 256 MiB sparse zero file; create 1M single-byte tombstones. |
| `edit_256mb_visual_tombstone_range` | 256 MiB sparse zero file; create tombstones through visual-range deletes totaling 1M slots. |
| `edit_256mb_paste_overwrite_overlay_64mb` | 256 MiB sparse zero file; apply 64 MiB compact bytes overwrite overlay. |
| `edit_256mb_paste_insert_16mb` | 256 MiB sparse zero file; insert 16 MiB at the middle as real insert. |
| `edit_256mb_fill_overlay_64mb` | 256 MiB sparse zero file; apply 64 MiB repeating pattern range overlay. |
| `edit_256mb_mixed_paste_overwrite_64mb` | 256 MiB sparse zero file; pre-apply 64 MiB pattern overlay, then overwrite same range with 64 MiB bytes overlay. |
| `edit_256mb_mixed_fill_overlay_64mb` | 256 MiB sparse zero file; pre-apply 64 MiB pattern overlay, then apply a different 64 MiB pattern overlay. |
| `edit_256mb_mixed_xor_overlay_64mb` | 256 MiB sparse zero file; pre-apply 64 MiB pattern overlay, then xor the same 64 MiB range. |
| `session_256mb_mixed_10k_ops` | 256 MiB sparse zero file; fixed-seed sequence of 10k mixed replace, insert, tombstone, real-delete, fill, xor, and logical-count operations. |
| `undo_redo_64mb_compact_paste` | 256 MiB sparse zero file; apply 64 MiB compact paste overlay, clear replacements for undo, then reapply overlay for redo. |
| `dirty_islands_paste_64mb` | 256 MiB sparse zero file; create 4096 sparse replacement islands, then overwrite 64 MiB with bytes overlay. |
| `dirty_islands_xor_64mb` | 256 MiB sparse zero file; create mixed tombstone/replacement islands, then xor a 64 MiB range. |
| `open_1gib_sparse` | Create a 1 GiB sparse zero file; measure `Document::open`. |
| `open_1gib_then_first_view` | Create a 1 GiB sparse zero file; measure `Document::open` plus reading the first 64 16-byte rows. |
| `viewport_1gib_random_10k_rows` | Create a 1 GiB sparse zero file; read 10k random 16-byte viewport rows. |
| `logical_bytes_large_copy` | 8 MiB patterned file; create one tombstone; materialize all logical bytes. |
| `export_stream_64mb` | 64 MiB patterned file; stream all logical bytes to an output file. |
| `export_stream_256mb_patterned` | 256 MiB patterned file; stream all logical bytes to an output file. |
| `hash_sha256_16mb` | 16 MiB patterned file; hash full logical range with SHA256. |
| `hash_crc32_16mb` | 16 MiB patterned file; hash full logical range with CRC32. |
| `hash_16mb_with_tombstones` | 16 MiB patterned file; create 2 tombstones; hash logical bytes with SHA256. |
| `hash_16mb_with_insert` | 16 MiB patterned file; insert 2 bytes; hash logical bytes with MD5. |
| `hash_crc32_1gib_clean` | 1 GiB sparse zero file; hash full logical range with CRC32. |
| `search_16mb_file` | 16 MiB in-memory fixture; forward needle at tail and backward needle near head. |
| `search_256mb_clean_memmem` | 256 MiB sparse zero file; tail needle; clean forward search. |
| `search_256mb_dirty_one_tombstone` | 256 MiB sparse zero file; tail needle; one tombstone near start; dirty forward search. |
| `search_256mb_dirty_many_islands` | 256 MiB sparse zero file; tail needle; 4096 replacement/tombstone islands; dirty forward search. |
| `search_1gib_clean_memmem` | 1 GiB sparse zero file; tail needle; clean forward search. |
| `search_1gib_dirty_many_islands` | 1 GiB sparse zero file; tail needle; 16384 replacement/tombstone islands; dirty forward search. |
| `diff_next_tail_mismatch_64mb` | Pair of 64 MiB sparse zero files; other file has final byte changed; scan forward to tail mismatch. |
| `diff_next_tail_mismatch_256mb` | Pair of 256 MiB sparse zero files; other file has final byte changed; scan forward to tail mismatch. |
| `diff_next_tail_mismatch_256mb_stepper` | Pair of 256 MiB sparse zero files; other file has final byte changed; scan with 128 MiB stepper. |
| `diff_next_tail_mismatch_1gib_stepper` | Pair of 1 GiB sparse zero files; other file has final byte changed; scan with 128 MiB stepper. |
| `diff_next_tail_mismatch_1gib_dirty_stepper` | Pair of 1 GiB sparse zero files; both sides have equal sparse dirty islands; other file has final byte changed; scan with 128 MiB stepper. |
