# syntax=docker/dockerfile:1.4
FROM rust:1.74.1-bookworm as builder
ARG TARGETPLATFORM
ARG GIT_V_VERSION
ARG ONNXRUNTIME_VERSION=1.17.0
RUN apt-get update && apt-get install -y llvm-dev libclang-dev clang cmake
WORKDIR /usr/src/edge-runtime
RUN --mount=type=cache,target=/usr/local/cargo/registry,id=${TARGETPLATFORM} \
    cargo install cargo-strip
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry,id=${TARGETPLATFORM} --mount=type=cache,target=/usr/src/edge-runtime/target,id=${TARGETPLATFORM} \
    GIT_V_TAG=${GIT_V_VERSION} cargo build --release && \
    cargo strip && \
    mv /usr/src/edge-runtime/target/release/edge-runtime /root
RUN ./scripts/install_onnx.sh $ONNXRUNTIME_VERSION $TARGETPLATFORM /root/libonnxruntime.so


FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y libssl-dev && rm -rf /var/lib/apt/lists/*
RUN apt-get remove -y perl && apt-get autoremove -y
COPY --from=builder /root/edge-runtime /usr/local/bin/edge-runtime
COPY --from=builder /root/libonnxruntime.so /usr/local/bin/libonnxruntime.so
COPY ./models /etc/sb_ai/models
ENV ORT_DYLIB_PATH=/usr/local/bin/libonnxruntime.so
ENV SB_AI_MODELS_DIR=/etc/sb_ai/models
ENTRYPOINT ["edge-runtime"]
