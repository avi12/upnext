# Self-hosted CI runners

While the repository is private, `ci` and `windows` (and `docker`, if its runner has Docker) can run on the owner's own machines instead of GitHub's runners. That's optional: GitHub's runners run a private repository's workflows too, counted against the private repository's Actions minutes, and every workflow uses them while its variable is unset. This page installs a runner on an Arch-based Linux machine and on a Windows 11 Home desktop, turns the workflows over to them, and undoes it all before the repository goes public. Every command runs as the owner, on the machine named in its section; nothing here belongs in the repository afterwards (no machine names, addresses or paths).

## What runs where

Each workflow reads one repository variable. Unset, the job runs on GitHub's runner; set to a label, it runs on the self-hosted runner that carries that label.

| Workflow | Jobs | Variable | Value while private | Unset |
|---|---|---|---|---|
| `ci` | `rust`, `web`, `verdicts-service`, `e2e` | `UPNEXT_RUNNER_LINUX` | `upnext-linux` | `ubuntu-latest` |
| `windows` | `rust` | `UPNEXT_RUNNER_WINDOWS` | `upnext-windows` | `windows-latest` |
| `docker` | `image` | `UPNEXT_RUNNER_DOCKER` | `upnext-linux`, only on a runner set up as in "Rootless Docker" below | `ubuntu-latest` |
| `android` | `build` | `UPNEXT_ANDROID_CI` set to `on` turns it on; then `UPNEXT_RUNNER_LINUX` as for `ci` | off until a runner has JDK 17 and the Android SDK (`ANDROID_HOME`) | off |
| `flatpak` | `build` | `UPNEXT_FLATPAK_CI` set to `on` turns it on; GitHub's runner only, since flatpak-builder needs a privileged container | off | off |
| `release` | all | none | (always GitHub's runners) | `ubuntu-22.04` |

On a self-hosted runner every job is skipped for a pull request whose branch is in a fork. Pushes, manual runs and pull requests from branches of this repository (Dependabot's included) run.

## Rules

- **Never set an `UPNEXT_RUNNER_*` variable on a public repository**, and remove the runners before this repository could become public ("Flipping back"). On a public repository a fork's pull request runs its own copy of the workflow files, so it could delete the fork check and run anything on these machines.
- Leave "Run workflows from fork pull requests" off (Settings, Actions, General, "Fork pull request workflows in private repositories"; off by default).
- Each runner runs as an account of its own that can't read the owner's files: a dedicated `github-runner` user on Linux, the built-in Network Service account on Windows. CI builds third-party code (crates, npm packages, Dependabot's updates), so it gets no access to a real library, keys or sign-ins.
- No runner account is ever in the `docker` group. The Linux runner's Docker, if it has one, is rootless (section 5).
- One runner per machine. The `e2e` job uses ports 8798 and 8799; two runners on one machine could run it twice at once.

## A registration token

A runner registers with a token that lasts one hour. With `gh` signed in as an admin of the repository:

```sh
R=zytocc/upnext
gh api -X POST repos/$R/actions/runners/registration-token --jq .token
```

Without `gh`: Settings, Actions, Runners, "New self-hosted runner". The page's "Configure" commands carry a token (the value after `--token`), and its "Download" commands show the current runner version and its SHA-256.

## Linux runner (Arch-based)

### 1. Packages

Arch wants the system up to date before installing (`sudo pacman -Syu`). Then:

```sh
# The runner (.NET needs ICU and Kerberos), building UpNext with the desktop
# feature (WebKitGTK 4.1, GTK 3), and the scripts in the runbook.
sudo pacman -S --needed base-devel git pkgconf jq icu krb5 webkit2gtk-4.1 gtk3
# Playwright's Chromium: the libraries `playwright install --with-deps`
# installs on Ubuntu, which it can't do on Arch, and fonts.
sudo pacman -S --needed nss nspr alsa-lib at-spi2-core libcups dbus libdrm libxkbcommon \
  libxcomposite libxdamage libxext libxfixes libxrandr mesa pango cairo ttf-liberation
```

### 2. An account for the runner

```sh
sudo useradd --create-home --shell /bin/bash github-runner
ls -ld /home/*    # your own home should read drwx------ (or drwx--x---), so the runner can't read it
```

### 3. Rust for that account

The workflows' `dtolnay/rust-toolchain` step installs rustup on the first run if it's missing; installing it now saves that run's time and shows problems early.

```sh
sudo -iu github-runner bash -c \
  'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable -c clippy,rustfmt'
```

Node 24, Playwright's Chromium and cargo-deny need nothing here: `actions/setup-node` downloads Node into the runner's tool cache once, `npx playwright install chromium` keeps the browser in `~/.cache/ms-playwright`, and `taiki-e/install-action` fetches cargo-deny on each run.

### 4. The runner, as a service

As `github-runner`, download the runner and check it against the release's SHA-256 (the release notes carry one per package):

```sh
sudo -iu github-runner
rel=$(curl -fsSL https://api.github.com/repos/actions/runner/releases/latest)
V=$(jq -r .tag_name <<<"$rel" | sed 's/^v//')
SHA=$(jq -r .body <<<"$rel" | sed -n 's/.*<!-- BEGIN SHA linux-x64 -->\([0-9a-f]\{64\}\)<!-- END SHA linux-x64 -->.*/\1/p')
echo "runner $V, sha256 $SHA"    # both must be set; or copy them from the "New self-hosted runner" page
mkdir -p ~/actions-runner && cd ~/actions-runner
curl -fsSL -o runner.tar.gz "https://github.com/actions/runner/releases/download/v$V/actions-runner-linux-x64-$V.tar.gz"
echo "$SHA  runner.tar.gz" | sha256sum -c -
tar xzf runner.tar.gz && rm runner.tar.gz
./config.sh --url https://github.com/zytocc/upnext --token <TOKEN> \
  --name upnext-linux-1 --labels upnext-linux --work _work --unattended
exit
```

`--name` keeps the machine's own name out of GitHub; the runner also gets the labels `self-hosted`, `Linux` and `X64` by itself. Then install it as a system service that runs as `github-runner` and starts at boot:

```sh
cd /home/github-runner/actions-runner
sudo ./svc.sh install github-runner
sudo ./svc.sh start
sudo ./svc.sh status    # active (running), "Listening for Jobs" in the log
```

The unit is `actions.runner.zytocc-upnext.upnext-linux-1.service`; `journalctl -u 'actions.runner.*' -f` follows it. The runner updates itself when GitHub releases a new version.

### 5. Rootless Docker (optional, for the `docker` workflow)

The `docker` job builds the image with buildx, smoke-tests it, builds the arm64 image and smoke-tests it under QEMU, then pushes to GHCR. On this runner Docker runs **rootless**: the daemon belongs to `github-runner` and runs inside its user namespace, so nothing a CI job runs (a crate's build script included) gets root on the machine. A rootful `docker.service` for other containers can stay as it is, but `github-runner` must never reach it: never add it to the `docker` group, and never point it at `/var/run/docker.sock`. The image itself runs as an unprivileged user (`USER 1000:1000` in the `Dockerfile`) either way. Skip this section to leave `docker` on GitHub's runners.

**a. The system's part** (your own account, with sudo):

```sh
# Docker and its buildx plugin, RootlessKit and slirp4netns for the rootless
# daemon, and QEMU's arm64 emulator registered at boot. (newuidmap and
# newgidmap come with shadow, which every Arch system has.)
sudo pacman -S --needed docker docker-buildx rootlesskit slirp4netns qemu-user-static qemu-user-static-binfmt
command -v iptables || sudo pacman -S --needed iptables-nft
# The rootless parts Arch's docker package leaves out, from the AUR's
# docker-rootless-extras: /usr/bin/dockerd-rootless.sh, the user units
# docker.socket and docker.service, and a sysctl file for user namespaces.
# Read its PKGBUILD before building it.
git clone https://aur.archlinux.org/docker-rootless-extras.git /tmp/docker-rootless-extras
(cd /tmp/docker-rootless-extras && less PKGBUILD && makepkg -si)
sudo sysctl --system
# The runner never reaches a rootful daemon: not in the docker group (the
# rootful docker.service may keep running for other containers).
id -nG github-runner | grep -qw docker && echo "remove github-runner from the docker group" || echo ok
# QEMU for arm64, registered with the F flag so it works inside containers.
sudo systemctl restart systemd-binfmt.service
grep -E '^(enabled|flags)' /proc/sys/fs/binfmt_misc/qemu-aarch64    # "enabled" and flags containing F
# Subordinate ids for the runner's user namespace. Useradd may have added them;
# if not, pick a range of 65536 that overlaps no other line in either file.
grep github-runner /etc/subuid /etc/subgid \
  || sudo usermod --add-subuids 231072-296607 --add-subgids 231072-296607 github-runner
# The runner's own systemd (and with it the daemon) starts at boot, without a login.
sudo loginctl enable-linger github-runner
# The daemon's networks need nf_tables (ip_tables if `iptables --version` says legacy).
echo nf_tables | sudo tee /etc/modules-load.d/docker-rootless.conf && sudo modprobe nf_tables
```

**b. The daemon** (as `github-runner`, in a real login session so `systemctl --user` works; `sudo -iu` doesn't give one):

```sh
sudo machinectl shell github-runner@ /bin/bash
systemctl --user enable --now docker.socket       # the package's user unit; the daemon starts on first use
export DOCKER_HOST=unix://$XDG_RUNTIME_DIR/docker.sock
docker info --format '{{.SecurityOptions}}'        # includes name=rootless
systemctl --user is-active docker.service          # active, started by that call
docker buildx version                              # prints a version
docker run --rm --platform linux/arm64 alpine uname -m    # aarch64
exit
```

The daemon is pacman's `dockerd`, run through `dockerd-rootless.sh` by the AUR package's user units, so updates come from pacman and the AUR; nothing is installed into the runner's home. `DOCKER_HOST` is what keeps the runner on this daemon rather than the rootful one's `/var/run/docker.sock`. If `docker info` fails, `journalctl --user -u docker.service` (as `github-runner`) names what's missing: usually the subordinate ids, `nf_tables` or `slirp4netns`.

**c. Point the runner at it.** The runner reads extra environment variables from `.env` in its folder at start:

```sh
cd /home/github-runner/actions-runner
echo "DOCKER_HOST=unix:///run/user/$(id -u github-runner)/docker.sock" | sudo -u github-runner tee -a .env
sudo ./svc.sh stop && sudo ./svc.sh start
```

The job's build cache is GitHub's Actions cache (`type=gha`), so each run downloads and uploads layers; the job removes its two test images at the end. The workflow skips `docker/setup-qemu-action` on a self-hosted runner, since the host registered QEMU in step a. The daemon's images and build data live in `~github-runner/.local/share/docker`.

### 6. A machine without working IPv6

Some machines have no working IPv6 even though their name lookups return IPv6 addresses, for example behind a VPN that drops IPv6 traffic instead of refusing it. There the `e2e` job hangs at `npx playwright install chromium`: Playwright's downloader tries IPv6 first and waits on a connection that never answers, and `NODE_OPTIONS=--dns-result-order=ipv4first` doesn't change that. The fix is to forbid IPv6 sockets to the runner service, so every connection attempt over IPv6 fails at once and the downloader falls back to IPv4:

```sh
d=/etc/systemd/system/actions.runner.zytocc-upnext.upnext-linux-1.service.d
sudo mkdir -p $d
printf '[Service]\nRestrictAddressFamilies=~AF_INET6\n' | sudo tee $d/no-ipv6.conf
sudo systemctl daemon-reload
sudo systemctl restart actions.runner.zytocc-upnext.upnext-linux-1.service
```

The unit's name is `actions.runner.<owner>-<repository>.<runner name>.service`; `systemctl list-units 'actions.runner.*'` shows it. The drop-in applies to the runner service and the jobs it starts, not to the rootless Docker daemon, which runs under `github-runner`'s own systemd. Nothing in the tests needs IPv6: the `e2e` server and the fake model listen on `127.0.0.1`.

## Windows runner (Windows 11 Home)

Everything below runs in PowerShell opened as Administrator.

### 1. Tools

```powershell
winget install -e --id Git.Git                  # skip if Git for Windows is installed
winget install -e --id Microsoft.PowerShell     # PowerShell 7 (pwsh), which the workflow's scripts use
winget install -e --id NASM.NASM                # aws-lc-sys assembles its x86_64 code with NASM
winget install -e --id Microsoft.VisualStudio.2022.BuildTools --override `
  "--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"   # MSVC and the Windows SDK
```

Put Git's `bin` folder (its `bash.exe`) and NASM on the machine's `PATH`, ahead of `System32`: the workflows' `shell: bash` steps (`dtolnay/rust-toolchain`) must find Git Bash, not WSL's `bash.exe`. In your own terminals `bash` then opens Git Bash too.

```powershell
$add = 'C:\Program Files\Git\bin', 'C:\Program Files\NASM'
$path = [Environment]::GetEnvironmentVariable('Path', 'Machine') -split ';'
[Environment]::SetEnvironmentVariable('Path', (($add + $path) | Where-Object { $_ } | Select-Object -Unique) -join ';', 'Machine')
```

Restart Windows afterwards: a Windows service takes its environment when Windows starts, so the runner only sees a changed machine `PATH` after a restart. To skip the restart, write the new machine `PATH` into the runner's `.env` file (section 2 creates the folder), which the runner reads each time it starts, and restart the service:

```powershell
"PATH=$([Environment]::GetEnvironmentVariable('Path', 'Machine'))" | Set-Content C:\actions-runner\.env
Restart-Service actions.runner.*
```

That copy is frozen: a later change to the machine `PATH` won't reach the runner while `.env` holds it. Delete `C:\actions-runner\.env` after the next restart of Windows, which gives the service the real machine `PATH` again.

PowerShell 7 and NASM must be installed for the whole machine. The runner's account (Network Service) can't see a per-user install in your own profile, which is where winget can put them by default. Install them for all users (winget's `--scope machine`, or the installer's own all-users choice), and check that `C:\Program Files\PowerShell\7\pwsh.exe` and `C:\Program Files\NASM\nasm.exe` exist.

Rust needs no install: the first run's `dtolnay/rust-toolchain` step installs rustup and the stable toolchain for the runner's account (Network Service, under `C:\Windows\ServiceProfiles\NetworkService\.cargo`), and later runs only fetch a new stable. Nothing else in the `windows` workflow downloads.

### 2. The runner, as a service

`gh` must be signed in as an admin of the repository (or paste a token from the "New self-hosted runner" page into `$token`).

```powershell
$R = 'zytocc/upnext'
$token = gh api -X POST "repos/$R/actions/runners/registration-token" --jq .token
New-Item -ItemType Directory -Force C:\actions-runner | Out-Null
Set-Location C:\actions-runner
$rel = Invoke-RestMethod https://api.github.com/repos/actions/runner/releases/latest
$V = $rel.tag_name.TrimStart('v')
$sha = [regex]::Match($rel.body, '<!-- BEGIN SHA win-x64 -->([0-9a-f]{64})<!-- END SHA win-x64 -->').Groups[1].Value
if (-not $sha) { throw 'no SHA-256 for win-x64 in the release notes; copy it from the "New self-hosted runner" page' }
Invoke-WebRequest "https://github.com/actions/runner/releases/download/v$V/actions-runner-win-x64-$V.zip" -OutFile runner.zip
if ((Get-FileHash runner.zip -Algorithm SHA256).Hash -ne $sha) { throw 'runner.zip does not match the release SHA-256' }
Expand-Archive runner.zip -DestinationPath . ; Remove-Item runner.zip
.\config.cmd --url "https://github.com/$R" --token $token --name upnext-windows-1 --labels upnext-windows `
  --work _work --runasservice --unattended
```

`--runasservice` with no `--windowslogonaccount` installs the service `actions.runner.zytocc-upnext.upnext-windows-1` running as `NT AUTHORITY\NETWORK SERVICE`, started automatically at boot; the configuration grants that account the runner's folder. The short folder keeps build paths well under Windows' 260-character limit. Check it:

```powershell
Get-Service actions.runner.* | Format-Table Name, Status, StartType
```

### 3. Sleep

A job waits while the PC sleeps and starts when it wakes (a queued job is cancelled after 24 hours). Nothing here changes the power settings.

Optional, for faster builds: `Add-MpPreference -ExclusionPath C:\actions-runner` stops Microsoft Defender scanning the runner's folder, which also means it doesn't scan the crates and packages CI downloads there.

## Turning the workflows over to the runners

Once both runners are online (next section):

```sh
R=zytocc/upnext
gh variable set UPNEXT_RUNNER_LINUX --body upnext-linux --repo $R
gh variable set UPNEXT_RUNNER_WINDOWS --body upnext-windows --repo $R
gh variable set UPNEXT_RUNNER_DOCKER --body upnext-linux --repo $R    # only after "Rootless Docker" above
gh workflow run ci --ref main --repo $R
gh workflow run windows --ref main --repo $R
gh run list --repo $R --limit 5
```

Or Settings, Secrets and variables, Actions, Variables, "New repository variable". The value is the label alone, with no quotes or brackets. A label no online runner carries leaves the job queued.

## Checking a runner is online

```sh
gh api repos/zytocc/upnext/actions/runners \
  --jq '.runners[] | {id, name, os, status, busy, labels: [.labels[].name]}'
```

`status` is `online` and the labels include `upnext-linux` or `upnext-windows`. The same list is under Settings, Actions, Runners. On the machine: `sudo ./svc.sh status` (Linux) or `Get-Service actions.runner.*` (Windows). A job's log names the runner in its "Set up job" step.

## Removing a runner

Linux, in `/home/github-runner/actions-runner`:

```sh
sudo ./svc.sh stop && sudo ./svc.sh uninstall
sudo -u github-runner ./config.sh remove --token <REMOVE_TOKEN>
sudo userdel -r github-runner    # also deletes its toolchains, caches and build folder
```

Windows, as Administrator, in `C:\actions-runner` (this also removes the service):

```powershell
.\config.cmd remove --token (gh api -X POST repos/zytocc/upnext/actions/runners/remove-token --jq .token)
Set-Location \; Remove-Item C:\actions-runner -Recurse -Force
```

`<REMOVE_TOKEN>` comes from `gh api -X POST repos/zytocc/upnext/actions/runners/remove-token --jq .token`, or from the runner's page in Settings, Actions, Runners ("Remove"). If the machine is gone, delete the runner from GitHub's side: `gh api -X DELETE repos/zytocc/upnext/actions/runners/<id>`, with the id from the list above. Rust for the Network Service account stays in `C:\Windows\ServiceProfiles\NetworkService\.cargo` and `.rustup`; delete both folders to free the space.

## Flipping back to GitHub's runners

Delete the variables; the next run uses GitHub's runners with no change to the workflow files:

```sh
for v in UPNEXT_RUNNER_LINUX UPNEXT_RUNNER_WINDOWS UPNEXT_RUNNER_DOCKER; do gh variable delete $v --repo zytocc/upnext; done
gh variable list --repo zytocc/upnext    # no UPNEXT_RUNNER_* left
```

Then remove both runners as above. Going public (`docs/dev/RELEASE.md`) creates a new repository, which starts with no variables and no runners; the old one, renamed to the archive, keeps both until they are removed, so remove them before or right after the rename.

## Tools each runner needs

| Tool | Linux runner | Windows runner | Used by |
|---|---|---|---|
| Git | `git` (pacman) | Git for Windows, `bin` on `PATH` | `actions/checkout`; Git Bash for `shell: bash` |
| Rust stable, clippy, rustfmt | rustup for `github-runner` (or the first run installs it) | the first run installs it | `dtolnay/rust-toolchain` checks it each run and fetches a new stable |
| C toolchain | `base-devel`, `pkgconf` | VS 2022 Build Tools (VCTools, Windows SDK) | crates with C code (aws-lc-sys, SQLite) |
| NASM | not needed | NASM, on `PATH` | aws-lc-sys on Windows x86_64 |
| WebKitGTK 4.1, GTK 3 | `webkit2gtk-4.1`, `gtk3` | not needed (WebView2's loader is linked statically) | the `desktop` feature: clippy and tests with default features |
| cargo-deny | downloaded each run | not used | `taiki-e/install-action` in `ci`'s `rust` job |
| Node 24 | downloaded once into the tool cache | not used | `actions/setup-node` |
| workerd (Cloudflare's runtime) | `npm ci` fetches its prebuilt binary each run; it needs only glibc | not used | Miniflare and wrangler in `ci`'s `verdicts-service` job |
| Playwright Chromium | downloaded once to `~/.cache/ms-playwright`; its libraries from pacman | not used | `npx playwright install chromium` in `e2e` |
| PowerShell 7 | not used | `Microsoft.PowerShell` | `shell: pwsh` steps |
| Docker, buildx, QEMU | optional: `docker`, `docker-buildx`, `rootlesskit`, `slirp4netns`, the AUR's `docker-rootless-extras` (its `docker.socket` user unit), `qemu-user-static-binfmt`; never the `docker` group or `/var/run/docker.sock` | not used | the `docker` workflow |
| .NET runtime libraries | `icu`, `krb5` | built in | the runner itself |

## When something fails

- **A job stays queued:** no online runner has its label. Check the list above, the variable's value, and that the machine is awake.
- **`bash` not found, or WSL starts, on Windows:** Git's `bin` isn't ahead of `System32` on the machine `PATH`, or Windows wasn't restarted since (or `.env` doesn't carry the new `PATH`; section 1).
- **`link.exe` not found, or NASM errors, on Windows:** the Build Tools' C++ workload or NASM is missing; install it and restart the service (`Restart-Service actions.runner.*`).
- **`e2e` hangs at `npx playwright install chromium` on Linux:** the machine has no working IPv6; add the drop-in in section 6.
- **A Playwright test runs out of time in `e2e`:** the job runs the whole suite (`web/e2e/`) on the runner, and a self-hosted machine can be slower than GitHub's. The viewport test ("every page fits a tablet and a small window", 63 page loads) is marked `test.slow()` for that reason; another long test may need the same.
- **Chromium won't start in `e2e` on Linux:** `sudo find ~github-runner/.cache/ms-playwright -type f -name '*headless*shell' -exec ldd {} \; | grep 'not found'` names the missing library; install its package.
- **`docker` can't reach the daemon:** `systemctl --user status docker.socket docker.service` as `github-runner` (through `machinectl shell`), `loginctl show-user github-runner --property=Linger` says `yes`, and `.env` has the `DOCKER_HOST` line; restart the runner after changing it.
- **`exec format error` in the arm64 smoke test:** QEMU isn't registered; check `/proc/sys/fs/binfmt_misc/qemu-aarch64` as in section 5a.
- **Disk filling up:** each runner keeps one build folder beside its checkout (`_work/upnext/cargo-target`), which the workflow deletes past 30 GB; it can be deleted any time between runs.
