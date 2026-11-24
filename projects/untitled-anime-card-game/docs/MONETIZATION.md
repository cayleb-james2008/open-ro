# Monetization Sheet — Untitled Anime Card Game (UACG)

> **STATUS: DRAFT — PENDING OWNER APPROVAL (`priceApproved: false` in `openro.project.json`)**
> All Robux prices below are benchmarked proposals awaiting explicit owner sign-off before any live `MarketplaceService` IDs are bound or sales are enabled.
> **Companion Docs:** [docs/GDD.md](./GDD.md) · [docs/ECONOMY.md](./ECONOMY.md) · [docs/RESEARCH.md](./RESEARCH.md)

---

## 1. Monetization Philosophy & Ethical Guardrails

1. **Fast Fun for F2P, Time-Saving & Luck Depth for Spenders:** F2P players hit their first `Legendary` in `< 15 min` and first `Rebirth` in `2–4 h` without spending Robux.
2. **No Player Trading at Launch:** Protects game integrity, prevents black-market RMT/scams, and eliminates rollback disputes.
3. **Transparent Odds & Confirmations:** Every banner shows exact `1-in-N` odds, current stacked `Effective Luck`, and Pity counters; purchases use native Roblox prompts with server-verified `ProcessReceipt` / `UserOwnsGamePassAsync`.
4. **Zero Jargon & Clean UI:** Plain user-facing names (`2x Luck`, `Fast Roll`, `Auto Roll`) and Light/Dark toggle kept strictly inside Settings.

---

## 2. Gamepasses (Permanent Account Upgrades — Draft)

Benchmarked against *Anime Nexus RNG X* (`40–499 R$`) and *Anime Card Battles: Rebirth*.

| Slug (`openro.project.json`) | Display Name | Draft Price | Status | Effect Summary |
| :--- | :--- | :--- | :--- | :--- |
| `pass_extra_storage` | **Extra Card Storage (+150)** | `49 R$` | `PENDING OWNER APPROVAL` | Expands card inventory cap by `+150` slots (base `150`, stackable/up to `300` with pass). |
| `pass_extra_deck_slots` | **+Deck Slots (+2)** | `79 R$` | `PENDING OWNER APPROVAL` | Unlocks `2` extra saved Active/Defense deck presets (total `4` presets) for Tower, Bosses, and Ranked. |
| `pass_lucky` | **Lucky (1.5x)** | `99 R$` | `PENDING OWNER APPROVAL` | Permanent `1.5x` multiplicative Luck on all World Banners. Stacks with other Luck passes. |
| `pass_fast_roll` | **Fast Roll** | `129 R$` | `PENDING OWNER APPROVAL` | Cuts roll cooldown/animation from `1.60s` to `0.60s`. |
| `pass_2x_luck` | **2x Luck** | `149 R$` | `PENDING OWNER APPROVAL` | Permanent `2.0x` multiplicative Luck on all World Banners. Stacks with `Lucky` and `Mega Lucky`. |
| `pass_auto_roll` | **Auto Roll** | `149 R$` | `PENDING OWNER APPROVAL` | Continuous hands-free rolling with customizable Rarity & Mutation Auto-Sell / Auto-Ascend filters (`Secret+` always protected). |
| `pass_mega_lucky` | **Mega Lucky (3x)** | `199 R$` | `PENDING OWNER APPROVAL` | Permanent `3.0x` multiplicative Luck on all World Banners. |
| `pass_multi_roll` | **Multi-Roll (5x)** | `299 R$` | `PENDING OWNER APPROVAL` | Rolls `5` cards simultaneously per pull cycle on Yen banners. |
| `pass_vip` | **VIP** | `349 R$` | `PENDING OWNER APPROVAL` | `1.20x` Luck, `+15%` Potion duration, `+25%` Yen & Gem battle drops, `[VIP]` badge, `100 Gems` daily bonus. |
| `pass_mutation_hunter` | **Mutation & Shiny Hunter** | `449 R$` | `PENDING OWNER APPROVAL` | Doubles (`2.0x`) the roll weight of `Shiny`, `Golden`, `Rainbow`, and `Void` mutations. |

---

## 3. Developer Products (Repeatable Consumables & Bundles — Draft)

