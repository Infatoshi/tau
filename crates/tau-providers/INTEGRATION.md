# tau-cli provider selection

`tau-providers` now exposes three `Provider` implementations:

- `AnthropicProvider`
- `OpenAiResponsesProvider`
- `OpenAiChatProvider`

`tau-cli` can add a provider selector without changing `tau-core::agent`, because each implementation returns the canonical `tau_llm::ProviderStream`.

```rust
use std::sync::Arc;
use tau_llm::Provider;
use tau_providers::{AnthropicProvider, OpenAiChatProvider, OpenAiResponsesProvider};

#[derive(Clone, Debug)]
enum ProviderKind {
    Anthropic,
    OpenAiResponses,
    OpenAiChat,
}

fn build_provider(
    kind: ProviderKind,
    model: Option<String>,
    base_url: Option<String>,
) -> anyhow::Result<Arc<dyn Provider>> {
    match kind {
        ProviderKind::Anthropic => Ok(Arc::new(AnthropicProvider::from_env()?)),
        ProviderKind::OpenAiResponses => {
            Ok(Arc::new(OpenAiResponsesProvider::from_env(model)?))
        }
        ProviderKind::OpenAiChat => {
            Ok(Arc::new(OpenAiChatProvider::from_env(model, base_url)?))
        }
    }
}
```

Suggested CLI flags:

```text
--provider anthropic|openai-responses|openai-chat
--model MODEL
--base-url URL
```

Use `--base-url` only for `openai-chat`; it defaults to `https://api.openai.com/v1` and is intended for OpenAI-compatible APIs such as OpenRouter, Groq, and xAI.

Environment variables:

```text
ANTHROPIC_API_KEY      required for anthropic
OPENAI_API_KEY         required for openai-responses and openai-chat
```

Model defaults:

```text
anthropic              existing tau-cli default
openai-responses       gpt-5
openai-chat            gpt-4o
```

One current trait limitation: OpenAI Responses has both an output item `id` and a `call_id`. The canonical `tau_llm::ToolCall` has only one `id`, so `OpenAiResponsesProvider` maps the canonical id to `call_id`. That is the value `tau-cli` must carry back in `ContentBlock::ToolResult.tool_use_id` so the provider can send `function_call_output.call_id`.
