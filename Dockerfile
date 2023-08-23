FROM docker.io/rust:1.71.1-alpine as build

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

RUN apk add --no-cache ca-certificates libssl1.1
RUN ln -s /lib/ld-musl-aarch64.so.1 /lib/ld-linux-aarch64.so.1

COPY --from=build /build/target/release/bws-rest-proxy /bws-rest-proxy

ENTRYPOINT ["/bws-rest-proxy"]
CMD ["0.0.0.0", "3030"]
