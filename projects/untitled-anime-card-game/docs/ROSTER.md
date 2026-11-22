# Anime Card Roster — Untitled Anime Card Game (UACG)

> **Document Status:** Design Milestone 1 (Canonical Reference)
> **Companion Docs:** [docs/GDD.md](./GDD.md) · [docs/ECONOMY.md](./ECONOMY.md) · [docs/QUALITY-BAR.md](./QUALITY-BAR.md) · [docs/RESEARCH.md](./RESEARCH.md)
> **Naming Convention:** Every card displays its **iconic in-show title/epithet** in-game (the *Anime Nexus* convention) and records the **exact anime character + show** and **dynamic action pose cue** below for OpenRO `reference_search` image sourcing.

---

## 1. Roster Rules, Roles & Stat Baselines

### 1.1 Card Roles (5)
| Role | Combat Identity | Primary Stat Focus |
| :--- | :--- | :--- |
| **Vanguard** | Frontline tank, taunt, shield, damage reduction, reflect | Highest `HP`, solid `ATK` |
| **Striker** | High single-target burst, crit scaling, pierce, execute | Highest `ATK` & `SPD` |
| **Tactician** | Multi-target AoE, defense shred, burn/bleed, true damage | Balanced `ATK` & `SPD` |
| **Controller** | Stun, Freeze, Silence, Buff Purge, Dodge/Counter denial | Highest `SPD`, moderate `HP` |
| **Support** | Team heal, Cleanse, ATK/SPD buffs, Resurrect, energy boost | High `HP` & `SPD` |

### 1.2 Base Level-1 Stat Anchors by Rarity (Normal `1.0x`, `★0`)
| Rarity | Base HP | Base ATK | Base SPD | Special Trait / Kit Budget |
| :--- | :--- | :--- | :--- | :--- |
| **Common** (`1-in-2`) | `450` | `85` | `100` | 1 simple Active skill |
| **Uncommon** (`1-in-5`) | `720` | `135` | `108` | 1 Active + minor self-stat Passive |
| **Rare** (`1-in-20`) | `1,300` | `240` | `116` | 1 Active + utility Passive |
| **Epic** (`1-in-100`) | `2,800` | `520` | `126` | 1 Active + synergy Passive |
| **Legendary** (`1-in-1,000`) | `7,500` | `1,400` | `140` | Signature Active + Role-defining Passive |
| **Mythic** (`1-in-25,000`) | `24,000` | `4,600` | `158` | Dual-effect Active + Team Aura Passive |
| **Secret** (`1-in-250,000`) | `82,000` | `16,500` | `178` | Counter-Mechanic Active + Boss-Breaker Passive |
| **Universal** (`1-in-2.5M`) | `310,000` | `64,000` | `202` | Arena-warping Active + Cross-Team Passive |
| **Nexus** (`1-in-25M`) | `1,250,000` | `265,000` | `230` | Domain Active + Uncapped Scaling Passive |

*Note: Level `100` (`+4.5%`/lvl), `★5` Ascension (`+20%`/star), `Void` Mutation (`7.5x`), Rebirths (`+10%`/Rebirth), Skill Tree, and Bonds scale endgame power cleanly into `M` / `B` / `T` / `Qa` formatted combat numbers.*

---

## 2. World 1 — Hidden Leaf Village (*Naruto*)
- **Banner:** `Shinobi Scroll Pack` | **Show Bond:** *Will of Fire* (`2+`: `+12% ATK/SPD`, `4`: `+28% ATK/SPD` + `+10% Dodge`)

