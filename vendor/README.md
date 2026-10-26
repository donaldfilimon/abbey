# Abbey GTK3 compatibility backports

These are complete published source archives with package identities retained:
`glib 0.18.5`, `glib-macros 0.18.5`, and `gtk3-macros 0.18.2`. They are local
Abbey sources, independent of other products. This directory is an excluded,
independent Cargo workspace, with its own patches, lock and optimized GLib dev
profile. Root patches are also required for the desktop consumer.

`source-manifest.json` inventories every pristine and patched archive file.
The verifier has separate review pins for both complete tree digests; changing
that manifest cannot bless source drift. The hash format sorts relative paths
and hashes `path + NUL + lowercase file SHA256 + newline` for every file.
Pristine archive hashes identify original downloads, never the patched bytes.
All upstream tests, README/license files, original manifests and VCS metadata
remain. gtk3-macros retains its original unmaintained description and dirty VCS
marker. `.cargo-checksum.json` and registry bookkeeping are not archive contents.

GLib changes only `VariantStrIter::impl_get`: the C out-pointer is mutable and
passed by mutable reference, exactly as [upstream b5a4071](https://github.com/gtk-rs/gtk-rs-core/commit/b5a4071e439bef2b5eea76c3aa25e5ae84839e34).
Both macro crates use the real dependency `proc-macro-error3 =3.1.1`, with
`default-features = false` and `features = ["syn2-error"]`, in both manifests.
They migrate the existing `proc_macro_error::` paths to `proc_macro_error3::`.
Existing attribute names and behavior remain. A dependency alias is insufficient
because attr3 expands `::proc_macro_error3::entry_point`. Explicit Syn 2 support
avoids the successor's default Syn 3 API. There are no other production source changes.

The three archives' MIT licenses have SHA256
`8cf56d10131ce201cf69ab74b111d3ebac1acca3833d7efb39ae357224b70edb`.
GLib and glib-macros record gtk-rs-core VCS
`42b9caf98e03ded086362d9653ca58fe94dc8658`; gtk3-macros records gtk3-rs VCS
`00133512bfc8bbb8e9be59b20e93406c6a3eeb21` with `dirty: true`.

Registry replacements retain their locked published identities:

| Package | Archive SHA256 | License |
| --- | --- | --- |
| proc-macro-error3 3.1.1 | `8f0d4471b3436c22106b21913b1dda531558918ae9b7ec55d58aa84b43552233` | MIT OR Apache-2.0 |
| proc-macro-error-attr3 3.1.1 | `9e564d14133360e1ae169ffde5da25881b5fa47261665b8e5713c212c27799da` | MIT OR Apache-2.0 |

proc-macro-error3's LICENSE-MIT and LICENSE-APACHE hashes are respectively
`4d6bf8cef395998be2de2da71eb023ca62603efbd8618b485bb3d1381d011d4b` and
`8cdfc2c3fe15f6bdf5926ebac8e983315133b91128d888e6f1bd3227bd0a7cfc`.
attr3's license hashes are respectively
`544b3aed1fd723d0cadea567affdcfe0431e43e18d997a718f9d67256b814fde` and
`6fd0f3522047150ca7c1939f02bc4a15662a4741a89bc03ae784eefa18caa299`.
Both record VCS `0f0e21bb2a46509c923b87459c6be314bc0cabb5` in
[gamma0987/proc-macro-error3](https://github.com/gamma0987/proc-macro-error3),
with paths root and `proc-macro-error-attr`. `source-manifest.json` records
these registry archive/license/VCS facts; the verifier independently pins them.
The earlier proc-macro-error2 2.0.1 candidate failed fresh mandatory scanning on
RUSTSEC-2026-0173. The controller amended the contract to this maintained API
successor; original candidate logs are retained privately, with no new ignore.
This inventory does not constitute complete distribution-license clearance.

From Abbey's root, use `tools/cargo.sh` and separate targets under `.work/gtk/`.
Run `python3 tools/security/check_gtk_vendor.py` before and after fixtures. It
checks all source bytes, licenses, package versions, both locks, both resolved
source graphs, both macro-parent edges and absence of the old error crates.
The lightweight default fixture is `--no-default-features --test diagnostics`;
it explicitly keeps GTK optional in subprocess manifests so malformed macros
reach their intended diagnostic without linking a GTK SDK. Its four diagnostics
and compiler 1.99.1 identity are reviewed expectations, never auto-blessed.
`--features glib-runtime --test glib_runtime` executes enum/Value, clone!, and
optimized iterator witnesses. Run dev and release, plus upstream GLib
`--lib variant_iter` in both profiles. Upstream crate warnings are retained.

`compat-tests/run-linux.sh` is the explicit native Linux GTK3 runner. It requires
real GLib/GTK3 SDKs and `xvfb-run`, then runs positive CompositeTemplate child
binding and all runtime witnesses in both profiles. Unsupported hosts fail with
exit 2. No SDK is installed and no zero-test or skipped platform run qualifies
GTK3. Windows and native Linux application acceptance remain separate gates.
See `compat-tests/VERIFICATION.md` for executed
host outcomes and remaining findings.
