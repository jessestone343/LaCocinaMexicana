FROM node:24-bookworm-slim AS frontend
WORKDIR /app
COPY package*.json ./
RUN npm ci
COPY . .
RUN npm run build

FROM rust:1.93-bookworm AS backend
WORKDIR /app
COPY backend ./backend
COPY content.seed.json ./content.seed.json
RUN cargo build --release --locked --manifest-path backend/Cargo.toml

FROM debian:bookworm-slim
RUN groupadd --gid 10001 cocina && useradd --uid 10001 --gid cocina --no-create-home cocina \
    && mkdir /data && chown cocina:cocina /data
WORKDIR /app
COPY --from=frontend /app/dist ./dist
COPY --from=backend /app/backend/target/release/cocina-dashboard ./cocina-dashboard
ENV LISTEN_ADDR=0.0.0.0:3000 STATIC_DIR=/app/dist DATA_DIR=/data COOKIE_SECURE=true RUST_LOG=info
USER cocina
EXPOSE 3000
VOLUME /data
CMD ["/app/cocina-dashboard"]
