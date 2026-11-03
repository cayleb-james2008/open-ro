# Model provider authentication

Open-Ro requires one model connection only. Users can add multiple providers and switch between them at any time. All credentials are entered directly into the local desktop app or into an official provider CLI.

## OpenAI API key

1. Create a key at https://platform.openai.com/api-keys.
2. Choose **OpenAI API key** in Open-Ro and paste the key.
3. Select a model from the model field (for example, a current model enabled for your API project).
4. Test and save. API usage is billed by OpenAI Platform, separately from a ChatGPT subscription.

## ChatGPT subscription (Codex)

1. Install Codex CLI using the official instructions: https://chatgpt.com/codex.
2. Run `codex login` in a terminal and select **Sign in with ChatGPT**.
3. In Open-Ro, choose **ChatGPT subscription (Codex CLI)** and select **Check connection**.

Open-Ro runs the local `codex exec` command against the selected project. The official CLI handles sign-in, subscription entitlements, and provider limits. Open-Ro does not read Codex's auth files.

## Anthropic API key

1. Create a key in the Anthropic Console: https://console.anthropic.com/settings/keys.
2. Choose **Anthropic API key** and paste it into Open-Ro.
3. Select an API model and test/save. Usage is billed through Anthropic Console.

## Claude subscription (Claude Code CLI)

1. Install Claude Code from https://code.claude.com/docs/en/setup.
2. Launch `claude` in a terminal and complete the sign-in flow inside Claude Code.
3. Choose **Claude subscription (Claude Code CLI)** in Open-Ro and select **Check connection**.

Open-Ro invokes the official, unmodified `claude` executable in the chosen project directory. Authentication is completed only in Claude Code. Open-Ro does not implement Claude.ai OAuth, extract session tokens, or send subscription credentials to another service. For third-party integrations, use an Anthropic API key instead.

## OpenRouter

1. Create an account and key at https://openrouter.ai/keys.
2. Choose **OpenRouter**, paste the key, then enter an exact model ID from the OpenRouter catalog (for example `openai/model-name` or `anthropic/model-name`).
3. Test and save. Billing and provider fallbacks are managed by OpenRouter.

## OpenCode / OpenCode Go

1. Install OpenCode from https://opencode.ai/download.
2. Run `opencode auth login` or `/connect` in the OpenCode TUI and follow its official flow for OpenCode Models/Go or the provider you use.
3. Choose **OpenCode CLI** in Open-Ro and select **Check connection**.

Open-Ro calls the local OpenCode executable. It does not parse `auth.json` or reuse provider credentials outside OpenCode's supported interface. Do not use unofficial Claude Pro/Max plugins: OpenCode's provider documentation says Anthropic prohibits them.

## Ollama

1. Install from https://ollama.com/download.
2. Start Ollama and pull a model, e.g. `ollama pull qwen2.5-coder`.
3. Choose **Ollama (local)**. Default endpoint: `http://127.0.0.1:11434/v1`; enter the local model ID.

## Custom OpenAI-compatible endpoint

Use this for self-hosted gateways and services that support `/v1/chat/completions`. Supply the base URL, model ID, and optional API key. Do not point it at a provider subscription endpoint unless that provider explicitly permits third-party use.
