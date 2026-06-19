# syntax=docker/dockerfile:1

# Build stage: compile a release binary against the pinned toolchain.
# Pin the Debian release (bookworm) to match the distroless runtime below: the
# binary is dynamically linked to glibc, so building on a newer Debian than the
# runtime makes it require a glibc the runtime lacks (`GLIBC_2.39 not found`). The
# bare `:slim` tag floats its base OS, so an upstream rebuild silently breaks the
# image; keep both stages on the same Debian (12/bookworm, glibc 2.36).
FROM rust:1.89.0-slim-bookworm AS build
# Use the image's bundled toolchain and bypass rust-toolchain.toml, whose pinned
# cross-compilation targets are only needed for release packaging, not this build.
ENV RUSTUP_TOOLCHAIN=1.89.0
WORKDIR /src
COPY . .
RUN cargo build --release --locked && cp target/release/screencomp /screencomp

# Runtime stage: distroless with a C runtime (the gnu binary needs libc/libgcc),
# running as a non-root user.
FROM gcr.io/distroless/cc-debian12:nonroot AS runtime
LABEL org.opencontainers.image.title="screencomp" \
      org.opencontainers.image.description="Classify, gallery, and PR-comment screenshots for the visual-docs framework." \
      org.opencontainers.image.source="https://github.com/nickderobertis/screencomp" \
      org.opencontainers.image.licenses="MIT"
COPY --from=build /screencomp /usr/local/bin/screencomp
WORKDIR /work
ENTRYPOINT ["/usr/local/bin/screencomp"]
