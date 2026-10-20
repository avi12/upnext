# UpNext's container image: the server and its web UI, without the
# desktop window (no GTK or WebKitGTK). contrib/docker/compose.yaml runs it;
# docs/install/docker.md has the steps. Build: `docker build -t upnext .`, or for
# both architectures `docker buildx build --platform linux/amd64,linux/arm64 .`
#
# Every build stage runs on the builder's own platform: the web UI is the
# same for both, and the server is cross-compiled for the other one. The
# image itself runs nothing while it's built, so an arm64 image builds on an
# amd64 machine at full speed, without QEMU.
#
# Each base image is pinned to a digest beside its tag, so a tag that moves
# upstream can't change what's built; Dependabot (.github/dependabot.yml)
# proposes the new digest when the tag moves. Debian is named by release
# (trixie, like the build stages) rather than `stable`, which would jump to
# the next release on its own.

# 1. The web UI.
FROM --platform=$BUILDPLATFORM node:24-trixie-slim@sha256:7f486cabe892bff8fef04c0f7da8dba84a560be3e4e2740b7a86afc57ebec315 AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
RUN npm run build

# 2. The server, with the UI built in (a release build needs web/dist),
# its symbols stripped to keep the image small (contrib/docker/smoke-test.sh
# has the size budget).
FROM --platform=$BUILDPLATFORM rust:1-trixie@sha256:6ff07edce8775d0f64be7aba9197229407301bddf2054d62c27b541a6238a181 AS server
ARG TARGETARCH
ARG BUILDARCH
WORKDIR /src
ENV CARGO_PROFILE_RELEASE_STRIP=symbols \
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    CC_x86_64_unknown_linux_gnu=x86_64-linux-gnu-gcc \
    CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
# Rust's target, and for another architecture Debian's cross compiler and C
# library, which the C in the dependencies (SQLite, aws-lc) builds with. A
# builder of the same architecture has its compiler under that name already.
RUN set -eu; \
    case "$TARGETARCH" in \
      amd64) target=x86_64-unknown-linux-gnu gnu=x86-64 ;; \
      arm64) target=aarch64-unknown-linux-gnu gnu=aarch64 ;; \
      *) echo "UpNext's image builds for amd64 and arm64, not $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    if [ "$TARGETARCH" != "$BUILDARCH" ] && ! command -v "${target%%-*}-linux-gnu-gcc" >/dev/null; then \
      apt-get update; \
      apt-get install -y --no-install-recommends "gcc-$gnu-linux-gnu" "libc6-dev-$TARGETARCH-cross"; \
      rm -rf /var/lib/apt/lists/*; \
    fi; \
    rustup target add "$target"; \
    echo "$target" > /target
# The dependencies first, in a layer of their own, so a change to UpNext's
# code doesn't build them again. The build script sets the version the
# code reads (`UPNEXT_VERSION`); with no `.git` here it is the plain one.
COPY Cargo.toml Cargo.lock build.rs ./
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    mkdir -p src/bin \
    && echo 'fn main() {}' > src/main.rs \
    && echo 'fn main() {}' > src/bin/upnext-app.rs \
    && touch src/lib.rs \
    && cargo build --release --locked --no-default-features --target "$(cat /target)" \
    && rm -rf src
COPY src src
COPY tests/fixtures tests/fixtures
# The server serves the third-party notices (src/server/ui.rs).
COPY THIRD_PARTY_NOTICES.txt ./
COPY --from=web /src/web/dist web/dist
# Newer than the placeholders above, so cargo builds UpNext itself again.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    touch src/main.rs src/lib.rs \
    && cargo build --release --locked --no-default-features --target "$(cat /target)" \
    && install -m 755 "target/$(cat /target)/release/upnext" /upnext

# 3. What the image adds to Debian, made here so the image's own stage runs
# nothing: tini for its architecture (checked by its SHA-256) as process 1,
# to pass on signals and reap the AI CLIs' child processes; the upnext user
# (uid 1000, home /data) added to the image's own passwd and group files;
# and /data.
FROM debian:trixie-slim@sha256:a29215f6a35e51e22adffa17f89e9d2ef06214e64a2bad10d765c46aea49f11f AS base
FROM --platform=$BUILDPLATFORM debian:trixie-slim@sha256:a29215f6a35e51e22adffa17f89e9d2ef06214e64a2bad10d765c46aea49f11f AS files
ARG TARGETARCH
ADD --checksum=sha256:c5b0666b4cb676901f90dfcb37106783c5fe2077b04590973b885950611b30ee \
    https://github.com/krallin/tini/releases/download/v0.19.0/tini-static /tini/amd64
ADD --checksum=sha256:eae1d3aa50c48fb23b8cbdf4e369d0910dfc538566bfd09df89a774aa84a48b9 \
    https://github.com/krallin/tini/releases/download/v0.19.0/tini-static-arm64 /tini/arm64
COPY --from=base /etc/passwd /etc/group /out/etc/
RUN install -D -m 755 "/tini/$TARGETARCH" /out/usr/bin/tini \
    && echo 'upnext:x:1000:1000::/data:/usr/sbin/nologin' >> /out/etc/passwd \
    && echo 'upnext:x:1000:' >> /out/etc/group \
    && install -d -o 1000 -g 1000 /out/data

# 4. What runs: Debian's slim image (its time zones are there already, for
# TZ) and the build stage's certificates for HTTPS. Nothing comes from apt,
# so the image is the same wherever it's built.
FROM base
COPY --from=server /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=files /out/etc/passwd /out/etc/group /etc/
COPY --from=files /out/usr/bin/tini /usr/bin/tini
COPY --from=files --chown=1000:1000 /out/data /data
COPY --from=server /upnext /usr/local/bin/upnext
# UpNext's licences and the notices of what is built into it, tini's among
# them (contrib/notices/generate.mjs).
COPY LICENSE-MIT LICENSE-APACHE THIRD_PARTY_NOTICES.txt /usr/share/doc/upnext/

# /data holds everything: the database, the GOG sign-in, backups, a .env if
# you add one, the AI CLIs' working folder, and Claude Code when the
# container installs it (UPNEXT_INSTALL_CLAUDE=1; its command goes in
# /data/.local/bin, which is on PATH). UPNEXT_CONTAINER makes the server
# print a pairing code to its log, since no browser here counts as "this
# computer".
ENV UPNEXT_CONTAINER=1 \
    UPNEXT_DATA_DIR=/data \
    UPNEXT_CONFIG_DIR=/data \
    UPNEXT_CACHE_DIR=/data/cache \
    UPNEXT_HOST=0.0.0.0 \
    UPNEXT_PORT=8787 \
    HOME=/data \
    PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/data/.local/bin \
    TZ=UTC
VOLUME /data
WORKDIR /data
USER 1000:1000
EXPOSE 8787

HEALTHCHECK --interval=30s --timeout=5s --start-period=30s --start-interval=2s \
    CMD ["upnext", "healthcheck"]
LABEL org.opencontainers.image.title="UpNext" \
    org.opencontainers.image.description="Picks what to play next from your Steam, GOG and Heroic libraries" \
    org.opencontainers.image.source="https://github.com/zytocc/upnext" \
    org.opencontainers.image.licenses="MIT OR Apache-2.0"
ENTRYPOINT ["/usr/bin/tini", "--", "upnext"]
CMD ["serve"]
