FROM node:24-slim AS installer

RUN corepack enable

WORKDIR /app

COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./

RUN pnpm install --frozen-lockfile

FROM installer AS builder

ENV NODE_OPTIONS=--dns-result-order=ipv4first

COPY . .

RUN pnpm run build

FROM scratch

COPY --from=builder /app/dist/client /client