| Display Name (In-Show Epithet) | Exact Character & Show | Rarity | Role | Active & Passive Kit | Reference Pose Cue |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Academy Genin** | Konohamaru Sarutobi (*Naruto*) | Common | Striker | *Rasengan Spark*: `135% ATK` single-target; `+10% ATK` when paired with Leaf allies | Mid-air lunge with small blue chakra sphere |
| **Tactical Chunin** | Shikamaru Nara (*Naruto*) | Uncommon | Controller | *Shadow Possession*: `110% ATK` + `35% Stun` (1 turn); `-10% enemy SPD` passive | Kneeling shadow-stitch hand seal pose |
| **Lotus Taijutsu** | Rock Lee (*Naruto*) | Rare | Striker | *Primary Lotus*: `175% ATK` single-target + `Pierce`; `+15% SPD` below `60% HP` | Green chakra aura airborne kick |
| **Crimson Copy Ninja** | Kakashi Hatake (*Naruto*) | Epic | Tactician | *Lightning Blade*: `190% ATK` + `Silence` (1 turn); copies first enemy buff | Low sprint with crackling blue Chidori in hand |
| **Sage of Toads** | Jiraiya (*Naruto*) | Legendary | Support | *Sage Art Bath*: Heals team `22% Max HP` + `Cleanse` + `+20% ATK` (2 turns) | Standing atop giant toad scroll in Sage cloak |
| **Uchiha Prodigy** | Itachi Uchiha (*Naruto*) | Mythic | Controller | *Tsukuyomi Gaze*: `240% ATK` AoE + `65% Stun` + `Burn`; `25% Dodge` vs first hit | Akatsuki cloak floating surrounded by crows |
| **Ghost of the Uchiha** | Madara Uchiha (*Naruto*) | Secret | Vanguard | *Perfect Susanoo Cleave*: `260% ATK` AoE + `35% Shield`; `Reflect 20%` incoming DMG | Armored crimson/blue Susanoo blade slash |
| **Seventh Hokage** | Naruto Uzumaki (*Naruto*) | Universal | Striker | *Baryon Rasenshuriken*: `340% ATK` + `Burn` + `Pierce`; drains `15%` target energy per hit | Glowing orange Six Paths cloak surging forward |
| **Rinnegan Shadow** | Sasuke Uchiha (*Naruto*) | Nexus | Tactician | *Indra Arrow*: `420% ATK` AoE + `Silence` + `Def Shred 40%`; `+15% ATK` per turn elapsed | Purple Susanoo drawing lightning bow |

---

## 3. World 2 — West City & Namek (*Dragon Ball*)
- **Banner:** `Saiyan Capsule Pack` | **Show Bond:** *Z-Warrior Spirit* (`2+`: `+15% ATK`, `4`: `+35% ATK` + `+15% Crit DMG`)

| Display Name (In-Show Epithet) | Exact Character & Show | Rarity | Role | Active & Passive Kit | Reference Pose Cue |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Desert Bandit** | Yamcha (*Dragon Ball*) | Common | Striker | *Wolf Fang Fist*: `140% ATK` multi-hit; `+8% Crit Chance` passive | Martial arts claw stance in desert wind |
| **Turtle Hermit Disciple** | Krillin (*Dragon Ball*) | Uncommon | Tactician | *Destructo Disc*: `130% ATK` with `50% Pierce`; `+10% Team ATK` vs bosses | Holding spinning golden ki disc overhead |
| **Namekian Guardian** | Piccolo (*Dragon Ball*) | Rare | Vanguard | *Special Beam Cannon*: `165% ATK` `Pierce` + `20% Shield`; regenerates `8% HP/turn` | Two-finger forehead charge in white cape |
| **Mystic Prodigy** | Gohan (*Dragon Ball*) | Epic | Striker | *Father-Son Kamehameha*: `215% ATK` + `+30% DMG` if ally fell | Golden aura one-handed beam blast |
| **Saiyan Prince** | Vegeta (*Dragon Ball*) | Legendary | Striker | *Final Flash*: `255% ATK` + `Crit Guarantee` every 3rd turn; `+20% ATK` per hit taken | Blue Super Saiyan Blue energy flare palms forward |
| **Future Savior** | Future Trunks (*Dragon Ball*) | Mythic | Tactician | *Sword of Hope*: `270% ATK` AoE + `Cleanse` team + `Ignore Shield`; `+25% SPD` turn 1 | Raising glowing blue spirit sword aloft |
| **Legendary Berserker** | Broly (*Dragon Ball*) | Secret | Vanguard | *Omega Blaster*: `290% ATK` AoE + `Taunt` + `+12% ATK/HP` stacked every turn | Hulking green Full Power aura roaring |
| **Fused God Warrior** | Vegito (*Dragon Ball*) | Universal | Striker | *Final Kamehameha*: `360% ATK` + `Double Strike` + `Buff Purge`; `20% Counter` | Blue-gold Potara fusion dual energy blast |
| **Silver Autonomous Master** | Goku (*Dragon Ball*) | Nexus | Controller | *Ultra Instinct Sign*: `440% ATK` + `55% Auto-Dodge` passive + guaranteed counter-hit | Silver hair Ultra Instinct surging silver-violet ki |