| Slug (`openro.project.json`) | Display Name | Draft Price | Status | Contents & Economy Role |
| :--- | :--- | :--- | :--- | :--- |
| `prod_starter_bundle` | **Shinobi Starter Bundle** | `149 R$` | `PENDING OWNER APPROVAL` | **One-time offer (first 72h):** `2,500 Gems`, `25 Roll Tickets`, `2x Major Luck Potions`, `10,000 Yen`, `1x Guaranteed Epic+ Card`. |
| `prod_gems_pouch` | **Pouch of Gems (1,200)** | `99 R$` | `PENDING OWNER APPROVAL` | `1,200 Gems` (`12.1 Gems/R$`). Entry Skill Tree / Potion pack. |
| `prod_gems_chest` | **Chest of Gems (3,500)** | `249 R$` | `PENDING OWNER APPROVAL` | `3,500 Gems` (`14.1 Gems/R$`, `+16%` bonus). |
| `prod_gems_vault` | **Vault of Gems (8,000)** | `499 R$` | `PENDING OWNER APPROVAL` | `8,000 Gems` (`16.0 Gems/R$`, `+32%` bonus). |
| `prod_gems_treasury` | **Treasury of Gems (20,000)** | `999 R$` | `PENDING OWNER APPROVAL` | `20,000 Gems` (`20.0 Gems/R$`, `+65%` bonus). |
| `prod_tickets_15` | **15 Roll Tickets** | `79 R$` | `PENDING OWNER APPROVAL` | `15 Roll Tickets` (`2.0x` built-in Ticket Luck, instant 5-pull packs). |
| `prod_tickets_50` | **50 Roll Tickets** | `229 R$` | `PENDING OWNER APPROVAL` | `50 Roll Tickets` (`14%` discount vs 15-pack). |
| `prod_tickets_150` | **150 Roll Tickets** | `599 R$` | `PENDING OWNER APPROVAL` | `150 Roll Tickets` (`24%` discount vs 15-pack). |
| `prod_potions_alchemist` | **Alchemist Luck Pack** | `99 R$` | `PENDING OWNER APPROVAL` | `3x Major Luck Potion (2.5x, 15m)` + `1x Super Luck Potion (4.0x, 20m)`. |
| `prod_potions_boss` | **Boss Hunter Potion Pack** | `249 R$` | `PENDING OWNER APPROVAL` | `3x Super Luck Potion (4.0x, 20m)` + `2x Boss Luck Potion (6.0x, 10m)`. |
| `prod_trait_crystals_25` | **25 Trait Crystals** | `149 R$` | `PENDING OWNER APPROVAL` | `25 Trait Crystals` (guarantees at least `1x Epic+` trait via 25-crystal pity). |
| `prod_season_pass_s1` | **Season 1 Pass: Dawn of Legends** | `399 R$` | `PENDING OWNER APPROVAL` | Unlocks the 30-Tier Premium Battle Pass track for the active 4-week season. |

---

## 4. Platform Perks, Private Servers & Season Pass

### 4.1 Roblox Premium Perk
- **Passive Bonus:** `1.15x` multiplicative Luck + `+10%` Potion duration (stacks multiplicatively with `VIP` for `1.38x` Luck and `+25%` Potion duration).
- **Playtime Boost:** `+10%` Gem payout on the `5m–120m` Playtime Reward ladder.
- **Group Join Bonus (Free for everyone):** `+1.0` Flat Luck (`2.0x` effective at baseline).

### 4.2 Private Servers
- **Recommended Price:** **`0 Robux` (Free)** — `PENDING OWNER APPROVAL`
- **Rationale:** Matches *Anime Nexus RNG X*'s free 8-player custom servers, giving friend groups a lag-free lobby for AFK Weather rolling and Co-op Raid coordination while increasing session length and social invites.

### 4.3 Season / Battle Pass (`Season 1: Dawn of Legends` — 4-Week Cadence)
- **30 Tiers** earned via Daily/Weekly Quest XP, Tower clears, and Raid completions:
  - **Free Track:** `75,000 Yen`, `2,000 Gems`, `20 Roll Tickets`, `10 Trait Crystals`, `3x Major Luck Potions`, Tier 30 `Legendary` Card.
  - **Premium Track (`399 R$`, `PENDING OWNER APPROVAL`):** All Free rewards + `6,000 Gems`, `40 Roll Tickets`, `35 Trait Crystals`, `2x Boss Luck Potions`, exclusive Seasonal Card Border, and Tier 30 Guaranteed `Mythic` Seasonal Variant Card.

---

## 5. Approval Checklist (Required Before Launch)

- [ ] Owner reviews and approves every price in `docs/MONETIZATION.md`.
- [ ] Replace `robloxId: null` with live Creator Dashboard Gamepass / Developer Product IDs.
- [ ] Flip `priceApproved` from `false` to `true` in `openro.project.json` per approved product.
- [ ] Verify `MarketplaceService.ProcessReceipt` idempotency with `ProfileStore` before public release.
