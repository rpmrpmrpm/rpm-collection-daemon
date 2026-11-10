# The daemon in an image of its own: one static binary and nothing else.
#
# Build for both architectures:
#   docker buildx build --platform linux/amd64,linux/arm64 -f packaging/docker/Dockerfile .
#
# It needs host networking (see compose.yaml): multicast DNS does not cross Docker's bridge network,
# so with a bridge network rpm cannot find the NAS.

FROM rust:1-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY . .
RUN cargo build --release -p rpm-daemon \
 && cp target/release/rpm-collection-daemon /rpm-collection-daemon

FROM scratch
COPY --from=build /rpm-collection-daemon /rpm-collection-daemon
# Mount the libraries at /music (read-only is enough) and a folder for the daemon's data at /data.
# The data folder must be writable by the user the container runs as.
ENV RPM_ROOT=/music \
    RPM_DATA=/data
USER 1000:1000
ENTRYPOINT ["/rpm-collection-daemon"]
