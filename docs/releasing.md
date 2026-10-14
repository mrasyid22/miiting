# Building and releasing

## What CI does

`.github/workflows/build.yml` runs on a GitHub-hosted `macos-26` runner (Apple Silicon, Xcode 26):

| Trigger | Steps | Download |
|---|---|---|
| Pull request | `make lint`, `make test`, build, `.dmg` | Artifact on the workflow run page |
| Push to `main` | same, then replace the **nightly** pre-release | Releases › Nightly |
| Tag `v*` | same, then create a release named after the tag | Releases › `vX.Y.Z` |

The version comes from the tag (`v0.2.0` → `0.2.0`) or, for nightlies, from `Info.plist` plus the run number (`0.1.0-nightly.57`). `CFBundleVersion` is the run number.

macOS runners are free for public repositories; on private repositories their minutes are billed at a higher rate than Linux.

## Cutting a release

1. Update `CFBundleShortVersionString` in `apps/macos/Resources/Info.plist` if the next nightlies should carry the new version.
2. Tag and push:

   ```bash
   git tag v0.1.0
   git push origin v0.1.0
   ```

3. CI builds, signs and notarizes (if configured) and attaches `Miiting-0.1.0.dmg` to the release, with notes generated from the commits since the previous tag.

## Signing and notarization

Without a Developer ID, the `.dmg` is ad-hoc signed. It works, but macOS blocks it on first launch and users have to allow it in System Settings › Privacy & Security › *Open Anyway* (the release notes say how). Microphone and system-audio permissions may also be requested again after every update.

To ship builds that open normally you need a paid [Apple Developer Program](https://developer.apple.com/programs/) membership. Then add these repository secrets (Settings › Secrets and variables › Actions, or `gh secret set NAME`):

| Secret | What it is | How to get it |
|---|---|---|
| `MACOS_CERT_P12_BASE64` | Developer ID Application certificate and private key | Create it in Xcode › Settings › Accounts › Manage Certificates (or on developer.apple.com), export it from Keychain Access as `.p12`, then `base64 -i cert.p12 \| pbcopy` |
| `MACOS_CERT_PASSWORD` | Password chosen when exporting the `.p12` | — |
| `MACOS_SIGN_IDENTITY` | e.g. `Developer ID Application: Your Name (TEAMID)` | `security find-identity -v -p codesigning` |
| `APPLE_API_KEY_P8_BASE64` | App Store Connect API key, for `notarytool` | App Store Connect › Users and Access › Integrations › Team Keys (Developer role); `base64 -i AuthKey_XXXX.p8 \| pbcopy` |
| `APPLE_API_KEY_ID` | The key's ID | Same page |
| `APPLE_API_ISSUER_ID` | Issuer ID | Same page, above the key list |

With the certificate secrets set, CI signs the app with the hardened runtime and signs the `.dmg`. With the API key set as well, it notarizes and staples the `.dmg`. Secrets are never available to pull requests from forks, so those builds stay ad-hoc signed.

## Doing it locally

```bash
make dmg                                         # ad-hoc signed, build/Miiting-<version>.dmg

SIGN_ID="Developer ID Application: Your Name (TEAMID)" VERSION=0.1.0 scripts/build-app.sh
SIGN_ID="Developer ID Application: Your Name (TEAMID)" scripts/make-dmg.sh build/Miiting.app
APPLE_API_KEY_PATH=~/keys/AuthKey_XXXX.p8 APPLE_API_KEY_ID=XXXX APPLE_API_ISSUER_ID=… \
  scripts/notarize.sh build/Miiting-0.1.0.dmg
```
