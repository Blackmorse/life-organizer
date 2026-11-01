# syntax=docker/dockerfile:1
FROM rust:1.89-alpine3.22 AS build
RUN apk add --no-cache build-base pkgconf openssl-dev openssl-libs-static
WORKDIR /app
ENV OPENSSL_STATIC=1
COPY Cargo.toml Cargo.lock build.rs ./
COPY src ./src
COPY migrations ./migrations
RUN cargo build --locked --release --bin backend

FROM alpine:3.22
RUN apk add --no-cache ca-certificates \
    && addgroup -S app && adduser -S -G app app
COPY --from=build /app/target/release/backend /usr/local/bin/backend
USER app
ENV BIND_ADDRESS=0.0.0.0:3000
EXPOSE 3000
ENTRYPOINT ["/usr/local/bin/backend"]
