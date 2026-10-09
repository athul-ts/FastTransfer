# FastTransfer — Linux build environment
#
# This image builds the Linux bundles (AppImage, DEB, RPM) and runs the test
# suites. It is NOT how the app is distributed — a desktop app is delivered as
# a signed installer, not a container. See SRS §83.
#
# Usage:
#   docker build -t fasttransfer-build .
#   docker run --rm -v "$PWD:/app" fasttransfer-build npm run tauri build

FROM ubuntu:22.04

ENV DEBIAN_FRONTEND=noninteractive
ENV CARGO_HOME=/usr/local/cargo
ENV RUSTUP_HOME=/usr/local/rustup
ENV PATH=/usr/local/cargo/bin:$PATH

# Tauri 2 Linux dependencies (WebKitGTK 4.1, not 4.0).
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential \
      curl \
      wget \
      file \
      pkg-config \
      libssl-dev \
      libwebkit2gtk-4.1-dev \
      libayatana-appindicator3-dev \
      librsvg2-dev \
      libgtk-3-dev \
      libsoup-3.0-dev \
      patchelf \
      rpm \
      fakeroot \
      xz-utils \
    && rm -rf /var/lib/apt/lists/*

# Node 20
RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

# Rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --default-toolchain stable --profile minimal \
    && rustup component add clippy rustfmt

WORKDIR /app

# Cache dependency layers. Source changes won't invalidate these.
COPY package.json package-lock.json* ./
RUN npm ci

COPY src-tauri/Cargo.toml src-tauri/Cargo.lock* ./src-tauri/
RUN mkdir -p src-tauri/src && echo "fn main() {}" > src-tauri/src/main.rs \
    && cargo build --release --manifest-path src-tauri/Cargo.toml \
    && rm -rf src-tauri/src

COPY . .

CMD ["npm", "run", "tauri", "build"]
