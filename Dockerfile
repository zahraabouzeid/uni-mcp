FROM node:22-bookworm-slim AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web ./
RUN npm run build

FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY profiles profiles
COPY .github/logo.svg .github/logo.svg
COPY --from=web /src/web/out web/out
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked --bin unimcp \
    && cp target/release/unimcp /usr/local/bin/unimcp \
    && mkdir /data

FROM gcr.io/distroless/cc-debian12:nonroot
LABEL org.opencontainers.image.source="https://github.com/zahraabouzeid/uni-mcp"
COPY --from=build /usr/local/bin/unimcp /usr/local/bin/unimcp
COPY --from=build --chown=nonroot:nonroot /data /data
VOLUME /data
EXPOSE 8787 8788
ENTRYPOINT ["/usr/local/bin/unimcp", "server"]
CMD ["--bind", "0.0.0.0", "--port", "8787", "--admin-port", "8788", "--data", "/data"]
