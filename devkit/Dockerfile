FROM emscripten/emsdk:5.0.4@sha256:ef91f658e0104636cf40a702c99169273969cf04d939f4f08e5d0223965d5788

ARG RUST_VERSION=1.85.1
ENV PATH="/root/.cargo/bin:${PATH}"

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --profile minimal --default-toolchain "${RUST_VERSION}" \
    && rustup target add wasm32-unknown-unknown \
    && rustc --version \
    && node --version \
    && emcc --version

WORKDIR /opt/cardputerzero-devkit
COPY . .
RUN cargo build --locked --release -p cp0ctl \
    && install -m 0755 target/release/cp0ctl /usr/local/bin/cp0ctl \
    && ./skills/cardputerzero-build-app/scripts/doctor.sh /opt/cardputerzero-devkit

ENV CP0_DEVKIT_ROOT=/opt/cardputerzero-devkit
WORKDIR /workspace
CMD ["bash"]
