# Jush's container image: one release's assets, from which
# `jush-server container` sets up or upgrades the server on the volume at
# /opt/jush (crates/server/src/container.rs). The release workflow builds it
# from the release's published assets, in a context laid out as
#
#   assets/<arch>/SHA256SUMS                          of these two
#   assets/<arch>/jush-<version>-server-linux-<arch>.tar.zst
#   assets/<arch>/jush-<version>-image-linux-<arch>.tar
#   bin/<arch>/jush-server                            the release's
#
# for amd64 and arm64.
FROM ubuntu:26.04

ARG TARGETARCH
# The update channel a new server starts on, as Settings › Updates shows it.
ARG CHANNEL=canary

# What the machine manager runs, as setup installs it on a machine
# (crates/server/src/setup.rs), and the roots TLS trusts.
RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      ca-certificates e2fsprogs xfsprogs libarchive-tools nftables \
    && rm -rf /var/lib/apt/lists/*

# The users setup makes, with ids of their own, since the volume's files keep
# them from one image to the next.
RUN groupadd --system --gid 990 jush-cloud \
    && groupadd --system --gid 991 jush \
    && useradd --system --uid 991 --gid jush --groups jush-cloud \
      --home-dir /opt/jush/data/backend --no-create-home \
      --shell /usr/sbin/nologin jush

COPY assets/${TARGETARCH}/ /usr/lib/jush/assets/
COPY --chmod=0755 bin/${TARGETARCH}/jush-server /usr/lib/jush/jush-server
# As setup links it on a machine, for `docker exec ... jush-server status`.
RUN ln -s /opt/jush/current/bin/jush-server /usr/local/bin/jush-server

ENV JUSH_UPGRADE_CHANNEL=${CHANNEL} \
    JUSH_BACKEND_LISTEN=0.0.0.0:3271 \
    JUSH_BACKEND_DATA=/opt/jush/data/backend \
    JUSH_MANAGED_DATA=/opt/jush/data/cloud

EXPOSE 3271
VOLUME /opt/jush
STOPSIGNAL SIGTERM
ENTRYPOINT ["/usr/lib/jush/jush-server", "container"]
