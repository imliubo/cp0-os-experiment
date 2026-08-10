# Store Listing v1

<!-- doc-locale: en -->
> **English** | [简体中文](STORE-LISTING-V1.zh-CN.md)

`store-listing-v1` is the public metadata contract for application submissions.
SDK projects should use the following fixed layout:

```text
my-app/
  app.json
  store/
    listing.json
    images/
      icon.png
      screen-1.png
```

Resource paths in `listing.json` are relative to the `store/` directory. Each
path must be a safe ASCII relative path to a PNG file and must not contain `..`,
backslashes, or empty components. Developer private keys and Store signatures
do not belong in this directory.

## Example

```json
{
  "schema_version": 1,
  "app_id": "dev.cardputerzero.notes",
  "version": "1.2.0",
  "default_locale": "zh-Hans-CN",
  "category": "productivity",
  "age_rating": "4+",
  "privacy_url": "https://example.com/privacy",
  "support_url": "https://example.com/support",
  "icon": {
    "path": "images/icon.png",
    "sha256": "1111111111111111111111111111111111111111111111111111111111111111",
    "bytes": 4096,
    "width": 48,
    "height": 48
  },
  "screenshots": [
    {
      "path": "images/screen-1.png",
      "sha256": "2222222222222222222222222222222222222222222222222222222222222222",
      "bytes": 32000,
      "width": 320,
      "height": 170
    }
  ],
  "localizations": [
    {
      "locale": "zh-Hans-CN",
      "name": "便签",
      "subtitle": "为小屏优化的快速便签",
      "description": "记录和整理短便签。完全离线工作。",
      "keywords": ["便签", "效率"],
      "release_notes": "首个公开版本。"
    }
  ]
}
```

## Frozen Contract

- Listing JSON is limited to 32 KiB, and unknown fields are rejected.
- `app_id` and `version` must exactly match the manifest in the
  developer-signed `.capp`.
- Locales use a bounded canonical subset of BCP 47, such as `en`, `en-US`,
  `zh-Hans-CN`, and `es-419`.
- A listing may contain at most 8 locales. They must be sorted by locale and
  include `default_locale`.
- Each locale may contain at most 8 keywords. Keywords must be unique and
  sorted lexicographically.
- Categories are limited to `developer-tools`, `education`, `entertainment`,
  `games`, `hardware`, `media`, `productivity`, and `utilities`.
- Age ratings are limited to `4+`, `9+`, `12+`, and `17+`; the developer's
  declaration remains subject to review.
- The icon must be a 48x48 PNG no larger than 64 KiB. A listing must contain
  1-5 screenshots, each a 320x170 PNG no larger than 512 KiB.
- Privacy and support URLs must use HTTPS and must not contain credentials,
  fragments, spaces, or control characters.

The JSON schema is in `schemas/store-listing-v1.schema.json`, and the shared
strict validator is in the `cp0-store-metadata` crate. After preparing the
developer-signed package and listing, run:

```sh
cp0ctl store validate dev.cardputerzero.notes-1.2.0.signed.capp store/listing.json
```

This command verifies the developer signature, package/listing identity,
resource paths, PNG structure, and pixel dimensions, then recalculates resource
sizes and SHA-256 digests. It rejects submissions that already carry a Store
signature. The scan worker independently repeats verification and fully decodes
the resources in an isolated environment, binding the listing, resources, and
package summary into one submission revision. Passing the local precheck alone
does not make an application eligible for publication.

After registering an App ID in the Developer Portal, submit the same files using
OAuth Device Flow:

```sh
cp0ctl store submit dev.cardputerzero.notes-1.2.0.signed.capp store/listing.json
```

Authorization instructions are written to stderr. On success, stdout emits only
JSON containing `submission_id`, `state`, `content_sha256`, and `portal_url`.
Uploads retry in 256 KiB chunks, and the token is kept only in process memory.
The production endpoint is fixed at `https://developer.cardputerzero.dev`;
development environments may use `CP0_STORE_API` to specify another HTTPS
origin.