---

## 4. World 3 — Grand Line Seas (*One Piece*)
- **Banner:** `Grand Line Bounty Pack` | **Show Bond:** *Grand Fleet Pact* (`2+`: `+14% HP/ATK`, `4`: `+32% HP/ATK` + `+15% Shield Strength`)

| Display Name (In-Show Epithet) | Exact Character & Show | Rarity | Role | Active & Passive Kit | Reference Pose Cue |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Sniper King** | Usopp (*One Piece*) | Common | Controller | *Fire Bird Star*: `125% ATK` + `Burn`; `+10% Accuracy` passive | Drawing oversized Kabuto slingshot |
| **Cotton Candy Doctor** | Tony Tony Chopper (*One Piece*) | Uncommon | Support | *Rumble Guard*: Heals lowest-HP ally `24%` + `+15% DEF` (2 turns) | Monster-cute reindeer doctor leaping |
| **Black Leg Chef** | Sanji (*One Piece*) | Rare | Striker | *Diable Jambe*: `180% ATK` + `Burn` (`10% HP` over 2 turns); `+15% SPD` | Flaming leg mid-air kick in dark suit |
| **Surgeon of Death** | Trafalgar Law (*One Piece*) | Epic | Controller | *ROOM Shambles*: Swaps enemy frontline/backline + `Silence` + `Pierce` | Blue spherical ROOM dome around katana |
| **Pirate Hunter** | Roronoa Zoro (*One Piece*) | Legendary | Striker | *King of Hell*: `265% ATK` + `Bleed` + `50% Crit DMG`; ignores `35% DEF` | Three-sword stance wreathed in green Conqueror haki |
| **Flame Emperor** | Sabo (*One Piece*) | Mythic | Tactician | *Dragon Claw Flame*: `275% ATK` AoE + heavy `Burn`; immune to `Burn/Freeze` | Swirling orange Mera-Mera fire fist |
| **Red-Haired Emperor** | Shanks (*One Piece*) | Secret | Controller | *Divine Departure*: `310% ATK` + `Silence` (2 turns) + strips enemy `Dodge` | Drawing Gryphon saber with red-black haki lightning |
| **Darkness Quake** | Marshall D. Teach (*One Piece*) | Universal | Vanguard | *Black Vortex*: `330% ATK` AoE + `Disable Passives` (1 turn) + `35% Damage Reduction` | Dark smoke left hand + white quake crack right hand |
| **Warrior of Liberation** | Monkey D. Luffy (*One Piece*) | Nexus | Striker | *Bajrang Gun*: `450% ATK` AoE + `Stun` + `1x Resurrect` (`50% HP`) with Gear 5 form | White-haired Gear 5 laughing with giant lightning fist |

---

## 5. World 4 — Tokyo Jujutsu High (*Jujutsu Kaisen*)
- **Banner:** `Cursed Seal Pack` | **Show Bond:** *Domain Resonance* (`2+`: `+15% Skill DMG`, `4`: `+35% Skill DMG` + `-1 Ability Cooldown`)

