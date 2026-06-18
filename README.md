# Lekta API

Backend API for Lekta — a multi-tenant SaaS platform for tutorial centers in Nigeria.

## Tech Stack

- **Language:** Rust 1.88
- **Framework:** Axum 0.8
- **Database:** PostgreSQL 16 (Neon in production, Docker locally)
- **Cache:** Redis 7 (Upstash in production, Docker locally)
- **Storage:** AWS S3
- **Email:** AWS SES
- **AI:** Anthropic Claude
- **Payments:** Paystack
- **Push Notifications:** Firebase Cloud Messaging v1
- **Deployment:** Railway
- **Monitoring:** Sentry

## Prerequisites

- Rust 1.88 or later (via [rustup](https://rustup.rs))
- Docker Desktop (for local Postgres and Redis)
- `sqlx-cli`: `cargo install sqlx-cli --no-default-features --features postgres,rustls`

## Local Setup

1. Clone the repo:
```bash
   git clone git@github.com:<your-username>/lekta-api.git
   cd lekta-api
```

2. Copy environment template and fill in values:
```bash
   cp .env.example .env
   # Edit .env with your credentials
```

3. Start local Postgres and Redis:
```bash
   docker run -d --name lekta-pg \
     -e POSTGRES_USER=lekta -e POSTGRES_PASSWORD=lekta -e POSTGRES_DB=lekta_dev \
     -p 5432:5432 -v lekta-pg-data:/var/lib/postgresql/data postgres:16

   docker run -d --name lekta-redis \
     -p 6379:6379 -v lekta-redis-data:/data \
     redis:7-alpine redis-server --appendonly yes
```

4. Run migrations:
```bash
   sqlx migrate run
```

5. Run the server:
```bash
   cargo run
```

## Testing

```bash
cargo test
```

## Deployment

Push to `main` — Railway auto-deploys after CI passes.

## License

MIT