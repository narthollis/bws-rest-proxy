ARG BWS_VERSION=0.3.0

FROM debian:stable-slim as dl
ARG BWS_VERSION

RUN apt update && \
    apt install -y wget unzip
RUN wget https://github.com/bitwarden/sdk/releases/download/bws-v${BWS_VERSION}/bws-x86_64-unknown-linux-gnu-${BWS_VERSION}.zip && \
    unzip bws-x86_64-unknown-linux-gnu-${BWS_VERSION}.zip && \
    chmod +x bws

RUN ls -lah

FROM debian:stable-slim

RUN apt-get update && \
    apt-get install -y ca-certificates openssl libssl-dev && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

COPY --from=dl /bws /bws

ENTRYPOINT ["/bws"]

