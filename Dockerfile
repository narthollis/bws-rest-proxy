FROM docker.io/rust:1.72.0-alpine as build

RUN apk add --no-cache git libssl1.1 musl-dev openssl openssl-dev pkgconfig

WORKDIR /build
# Copy cargo toml and lock so we can cache the fetch 
COPY Cargo.lock Cargo.toml /build/
# Create dummy files for being able to cache deps fetch and deps build
RUN mkdir /build/src && \
    echo 'fn main() {}' > /build/src/main.rs && \
    cargo fetch --config net.git-fetch-with-cli=true && \
    cargo build --release && \
    rm -Rvf /build/src

# Now copy the source and build it
COPY src/ src/
RUN cargo build --release

FROM docker.io/alpine:3.18

RUN apk add --no-cache ca-certificates libssl1.1
RUN ln -s /lib/ld-musl-aarch64.so.1 /lib/ld-linux-aarch64.so.1

COPY --from=build /build/target/release/bws-rest-proxy /bws-rest-proxy

ENTRYPOINT ["/bws-rest-proxy"]
CMD ["0.0.0.0", "3030"]
