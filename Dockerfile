FROM docker.io/rust:1.86.0-alpine as build
ARG TARGETARCH

RUN apk add --no-cache git musl-dev openssl-dev pkgconfig

WORKDIR /build
# Copy cargo toml and lock so we can cache the fetch 
COPY Cargo.lock Cargo.toml /build/
# Create dummy files for being able to cache deps fetch and deps build
RUN mkdir /build/src && \
    echo 'fn main() { println!("fake build"); }' > /build/src/main.rs && \
    cargo fetch --config net.git-fetch-with-cli=true
RUN case "$TARGETARCH" in arm64) a="aarch64" ;; amd64) a="x86_64" ;; esac && \
    RUSTFLAGS="-Ctarget-feature=-crt-static -Clink-arg=-Wl,--dynamic-linker=/lib/ld-musl-$a.so.1" \
    cargo build --release && \
    rm -Rvf /build/src

# Now copy the source and build it
COPY src/ /build/src/
RUN touch src/main.rs && \
    case "$TARGETARCH" in arm64) a="aarch64" ;; amd64) a="x86_64" ;; esac && \
    RUSTFLAGS="-Ctarget-feature=-crt-static -Clink-arg=-Wl,--dynamic-linker=/lib/ld-musl-$a.so.1" \
    cargo build --release

RUN ls /build/target/release

FROM docker.io/alpine:3.21
ARG TARGETARCH

RUN apk add --no-cache ca-certificates libssl3 libgcc

COPY --from=build /build/target/release/bws-rest-proxy /bws-rest-proxy

ENTRYPOINT ["/bws-rest-proxy"]
CMD ["0.0.0.0", "3030"]
