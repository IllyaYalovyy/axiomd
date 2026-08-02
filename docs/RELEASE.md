# Release Checklist

Use this checklist when preparing a release.

## Before Release

- [ ] Update version numbers in every package manifest.
- [ ] Update `CHANGELOG.md` or release notes.
- [ ] Run `./scripts/quality.sh`.
- [ ] Run any project-specific packaging, installer, or smoke tests.
- [ ] Verify docs describe the released behavior.
- [ ] Confirm no secrets, local paths, or agent files are staged.

## Release Notes

Include:

- User-visible changes
- Bug fixes
- Breaking changes or migrations
- Known issues
- Upgrade or rollback notes

## After Release

- [ ] Tag the release.
- [ ] Publish artifacts.
- [ ] Verify install / upgrade from the published artifact.
- [ ] Open follow-up issues for deferred work.
