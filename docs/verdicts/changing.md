# Changing the contract

[Backlog verdicts](../VERDICTS.md) · [A verdict](verdict.md) · [The published snapshot](snapshot.md)

- **A new answer or question** is a new vocabulary: bump `VOCAB` in `src/verdicts/vocab.rs`. Older verdicts stay readable with their own `vocab`.
- **New consent words** are a new consent version: bump `CONSENT_VERSION` in `src/verdicts/consent.rs` and pin the new text in its test. Raise `OLDEST_CONSENT` in `src/verdicts/verdict.rs` only when older consent must stop being accepted.
- **A different file layout** is a new format: bump `FORMAT` in `src/verdicts/format.rs`. Readers refuse a newer format and say to update.

After any of these, `UPNEXT_UPDATE_VERDICTS=1 cargo test verdicts` rewrites `docs/verdicts-contract.json` and the fixture snapshot; commit them with the change, and the service follows from the same files.
