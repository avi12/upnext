# Maintainers' runbooks

These pages are for whoever runs UpNext's releases and services. You don't need any of them to use UpNext or to send a pull request; CONTRIBUTING.md is enough for that.

- [RELEASE.md](RELEASE.md): releasing a version and checking it on a clean machine, the third-party notices, the package repositories, the Android app's key, the Flatpak, the Microsoft Store package, the Steam copy, building the shared game data, and the one-time steps for going public.
- [CI-RUNNERS.md](CI-RUNNERS.md): running `ci` and `windows` on self-hosted runners while the repository is private, and handing them back to GitHub's runners before it goes public.
- [VERDICTS-SERVICE.md](VERDICTS-SERVICE.md): setting up, deploying and running the backlog verdicts service (`services/verdicts/`) on Cloudflare.
