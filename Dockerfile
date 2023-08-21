ARG RUST_VERSION=1.65.0

FROM docker.io/rust:$RUST_VERSION-alpine as build

RUN apk add --no-cache git musl-dev openssl openssl-dev pkgconfig 

WORKDIR /build
# Copy cargo toml and lock so we can cache the fetch 
COPY ./Cargo.lock ./Cargo.toml /build/
RUN mkdir src && touch src/main.rs
RUN cargo fetch --config net.git-fetch-with-cli=true 


# Now copy the source and build it
COPY src/ src/
RUN cargo build --release

FROM docker.io/alpine:3.18

RUN apk add --no-cache ca-certificates openssl

COPY --from=build /build/target/release/bws-rest-proxy /bws-rest-proxy

ENTRYPOINT ["/bws-rest-proxy"]
CMD ["0.0.0.0", "3030"]