| Display Name (In-Show Epithet) | Exact Character & Show | Rarity | Role | Active & Passive Kit | Reference Pose Cue |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Cursed Speech Student** | Toge Inumaki (*Jujutsu Kaisen*) | Common | Controller | *Dont Move*: `115% ATK` + `30% Stun`; `-8% enemy ATK` | Unzipped collar showing snake-fang mouth seal |
| **Ten Shadows Initiate** | Megumi Fushiguro (*Jujutsu Kaisen*) | Uncommon | Tactician | *Chimera Shadow*: `140% ATK` AoE + `-12% enemy DEF` | Shadow dog shikigami hand shadow puppet |
| **Black Flash Vessel** | Yuji Itadori (*Jujutsu Kaisen*) | Rare | Striker | *Divergent Black Flash*: `185% ATK` (`2.5x` on Crit); `+10% Crit Chance` per turn | Crimson-black lightning punch mid-impact |
| **Thunder God Sorcerer** | Hajime Kashimo (*Jujutsu Kaisen*) | Epic | Striker | *Mythical Beast Amber*: `220% ATK` + `Stun` (`45%`); `+25% SPD` passive | Crackling cyan lightning staff strike |
| **Queen of Curses** | Yuta Okkotsu (*Jujutsu Kaisen*) | Legendary | Support | *Rika Manifestation*: `220% ATK` AoE + team `Cleanse` + `20% Heal` | Holding katana beside towering cursed spirit Rika |
| **Sorcerer Killer** | Toji Fushiguro (*Jujutsu Kaisen*) | Mythic | Striker | *Inverted Spear*: `285% ATK` + `Purge Shields & Dodge`; immune to `Silence` | Lunging with cursed blade and coiled chain |
| **Brain of Curses** | Kenjaku (*Jujutsu Kaisen*) | Secret | Tactician | *Womb Profusion*: `305% ATK` AoE + `Gravity Crush` (`-30% enemy SPD/ATK`) | Stitched forehead sorcerer summoning cursed swarm |
| **King of Curses** | Ryomen Sukuna (*Jujutsu Kaisen*) | Universal | Tactician | *Malevolent Shrine*: `375% ATK` AoE + `Bleed` + `Pierce`; hits twice on debuffed foes | Four-armed Heian silhouette amid crimson slashing shrine |
| **Honored One** | Satoru Gojo (*Jujutsu Kaisen*) | Nexus | Controller | *Hollow Purple*: `460% ATK` True DMG + `Limitless` (`40% DMG Reduction` + `Freeze` counter) | Blindfold off glowing Six Eyes converging violet orb |

---

## 6. World 5 — Soul Society (*Bleach*)
- **Banner:** `Soul Reaper Zanpakuto Pack` | **Show Bond:** *Spiritual Pressure* (`2+`: `+14% All Stats`, `4`: `+30% All Stats` + `15% Lifesteal`)

| Display Name (In-Show Epithet) | Exact Character & Show | Rarity | Role | Active & Passive Kit | Reference Pose Cue |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Eleventh Squad Lieutenant** | Renji Abarai (*Bleach*) | Common | Vanguard | *Zabimaru Whip*: `135% ATK` + `+15% DEF` (2 turns) | Extended segmented bone-blade whip swing |
| **Ice Prodigy Captain** | Toshiro Hitsugaya (*Bleach*) | Uncommon | Controller | *Daiguren Hyorinmaru*: `135% ATK` + `40% Freeze` (1 turn) | Ice dragon wings and frozen sword thrust |
| **Thunder Flash Goddess** | Yoruichi Shihoin (*Bleach*) | Rare | Controller | *Shunko Raijin*: `175% ATK` + `25% Dodge`; always acts first in round 1 | Golden-violet lightning shroud high-speed dash |
| **Noble Senbonzakura** | Byakuya Kuchiki (*Bleach*) | Epic | Tactician | *Senbonzakura Kageyoshi*: `210% ATK` AoE + `Bleed`; `+15% ATK` per ally alive | Surrounded by swirling pink cherry-blossom blades |
| **Battle-Crazed Kenpachi** | Kenpachi Zaraki (*Bleach*) | Legendary | Vanguard | *Nozarashi Cleave*: `270% ATK` + `Lifesteal 20%`; gains `+15% ATK` below `50% HP` | Eyepatch off swinging massive jagged cleaver |
| **Clever Scientist Captain** | Kisuke Urahara (*Bleach*) | Mythic | Support | *Benihime Restructure*: Team `+25% ATK`, `Cleanse`, and `1x Resurrect` (`35% HP`) | Hat-shadowed smirk raising crimson energy stitch |
| **Head Captain Inferno** | Yamamoto Genryusai (*Bleach*) | Secret | Tactician | *Zanka no Tachi*: `325% ATK` AoE + intense `Burn` (`18% HP/turn`); `Reflect 20%` | Cloak of sunfire flames with scorched katana |
| **Transcendent Strategist** | Sosuke Aizen (*Bleach*) | Universal | Controller | *Kyoka Suigetsu*: `350% ATK` AoE + `Confusion/Silence` (2 turns) + `35% Dodge` | White Hogyoku robe floating above shattered mirror |
| **True Bankai Substitute** | Ichigo Kurosaki (*Bleach*) | Nexus | Striker | *True Getsuga Tensho*: `470% ATK` + `Execute` (`+50% DMG` vs `<45% HP`) + `Pierce` | Horn of Salvation half-mask firing crescent black-gold slash |

