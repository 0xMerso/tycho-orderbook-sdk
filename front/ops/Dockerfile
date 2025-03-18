# syntax=docker.io/docker/dockerfile:1

FROM node:20-alpine AS base

# Install dependencies only when needed
FROM base AS deps
RUN apk add --no-cache libc6-compat
WORKDIR /app

# Enable pnpm globally
RUN corepack enable && corepack prepare pnpm@latest --activate

# Copy package manager files
COPY package.json pnpm-lock.yaml* .npmrc* ./

# Install dependencies
RUN pnpm install --frozen-lockfile

# Rebuild the source code only when needed
FROM base AS builder
WORKDIR /app

# Enable pnpm in builder stage
RUN corepack enable && corepack prepare pnpm@latest --activate

COPY --from=deps /app/node_modules ./node_modules
COPY . .

# Run the build process
RUN pnpm run build

# Ensure public folder exists
RUN mkdir -p /app/public

# Production image, copy all the files and run Next.js
FROM base AS runner
WORKDIR /app

# ENV NODE_ENV=development
ENV NODE_ENV=development

# Enable corepack and prepare pnpm as root
RUN corepack enable && corepack prepare pnpm@latest --activate

RUN addgroup --system --gid 1001 nodejs
RUN adduser --system --uid 1001 nextjs

# Only copy public/ if it exists
COPY --from=builder /app/public ./public

# >>> ADDED <<<
COPY --from=builder /app/.next/static ./.next/static

# Ensure standalone exists before copying
COPY --from=builder --chown=nextjs:nodejs /app/.next ./.next


USER nextjs

EXPOSE 3000
ENV PORT=3000
ENV HOSTNAME="0.0.0.0"

# CMD ["node", ".next/standalone/server.js"]
CMD ["pnpm", "start"]
