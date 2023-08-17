ARG RUST_VERSION=1.65.0

FROM docker.io/rust:$RUST_VERSION-alpine as build

WORKDIR /build
# Copy cargo toml and lock so we can cache the fetch 
COPY ./Cargo.lock ./Cargo.toml /build/
RUN mkdir src && touch src/main.rs
RUN cargo fetch

RUN apk add --no-cache musl-dev openssl openssl-dev pkgconfig 

# Now copy the source and build it
COPY src/ src/
RUN cargo build --release

FROM docker.io/alpine:3.14

RUN apk add --no-cache ca-certificates openssl

COPY --from=build /build/target/release/bws-rest-proxy /bws-rest-proxy

ENTRYPOINT ["/bws-rest-proxy"]