---

## 7. Exclusive Drop Cards (Bosses, Tower, Raids & Ranked)
These 9 cards **cannot be rolled on standard banners**—they validate endgame mastery across every mode:

| Display Name | Exact Character & Show | Rarity | Role | Unlock Source | Signature Mechanic |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Ten-Tails Vessel** | Obito Uchiha (*Naruto*) | Legendary | Vanguard | World 1 Boss (`Ten-Tails Jinchuriki`) | Truth-Seeking Orb shield (`-35% Skill DMG taken`) |
| **Golden Tyrant** | Frieza (*Dragon Ball*) | Mythic | Striker | World 2 Boss (`Galactic Tyrant`) | `Reflect 25%` + `Execute` finisher |
| **Strongest Creature** | Kaido (*One Piece*) | Mythic | Vanguard | World 3 Boss (`Azure Dragon Emperor`) | `Thunder Bagua` (`Stun` + `40% Shield`) |
| **Divine General** | Mahoraga (*Jujutsu Kaisen*) | Secret | Vanguard | World 4 Boss (`Eight-Handled Divergent General`) | Wheel Adaptation (`+15% DR` per repeated skill, max `60%`) |
| **Almighty King** | Yhwach (*Bleach*) | Universal | Controller | World 5 Boss (`Father of the Quincy`) | `1x Lethal Negate` + steals `1` enemy buff per turn |
| **Temporal Attendant** | Whis (*Dragon Ball*) | Universal | Support | Tower Floor `100` Clear (`Heavens Arena`) | Team `Cleanse` + `+30% SPD` + `1x Resurrect` |
| **Pirate King** | Gol D. Roger (*One Piece*) | Universal | Striker | Raid 1 (`Skull Island Raid`) | `Divine Departure` (`Ignore 100% Shield & DEF`) |
| **Disaster Flame** | Jogo (*Jujutsu Kaisen*) | Mythic | Tactician | Raid 2 (`Shibuya Breach Raid`) | `Coffin of the Iron Mountain` (un-cleansable `Burn`) |
| **Hokage Flash (KCM)** | Minato Namikaze (*Naruto*) | Secret | Controller | Async Ranked Seasonal Reward (`Grandmaster+`) | `Flying Raijin` (always acts first + `35% Team Dodge`) |

---

## 8. Iconic Cross-Card & Show Bonds
Equipping matching cards in the **Active Deck** or **Defense Deck** triggers bonus effects:

| Bond Name | Required Cards | Bonus Effect |
| :--- | :--- | :--- |
| **Team 7 Reunion** | `Seventh Hokage` + `Rinnegan Shadow` + `Crimson Copy Ninja` | `+25% ATK`, `+15% SPD`, `-1` turn ability cooldown |
| **Gods of Destruction** | `Silver Autonomous Master` + `Saiyan Prince` + `Temporal Attendant` | `+30% Crit DMG`, `+20% Dodge`, `+15% Max HP` |
| **Four Emperors** | `Warrior of Liberation` + `Red-Haired Emperor` + `Strongest Creature` + `Darkness Quake` | `+35% All Stats` + `20% Boss Damage` |
| **Special Grade Apex** | `Honored One` + `King of Curses` + `Queen of Curses` | `+30% Skill DMG` + `Ignore 25% Damage Reduction` |
| **War Powers of Soul Society** | `True Bankai Substitute` + `Transcendent Strategist` + `Battle-Crazed Kenpachi` | `+28% ATK/HP` + `15% Lifesteal` |
