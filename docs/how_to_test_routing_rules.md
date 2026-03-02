# How To Test Routing Rules

Example routing rules in `~/.vibemate/config.toml`:

```toml
[router]
default_provider = "openai"

[[router.rules]]
pattern = "gpt-mini"
provider = "openai"
model = "gpt-5-mini"

[[router.rules]]
pattern = "claude-sonnet"
provider = "anthropic"
model = "claude-sonnet-4.6"
```

## 1. Start router

```bash
cargo run -- router
```

## 2. Test `/v1/chat/completions`

```bash
curl -N -X POST http://127.0.0.1:12345/api/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -H 'Accept: text/event-stream' \
  --data '{"model":"gpt-mini","messages":[{"role":"user","content":"hello"}],"stream":true}'
```

## 3. Test `/v1/responses`

```bash
curl -N -X POST http://127.0.0.1:12345/api/v1/responses \
  -H 'Content-Type: application/json' \
  -H 'Accept: text/event-stream' \
  --data '{"model":"gpt-mini","input":"hello","stream":true}'
```

## 4. Test `/v1/messages`

```bash
curl -N -X POST http://127.0.0.1:12345/api/v1/messages \
  -H 'Content-Type: application/json' \
  -H 'Accept: text/event-stream' \
  --data '{"model":"claude-sonnet","max_tokens":64,"messages":[{"role":"user","content":"hello"}],"stream":true}'
```
