# Store Submission

<!-- doc-locale: en -->
> **English** | [简体中文](store-submission.zh-CN.md)

Read this only when the user requests Store distribution. Developer submission
ends at an immutable uploaded revision; review, Store signing and publication
belong to independent operators.

## Prepare the immutable inputs

Use a developer-signed `.capp` with no Store signature. Register its stable App
ID in Developer Portal before upload. Keep Store resources beside the project:

```text
store/
  listing.json
  images/icon.png
  images/screen-1.png
```

The Listing is at most 32 KiB, rejects unknown fields and must exactly match the
package App ID and version. Use one 48x48 PNG icon up to 64 KiB and one to five
320x170 PNG screenshots up to 512 KiB each. Paths are ASCII relative to
`store/`, never symbolic links or `..`. Declare sorted bounded localizations,
keywords, supported category, age rating, HTTPS privacy URL and HTTPS support
URL according to `schemas/store-listing-v1.schema.json` in a source checkout.
The same schema is at `ROOT/schemas/store-listing-v1.schema.json` in a released
DevKit.

For a physical screenshot, use the trusted System Shell `Fn+J` capture. An App
cannot invoke or read the screenshot service; retrieving the generated PNG is
an explicit device-owner/operator action. Do not invent a trusted status bar or
stretch a 320x150 simulator surface into a Store screenshot.

## Validate and submit

Run local validation after every input change:

```sh
cp0ctl store validate APP.developer.capp store/listing.json
```

It verifies the developer signature, identity, paths, PNG structure and pixels,
sizes, SHA-256 values and absence of a Store signature. Only then run:

```sh
cp0ctl store submit APP.developer.capp store/listing.json
```

The CLI prints OAuth Device Flow instructions to stderr. Approve in the shown
Developer Portal with an eligible owner/developer account and current 2FA. The
token remains in process memory, has only `store.submit` scope and is never a
substitute for the developer signing key. Successful stdout is JSON containing
`submission_id`, `state`, `content_sha256` and `portal_url`.

The production origin is fixed. Use `CP0_STORE_API` only for an explicitly
requested HTTPS development control plane. Never upload a private key, persist
an OAuth token, retry fatal 4xx responses blindly, run `store publish`, or sign
the package with a Store key.
